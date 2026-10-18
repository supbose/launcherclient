//! 更新流程编排：读取配置 → 检查 → 下载（SHA256 校验）→ 安装 → 拉起目标程序。
//!
//! 与 Tauri 版的差异：窗口前完成启动决策——无更新且 launch 模式直接静默退出
//! （进程从未创建窗口）；需要 UI 时返回启动上下文，下载流程由后台线程执行，
//! 通过 [`UiProxy`] 把阶段状态写回共享快照驱动视图刷新。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use zsui::UiInvalidationHandle;

use crate::cleanup;
use crate::cli::CliArgs;
use crate::config;
use crate::download;
use crate::install;
use crate::manifest;
use crate::services;
use crate::ui::{format_bytes, SharedUi, Stage, UiProxy, UiSnapshot};

/// 默认服务器 manifest 地址（按序故障转移，第一个失败自动切换下一个）
const DEFAULT_MANIFEST_URLS: &[&str] = &["https://qsc.cdn.qcsm.ltd/update/qsm/version.json","https://qsw.us.ci/update/qsm/version.json"];

/// 默认应用名称（决定配置/下载/安装目录）
const DEFAULT_APP_NAME: &str = "qsm";

/// 进度事件推送间隔（毫秒）
const PROGRESS_INTERVAL_MS: u64 = 300;

/// 配置合并后的生效配置（优先级：命令行 > update.json > 默认值）
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub app_name: String,
    /// manifest 地址列表（按序故障转移）
    pub manifest_urls: Vec<String>,
    pub launch: Option<String>,
    /// 保留的版本目录数量（含当前版本）
    pub max_versions: usize,
    /// 更新前需停止、更新后恢复的 Windows 服务名列表
    pub services: Vec<String>,
}

/// 启动决策：窗口创建前确定是否需要 UI
pub enum StartupDecision {
    /// 无需 UI：静默处理完毕（launch 模式无更新），进程按退出码结束
    SilentExit(i32),
    /// 需要 UI：携带启动上下文
    Show(StartupContext),
}

/// 需要 UI 时的启动上下文
pub struct StartupContext {
    pub cfg: ResolvedConfig,
    /// 服务器 manifest（Some 表示发现新版本，需要后台执行下载流程）
    pub manifest: Option<manifest::FlatManifest>,
    pub current_version: String,
    /// UI 首帧渲染的初始快照
    pub initial: UiSnapshot,
}

/// 过滤空字符串配置项（update.json 中 "" 视为未配置）
fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

/// 合并命令行参数与 update.json 配置
pub fn resolve_config(args: &CliArgs) -> ResolvedConfig {
    let settings = config::load();

    let app_name = if args.app_name != "QsLtd" {
        // 命令行显式指定了应用名
        args.app_name.clone()
    } else {
        non_empty(settings.app_name.clone()).unwrap_or_else(|| DEFAULT_APP_NAME.to_string())
    };

    ResolvedConfig {
        app_name,
        manifest_urls: args
            .manifest_url
            .clone()
            .map(|u| vec![u])
            .or_else(|| settings.manifest_url.as_ref().map(|m| m.to_urls()))
            .unwrap_or_else(|| {
                DEFAULT_MANIFEST_URLS
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            }),
        launch: args.launch.clone().or_else(|| non_empty(settings.launch)),
        max_versions: settings
            .max_versions
            .filter(|v| *v > 0)
            .unwrap_or(config::DEFAULT_MAX_VERSIONS),
        services: settings
            .services
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect(),
    }
}

/// 按 app_name 派生下载目录
fn download_dir(app_name: &str) -> PathBuf {
    qingshu::update::get_myapp_dir(Some(app_name)).join("downloads")
}

/// 按 app_name 派生版本根目录（各版本目录的父目录）
fn install_root(app_name: &str) -> PathBuf {
    qingshu::update::get_myapp_dir(Some(app_name)).join("app")
}

/// 按 app_name 派生版本安装目录（app_{version}）
fn install_dir(app_name: &str, version: &str) -> PathBuf {
    install_root(app_name).join(format!("app_{}", version))
}

/// 本地版本记录 app.json 路径
fn local_manifest_path(app_name: &str) -> PathBuf {
    qingshu::update::get_myapp_dir(Some(app_name)).join("app.json")
}

