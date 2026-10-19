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
        .setup(|app| {
            // 启动即解析命令行参数并执行更新检查
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                updater::run_check(handle, args).await;
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri 应用运行失败");
}
