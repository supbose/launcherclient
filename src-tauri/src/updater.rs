//! 更新流程编排：读取配置 → 检查 → 下载（SHA256 校验）→ 安装 → 拉起目标程序。
//! 无更新时拉起目标程序并退出，出错时显示窗口提示。
//!
//! 窗口可见性策略（loading 方案对齐 qsm）：
//! - 直接运行（无 --launch 交接参数）：splash 启动加载页样式声明在
//!   tauri.conf.json（"create": false，qsm 同款配置项），由 lib.rs
//!   setup 在直接运行模式按配置构建（纯静态零 JS，首帧快），检测期
//!   全程有界面反馈 —— 对应 qsm「等服务端连接」期间显示 loading；
//!   主窗口在「发现新版本」或「出错」时才显示，且遵循「先 show main、
//!   等其首帧渲染、再撤 splash」的顺序（qsm show_main_and_close_splash
//!   同款），避免空窗闪动
//! - 被目标程序以 --launch 交接：全程静默（splash 不构建，零窗口
//!   零闪现），直到发现新版本或出错才显示窗口
//! - `--check-only` 模式不进 Tauri，无窗口

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use qsltd_updater_core::cleanup;
use qsltd_updater_core::cli::CliArgs;
use qsltd_updater_core::config;
use qsltd_updater_core::defaults::{DEFAULT_APP_NAME, DEFAULT_MANIFEST_URLS, PROGRESS_INTERVAL_MS};
use qsltd_updater_core::download;
use qsltd_updater_core::install;
use qsltd_updater_core::manifest;
use qsltd_updater_core::services;

/// splash 启动加载窗口 label。
/// 样式声明在 tauri.conf.json（"create": false，qsm 同款配置项），
/// 由 lib.rs setup 在直接运行模式按配置构建（交接模式不构建，
/// 零窗口零闪现）；检测期提供「正在检查更新」反馈，
/// 主窗口显示前由本模块按 qsm 同款顺序撤掉。
pub const SPLASH_LABEL: &str = "splash";

/// splash 最短展示时长（毫秒）。
///
/// 无更新等秒级路径下，若 splash 刚出现就关闭退出，用户只看到
/// 「窗口一闪而过」甚至以为双击没生效。补足到该时长，
/// 保证「正在检查更新」的加载反馈可感知（已是最新也会停满该时长）。
pub(crate) const MIN_SPLASH_DISPLAY_MS: u64 = 1000;

/// splash 对用户可见的时刻。
///
/// 由 lib.rs setup 在直接运行模式下登记（静态窗口先于 setup 回调创建，
/// 登记时刻与用户可见时刻相差在毫秒级）；交接模式不登记 —— splash
/// 已被立即关闭，无需补足。
static SPLASH_SHOWN_AT: OnceLock<Instant> = OnceLock::new();

/// 登记 splash 已展示（lib.rs setup 中直接运行模式调用）。
pub(crate) fn mark_splash_shown() {
    let _ = SPLASH_SHOWN_AT.set(Instant::now());
}

/// 补足 splash 最短展示时长（无更新静默退出前调用）。
///
/// 未登记（交接模式）或已展示足够久时立即返回，不引入额外等待。
pub(crate) async fn ensure_min_splash_display() {
    if let Some(at) = SPLASH_SHOWN_AT.get() {
        let shown = Duration::from_millis(MIN_SPLASH_DISPLAY_MS);
        let elapsed = at.elapsed();
        if elapsed < shown {
            tokio::time::sleep(shown - elapsed).await;
        }
    }
}

/// 配置合并后的生效配置（优先级：命令行 > update.json > 默认值）
pub(crate) struct ResolvedConfig {
    app_name: String,
    /// manifest 地址列表（按序故障转移）
    manifest_urls: Vec<String>,
    launch: Option<String>,
    /// 保留的版本目录数量（含当前版本）
    max_versions: usize,
    /// 更新前需停止、更新后恢复的 Windows 服务名列表
    services: Vec<String>,
    /// 生效的启动器界面模板（A/B/C/D/E，见 [`config::resolve_launcher_template`]）
    template: String,
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