/// --check-only 纯检测模式：只判断是否有更新，不建窗、不下载、不拉回目标程序。
///
/// 退出码：0 = 无更新，1 = 检测失败，2 = 有更新。
/// 供客户端快速分支：无更新时客户端自己继续启动（单次启动，无交接开销），
/// 有更新时客户端退出并改用 --launch 模式交接更新器。
pub async fn run_check_only(args: &CliArgs, cfg: &ResolvedConfig) -> i32 {
    // 先清理上次删除失败的残留目录
    let app_root = install_root(&cfg.app_name);
    cleanup::cleanup_pending(&app_root).await;

    let current_version = resolve_current_version(args, &cfg.app_name).await;
    eprintln!("[更新器] 当前版本: {}", current_version);

    eprintln!("[更新器] 开始拉取 manifest...");
    let m = match manifest::fetch_with_failover(&cfg.manifest_urls).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[更新器] 检查更新失败: {}", e);
            return 1;
        }
    };

    if manifest::need_update(&current_version, &m) {
        eprintln!(
            "[更新器] 发现新版本: {} → {}",
            current_version, m.version
        );
        2
    } else {
        eprintln!("[更新器] 已是最新版本: {}", current_version);
        0
    }
}

/// 启动阶段（窗口创建前）：清理残留 → 解析当前版本 → 拉取 manifest → 更新判断
pub async fn run_startup(args: &CliArgs, cfg: &ResolvedConfig) -> StartupDecision {
    // 先清理上次删除失败的残留目录
    let app_root = install_root(&cfg.app_name);
    cleanup::cleanup_pending(&app_root).await;

    // 当前版本：命令行 > update.json > 本地 app.json > 0.0.0（视为未安装/损坏自恢复）
    let current_version = resolve_current_version(args, &cfg.app_name).await;
    eprintln!("[更新器] 当前版本: {}", current_version);

    eprintln!("[更新器] 开始拉取 manifest...");
    let m = match manifest::fetch_with_failover(&cfg.manifest_urls).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[更新器] 检查更新失败: {}", e);
            return StartupDecision::Show(StartupContext {
                cfg: cfg.clone(),
                manifest: None,
                current_version,
                initial: UiSnapshot::with_phase(Stage::Error, "更新检查失败", &e, None),
            });
        }
    };

    if !manifest::need_update(&current_version, &m) {
        eprintln!("[更新器] 已是最新版本: {}", current_version);
        return match &cfg.launch {
            // 被目标程序拉起：静默拉起目标程序后退出，窗口全程不显示
            Some(_) => {
                // 安装目录从本地 app.json 读取（相对路径 launch 按此目录解析）
                let installed = read_installed_path(&cfg.app_name).await;
                launch_target(cfg, installed.as_deref().map(Path::new));
                StartupDecision::SilentExit(0)
            }
            // 仅检测模式（由目标程序启动时拉起）：无更新静默退出，不打扰目标程序
            None => StartupDecision::SilentExit(0),
        };
    }

    // ---- 发现新版本：UI 显示"发现新版本"，后台执行下载 → 校验 → 安装 ----
    let is_parts = m.parts.as_ref().is_some_and(|p| !p.is_empty());
    eprintln!(
        "[更新器] 发现新版本: {} -> {}（{}）",
        current_version,
        m.version,
        if is_parts {
            format!("分卷 {} 个", m.parts.as_ref().unwrap().len())
        } else {
            format!("下载地址: {}", m.url)
        }
    );
    let sub = format!("{} → {}", current_version, m.version);
    StartupDecision::Show(StartupContext {
        cfg: cfg.clone(),
        manifest: Some(m),
        current_version,
        initial: UiSnapshot::with_phase(Stage::NewVersion, "发现新版本", &sub, None),
    })
}

/// 启动后台更新线程（发现新版本后由 UI 侧调用）
///
/// 线程内建独立 tokio 运行时执行下载→校验→安装→收尾；失败时把错误阶段
/// 写回共享快照；launch 模式成功完成后拉起目标程序并退出整个进程。
pub fn spawn_update_flow(
    cfg: ResolvedConfig,
    m: manifest::FlatManifest,
    snapshot: SharedUi,
    handle: UiInvalidationHandle,
) {
    let spawned = std::thread::Builder::new()
        .name("update-flow".into())
        .spawn(move || {
            let proxy = UiProxy::new(snapshot, handle);
            let result = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(rt) => rt.block_on(run_update_flow(&cfg, &m, &proxy)),
                Err(e) => Err(format!("创建异步运行时失败: {}", e)),
            };
            if let Err(e) = result {
                // 各失败点已把错误写回 UI 快照，这里只留运行日志
                eprintln!("[更新器] 更新流程失败: {}", e);
            }
        });
    if let Err(e) = spawned {
        eprintln!("[更新器] 后台更新线程启动失败: {}", e);
    }
}

