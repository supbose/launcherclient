//! 在系统默认浏览器中打开链接（错误态「前往官网」/强制更新「前往应用商店」用）
//!
//! 只做两件事：把地址**校验干净**，然后交给平台默认协议处理器。
//!
//! 为什么必须校验：这两个入口的地址来源不同，可信度也不同——
//! - `homepage` 来自本地 `update.json` / 编译期烘焙值（用户或打包者可控）
//! - `store_url` 来自**服务器下发的 version.json**，更新器拿不到任何签名保证，
//!   服务端被改写或 CDN 被劫持时它就是一个远程可控字符串
//!
//! 而 Windows 分支走的是 `cmd /C start "" "<url>"`：地址里若含 `"`，就能提前闭合
//! 引号逃逸出参数，后面的内容会被 cmd 当成命令执行——更新器此时可能是管理员权限，
//! 这条路径等于把远程字符串喂进了一个提权 shell。故这里按「不可信输入」处理：
//! scheme 白名单（http/https）+ 由 URL 解析器重新序列化（非法字符按 RFC 3986
//! 百分号编码）+ 序列化后再过一道元字符黑名单，任何一环不过就拒绝打开。

use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// 允许的协议：只有这两类才是「用浏览器打开」的语义
///
/// 明确拒绝 `file:`（可指向本地程序）、`javascript:`、`data:`，以及 Windows 下
/// 最危险的 `ms-msdt:`/`searchms:` 之类协议处理器和裸盘符路径。
const ALLOWED_SCHEMES: &[&str] = &["http", "https"];

/// 校验并规范化要在系统中打开的 URL
///
/// 三步，任一不过返回 `Err`（中文原因，直接进错误态文案）：
/// 1. [`reqwest::Url::parse`] 解析并要求 http/https、带主机名。这一步会按
///    WHATWG URL 规则重新序列化地址，实测的中和效果：
///    - `"` → `%22`、空格 → `%20`、`\r\n` 直接剔除 —— cmd 再也拿不到裸引号，
///      也就无法提前闭合 `"…"` 把后半截当命令执行
///    - `%USERPROFILE%\..\x` → `x`（dot-segment 与转义一并归一）
///    - `C:\Windows\...` → scheme 归一为 `c`，被白名单拒掉
/// 2. 百分号序列必须形如 `%XX`（XX 为十六进制）——裸 `%` 解析器不会自动补码，
///    留着就是 cmd 的变量展开语法
/// 3. 最终串不得含 `"`、控制字符与 `| < > ^` ——第 1 步已覆盖前两者，这步是兜底，
///    防解析器行为变化后悄悄放开口子；`&` `'` 反引号在 cmd 的双引号内本就是
///    字面量，且 `?a=1&b=2` 是正常查询串，故不拦
pub fn sanitize_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("链接为空".to_string());
    }

    let parsed =
        reqwest::Url::parse(trimmed).map_err(|e| format!("链接无法解析（{}）: {}", trimmed, e))?;

    if !ALLOWED_SCHEMES.contains(&parsed.scheme()) {
        return Err(format!(
            "链接协议不受支持（{}），只允许 http/https",
            parsed.scheme()
        ));
    }
    // 拒绝 http:// 这类无主机的形式：没有站点就没有「用浏览器打开」的意义
    if parsed.host_str().unwrap_or_default().is_empty() {
        return Err(format!("链接缺少主机名: {}", trimmed));
    }

    let candidate = parsed.as_str();

    // 第 2 步：百分号必须是合法的转义序列
    if let Some(bad) = find_stray_percent(candidate) {
        return Err(format!("链接含非法的百分号转义（{} 处）", bad));
    }

    // 第 3 步：shell 元字符兜底黑名单
    if let Some(idx) = candidate.find(['"', '|', '<', '>', '^']) {
        return Err(format!("链接含 shell 元字符（第 {} 个字符处）", idx + 1));
    }
    if candidate.chars().any(char::is_control) {
        return Err("链接含控制字符".to_string());
    }

    Ok(candidate.to_string())
}

/// 找出第一个不合法的 `%` 转义位置（返回其字符下标，全部合法时 None）
///
/// URL 规范允许路径里出现裸 `%`（解析器不会自动补成 `%25`），而 cmd 会把
/// `%VAR%` 展开成环境变量值，所以这里要求每个 `%` 后面必须跟两位十六进制。
fn find_stray_percent(url: &str) -> Option<usize> {
    let bytes = url.as_bytes();
    let hex = |c: u8| c.is_ascii_hexdigit();
    for (i, b) in bytes.iter().enumerate() {
        if *b != b'%' {
            continue;
        }
        let next_two = bytes.get(i + 1).copied().map(hex).unwrap_or(false)
            && bytes.get(i + 2).copied().map(hex).unwrap_or(false);
        if !next_two {
            return Some(i);
        }
    }
    None
}