    // 界面模板优先级：命令行 --template > update.json launcher_template > 内置默认（C）。
    // 未配置或写了未知值时 resolve 内部回落默认，界面不会因配置笔误白屏。
    // 先于 services 取值：settings.services 下方会被整体移出，之后不能再用 settings。
    let settings_template = settings.launcher_template();
    let template = config::resolve_launcher_template(
        args.template.as_deref().or(settings_template.as_deref()),
    );

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
        template,
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

/// 清理本次更新的下载缓存：完整 zip + 分卷目录（安装成功后调用）
///
/// downloads 目录定位为「暂存区」：安装完成后即无价值。此前只清 app
/// 根目录的旧版本安装目录（`cleanup_old_versions`），downloads 里的
/// `update_{ver}.zip` 与 `update_{ver}_parts/` 每次升级各留一份完整
/// 包，只增不减（客户端长期运行后占用可达数 GB）。
///
/// 失败仅告警不阻塞：缓存而已，重下即可，不影响已完成的安装。
/// 安装失败**不**清理——保留的包供重跑时复用（`run_download` 的
/// 本地校验复用路径）。
async fn cleanup_download_cache(app_name: &str, version: &str) {
    let dir = download_dir(app_name);
    let zip = dir.join(format!("update_{}.zip", version));
    let parts = dir.join(format!("update_{}_parts", version));

    if zip.exists() {
        match tokio::fs::remove_file(&zip).await {
            Ok(()) => eprintln!("[更新器] 已清理下载缓存: {}", zip.display()),
            Err(e) => eprintln!(
                "[更新器] 清理下载缓存失败（忽略）: {}（{}）",
                zip.display(),
                e
            ),
        }
    }
    if parts.exists() {
        match tokio::fs::remove_dir_all(&parts).await {
            Ok(()) => eprintln!("[更新器] 已清理分卷缓存: {}", parts.display()),
            Err(e) => eprintln!(
                "[更新器] 清理分卷缓存失败（忽略）: {}（{}）",
                parts.display(),
                e
            ),
        }
    }
}

/// --check-only 入口（供 lib.rs 调用）：合并配置 → 纯检测 → 返回退出码
pub fn check_only_exit(args: &CliArgs) -> i32 {
    let cfg = resolve_config(args);
    match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt.block_on(run_check_only(args, &cfg)),
        Err(e) => {
            eprintln!("[更新器] 创建异步运行时失败: {}", e);
            1
        }
    }
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
        eprintln!("[更新器] 发现新版本: {} → {}", current_version, m.version);
        2
    } else {
        eprintln!("[更新器] 已是最新版本: {}", current_version);
        0
    }
}

