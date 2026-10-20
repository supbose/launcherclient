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
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            updater::finish_and_launch,
            updater::current_phase
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

            // splash 窗口由 tauri.conf.json 静态声明「声明即创建」（qsm 同款）：
            // Tauri 的 setup() 先创建 app.windows 配置的窗口、再执行本回调
            // （tauri-2.12.4 src/app.rs:2691-2699），splash 首帧早于任何动态
            // 创建，且本回调里操作它时窗口必然已存在（无竞态）。
            //
            // 关闭方式照搬 qsm（main.rs show_main_and_close_splash）：检测有
            // 更新/出错时先 show main、等其首帧渲染、再撤 splash，见
            // updater::run_check；无更新时补足最短展示时长后退出。
            //
            // 交接模式（--launch）要求全程静默（本项目特有约束，qsm 无此
            // 模式）：splash 被配置无条件创建，这里立即关闭。
            if args.launch.is_some() {
                updater::close_splash(&handle);
            } else {
                updater::mark_splash_shown();
            }

            tauri::async_runtime::spawn(async move {
                updater::run_check(handle, args).await;
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri 应用运行失败");
}