/// 完整更新流程：下载 → 校验 → 安装 → 记录 → 清理 → 收尾
async fn run_update_flow(cfg: &ResolvedConfig, m: &manifest::FlatManifest, proxy: &UiProxy) -> Result<(), String> {
    let is_parts = m.parts.as_ref().is_some_and(|p| !p.is_empty());
    let zip_path = download_dir(&cfg.app_name).join(format!("update_{}.zip", m.version));

    // 下载：分卷模式（parts 存在）或单包模式
    if is_parts {
        run_download_parts(proxy, m, &cfg.app_name, &zip_path).await?;
    } else {
        if m.url.is_empty() {
            let e = "manifest 缺少 url 字段（单包模式必填）".to_string();
            eprintln!("[更新器] 下载失败: {}", e);
            proxy.set_phase(Stage::Error, "更新包下载失败", &e, None);
            return Err(e);
        }
        run_download(proxy, m, &zip_path).await?;
    }

    // SHA256 校验
    proxy.set_phase(Stage::Checking, "正在校验更新包...", "", None);
    if let Err(e) = tokio::task::spawn_blocking({
        let zip_path = zip_path.clone();
        let sha256 = m.sha256.clone();
        move || download::verify_sha256(&zip_path, &sha256)
    })
    .await
    .unwrap_or_else(|e| Err(format!("校验任务异常: {}", e)))
    {
        eprintln!("[更新器] 更新包校验失败: {}", e);
        proxy.set_phase(Stage::Error, "更新包校验失败", &e, None);
        return Err(e);
    }
    eprintln!("[更新器] 更新包校验通过: {}", zip_path.display());

    // 安装前卸载用户配置的服务（运行中先停止；卸载后不恢复、不重启）
    if !cfg.services.is_empty() {
        let svc_names = cfg.services.clone();
        let svc_proxy = proxy.clone();
        tokio::task::spawn_blocking(move || {
            services::uninstall_configured(&svc_names, &|msg| {
                svc_proxy.set_phase(Stage::Installing, "正在处理服务...", msg, None);
            })
        })
        .await
        .ok();
    }

    // 安装
    let target_dir = install_dir(&cfg.app_name, &m.version);
    let install_proxy = proxy.clone();
    let install_cb: install::StageCallback =
        Arc::new(move |msg, pct| install_proxy.set_phase(Stage::Installing, "正在安装更新...", msg, pct));
    if let Err(e) = install::install(&zip_path, &target_dir, install_cb).await {
        eprintln!("[更新器] 安装失败: {}", e);
        proxy.set_phase(Stage::Error, "更新安装失败", &e, None);
        return Err(e);
    }
    eprintln!("[更新器] 安装完成: {}", target_dir.display());

    // 写入本地版本记录，供下次检查与目标程序定位
    if let Err(e) = install::save_local_manifest(
        &local_manifest_path(&cfg.app_name),
        &cfg.app_name,
        &m.version,
        &target_dir,
        cfg.launch.clone(),
    )
    .await
    {
        eprintln!("[更新器] 本地版本记录写入失败: {}", e);
        proxy.set_phase(Stage::Error, "本地版本记录写入失败", &e, None);
        return Err(e);
    }

    // 清理旧版本目录（旧目录即回滚资产，保留 max_versions 个）
    cleanup::cleanup_old_versions(&install_root(&cfg.app_name), &m.version, cfg.max_versions).await;

    // 更新完成后拉起目标程序并退出；手动模式则保留窗口由用户关闭
    match &cfg.launch {
        Some(_) => {
            launch_target(cfg, Some(&target_dir));
            std::process::exit(0);
        }
        None => {
            eprintln!("[更新器] 手动模式，更新完成，等待用户关闭");
            proxy.set_phase(
                Stage::Done,
                "更新完成",
                &format!("已更新到版本 {}", m.version),
                None,
            );
            Ok(())
        }
    }
}