/// 用系统默认浏览器打开 `url`（先经 [`sanitize_url`] 校验）
///
/// Windows 走 `cmd /C start`（空 title 参数占位，避免 start 把带引号的
/// 地址当成窗口标题）；地址经校验后不含裸引号、控制字符与 cmd 变量展开，
/// 用 `raw_arg` 显式加引号传入即可安全穿过 cmd。macOS 走 `open`；其余走 `xdg-open`。
pub fn open_url(url: &str) -> Result<(), String> {
    let url = sanitize_url(url)?;

    #[cfg(target_os = "windows")]
    let result = {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", ""])
            .raw_arg(format!("\"{}\"", url));
        cmd.spawn()
    };

    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(&url).spawn();

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(&url).spawn();

    match result {
        Ok(mut child) => {
            // start/open 会立刻返回，但子进程未回收会留下僵尸，等一手即可
            let _ = child.wait();
            crate::qlog!("[更新器] 已在浏览器打开: {}", url);
            Ok(())
        }
        Err(e) => {
            let msg = format!("无法打开浏览器（{}）: {}", url, e);
            crate::qlog!("[更新器] {}", msg);
            Err(msg)
        }
    }
}

// 测试只覆盖 sanitize_url（纯字符串判定，不启动进程、不联网）
#[cfg(test)]
mod tests {
    use super::*;

    fn ok(raw: &str) -> String {
        sanitize_url(raw).unwrap_or_else(|e| panic!("应通过校验 {:?}: {}", raw, e))
    }

    #[test]
    fn accepts_plain_http_and_https() {
        assert_eq!(
            ok("https://www.qcsm.ltd/download"),
            "https://www.qcsm.ltd/download"
        );
        // 首尾空白按未输入处理；查询串与片段是正常形态
        assert_eq!(
            ok("  http://a.test/p?x=1#frag  "),
            "http://a.test/p?x=1#frag"
        );
    }

    #[test]
    fn rejects_empty_and_non_url() {
        assert!(sanitize_url("").is_err());
        assert!(sanitize_url("   ").is_err());
        assert!(sanitize_url("not a url").is_err());
    }

    #[test]
    fn rejects_non_http_schemes() {
        // 提权进程绝不能把这些交给协议处理器
        for bad in [
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "ms-msdt:/id foo",
            "cmd:///c calc",
            "ftp://host/x",
        ] {
            assert!(sanitize_url(bad).is_err(), "应拒绝 {:?}", bad);
        }
    }

    #[test]
    fn quotes_are_percent_encoded_not_passed_through() {
        // 关键回归：注入用的引号必须变成 %22，绝不能以裸字符出现在最终串里
        let out = ok(r#"https://store.test/"&start calc.exe&""#);
        assert_eq!(out, "https://store.test/%22&start%20calc.exe&%22");
        assert!(!out.contains('"'), "最终地址不应含裸引号: {out}");
    }

    #[test]
    fn ampersand_in_query_is_kept() {
        // 商店链接里 ?a=1&b=2 极常见：& 在 cmd 双引号内是字面量，不该拦
        assert_eq!(ok("https://a.test/p?a=1&b=2"), "https://a.test/p?a=1&b=2");
    }

    #[test]
    fn shell_expansion_forms_are_neutralised() {
        // %VAR% 与 dot-segment 在 URL 归一阶段就被吃掉，剩个干净的路径
        assert_eq!(ok(r"https://a.test/%USERPROFILE%\..\x"), "https://a.test/x");
        // 行归一时的 \r\n 被剔除（不是报错，但结果里不含控制字符）
        let out = ok("https://a.test/\r\ncalc");
        assert_eq!(out, "https://a.test/calc");
        assert!(!out.chars().any(char::is_control));
        // 控制字符落在路径里则被编码为 %07，仍是合法转义
        assert_eq!(ok("https://a.test/\u{7}bell"), "https://a.test/%07bell");
    }

    #[test]
    fn stray_percent_is_rejected() {
        // 裸 % 解析器不会自动补码，留着就是 cmd 的变量展开语法
        assert!(sanitize_url(r"https://a.test/pct%").is_err());
        assert!(sanitize_url(r"https://a.test/p%2").is_err());
        // 合法百分号转义放行
        assert_eq!(ok("https://a.test/%41%20b"), "https://a.test/%41%20b");
    }

    #[test]
    fn hostless_url_is_rejected() {
        assert!(sanitize_url("http://").is_err());
        assert!(sanitize_url("https://").is_err());
    }
}
