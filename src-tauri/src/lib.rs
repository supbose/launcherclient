mod cleanup;
mod cli;
mod config;
mod download;
mod install;
mod manifest;
mod updater;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // 启动即解析命令行参数并执行更新检查
            let args = cli::parse();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                updater::run_check(handle, args).await;
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri 应用运行失败");
}