/// 执行分卷下载：逐卷下载（复用合格卷）→ 合并为完整 zip → 整体校验
///
/// 合并产物与单包模式共用 zip_path，后续校验/安装/清理逻辑完全一致
async fn run_download_parts(
    proxy: &UiProxy,
    m: &manifest::FlatManifest,
    app_name: &str,
    zip_path: &Path,
) -> Result<(), String> {
    let urls = m.parts.as_ref().ok_or("分卷地址列表缺失")?;
    let parts_dir = download_dir(app_name).join(format!("update_{}_parts", m.version));
    let sha256s = m.part_sha256.as_deref().filter(|s| s.len() == urls.len());
    // 整体总大小（manifest.size_bytes），分卷之和等于完整 zip 大小
    let total_size = m.size_bytes;

    proxy.set_phase(
        Stage::Downloading,
        "正在下载更新包（分卷）...",
        &format!("共 {} 卷", urls.len()),
        Some(0.0),
    );

    let tracker = std::sync::Mutex::new(SpeedTracker::new());
    let proxy_cb = proxy.clone();
    let result =
        download::download_parts(urls, sha256s, &parts_dir, total_size, move |i, total, p| {
            let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
            let (should, speed) = t.tick(p.downloaded, p.percent() == Some(100.0));
            if should {
                let msg = format!("正在下载更新包（分卷 {}/{}）...", i + 1, total);
                // 总大小未知时按卷数估算百分比
                let percent = p
                    .percent()
                    .unwrap_or(((i + 1) as f64 / total as f64) * 100.0);
                proxy_cb.set_download(&msg, Some(percent), p.downloaded, p.total, Some(speed));
            }
        })
        .await?;

    proxy.set_phase(Stage::Checking, "正在合并分卷...", "", Some(0.0));
    eprintln!("[更新器] 合并 {} 个分卷...", result.part_paths.len());
    let merge_proxy = proxy.clone();
    download::merge_parts(&result.part_paths, zip_path, &m.sha256, move |done, total| {
        let pct = if total > 0 {
            (done as f64 / total as f64 * 100.0).min(100.0)
        } else {
            0.0
        };
        merge_proxy.set_phase(
            Stage::Checking,
            "正在合并分卷...",
            &format!("{} / {}", fmt_mb(done), fmt_mb(total)),
            Some(pct),
        );
    })
    .await?;
    eprintln!("[更新器] 分卷合并完成: {}", zip_path.display());
    Ok(())
}

/// 字节数格式化为 MB 显示（保留 1 位小数）
fn fmt_mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
}

