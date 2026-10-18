mod cleanup;
mod cli;
mod config;
mod download;
mod install;
mod manifest;
mod services;
mod updater;

pub fn run() {
    // --check-only 纯检测：不进 Tauri（不建窗口/WebView），输出结果后按退出码结束
    let args = cli::parse();
    if args.check_only {
        std::process::exit(updater::check_only_exit(&args));
    }

    tauri::Builder::default()
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
