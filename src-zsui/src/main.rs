// 避免 release 模式下弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! QsLtd 独立更新程序（zsui 原生 UI 版）
//!
//! 流程：命令行解析 → 窗口前启动决策（无更新 + launch 模式静默退出）→
//! 需要时进入 zsui UI 循环，后台线程执行下载/安装并驱动视图刷新。

mod cleanup;
mod cli;
mod config;
mod download;
mod install;
mod manifest;
mod ui;
mod updater;

use std::sync::{Arc, Mutex};

fn main() {
    let args = cli::parse();
    let cfg = updater::resolve_config(&args);
    eprintln!(
        "[更新器] 启动，应用: {}，manifest 地址 {} 个: {}",
        cfg.app_name,
        cfg.manifest_urls.len(),
        cfg.manifest_urls.join(" → ")
    );

    // --check-only 纯检测：输出结果后按退出码结束（0=无更新 1=失败 2=有更新），不进 UI
    if args.check_only {
        let code = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(rt) => rt.block_on(updater::run_check_only(&args, &cfg)),
            Err(e) => {
                eprintln!("[更新器] 创建异步运行时失败: {}", e);
                1
            }
        };
        std::process::exit(code);
    }

    // 启动决策在窗口创建前完成：无更新 + launch 模式全程无窗口
    let decision = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt.block_on(updater::run_startup(&args, &cfg)),
        Err(e) => {
            eprintln!("[更新器] 创建异步运行时失败: {}", e);
            std::process::exit(1);
        }
    };

    match decision {
        updater::StartupDecision::SilentExit(code) => std::process::exit(code),
        updater::StartupDecision::Show(ctx) => run_ui(ctx),
    }
}

/// 进入 UI：创建共享快照与失效句柄，有新版本时先启动后台下载流程，再运行窗口
fn run_ui(ctx: updater::StartupContext) {
    let updater::StartupContext {
        cfg,
        manifest,
        initial,
        ..
    } = ctx;
    let snapshot = Arc::new(Mutex::new(initial));
    let handle = zsui::UiInvalidationHandle::new();

    if let Some(m) = manifest {
        updater::spawn_update_flow(cfg, m, snapshot.clone(), handle.clone());
    }

    // 窗口图标：exe 同目录 app_icon.ico（可选，缺省用系统默认）
    let icon = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("app_icon.ico")))
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned());

    if let Err(e) = ui::run(snapshot, handle, icon) {
        eprintln!("[更新器] {}", e);
        std::process::exit(1);
    }
}
