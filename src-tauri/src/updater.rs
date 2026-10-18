//! 更新流程编排：读取 update.json 与命令行参数 → 拉取平铺 manifest → 判断更新，
//! 无更新时静默拉起目标程序并退出，有更新或出错时显示窗口。

use std::path::Path;

use qsltd_update::LocalManifest;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::cli::CliArgs;
use crate::config;
use crate::manifest;

/// 默认服务器 manifest 地址
const DEFAULT_MANIFEST_URL: &str = "https://qsw.us.ci/update/qsm/version.json";

/// 默认应用名称（决定配置/下载/安装目录）
const DEFAULT_APP_NAME: &str = "qsm";

/// 配置合并后的生效配置（优先级：命令行 > update.json > 默认值）
struct ResolvedConfig {
    app_name: String,
    manifest_url: String,
    launch: Option<String>,
    current_version: String,
}

/// 合并命令行参数与 update.json 配置
fn resolve_config(args: &CliArgs) -> ResolvedConfig {
    let settings = config::load();

    let app_name = if args.app_name != "QsLtd" {
        // 命令行显式指定了应用名
        args.app_name.clone()
    } else {
        settings.app_name.clone().unwrap_or_else(|| DEFAULT_APP_NAME.to_string())
    };

    ResolvedConfig {
        app_name,
        manifest_url: args
            .manifest_url
            .clone()
            .or(settings.manifest_url)
            .unwrap_or_else(|| DEFAULT_MANIFEST_URL.to_string()),
        launch: args.launch.clone().or(settings.launch),
        current_version: String::new(),
    }
}

/// 更新检查主流程
pub async fn run_check(app: AppHandle, args: CliArgs) {
    let cfg = resolve_config(&args);
    eprintln!(
        "[更新器] 启动，应用: {}，manifest: {}",
        cfg.app_name, cfg.manifest_url
    );

    // 当前版本：命令行 > update.json > 本地 app.json > 0.0.0（视为未安装）
    let current_version = resolve_current_version(&args, &cfg).await;
    eprintln!("[更新器] 当前版本: {}", current_version);

    emit_phase(&app, "checking", "正在检查更新...", "");

    eprintln!("[更新器] 开始拉取 manifest...");
    match manifest::fetch(&cfg.manifest_url).await {
        Ok(m) => {
            if manifest::need_update(&current_version, &m) {
                eprintln!(
                    "[更新器] 发现新版本: {} -> {}（下载地址: {}）",
                    current_version, m.version, m.url
                );
                emit_phase(
                    &app,
                    "available",
                    "发现新版本",
                    &format!("{} → {}", current_version, m.version),
                );
                show_main_window(&app);
                // P1 阶段：在此接入下载（m.url + m.sha256 校验）与安装流程
            } else {
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
                        );
                        show_main_window(&app);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("[更新器] 检查更新失败: {}", e);
            emit_phase(&app, "error", "更新检查失败", &e);
            show_main_window(&app);
        }
    }
}

/// 解析当前版本：命令行 > update.json > 本地 app.json > 默认 0.0.0
async fn resolve_current_version(args: &CliArgs, cfg: &ResolvedConfig) -> String {
    if let Some(v) = &args.current_version {
        return v.clone();
    }
    if let Some(v) = config::load().current_version {
        return v;
    }
    let local_manifest_path = qsltd_update::get_myapp_dir(Some(cfg.app_name.as_str()))
        .join("app.json");
    if local_manifest_path.exists() {
        match LocalManifest::load(&local_manifest_path).await {
            Ok(Some(m)) if !m.version.is_empty() => return m.version,
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
fn emit_phase(app: &AppHandle, stage: &str, message: &str, sub: &str) {
    let _ = app.emit(
        "update-phase",
        PhasePayload {
            stage: stage.to_string(),
            message: message.to_string(),
            sub: sub.to_string(),
        },
    );
}

/// 推送给前端的阶段事件载荷
#[derive(Debug, Clone, Serialize)]
struct PhasePayload {
    /// 阶段：checking | up-to-date | available | error
    stage: String,
    /// 主提示文本（中文）
    message: String,
    /// 附加说明（版本变化、错误详情等）
    sub: String,
}

/// 显示主窗口并置顶聚焦
fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}