/// 执行下载：本地已有同名包且校验通过则复用，否则流式下载并推送进度
async fn run_download(
    proxy: &UiProxy,
    m: &manifest::FlatManifest,
    zip_path: &Path,
) -> Result<(), String> {
    if zip_path.exists() {
        let cached = zip_path.to_path_buf();
        let sha256 = m.sha256.clone();
        let ok = tokio::task::spawn_blocking(move || download::verify_sha256(&cached, &sha256))
            .await
            .unwrap_or_else(|e| Err(format!("校验任务异常: {}", e)))
            .is_ok();
        if ok {
            let size = tokio::fs::metadata(zip_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            eprintln!(
                "[更新器] 复用已下载的更新包: {}（{}）",
                zip_path.display(),
                format_bytes(size)
            );
            proxy.set_download("使用已下载的更新包", Some(100.0), size, Some(size), None);
            return Ok(());
        }
        eprintln!("[更新器] 本地更新包校验不通过，重新下载");
        let _ = tokio::fs::remove_file(zip_path).await;
    }

    if let Some(parent) = zip_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建下载目录失败 {}: {}", parent.display(), e))?;
    }

    proxy.set_phase(Stage::Downloading, "正在下载更新包...", "", Some(0.0));
    let tracker = std::sync::Mutex::new(SpeedTracker::new());
    let proxy_cb = proxy.clone();
    let result = download::download_file(&m.url, zip_path, move |p| {
        let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
        let (should, speed) = t.tick(p.downloaded, p.percent() == Some(100.0));
        if should {
            proxy_cb.set_download(
                "正在下载更新包...",
                p.percent(),
                p.downloaded,
                p.total,
                Some(speed),
            );
        }
    })
    .await;

    match result {
        Ok((_, sha)) => {
            let size = tokio::fs::metadata(zip_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            eprintln!(
                "[更新器] 下载完成，大小 {}，SHA256: {}",
                format_bytes(size),
                sha
            );
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// 下载进度节流 + 速度平滑器（单包/分卷共用）
///
/// 按 PROGRESS_INTERVAL_MS 节流推送，速度用指数平滑（0.6 旧值 + 0.4 瞬时）避免数字跳动
struct SpeedTracker {
    /// 上次推送时间
    last_emit: Instant,
    /// 上次推送时的已下载字节
    last_bytes: u64,
    /// 平滑后的速度（字节/秒）
    smooth: f64,
}

impl SpeedTracker {
    fn new() -> Self {
        Self {
            last_emit: Instant::now(),
            last_bytes: 0,
            smooth: 0.0,
        }
    }

    /// 记录一次进度，判断是否应推送
    ///
    /// - `downloaded`: 当前已下载字节
    /// - `force`: 强制推送（如进度到 100%）
    ///
    /// 返回 (是否应推送, 平滑速度 字节/秒)
    fn tick(&mut self, downloaded: u64, force: bool) -> (bool, u64) {
        let elapsed = self.last_emit.elapsed().as_secs_f64();
        if !force && elapsed < PROGRESS_INTERVAL_MS as f64 / 1000.0 {
            return (false, 0);
        }
        let instant = if elapsed > 0.0 {
            (downloaded - self.last_bytes) as f64 / elapsed
        } else {
            0.0
        };
        self.smooth = if self.smooth > 0.0 {
            self.smooth * 0.6 + instant * 0.4
        } else {
            instant
        };
        self.last_bytes = downloaded;
        self.last_emit = Instant::now();
        (true, self.smooth as u64)
    }
}

/// 解析当前版本：命令行 > update.json > 本地 app.json > 默认 0.0.0
///
/// 损坏自恢复：本地 app.json 存在但其记录的安装目录缺失（被误删/损坏）时，
/// 视为未安装（0.0.0），触发自动重装
async fn resolve_current_version(args: &CliArgs, app_name: &str) -> String {
    if let Some(v) = &args.current_version {
        return v.clone();
    }
    if let Some(v) = non_empty(config::load().current_version) {
        return v;
    }
    let path = local_manifest_path(app_name);
    if path.exists() {
        match qingshu::update::LocalManifest::load(&path).await {
            Ok(Some(m)) if !m.version.is_empty() => {
                // 校验安装目录是否存在，缺失则自恢复重装
                if !m.installed_path.is_empty() && !Path::new(&m.installed_path).exists() {
                    eprintln!(
                        "[更新器] 本地版本 {} 的安装目录缺失（{}），视为未安装并自动重装",
                        m.version, m.installed_path
                    );
                    return "0.0.0".to_string();
                }
                return m.version;
            }
            Ok(_) => {}
            Err(e) => eprintln!("[更新器] 本地版本记录读取失败: {}", e),
        }
    }
    "0.0.0".to_string()
}

/// 拉起目标程序
///
/// `launch` 为绝对路径时直接使用；为相对路径（通常是文件名）时按以下顺序解析：
/// 1. 目标程序安装目录（版本目录，优先——目标程序由更新器安装在此）
/// 2. 更新器 exe 所在目录（兜底）
/// 都不存在时打印错误
///
/// - `installed_path`: 目标程序安装目录（更新完成分支传入本次安装目录；无更新分支从本地 app.json 读取）
fn launch_target(cfg: &ResolvedConfig, installed_path: Option<&Path>) {
    let Some(exe) = &cfg.launch else {
        return;
    };
    let raw = Path::new(exe);
    let exe_path = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(dir) = installed_path {
            candidates.push(dir.join(raw));
        }
        if let Ok(cur) = std::env::current_exe() {
            if let Some(dir) = cur.parent() {
                candidates.push(dir.join(raw));
            }
        }
        // 都不存在时回退第一个候选路径，用于生成错误信息
        candidates
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or_else(|| {
                installed_path
                    .map(|d| d.join(raw))
                    .unwrap_or_else(|| raw.to_path_buf())
            })
    };
    if !exe_path.exists() {
        eprintln!(
            "[更新器] 目标程序不存在: {}（解析路径: {}）",
            exe,
            exe_path.display()
        );
        return;
    }
    let workdir = exe_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    match std::process::Command::new(&exe_path)
        .current_dir(workdir)
        // 标记"本次启动由更新器拉回"，客户端用它防止启动检测重入循环
        .env("QSLTD_FROM_UPDATER", "1")
        .spawn()
    {
        Ok(_) => eprintln!("[更新器] 已拉起目标程序: {}", exe_path.display()),
        Err(e) => eprintln!("[更新器] 拉起目标程序失败: {}", e),
    }
}

/// 读取本地 app.json 记录的安装目录（不存在或未记录时返回 None）
async fn read_installed_path(app_name: &str) -> Option<String> {
    let path = local_manifest_path(app_name);
    if !path.exists() {
        return None;
    }
    match qingshu::update::LocalManifest::load(&path).await {
        Ok(Some(m)) if !m.installed_path.is_empty() => Some(m.installed_path),
        _ => None,
    }
}
