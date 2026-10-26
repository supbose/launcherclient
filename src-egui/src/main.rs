// 避免 release 模式下弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! QsLtd 独立更新程序（egui/eframe 原生 UI 版）
//!
//! 流程：命令行解析 → 窗口前启动决策（无更新 + launch 模式静默退出）→
//! 需要时进入 eframe 事件循环，后台线程执行下载/安装并经共享快照
//! （`qsltd_updater_core::phase::PhaseHandle`）驱动视图刷新。
//!
//! 界面复刻 Tauri + Vue 版（`src/App.vue` + `src/components/templates/`）：
//! 五套模板 A/B/C/D/E 与后端 `launcher_template` 配置一一对应，默认 C。
//! 后端模块与 UI 编排层已抽离至共享 crate `qsltd-updater-core`
//! （src-core 的 phase/flow 模块），本 crate 只保留视图层。

mod app;
mod preview;
mod templates;
mod theme;
mod widgets;

use qsltd_updater_core::cli::CliArgs;
use qsltd_updater_core::flow::{self, StartupDecision};
use qsltd_updater_core::phase::PhaseHandle;

fn main() {
    // --preview 模板预览模式（复刻 preview.html，仅本地验证五套布局，
    // 不属于核心 CLI 参数——先摘除再交给 core 解析，避免「未知参数」告警）
    // 预览初始阶段：--stage <0-7|名称>（检测中/下载中/校验中/安装中/发现新版本/已是最新/更新完成/错误）
    let raw_argv: Vec<String> = std::env::args().skip(1).collect();
    let preview_mode = raw_argv.iter().any(|a| a == "--preview");
    let mut preview_stage = 1usize;
    let mut argv: Vec<String> = Vec::with_capacity(raw_argv.len());
    let mut skip_next = false;
    for a in raw_argv {
        if skip_next {
            skip_next = false;
            preview_stage = preview::stage_index(&a).unwrap_or(1);
            continue;
        }
        if a == "--preview" {
            continue;
        }
        if a == "--stage" {
            skip_next = true;
            continue;
        }
        argv.push(a);
    }
    let args: CliArgs = qsltd_updater_core::cli::parse_from(&argv);

    // 登记原始参数供错误态「重试」使用（retry 重新解析，保证重试与首次完全同参）
    flow::register_cli_argv(argv.clone());

    // --check-only 纯检测：不建窗，输出结果后按退出码结束（0=无更新 1=失败 2=有更新）
    if args.check_only {
        std::process::exit(flow::check_only_exit(&args));
    }

    // 共享阶段快照：后台更新线程写、UI 线程每帧轮询
    let phase = PhaseHandle::new();

    if preview_mode {
        run_ui(&args, phase, true, preview_stage);
        return;
    }

    let cfg = flow::resolve_config(&args);
    eprintln!(
        "[更新器] 启动，应用: {}，界面模板: {}，manifest 地址 {} 个: {}",
        cfg.app_name,
        cfg.template,
        cfg.manifest_urls.len(),
        cfg.manifest_urls.join(" → ")
    );

    // 启动决策在窗口创建前完成：无更新 + launch 模式全程无窗口（早期 zsui 版亦采用此策略）
    let decision = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt.block_on(flow::run_startup(&args, &cfg)),
        Err(e) => {
            eprintln!("[更新器] 创建异步运行时失败: {}", e);
            std::process::exit(1);
        }
    };

    match decision {
        StartupDecision::SilentExit(code) => std::process::exit(code),
        StartupDecision::Show(ctx) => {
            let flow::StartupContext {
                cfg,
                manifest,
                initial,
            } = *ctx;
            // 有新版本：先登记初始阶段快照并启动后台下载流程，再运行窗口
            phase.set(initial);
            if let Some(m) = manifest {
                let flow_phase = phase.clone();
                std::thread::Builder::new()
                    .name("update-flow-spawn".into())
                    .spawn(move || flow::spawn_flow_with(qsltd_updater_core::cli::parse(), flow_phase, m))
                    .ok();
            }
            run_ui(&args, phase, false, 0);
        }
    }
}

/// 进入 eframe 事件循环
///
/// - `phase`: 共享阶段快照（真实模式已含首帧阶段；预览模式为空）
/// - `preview`: true 时渲染模板/阶段切换面板（复刻 Preview.vue）
fn run_ui(args: &CliArgs, phase: PhaseHandle, preview: bool, preview_stage: usize) {
    // 生效模板：命令行 --template > 默认（C）；预览模式可在面板里切换
    let template = qsltd_updater_core::phase::resolve_template_key(
        args.template.as_deref().unwrap_or(""),
    );

    let window_h = if preview {
        app::WINDOW_SIZE.1 + app::PREVIEW_EXTRA_HEIGHT
    } else {
        app::WINDOW_SIZE.1
    };
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(app::WINDOW_TITLE)
            .with_inner_size([app::WINDOW_SIZE.0, window_h])
            .with_resizable(false)
            .with_decorations(false)
            .with_always_on_top()
            // 隐藏启动：首帧居中（app.rs logic，对齐 tauri.conf.json "center": true）
            // 完成后再显示，避免窗口先出现在默认级联位置再跳到屏幕中心
            .with_visible(false),
        ..Default::default()
    };

    let result = eframe::run_native(
        app::WINDOW_TITLE,
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(app::UpdaterApp::new(cc, phase, template, preview, preview_stage))
                as Box<dyn eframe::App>)
        }),
    );
    if let Err(e) = result {
        eprintln!("[更新器] UI 循环异常退出: {}", e);
        std::process::exit(1);
    }
}
