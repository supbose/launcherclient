//! QsLtd 独立更新程序（Tauri 2 + Vue 3 版）
//!
//! 后端模块已抽离至共享 crate `qsltd-updater-core`（src-core），
//! 本 crate 只保留 Tauri 装配与 UI 编排层。

mod updater;

pub fn run() {
    // --check-only 纯检测：不进 Tauri（不建窗口/WebView），输出结果后按退出码结束
    let args = qsltd_updater_core::cli::parse();
    if args.check_only {
        std::process::exit(updater::check_only_exit(&args));
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            updater::finish_and_launch,
            updater::current_phase
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // 直接运行（双击启动器，命令行未带 --launch 交接参数）时先创建
            // splash 启动加载窗口：纯静态零 JS 页面，首帧远快于 main 的 Vue
            // 应用，用于消除 manifest 检测期间（多地址故障转移，最坏 5s×N）
            // 的「无界面」空档——此前该阶段只有 main 的 visible:false 窗口，
            // 双击后像没启动/卡死。
            //
            // 被目标程序以 --launch 交接时保持完全静默：不创建任何窗口，
            // 仍只在发现新版本或出错时显示 main（见 updater::run_check）。
            //
            // 创建失败不阻塞更新流程：splash 是体验增强，main 窗口兜底。
            if args.launch.is_none() {
                create_splash(&handle);
            }

            tauri::async_runtime::spawn(async move {
                updater::run_check(handle, args).await;
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri 应用运行失败");
}

/// 创建 splash 启动加载窗口（仅直接运行模式调用，交接模式不建任何窗口）。
///
/// 页面为 `public/splash.html`：Vite 原样拷入 dist 的纯静态页，
/// 不经前端构建产物，WebView 拿到即可绘制首帧。
fn create_splash(app: &tauri::AppHandle) {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    let result = WebviewWindowBuilder::new(
        app,
        updater::SPLASH_LABEL,
        WebviewUrl::App("splash.html".into()),
    )
    .title("更新程序")
    .inner_size(280.0, 160.0)
    .center()
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(true)
    .build();

    if let Err(e) = result {
        eprintln!("[更新器] splash 窗口创建失败（继续无 splash 运行）: {e}");
    }
}
