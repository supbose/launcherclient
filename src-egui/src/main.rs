// 避免 release 模式下弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! QsLtd 独立更新程序（egui/eframe 原生 UI 版）
//!
//! 流程：命令行解析 → 窗口前启动决策（无更新 + launch 模式静默退出）→
//! 需要时进入 eframe 事件循环，后台线程执行下载/安装并经共享快照
//! （`qsltd_updater_core::phase::PhaseHandle`）驱动视图刷新。
//!
//! 界面为两套模板：主模板 F（环形进度 + 步骤条，双击直启）与副模板 H
//! （极简单行状态，被其他应用带参调用）。`--template F|H` 与 update.json
//! 的 `launcher_template` 可显式覆盖。
//! 后端模块与 UI 编排层已抽离至共享 crate `qsltd-updater-core`
//! （src-core 的 phase/flow 模块），本 crate 只保留视图层。

mod app;
mod preview;
mod templates;
mod theme;
mod widgets;

use std::sync::{Arc, Mutex};

use qsltd_updater_core::cli::CliArgs;
use qsltd_updater_core::flow::{self, StartupContext, StartupDecision};
use qsltd_updater_core::phase::{PhaseHandle, PhasePayload, Stage};

fn main() {
    // --preview 模板预览模式（复刻 preview.html，仅本地验证 F 主 / H 副两套布局，
    // 不属于核心 CLI 参数——先摘除再交给 core 解析，避免「未知参数」告警）
    // 预览初始阶段：--stage <0-10|名称>（检测中/下载中/校验中/安装中/发现新版本/已是最新/更新完成/错误/重试中/强制更新×2）
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

    // --check-only 纯检测：不建窗，输出结果后按退出码结束（0=无更新 1=失败 2=有更新可跳过 3=必须更新）
    // 日志在 check_only_exit 内部初始化（那里已经解析出 app_name，避免重复读配置）
    if args.check_only {
        std::process::exit(flow::check_only_exit(&args));
    }

    // 共享阶段快照：后台更新线程写、UI 线程每帧轮询
    let phase = PhaseHandle::new();

    if preview_mode {
        // 预览模式不读 update.json（避免在预览目录里生成配置模板），
        // 标题只走「命令行 --name > 编译期烘焙默认」两级；
        // 模板默认 F，可用 --template H 预览副模板
        let title =
            qsltd_updater_core::config::resolve_launcher_name(args.launcher_name.as_deref(), None);
        let template = {
            let key = qsltd_updater_core::phase::resolve_template_key(
                args.template.as_deref().unwrap_or(""),
            );
            if key.is_empty() {
                "F".to_string()
            } else {
                key
            }
        };
        run_ui(
            phase,
            Arc::new(Mutex::new(None)),
            true,
            preview_stage,
            &title,
            &template,
        );
        return;
    }

    let cfg = flow::resolve_config(&args);
    let app_root = flow::app_dir(&cfg.app_name);
    // 日志尽早初始化：release 的 GUI 进程没有控制台，之后所有 qlog! 是唯一可
    // 事后追溯的通道（含单实例抢锁失败这类早期路径）。check-only 的日志
    // 在 flow::check_only_exit 内部初始化，那边已经解析过 app_name。
    qsltd_updater_core::log::init(&app_root);
    qsltd_updater_core::qlog!(
        "[更新器] 启动，版本 {}，应用: {}，标题: {}，界面模板: {}，manifest 地址 {} 个: {}",
        env!("CARGO_PKG_VERSION"),
        cfg.app_name,
        cfg.launcher_name,
        cfg.template,
        cfg.manifest_urls.len(),
        cfg.manifest_urls.join(" → ")
    );

    // 单实例互斥：两个更新器同时写同一批版本目录会互相踩（最坏形态是把对方
    // 刚解压好的临时目录当残留删掉，装出半套版本）。`--check-only` 与
    // `--preview` 在上面就已经返回，不受这条影响——纯检测是只读的，预览是开发用的。
    // 最多等 5 秒是给提权交接留的：父进程 spawn 完管理员副本就退出，
    // 副本稍后必然能拿到锁；真正并存的另一个实例则会等满后安静退场。
    let _instance =
        match qsltd_updater_core::instance::acquire(&app_root, std::time::Duration::from_secs(5)) {
            Ok(guard) => guard,
            Err(e) => {
                qsltd_updater_core::qlog!("[更新器] {e}，本次启动退出");
                return;
            }
        };

    // 启动决策在后台线程异步执行（窗口创建前不再同步 await）：
    // 无更新 + launch 模式静默退出；有更新才建窗。窗口以隐藏方式启动，
    // 首检在后台跑，弱网/慢 manifest（>300ms）时再显示「正在检查更新」窗口
    // （懒加载），避免无更新场景窗口闪现拖累「秒开」观感（早期 zsui 版亦采用
    // 「先决策后建窗」策略，此处进一步细化到「按需可见」）。
    let startup = Arc::new(Mutex::new(None::<StartupDecision>));
    {
        let startup_bg = startup.clone();
        let argv_bg = argv.clone();
        let cfg_bg = cfg.clone();
        let spawned = std::thread::Builder::new()
            .name("startup-check".into())
            .spawn(move || {
                let args_bg = qsltd_updater_core::cli::parse_from(&argv_bg);
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        // 同样不能留空：写入错误决策让界面结束「检查中」
                        qsltd_updater_core::qlog!("[更新器] 创建异步运行时失败: {}", e);
                        let msg = format!("创建异步运行时失败: {}", e);
                        *startup_bg.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(StartupDecision::Show(Box::new(StartupContext {
                                cfg: cfg_bg,
                                manifest: None,
                                initial: PhasePayload::new(
                                    Stage::Error,
                                    "更新检查失败",
                                    &msg,
                                    None,
                                ),
                            })));
                        return;
                    }
                };
                let decision = rt.block_on(flow::run_startup(&args_bg, &cfg_bg));
                *startup_bg.lock().unwrap_or_else(|e| e.into_inner()) = Some(decision);
            });
        if let Err(e) = spawned {
            // 线程都没起来时不能把 startup 留空：界面会永远停在「正在检查更新」。
            // 写入一个错误决策，让 UI 正常显示失败原因并给出重试/官网入口。
            qsltd_updater_core::qlog!("[更新器] 启动检查线程创建失败: {}", e);
            let msg = format!("启动检查线程创建失败: {}", e);
            *startup.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(StartupDecision::Show(Box::new(StartupContext {
                    cfg: cfg.clone(),
                    manifest: None,
                    initial: PhasePayload::new(Stage::Error, "更新检查失败", &msg, None),
                })));
        }
    }

    run_ui(phase, startup, false, 0, &cfg.launcher_name, &cfg.template);
}

