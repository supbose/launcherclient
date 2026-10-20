//! QsLtd 独立更新程序（Tauri 2 + Vue 3 版）
//!
//! 后端模块已抽离至共享 crate `qsltd-updater-core`（src-core），
//! 本 crate 只保留 Tauri 装配与 UI 编排层。

mod updater;

pub fn run() {
    // --check-only 纯检测：不进 Tauri（不建窗口/WebView），输出结果后按退出码结束
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = qsltd_updater_core::cli::parse_from(&argv);
    if args.check_only {
        std::process::exit(updater::check_only_exit(&args));
    }

    // 登记原始参数供错误态「重试」使用（retry_check 重新解析，
    // 保证重试与首次完全同参）
    updater::register_cli_argv(argv);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            updater::finish_and_launch,
            updater::current_phase,
            updater::retry_check
        ])
        .on_window_event(|window, event| {
            use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

            // 关键阶段（检测/下载/安装中）关闭主窗口前必须确认：
            // 直接退出会留下「服务已卸、app.json 未写」的不一致态，
            // 或浪费一次已发起的检测。确认后才放行随后的 close
            // （close_allowed 守卫避免递归拦截）。
            // available/done/error 阶段关闭无副作用，直接放行。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main"
                    && updater::is_critical_phase()
                    && !updater::close_allowed()
                {
                    api.prevent_close();
                    let win = window.clone();
                    win.dialog()
                        .message("更新正在进行中。中断可能导致安装不完整或版本记录不一致，确定要退出吗？")
                        .title("中断更新？")
                        .buttons(MessageDialogButtons::OkCancel)
                        .kind(MessageDialogKind::Warning)
                        .show(move |confirmed| {
                            if confirmed {
                                updater::allow_close();
                                let _ = win.close();
                            }
                        });
                }
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();

            // splash 窗口样式唯一定义在 tauri.conf.json（qsm 同款配置项），
            // 但 "create": false —— 由本回调按配置构建，原因：
            // 1. 交接模式（--launch）要求全程静默：create: true 时 splash
            //    被无条件创建，setup 里 close 的窗口销毁有 ~1s 延迟
            //    （WebView2 固定耗时），用户会看到 splash 闪现；
            // 2. 直接运行模式（无 --launch）在此构建，时机与配置自动创建
            //    仅差同函数内 main 的构建耗时（毫秒级），首帧优势不受
            //    影响（Tauri 先建配置窗口再执行本回调，但那一步现在只建
            //    main，splash 在此构建后同样远早于 Vue 应用首帧）；
            // 3. from_config 读配置构建，样式不重复维护。
            //
            // 关闭方式照搬 qsm（main.rs show_main_and_close_splash）：检测有
            // 更新/出错时先 show main、等其首帧渲染、再撤 splash，见
            // updater::run_check；无更新时补足最短展示时长后退出。
            if args.launch.is_none() {
                create_splash_from_config(app.handle());
                updater::mark_splash_shown();
            }

            // 经 spawn_check 统一入口（运行中守卫 + 完成后复位），
            // 错误态「重试」与首次运行共用同一路径
            updater::spawn_check(handle, args);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri 应用运行失败");
}

/// 按 tauri.conf.json 的 splash 配置构建 splash 窗口（仅直接运行模式调用）。
///
/// 配置里 "create": false 防止 Tauri 自动创建（交接模式零窗口），
/// 样式（尺寸/无边框/置顶等）仍以 JSON 为唯一来源，此处只负责构建。
fn create_splash_from_config(app: &tauri::AppHandle) {
    use tauri::WebviewWindowBuilder;

    let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == updater::SPLASH_LABEL)
    else {
        eprintln!("[更新器] tauri.conf.json 缺少 splash 窗口配置，跳过");
        return;
    };

    if let Err(e) = WebviewWindowBuilder::from_config(app, config)
        .and_then(|builder| builder.build())
    {
        eprintln!("[更新器] splash 窗口创建失败（继续无 splash 运行）: {e}");
    }
}