/// 更新检查与执行主流程
pub async fn run_check(app: AppHandle, args: CliArgs) {
    let cfg = resolve_config(&args);
    // 登记生效的界面模板：此后所有阶段事件（含 current_phase 快照）都携带它
    set_template(&cfg.template);
    eprintln!(
        "[更新器] 启动，应用: {}，界面模板: {}，manifest 地址 {} 个: {}",
        cfg.app_name,
        cfg.template,
        cfg.manifest_urls.len(),
        cfg.manifest_urls.join(" → ")
    );

    // 先清理上次删除失败的残留目录
    let app_root = install_root(&cfg.app_name);
    cleanup::cleanup_pending(&app_root).await;

    // 当前版本：命令行 > update.json > 本地 app.json > 0.0.0（视为未安装/损坏自恢复）
    let current_version = resolve_current_version(&args, &cfg.app_name).await;
    eprintln!("[更新器] 当前版本: {}", current_version);

    emit_phase(&app, "checking", "正在检查更新...", "", None);

    // 检测期的界面反馈由 splash 承担（lib.rs 在直接运行模式已创建；
    // 交接模式保持静默）。主窗口直到发现新版本或出错才显示 ——
    // 见下方 need_update / error 分支的 show_main_and_close_splash。

    eprintln!("[更新器] 开始拉取 manifest...");
    // manifest 拉取走 TTL 缓存（5 分钟内重复启动免联网）：本路径频率低
    // （客户端先经 --check-only 判定有更新才交接过来），缓存主要服务
    // 双击运行/手动启动场景。check_only 不读缓存（准确优先）。
    let m = match manifest::fetch_with_cache(&cfg.manifest_urls, &cfg.app_name).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[更新器] 检查更新失败: {}", e);
            emit_phase(&app, "error", "更新检查失败", &e, None);
            show_main_and_close_splash(&app).await;
            return;
        }
    };
    // 登记更新日志：此后所有阶段事件都携带它，界面在发现新版本起到
    // 更新完成全程可见（下载/安装期间用户正好有空看更新了什么）
    set_changelog(m.changelog.clone());

    if !manifest::need_update(&current_version, &m) {
        eprintln!("[更新器] 已是最新版本: {}", current_version);
        // 先补足 splash 最短展示时长，再关闭退出：manifest 秒回时 splash
        // 刚出现，不补足就是「一闪而过」，用户看不到加载反馈（曾反馈
        // 「没有更新时没有 loading 页面」）。交接模式未登记展示时刻
        // （lib.rs setup 已关 splash 且不登记），此处立即返回，静默不受影响。
        ensure_min_splash_display().await;
        // 检测期由 splash 提供反馈，此时先撤 splash 再退出：
        // 无更新无需用户任何操作，主窗口全程不显示（含交接模式的静默退出）
        close_splash(&app);
        match &cfg.launch {
            // 拉起目标程序后退出
            Some(_) => {
                // 安装目录从本地 app.json 读取（相对路径 launch 按此目录解析）
                let installed = read_installed_path(&cfg.app_name).await;
                match launch_target(&cfg, installed.as_deref().map(Path::new)) {
                    Ok(()) => app.exit(0),
                    // 启动失败不能静默退出：交接模式下旧客户端已退出，
                    // 无提示退出等于「应用凭空消失」。显示错误窗口让用户
                    // 知道发生了什么（可重试或手动启动）。
                    Err(e) => {
                        eprintln!("[更新器] 无更新但启动失败: {}", e);
                        emit_phase(&app, "error", "启动失败", &e, None);
                        show_main_and_close_splash(&app).await;
                    }
                }
            }
            // 手动模式（无 launch 配置）：无更新静默退出，不打扰用户
            None => {
                app.exit(0);
            }
        }
        return;
    }

    // ---- 发现新版本：下载 → 校验 → 安装 ----
    let is_parts = m.parts.as_ref().is_some_and(|p| !p.is_empty());
    eprintln!(
        "[更新器] 发现新版本: {} -> {}（{}）",
        current_version,
        m.version,
        if is_parts {
            format!("分卷 {} 个", m.parts.as_ref().map_or(0, Vec::len))
        } else {
            format!("下载地址: {}", m.url)
        }
    );
    emit_phase(
        &app,
        "available",
        "发现新版本",
        &format!("{} → {}", current_version, m.version),
        None,
    );
    show_main_and_close_splash(&app).await;

    // 下载：分卷模式（parts 存在）或单包模式
    let zip_path = download_dir(&cfg.app_name).join(format!("update_{}.zip", m.version));
    if is_parts {
        if let Err(e) = run_download_parts(&app, &m, &cfg.app_name, &zip_path).await {
            eprintln!("[更新器] 分卷下载失败: {}", e);
            emit_phase(&app, "error", "更新包下载失败", &e, None);
            return;
        }
    } else {
        if m.url.is_empty() {
            let e = "manifest 缺少 url 字段（单包模式必填）".to_string();
            eprintln!("[更新器] 下载失败: {}", e);
            emit_phase(&app, "error", "更新包下载失败", &e, None);
            return;
        }
        let download_result = run_download(&app, &m, &zip_path).await;
        if let Err(e) = download_result {
            eprintln!("[更新器] 下载失败: {}", e);
            emit_phase(&app, "error", "更新包下载失败", &e, None);
            return;
        }
    }

    // SHA256 校验
    emit_phase(&app, "checking", "正在校验更新包...", "", None);
    if let Err(e) = tokio::task::spawn_blocking({
        let zip_path = zip_path.clone();
        let sha256 = m.sha256.clone();
        move || download::verify_sha256(&zip_path, &sha256)
    })
    .await
    .unwrap_or_else(|e| Err(format!("校验任务异常: {}", e)))
    {
        eprintln!("[更新器] 更新包校验失败: {}", e);
        emit_phase(&app, "error", "更新包校验失败", &e, None);
        return;
    }
    eprintln!("[更新器] 更新包校验通过: {}", zip_path.display());

    // 安装前卸载用户配置的服务（运行中先停止；卸载后不恢复、不重启）。
    // 卸载失败必须在此终止：否则服务仍注册指向旧路径而新版本已装完，
    // 形成「半更新」状态（服务是旧的、文件是新的）；且 release 无控制台，
    // 失败仅写 stderr，现场对「服务没更新」完全无感知。
    if !cfg.services.is_empty() {
        let svc_app = app.clone();
        let svc_names = cfg.services.clone();
        let svc_result = tokio::task::spawn_blocking(move || {
            services::uninstall_configured(&services::SystemScRunner, &svc_names, &|msg| {
                emit_phase(&svc_app, "installing", "正在处理服务...", msg, None);
            })
        })
        .await;
        let svc_detail = match svc_result {
            // 任务内全部成功（含未安装跳过）
            Ok(Ok(())) => None,
            // 有服务卸载失败（详情已聚合）
            Ok(Err(failures)) => Some(failures.join("；")),
            // 任务本身异常（阻塞线程 panic 被 JoinError 捕获）
            Err(e) => Some(format!("服务卸载任务异常: {e}")),
        };
        if let Some(detail) = svc_detail {
            eprintln!("[更新器] 服务卸载失败，终止更新: {}", detail);
            emit_phase(&app, "error", "服务卸载失败，更新已终止", &detail, None);
            return;
        }
    }

    // 安装
    let target_dir = install_dir(&cfg.app_name, &m.version);
    let install_app = app.clone();
    let install_cb: install::StageCallback = Arc::new(move |msg, pct| {
        emit_phase(&install_app, "installing", "正在安装更新...", msg, pct);
    });
    if let Err(e) = install::install(&zip_path, &target_dir, install_cb).await {
        eprintln!("[更新器] 安装失败: {}", e);
        emit_phase(&app, "error", "更新安装失败", &e, None);
        return;
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
        emit_phase(&app, "error", "本地版本记录写入失败", &e, None);
        return;
    }

    // 清理本次更新的下载缓存（zip + 分卷），downloads 回归暂存区语义
    cleanup_download_cache(&cfg.app_name, &m.version).await;

    // 清理旧版本目录（旧目录即回滚资产，保留 max_versions 个）
    cleanup::cleanup_old_versions(&install_root(&cfg.app_name), &m.version, cfg.max_versions)
        .await;

    emit_phase(
        &app,
        "done",
        "更新完成",
        &format!("已更新到版本 {}", m.version),
        None,
    );

    // 更新完成后拉起目标程序并退出；手动模式则保留窗口由用户关闭
    match &cfg.launch {
        Some(_) => match launch_target(&cfg, Some(&target_dir)) {
            Ok(()) => app.exit(0),
            // 安装已成功（app.json 已写入新版本），仅启动失败：
            // 保留窗口显示错误，用户可点「重试」重新走检测（无更新
            // 分支会再次尝试启动）或手动启动。直接退出会让「更新完成」
            // 的窗口一闪即逝，用户以为成功了但应用没回来。
            Err(e) => {
                eprintln!("[更新器] 更新完成但启动失败: {}", e);
                emit_phase(&app, "error", "更新已完成，但启动应用失败", &e, None);
            }
        },
        None => {
            eprintln!("[更新器] 手动模式，更新完成，等待用户点击「完成」启动应用");
        }
    }
}

