//! 在系统默认浏览器中打开链接（错误态「前往官网」用）
//!
//! 只做一件事：把 URL 交给平台默认协议处理器。不做沙箱校验之外的
//! 任何样式处理 —— 地址来自配置（`homepage`）或内置默认，属于信任来源。

use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// 用系统默认浏览器打开 `url`
///
/// Windows 走 `cmd /C start`（空 title 参数占位，避免 start 把带引号的
/// 地址当成窗口标题；地址里的 `&` 等字符由 `raw_arg` 原样传入，不加引号
/// 会被 cmd 拆成命令，故显式加引号转义）；macOS 走 `open`；其余走 `xdg-open`。
pub fn open_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("官网地址为空".to_string());
    }

    #[cfg(target_os = "windows")]
    let result = Command::new("cmd")
        .args(["/C", "start", ""])
        .raw_arg(format!("\"{}\"", url))
        .spawn();

    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).spawn();

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(url).spawn();

    match result {
        Ok(mut child) => {
            // start/open 会立刻返回，但子进程未回收会留下僵尸，等一手即可
            let _ = child.wait();
            eprintln!("[更新器] 已在浏览器打开: {}", url);
            Ok(())
        }
        Err(e) => {
            let msg = format!("无法打开浏览器（{}）: {}", url, e);
            eprintln!("[更新器] {}", msg);
            Err(msg)
        }
    }
}
