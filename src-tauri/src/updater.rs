//! 更新流程编排：读取配置 → 检查 → 下载（SHA256 校验）→ 安装 → 拉起目标程序。
//! 无更新时静默拉起目标程序并退出，出错时显示窗口提示。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::cleanup;
use crate::cli::CliArgs;
use crate::config;
use crate::download;
use crate::install;
use crate::manifest;

/// 默认服务器 manifest 地址（按序故障转移，第一个失败自动切换下一个）
const DEFAULT_MANIFEST_URLS: &[&str] = &["https://qsw.us.ci/update/qsm/version.json"];

/// 默认应用名称（决定配置/下载/安装目录）
const DEFAULT_APP_NAME: &str = "qsm";

/// 进度事件推送间隔（毫秒）
const PROGRESS_INTERVAL_MS: u64 = 300;

/// 配置合并后的生效配置（优先级：命令行 > update.json > 默认值）
struct ResolvedConfig {
    app_name: String,
    /// manifest 地址列表（按序故障转移）
    manifest_urls: Vec<String>,
    launch: Option<String>,
    /// 保留的版本目录数量（含当前版本）
    max_versions: usize,
}

/// 过滤空字符串配置项（update.json 中 "" 视为未配置）
fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

/// 合并命令行参数与 update.json 配置
fn resolve_config(args: &CliArgs) -> ResolvedConfig {
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
    }
}

/// 按 app_name 派生下载目录
fn download_dir(app_name: &str) -> PathBuf {
    qsltd_update::get_myapp_dir(Some(app_name)).join("downloads")
}

/// 按 app_name 派生版本根目录（各版本目录的父目录）
fn install_root(app_name: &str) -> PathBuf {
    qsltd_update::get_myapp_dir(Some(app_name)).join("app")
}

/// 按 app_name 派生版本安装目录（app_{version}）
fn install_dir(app_name: &str, version: &str) -> PathBuf {
    install_root(app_name).join(format!("app_{}", version))
}

/// 本地版本记录 app.json 路径
fn local_manifest_path(app_name: &str) -> PathBuf {
    qsltd_update::get_myapp_dir(Some(app_name)).join("app.json")
}