/// 执行分卷下载：逐卷下载（复用合格卷）→ 合并为完整 zip → 整体校验
///
/// 合并产物与单包模式共用 zip_path，后续校验/安装/清理逻辑完全一致
async fn run_download_parts(
    app: &AppHandle,
    m: &manifest::FlatManifest,
    app_name: &str,
    zip_path: &Path,
) -> Result<(), String> {
    let urls = m.parts.as_ref().ok_or("分卷地址列表缺失")?;
    let parts_dir = download_dir(app_name).join(format!("update_{}_parts", m.version));
    let sha256s = m.part_sha256.as_deref().filter(|s| s.len() == urls.len());
    // 整体总大小（manifest.size_bytes），分卷之和等于完整 zip 大小
    let total_size = m.size_bytes;

    emit_phase(
        app,
        "downloading",
        "正在下载更新包（分卷）...",
        &format!("共 {} 卷", urls.len()),
        Some(0.0),
    );

    let tracker = std::sync::Mutex::new(SpeedTracker::new());
    let app_handle = app.clone();
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
                emit_download(
                    &app_handle,
                    &msg,
                    Some(format!("分卷 {}/{}", i + 1, total)),
                    Some(percent),
                    p.downloaded,
                    p.total,
                    Some(speed),
                );
            }
        })
        .await?;

    emit_phase(app, "checking", "正在合并分卷...", "", Some(0.0));
    eprintln!("[更新器] 合并 {} 个分卷...", result.part_paths.len());
    let merge_app = app.clone();
    download::merge_parts(&result.part_paths, zip_path, &m.sha256, move |done, total| {
        let pct = if total > 0 {
            (done as f64 / total as f64 * 100.0).min(100.0)
        } else {
            0.0
        };
        emit_phase(
            &merge_app,
            "checking",
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
    app: &AppHandle,
    m: &manifest::FlatManifest,
    zip_path: &Path,
) -> Result<(), String> {
    // 复用前提：manifest 提供了非空 SHA256。无校验值时 verify_sha256 直接
    // 放行，等于把上次下载中断的半包当完整包复用（后面安装的就是损坏包），
    // 故强制重新下载。
    let has_hash = m
        .sha256
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if zip_path.exists() && has_hash {
        let cached = zip_path.to_path_buf();
        let sha256 = m.sha256.clone();
        let ok = tokio::task::spawn_blocking(move || download::verify_sha256(&cached, &sha256))
            .await
            .unwrap_or_else(|e| Err(format!("校验任务异常: {}", e)))
            .is_ok();
        if ok {
            let size = tokio::fs::metadata(zip_path).await.map(|m| m.len()).unwrap_or(0);
            eprintln!(
                "[更新器] 复用已下载的更新包: {}（{}）",
                zip_path.display(),
                format_bytes(size)
            );
            emit_download(app, "使用已下载的更新包", None, Some(100.0), size, Some(size), None);
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

    emit_phase(app, "downloading", "正在下载更新包...", "", Some(0.0));
    let tracker = std::sync::Mutex::new(SpeedTracker::new());
    let app_handle = app.clone();
    let result = download::download_file(&m.url, zip_path, move |p| {
        let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
        let (should, speed) = t.tick(p.downloaded, p.percent() == Some(100.0));
        if should {
            emit_download(
                &app_handle,
                "正在下载更新包...",
                None,
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
            let size = tokio::fs::metadata(zip_path).await.map(|m| m.len()).unwrap_or(0);
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

/// 字节数格式化为易读文本（与登录器 QsLtdLauncherClient 同款格式）
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let i = (bytes as f64).log(1024.0).floor() as usize;
    let i = i.min(UNITS.len() - 1);
    let value = bytes as f64 / 1024f64.powi(i as i32);
    format!("{:.2} {}", value, UNITS[i])
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

/// 解析启动目标 exe 的路径（纯函数，供单测）
///
/// 优先级（更新后要启动的是**新版本**，安装目录是权威来源）：
/// 1. 安装目录内的目标程序：绝对路径取同名文件、相对路径按原相对
///    结构拼接；直接拼接未命中时在安装目录内按文件名递归查找
///    （限深 3 层，兼容 zip 根带一层目录的结构）
/// 2. 绝对路径原样使用 —— 仅当安装目录内找不到时回退。
///    **不能无条件优先**：交接模式下 `--launch` 是旧版客户端传入的
///    自身绝对路径，原样使用会启动旧二进制（旧目录还在时）或直接
///    启动失败（旧目录已被 max_versions 清理时）
/// 3. 相对路径回退到更新器 exe 所在目录（客户端与更新器同目录部署
///    的兜底，见 qsm main.rs 的定位逻辑）
fn resolve_launch_exe(launch: &str, installed_path: Option<&Path>) -> PathBuf {
    let raw = Path::new(launch);

    if let Some(dir) = installed_path {
        // 绝对路径只取文件名（Path::join 遇绝对路径会整体替换，丢失 dir）；
        // 相对路径保留原相对结构
        let direct = if raw.is_absolute() {
            raw.file_name().map(|n| dir.join(n))
        } else {
            Some(dir.join(raw))
        };
        if let Some(p) = direct.filter(|p| p.exists()) {
            return p;
        }
        // 嵌套结构兜底：安装目录内递归找同名文件
        if let Some(name) = raw.file_name() {
            if let Some(found) = find_file_by_name(dir, name, 3) {
                return found;
            }
        }
    }

    if raw.is_absolute() {
        return raw.to_path_buf();
    }
    std::env::current_exe()
        .ok()
        .and_then(|cur| cur.parent().map(|d| d.join(raw)))
        .unwrap_or_else(|| raw.to_path_buf())
}

/// 在目录内按文件名递归查找（限深，防止异常目录树拖慢启动）
fn find_file_by_name(dir: &Path, name: &std::ffi::OsStr, max_depth: usize) -> Option<PathBuf> {
    if max_depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name() == Some(name) {
            return Some(path);
        }
    }
    // 第二轮再递归（同名文件优先出现在浅层）
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_by_name(&path, name, max_depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

/// 拉起目标程序
///
/// `launch` 为绝对路径时直接使用；为相对路径（通常是文件名）时按以下顺序解析：
///
/// 1. 目标程序安装目录（版本目录，优先——目标程序由更新器安装在此）
/// 2. 更新器 exe 所在目录（兜底）
///
/// 启动失败（目标不存在 / spawn 失败）返回 Err 详情，由调用方决定
/// 是提示用户还是静默退出 —— 更新完成后静默失败会让用户「更新完
/// 了应用却没回来」，交接模式下旧客户端已退出，现场无任何界面。
///
/// - `installed_path`: 目标程序安装目录（更新完成分支传入本次安装目录；无更新分支从本地 app.json 读取）
fn launch_target(cfg: &ResolvedConfig, installed_path: Option<&Path>) -> Result<(), String> {
    let Some(exe) = &cfg.launch else {
        return Ok(());
    };
    let exe_path = resolve_launch_exe(exe, installed_path);

    if !exe_path.exists() {
        return Err(format!(
            "目标程序不存在: {}（解析路径: {}）",
            exe,
            exe_path.display()
        ));
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
        Ok(_) => {
            eprintln!("[更新器] 已拉起目标程序: {}", exe_path.display());
            Ok(())
        }
        Err(e) => Err(format!(
            "拉起目标程序失败: {}（{}）",
            exe_path.display(),
            e
        )),
    }
}

/// 以分离进程方式启动指定 exe（工作目录取 exe 所在目录）
///
/// 注入 `QSLTD_FROM_UPDATER=1`：客户端据此跳过启动检测，防止「拉回→再交接」循环
fn spawn_detached(exe_path: &Path) -> Result<(), String> {
    let workdir = exe_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    match std::process::Command::new(exe_path)
        .current_dir(workdir)
        .env("QSLTD_FROM_UPDATER", "1")
        .spawn()
    {
        Ok(_) => {
            eprintln!("[更新器] 已启动: {}", exe_path.display());
            Ok(())
        }
        Err(e) => Err(format!("启动失败 {}: {}", exe_path.display(), e)),
    }
}

/// 「完成」按钮重入守卫。
///
/// Tauri command 并发执行：「完成」被快速双击或 IPC 重试时，两次
/// `finish_and_launch` 可能都走到 spawn_detached，启动两个应用实例。
/// 首次调用后置位，后续调用直接退出。
static FINISH_CALLED: AtomicBool = AtomicBool::new(false);

/// 「完成」按钮（前端 done 阶段调用）：启动更新后的应用并退出更新器
///
/// - launch 已配置（含 update.json）：走 [`launch_target`] 原解析逻辑
/// - 手动模式（launch 未配置）：从安装目录定位主 exe 启动
///   （见 `qsltd_updater_core::install::find_main_executable`）
/// - 均找不到可执行文件时仅退出，不阻塞用户关闭窗口
#[tauri::command]
pub async fn finish_and_launch(app: AppHandle) {
    // 重入守卫：已处理过一次则不再启动，直接退出（见 FINISH_CALLED 注释）
    if FINISH_CALLED.swap(true, Ordering::SeqCst) {
        app.exit(0);
        return;
    }
    let args = qsltd_updater_core::cli::parse();
    let cfg = resolve_config(&args);
    // 启动结果：成功才退出；失败切错误态保留窗口（用户可重试或手动启动），
    // 不能再静默 exit —— 那等于「点了完成，应用没出来，窗口也关了」
    let outcome = match read_installed_path(&cfg.app_name).await {
        Some(dir) => match &cfg.launch {
            Some(_) => launch_target(&cfg, Some(Path::new(&dir))),
            None => {
                match qsltd_updater_core::install::find_main_executable(
                    Path::new(&dir),
                    &cfg.app_name,
                ) {
                    Some(exe) => spawn_detached(&exe),
                    None => Err(format!("安装目录中未找到可执行文件: {}", dir)),
                }
            }
        },
        None => Err("无本地版本记录，无法定位已安装的应用".to_string()),
    };
    match outcome {
        Ok(()) => app.exit(0),
        Err(e) => {
            eprintln!("[更新器] 「完成」启动失败: {}", e);
            emit_phase(&app, "error", "启动失败", &e, None);
        }
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

/// 推送下载阶段事件（携带字节数与速度，供前端显示"已下载/总大小 · 速度"）
///
/// `badge` 为右上角小徽标文本（如分卷进度「分卷 1/28」），无则不下发该字段
#[allow(clippy::too_many_arguments)]
fn emit_download(
    app: &AppHandle,
    message: &str,
    badge: Option<String>,
    percent: Option<f64>,
    downloaded: u64,
    total: Option<u64>,
    speed_bps: Option<u64>,
) {
    push_phase(
        app,
        PhasePayload {
            stage: "downloading".to_string(),
            message: message.to_string(),
            sub: String::new(),
            percent,
            downloaded_bytes: Some(downloaded),
            total_bytes: total,
            speed_bps,
            changelog: Vec::new(),
            template: String::new(),
            badge,
        },
    );
}

/// 推送阶段事件给前端
fn emit_phase(
    app: &AppHandle,
    stage: &str,
    message: &str,
    sub: &str,
    percent: Option<f64>,
) {
    push_phase(
        app,
        PhasePayload {
            stage: stage.to_string(),
            message: message.to_string(),
            sub: sub.to_string(),
            percent,
            downloaded_bytes: None,
            total_bytes: None,
            speed_bps: None,
            changelog: Vec::new(),
            template: String::new(),
            badge: None,
        },
    );
}

/// 本次更新的更新日志（manifest.changelog 的进程级副本）
///
/// 与阶段无关：manifest 拉取成功后即固定，后续所有阶段事件都携带同一份。
/// 单独存放而非随每个 emit 调用点传递，避免十几个调用点重复传参、也避免
/// 某个阶段漏传导致界面日志闪没。
static CHANGELOG: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

/// 登记本次更新的更新日志（manifest 拉取成功后调用）
pub(crate) fn set_changelog(items: Vec<String>) {
    let cell = CHANGELOG.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut guard) = cell.lock() {
        *guard = items;
    }
}

/// 取更新日志副本（供 [`push_phase`] 注入事件载荷）
fn changelog_snapshot() -> Vec<String> {
    CHANGELOG
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// 记录最新阶段快照并推送事件
///
/// 快照供 [`current_phase`] 查询：本地小更新包可能秒级走完全部流程，
/// 前端 WebView 挂载 + 动态 import 监听建立存在时延，早期事件会错过，
/// 界面将停在初始态。监听建立后主动拉取一次快照即可对齐。
/// 生效的启动器界面模板（进程级副本，manifest 无关，随每个阶段事件下发）
///
/// 与 CHANGELOG 同样由 [`push_phase`] 统一注入：前端靠它决定渲染哪套布局，
/// 任一阶段事件漏带都会让界面在切换阶段时闪回默认模板。
static TEMPLATE: OnceLock<Mutex<String>> = OnceLock::new();

/// 登记生效的界面模板（run_check 解析配置后调用）
pub(crate) fn set_template(template: &str) {
    let cell = TEMPLATE.get_or_init(|| Mutex::new(String::new()));
    if let Ok(mut guard) = cell.lock() {
        *guard = template.to_string();
    }
}

/// 取生效的界面模板（供 [`push_phase`] 注入事件载荷）
fn template_snapshot() -> String {
    TEMPLATE
        .get_or_init(|| Mutex::new(String::new()))
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

fn push_phase(app: &AppHandle, mut payload: PhasePayload) {
    // 更新日志与界面模板在此统一注入（见 CHANGELOG / TEMPLATE 注释）：
    // 检测阶段的事件发生在 manifest 拉取之前，此时日志为空，界面不展示该区块
    payload.changelog = changelog_snapshot();
    payload.template = template_snapshot();
    if let Ok(mut cell) = latest_phase_cell().lock() {
        *cell = Some(payload.clone());
    }
    let _ = app.emit("update-phase", payload);
}

/// 最新阶段快照存储（进程级单例）
static LATEST_PHASE: OnceLock<Mutex<Option<PhasePayload>>> = OnceLock::new();

fn latest_phase_cell() -> &'static Mutex<Option<PhasePayload>> {
    LATEST_PHASE.get_or_init(|| Mutex::new(None))
}

/// 前端 WebView 挂载后主动拉取当前阶段（弥补监听建立前错过的事件）
#[tauri::command]
pub fn current_phase() -> Option<PhasePayload> {
    latest_phase_cell()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// 关闭放行标志：用户在关键阶段确认中断后置位，允许随后的 close 真正执行
static ALLOW_CLOSE: AtomicBool = AtomicBool::new(false);

/// 当前是否处于「不可中断」的关键阶段（检测中 / 下载中 / 安装中）
///
/// 关闭窗口默认直接退出进程。若发生在下载/安装中途，可能留下
/// 「服务已卸载但 app.json 未写入」的不一致态（下次启动时本地版本
/// 记录与实际文件对不上）；检测期关窗则浪费一次已发起的检测。
/// 这些阶段关闭前必须弹确认框（见 lib.rs 的 on_window_event）。
///
/// available/done/error 阶段关闭无副作用，直接放行。
pub fn is_critical_phase() -> bool {
    let guard = latest_phase_cell()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let stage = guard
        .as_ref()
        .map(|p| p.stage.as_str())
        .unwrap_or("");
    matches!(stage, "checking" | "downloading" | "installing")
}

// 测试里构造失败即用例本身失败，按 clippy 惯例局部放行 expect_used
#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "qsltd_launch_test_{}_{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
    }

    #[test]
    fn template_registered_then_snapshot() {
        // 界面模板登记后应可被后续阶段事件取到（前端据此切换布局）
        set_template("B");
        assert_eq!(template_snapshot(), "B");
    }

    #[test]
    fn changelog_registered_then_snapshot() {
        // 更新日志登记后应可被后续阶段事件取到（界面据此展示更新内容）
        set_changelog(vec!["修复登录".to_string(), "优化启动速度".to_string()]);
        let got = changelog_snapshot();
        assert_eq!(got.len(), 2, "登记的两条日志都应可取回: {got:?}");
        assert_eq!(got[0], "修复登录");
    }

    #[test]
    fn absolute_launch_prefers_same_named_exe_in_install_dir() {
        // 核心回归：交接模式下 --launch 是旧版客户端传入的自身绝对路径，
        // 更新完成后必须启动新安装目录内的同名 exe —— 否则启动的是旧
        // 二进制（旧目录还在时）或直接启动失败（旧目录被清理时）。
        let install = temp_dir("prefer_new");
        std::fs::write(install.join("qsm.exe"), b"new").expect("写新版本 exe");
        let old = temp_dir("old_bin");
        std::fs::write(old.join("qsm.exe"), b"old").expect("写旧版本 exe");

        let resolved = resolve_launch_exe(&old.join("qsm.exe").to_string_lossy(), Some(&install));
        assert_eq!(resolved, install.join("qsm.exe"), "必须解析到新安装目录");
        let _ = std::fs::remove_dir_all(&install);
        let _ = std::fs::remove_dir_all(&old);
    }

    #[test]
    fn absolute_launch_falls_back_when_install_dir_lacks_exe() {
        // 安装目录内没有同名 exe（launch 指向更新器外的其他程序，如
        // 绝对路径指向独立工具）时才回退绝对路径原值
        let install = temp_dir("fallback_abs");
        let other = temp_dir("other_bin");
        std::fs::write(other.join("tool.exe"), b"x").expect("写文件");

        let resolved = resolve_launch_exe(&other.join("tool.exe").to_string_lossy(), Some(&install));
        assert_eq!(resolved, other.join("tool.exe"));
        let _ = std::fs::remove_dir_all(&install);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn relative_launch_resolves_in_install_dir() {
        let install = temp_dir("relative");
        std::fs::write(install.join("qsm.exe"), b"new").expect("写 exe");
        let resolved = resolve_launch_exe("qsm.exe", Some(&install));
        assert_eq!(resolved, install.join("qsm.exe"));
        let _ = std::fs::remove_dir_all(&install);
    }

    #[test]
    fn nested_install_dir_found_by_name() {
        // zip 根带一层目录：app_2.0.0/bin/qsm.exe，--launch 指旧版 bin 路径，
        // 直接拼接未命中，应递归（限深）找到嵌套目录内的新 exe
        let install = temp_dir("nested");
        let bin = install.join("bin");
        std::fs::create_dir_all(&bin).expect("建 bin");
        std::fs::write(bin.join("qsm.exe"), b"new").expect("写 exe");
        let old = temp_dir("old_nested");
        std::fs::write(old.join("qsm.exe"), b"old").expect("写旧 exe");

        let resolved = resolve_launch_exe(&old.join("qsm.exe").to_string_lossy(), Some(&install));
        assert_eq!(resolved, bin.join("qsm.exe"), "应递归找到嵌套目录内的新 exe");
        let _ = std::fs::remove_dir_all(&install);
        let _ = std::fs::remove_dir_all(&old);
    }

    #[test]
    fn absolute_launch_without_install_dir_used_verbatim() {
        let p = PathBuf::from("D:/apps/qsm.exe");
        assert_eq!(resolve_launch_exe(&p.to_string_lossy(), None), p);
    }
}

/// 关键阶段的关闭是否已被用户确认放行（确认后 close 会再次触发
/// CloseRequested，靠此标志避免无限递归拦截）
pub fn close_allowed() -> bool {
    ALLOW_CLOSE.load(Ordering::SeqCst)
}

/// 放行关闭（用户在关键阶段确认对话框中点了「确定」）
pub fn allow_close() {
    ALLOW_CLOSE.store(true, Ordering::SeqCst);
}

/// 更新流程运行中标志：首次运行与「重试」共用，防并发双跑
static CHECK_RUNNING: AtomicBool = AtomicBool::new(false);

/// 原始命令行参数（不含程序名）：错误态「重试」时用它重新解析
/// （CliArgs 无需 Clone，且重试与首次完全同参）
static CLI_ARGV: OnceLock<Vec<String>> = OnceLock::new();

/// 登记原始命令行参数（lib.rs 启动时调用一次，重试时取用）
pub fn register_cli_argv(argv: Vec<String>) {
    let _ = CLI_ARGV.set(argv);
}

/// 启动一次更新流程（首次运行与错误态「重试」共用入口）
///
/// 运行中标志防并发：流程进行中再启动直接忽略（前端按钮会同时
/// disabled，双保险；IPC 重试/脚本调用场景靠这里兜底）。
pub fn spawn_check(app: AppHandle, args: CliArgs) {
    if CHECK_RUNNING.swap(true, Ordering::SeqCst) {
        eprintln!("[更新器] 已有更新流程在运行，忽略本次启动");
        return;
    }
    tauri::async_runtime::spawn(async move {
        run_check(app, args).await;
        CHECK_RUNNING.store(false, Ordering::SeqCst);
    });
}

/// 错误态「重试」按钮（前端 invoke）：用相同命令行参数重走一遍流程
///
/// 重试与首次运行完全同参（含 --manifest 等覆盖）——「同一配置再试
/// 一次」正是弱网/服务端临时故障时用户的诉求；想换地址应重启更新器。
/// run_check 首行即 emit checking，前端自动回到检测态。
#[tauri::command]
pub async fn retry_check(app: AppHandle) {
    let Some(argv) = CLI_ARGV.get().cloned() else {
        eprintln!("[更新器] 未登记命令行参数，无法重试");
        return;
    };
    let args = qsltd_updater_core::cli::parse_from(&argv);
    eprintln!("[更新器] 用户请求重试，重新开始检查更新");
    spawn_check(app, args);
}

/// 推送给前端的阶段事件载荷
#[derive(Debug, Clone, Serialize)]
pub(crate) struct PhasePayload {
    /// 阶段：checking | downloading | installing | up-to-date | available | done | error
    stage: String,
    /// 主提示文本（中文）
    message: String,
    /// 附加说明（版本变化、错误详情等）
    sub: String,
    /// 进度百分比（仅 downloading 阶段有效）
    percent: Option<f64>,
    /// 已下载字节数（仅下载阶段携带）
    #[serde(skip_serializing_if = "Option::is_none")]
    downloaded_bytes: Option<u64>,
    /// 总字节数（仅下载阶段携带，服务器未返回时缺失）
    #[serde(skip_serializing_if = "Option::is_none")]
    total_bytes: Option<u64>,
    /// 下载速度 字节/秒（仅下载阶段携带）
    #[serde(skip_serializing_if = "Option::is_none")]
    speed_bps: Option<u64>,
    /// 更新日志（manifest.changelog，空则不序列化 —— 前端据此不展示该区块）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    changelog: Vec<String>,
    /// 生效的启动器界面模板（A/B/C/D/E），前端据此渲染对应布局
    template: String,
    /// 右上角小徽标文本（分卷进度等，仅下载阶段携带）
    #[serde(skip_serializing_if = "Option::is_none")]
    badge: Option<String>,
}

/// 显示主窗口并置顶聚焦
fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// 关闭 splash 窗口（不存在时静默跳过 —— 交接模式从未创建 splash）。
pub(crate) fn close_splash(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(SPLASH_LABEL) {
        if let Err(e) = win.close() {
            eprintln!("[更新器] 关闭 splash 失败: {e}");
        }
    }
}

/// 显示主窗口并关闭 splash（顺序照搬 qsm：先 show main，等其首帧，再撤 splash）。
///
/// 反过来会出现「主窗口还没绘制完，splash 已消失」的空窗闪动 ——
/// main 是 Vue 应用，show 到首帧渲染有几百毫秒，这段时间必须由 splash 撑着。
async fn show_main_and_close_splash(app: &AppHandle) {
    show_main_window(app);
    // 等主窗口首帧渲染出来再撤 splash，避免闪空
    tokio::time::sleep(Duration::from_millis(150)).await;
    close_splash(app);
}