/// 进入 eframe 事件循环
///
/// - `phase`: 共享阶段快照（真实模式已含首帧阶段；预览模式为空）
/// - `preview`: true 时渲染模板/阶段切换面板（复刻 Preview.vue）
/// - `title`: 生效标题（`--name` > update.json `name` > 编译期烘焙默认），
///   同时用于系统窗口标题与自绘标题栏左侧文字
/// - `template`: 生效界面模板（F 主 / H 副），真实模式由启动方式与配置决定
fn run_ui(
    phase: PhaseHandle,
    startup: Arc<Mutex<Option<StartupDecision>>>,
    preview: bool,
    preview_stage: usize,
    title: &str,
    template: &str,
) {
    let template = template.to_string();

    let window_h = if preview {
        app::WINDOW_SIZE.1 + app::PREVIEW_EXTRA_HEIGHT
    } else {
        app::WINDOW_SIZE.1
    };
    let native_options = eframe::NativeOptions {
        viewport: {
            let viewport = egui::ViewportBuilder::default()
                .with_title(title)
                .with_inner_size([app::WINDOW_SIZE.0, window_h])
                .with_resizable(false)
                .with_decorations(false)
                .with_always_on_top()
                // 隐藏启动：首帧居中（app.rs logic，对齐 tauri.conf.json "center": true）
                // 完成后再显示，避免窗口先出现在默认级联位置再跳到屏幕中心
                .with_visible(false);
            // 应用图标（任务栏 / Alt+Tab）：exe 文件图标由 build.rs 嵌入
            match load_window_icon() {
                Some(icon) => viewport.with_icon(icon),
                None => viewport,
            }
        },
        ..Default::default()
    };

    // 标题栏文字用一份 owned 标题（闭包 move 进 UpdaterApp），系统窗口标题/
    // eframe app_name 直接用形参 `title` 的借用，避免同一值既借又移
    let app_name = title.to_string();
    let result = eframe::run_native(
        title,
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(app::UpdaterApp::new(
                cc,
                phase,
                startup,
                template,
                preview,
                preview_stage,
                app_name,
            )) as Box<dyn eframe::App>)
        }),
    );
    if let Err(e) = result {
        qsltd_updater_core::qlog!("[更新器] UI 循环异常退出: {}", e);
        std::process::exit(1);
    }
}

/// 从内嵌 PNG 解码窗口图标（任务栏 / Alt+Tab）
///
/// exe 文件图标由 build.rs 通过 rc 资源嵌入；此处设置的是运行时窗口图标。
fn load_window_icon() -> Option<egui::IconData> {
    let mut reader = png::Decoder::new(&include_bytes!("../assets/icon.png")[..])
        .read_info()
        .ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut rgba).ok()?;
    if info.color_type != png::ColorType::Rgba {
        qsltd_updater_core::qlog!("[更新器] 图标 PNG 非 RGBA 格式，跳过窗口图标设置");
        return None;
    }
    rgba.truncate(info.buffer_size());
    Some(egui::IconData {
        rgba,
        width: info.width,
        height: info.height,
    })
}