/// 更新检查与执行主流程
pub async fn run_check(app: AppHandle, args: CliArgs) {
    let cfg = resolve_config(&args);
    eprintln!(
        "[更新器] 启动，应用: {}，manifest 地址 {} 个: {}",
        cfg.app_name,
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

    eprintln!("[更新器] 开始拉取 manifest...");
    let m = match manifest::fetch_with_failover(&cfg.manifest_urls).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[更新器] 检查更新失败: {}", e);
            emit_phase(&app, "error", "更新检查失败", &e, None);
            show_main_window(&app);
            return;
        }
    };

    if !manifest::need_update(&current_version, &m) {
        eprintln!("[更新器] 已是最新版本: {}", current_version);
        match &cfg.launch {
            // 被目标程序拉起：静默拉起目标程序后退出，窗口全程不显示
            Some(_) => {
                launch_target(&cfg);
                app.exit(0);
            }
            // 手动运行：显示检查结果，由用户关闭
            None => {
                emit_phase(
                    &app,
                    "up-to-date",
                    "已是最新版本",
                    &format!("当前版本 {}", current_version),
                    None,
                );
                show_main_window(&app);
            }
        }
        return;
    }

    // ---- 发现新版本：下载 → 校验 → 安装 ----
    eprintln!(
        "[更新器] 发现新版本: {} -> {}（下载地址: {}）",
        current_version, m.version, m.url
    );
    emit_phase(
        &app,
        "available",
        "发现新版本",
        &format!("{} → {}", current_version, m.version),
        None,
    );
    show_main_window(&app);

    // 下载：分卷模式（parts 存在）或单包模式
    let zip_path = download_dir(&cfg.app_name).join(format!("update_{}.zip", m.version));
    if m.parts.as_ref().is_some_and(|p| !p.is_empty()) {
        if let Err(e) = run_download_parts(&app, &m, &cfg.app_name, &zip_path).await {
            eprintln!("[更新器] 分卷下载失败: {}", e);
            emit_phase(&app, "error", "更新包下载失败", &e, None);
            return;
        }
    } else {
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

    // 安装
    let target_dir = install_dir(&cfg.app_name, &m.version);
    if let Err(e) = install::install(&zip_path, &target_dir, |msg| {
        emit_phase(&app, "installing", "正在安装更新...", &msg, None);
    })
    .await
    {
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
        Some(_) => {
            launch_target(&cfg);
            app.exit(0);
        }
        None => {
            eprintln!("[更新器] 手动模式，更新完成，等待用户关闭");
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

    emit_phase(
        app,
        "downloading",
        "正在下载更新包（分卷）...",
        &format!("共 {} 卷", urls.len()),
        Some(0.0),
    );

    let app_handle = app.clone();
    let result = download::download_parts(urls, sha256s, &parts_dir, move |i, total, msg| {
        let percent = ((i + 1) as f64 / total as f64) * 100.0;
        emit_phase(
            &app_handle,
            "downloading",
            "正在下载更新包（分卷）...",
            &msg,
            Some(percent),
        );
    })
    .await?;

    emit_phase(app, "checking", "正在合并分卷...", "", None);
    eprintln!("[更新器] 合并 {} 个分卷...", result.part_paths.len());
    download::merge_parts(&result.part_paths, zip_path, &m.sha256).await?;
    eprintln!("[更新器] 分卷合并完成: {}", zip_path.display());
    Ok(())
}

/// 执行下载：本地已有同名包且校验通过则复用，否则流式下载并推送进度
async fn run_download(
    app: &AppHandle,
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
            eprintln!("[更新器] 复用已下载的更新包: {}", zip_path.display());
            emit_phase(app, "downloading", "使用已下载的更新包", "", Some(100.0));
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
    let last_emit = Instant::now();
    let app_handle = app.clone();
    let result = download::download_file(&m.url, zip_path, move |p| {
        // 进度事件节流，避免事件风暴
        if last_emit.elapsed() >= Duration::from_millis(PROGRESS_INTERVAL_MS) || p.percent() == Some(100.0) {
            emit_phase(
                &app_handle,
                "downloading",
                "正在下载更新包...",
                &format!("{:.1}%", p.percent().unwrap_or(0.0)),
                p.percent(),
            );
        }
    })
    .await;

    match result {
        Ok((_, sha)) => {
            eprintln!("[更新器] 下载完成，SHA256: {}", sha);
            Ok(())
        }
        Err(e) => Err(e),
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
        match qsltd_update::LocalManifest::load(&path).await {
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

/// 拉起目标程序，工作目录设为程序所在目录
fn launch_target(cfg: &ResolvedConfig) {
    let Some(exe) = &cfg.launch else {
        return;
    };
    let exe_path = Path::new(exe);
    if !exe_path.exists() {
        eprintln!("[更新器] 目标程序不存在: {}", exe);
        return;
    }
    let workdir = exe_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    match std::process::Command::new(exe_path).current_dir(workdir).spawn() {
        Ok(_) => eprintln!("[更新器] 已拉起目标程序: {}", exe),
        Err(e) => eprintln!("[更新器] 拉起目标程序失败: {}", e),
    }
}

/// 推送阶段事件给前端
fn emit_phase(
    app: &AppHandle,
    stage: &str,
    message: &str,
    sub: &str,
    percent: Option<f64>,
) {
    let _ = app.emit(
        "update-phase",
        PhasePayload {
            stage: stage.to_string(),
            message: message.to_string(),
            sub: sub.to_string(),
            percent,
        },
    );
}

/// 推送给前端的阶段事件载荷
#[derive(Debug, Clone, Serialize)]
struct PhasePayload {
    /// 阶段：checking | downloading | installing | up-to-date | available | done | error
    stage: String,
    /// 主提示文本（中文）
    message: String,
    /// 附加说明（版本变化、错误详情等）
    sub: String,
    /// 进度百分比（仅 downloading 阶段有效）
    percent: Option<f64>,
}

/// 显示主窗口并置顶聚焦
fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}
