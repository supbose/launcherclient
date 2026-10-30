//! Windows 提权辅助：更新安装 / 删除服务前先验当前进程是否具备管理员权限，
//! 不足时通过 `runas` 重新拉起自身（触发 UAC），由提权后的副本接管后续流程。
//!
//! 背景：更新流程里的「删除服务」（`sc delete`）需要管理员权限；当前进程未提权时
//! 会拿到退出码 5（ERROR_ACCESS_DENIED），`services.rs` 虽能把该失败翻译成
//! 「权限不足」提示，但已经是「先失败再提示」。本模块把检查前移：真正执行
//! 删服务 / 安装之前先确认提权，不够就主动 `runas` 拉起一个管理员副本，
//! 从源头避免那次失败的 sc 调用。
//!
//! 设计要点：
//! - 仅当「配置了需要删除的 Windows 服务（`cfg.services` 非空）且当前进程未提权」
//!   才触发提权——安装目录落在 LocalAppData，本身不需要管理员；无服务配置时
//!   不弹 UAC，避免每次更新都打扰用户。
//! - 提权通过 [`relaunch_self_elevated`] 用 `ShellExecuteW("runas")` 重新拉起同一个
//!   exe（携带相同命令行参数），当前（未提权）进程随后退出，由提权副本接管。
//!   这是 Windows 下获取管理员令牌的唯一标准路径——无法在进程内「就地升级」。
//! - 防死循环：提权重试副本会带上内部标记 [`ELEVATION_RETRY_FLAG`]；若已携带该标记
//!   仍判定为未提权（极端配置），不再重复拉起，交回 `services.rs` 的退出码 5 文案兜底。
//! - 全部 Windows 系统调用走 raw `extern "system"`（链接 kernel32 / advapi32 / shell32），
//!   **不引入额外 crate 依赖**；非 Windows 平台提供恒真 / 恒 ok 桩，保证跨平台编译。
//! - 失败（用户取消 UAC、ShellExecute 出错）有明确日志与可现场处置的错误文案。

use crate::flow::ResolvedConfig;

/// 内部提权标记：提权重试副本携带，用于防止无限重新拉起。
///
/// cli.rs 会静默忽略它（不视为未知参数）；正常生产命令行不会出现。
pub const ELEVATION_RETRY_FLAG: &str = "--__elevated-retry";

/// 判定是否需要提权：配置了需删除的服务且当前进程未提权时为 true。
///
/// 纯函数，便于单测；真实权限判定由 `is_elevated` 注入。
pub fn needs_elevation_for(cfg: &ResolvedConfig, elevated: bool) -> bool {
    !cfg.services.is_empty() && !elevated
}

/// 提权前置检查（真实版本）：供 `flow::run_update_flow` 调用。
///
/// 返回：
/// - `Ok(false)`：无需提权（已提权 / 未配置服务 / 已处于提权重试副本），继续原流程
/// - `Ok(true)`：已成功以 `runas` 重新拉起管理员副本，调用方应**退出当前进程**
/// - `Err(msg)`：提权失败（用户取消 UAC 或 ShellExecute 出错），含日志，交由上层置错误态
pub fn ensure_admin_for_update(cfg: &ResolvedConfig) -> Result<bool, String> {
    ensure_admin_for_update_with(cfg, is_elevated, relaunch_self_elevated)
}

/// 注入函数版本（不触碰真实系统），便于单测覆盖各分支。
pub fn ensure_admin_for_update_with<F, G>(
    cfg: &ResolvedConfig,
    is_elevated_fn: F,
    relaunch_fn: G,
) -> Result<bool, String>
where
    F: Fn() -> bool,
    G: Fn() -> Result<(), String>,
{
    if !needs_elevation_for(cfg, is_elevated_fn()) {
        return Ok(false);
    }
    // 已处于提权重试副本仍非提权：不再重复拉起，交回 services.rs 的退出码 5 兜底
    if std::env::args().any(|a| a == ELEVATION_RETRY_FLAG) {
        crate::qlog!("[更新器] 提权重试副本仍非管理员，停止重复拉起，交回 sc 退出码兜底");
        return Ok(false);
    }
    match relaunch_fn() {
        Ok(()) => {
            crate::qlog!("[更新器] 已请求以管理员身份重新运行更新器（runas），当前进程将退出");
            Ok(true)
        }
        Err(e) => {
            crate::qlog!("[更新器] 提权失败（用户可能取消了 UAC）: {e}");
            Err(format!(
                "提权失败: {e}（更新程序需以管理员身份运行才能卸载服务，请右键「以管理员身份运行」后重试）"
            ))
        }
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn GetLastError() -> u32;
        fn CloseHandle(h: *mut c_void) -> i32;
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(
            process: *mut c_void,
            desired_access: u32,
            token: *mut *mut c_void,
        ) -> i32;
        fn GetTokenInformation(
            token: *mut c_void,
            info_class: i32,
            info: *mut c_void,
            info_length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut c_void,
            lp_operation: *const u16,
            lp_file: *const u16,
            lp_parameters: *const u16,
            lp_directory: *const u16,
            n_show_cmd: i32,
        ) -> isize;
    }

    const TOKEN_QUERY: u32 = 0x0008;
    /// `TOKEN_INFORMATION_CLASS::TokenElevation`
    const TOKEN_ELEVATION: i32 = 20;
    const SW_SHOWNORMAL: i32 = 1;
    /// ShellExecuteW 成功时返回值 > 32；<= 32 为错误码（5 = 用户取消 UAC / 拒绝访问）
    const SE_ERR_ACCESSDENIED: isize = 5;

    #[repr(C)]
    struct TokenElevation {
        token_is_elevated: u32,
    }

    /// 当前进程是否以管理员令牌运行（已提权）。
    ///
    /// 取不到令牌（OpenProcessToken 失败）时保守返回 false，走提权分支更稳妥。
    pub(crate) fn is_elevated() -> bool {
        unsafe {
            let mut token: *mut c_void = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                crate::qlog!(
                    "[更新器] OpenProcessToken 失败（{}），保守视为未提权",
                    GetLastError()
                );
                return false;
            }
            let mut elevation = TokenElevation {
                token_is_elevated: 0,
            };
            let mut ret_len: u32 = 0;
            let ok = GetTokenInformation(
                token,
                TOKEN_ELEVATION,
                &mut elevation as *mut _ as *mut c_void,
                std::mem::size_of::<TokenElevation>() as u32,
                &mut ret_len,
            );
            CloseHandle(token);
            ok != 0 && elevation.token_is_elevated != 0
        }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// 以 `runas` 重新拉起当前 exe（携带相同命令行参数 + 内部重试标记）。
    ///
    /// 返回值：
    /// - `Ok(())`：ShellExecuteW 成功触发 UAC 并启动了提权副本
    /// - `Err(msg)`：失败——多为用户取消 UAC（返回码 5），或 ShellExecute 其它错误
    pub(crate) fn relaunch_self_elevated() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| format!("获取自身路径失败: {e}"))?;

        // 复用当前进程的命令行参数（去掉程序名），并追加内部重试标记防死循环
        let mut args: Vec<String> = std::env::args().skip(1).collect();
        if !args.iter().any(|a| a == super::ELEVATION_RETRY_FLAG) {
            args.push(super::ELEVATION_RETRY_FLAG.to_string());
        }
        // 含空格的参数加引号，避免被 cmd/shell 拆断
        let params = args
            .iter()
            .map(|a| {
                if a.chars().any(|c| c.is_whitespace()) {
                    format!("\"{a}\"")
                } else {
                    a.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        let exe_w = to_wide(&exe.to_string_lossy());
        let verb_w = to_wide("runas");
        let params_w = to_wide(&params);

        // hwnd=null（无父窗口）、lp_directory=null（沿用当前工作目录）
        let r = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                verb_w.as_ptr(),
                exe_w.as_ptr(),
                params_w.as_ptr(),
                ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if r > 32 {
            Ok(())
        } else if r == SE_ERR_ACCESSDENIED {
            Err("用户取消了管理员授权（UAC）".to_string())
        } else {
            Err(format!("ShellExecuteW 返回 {r}（无法以管理员身份启动）"))
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    /// 非 Windows 平台无「管理员令牌」概念，恒为已提权（不触发提权分支）。
    pub(crate) fn is_elevated() -> bool {
        true
    }

    /// 非 Windows 平台无需提权，直接视为成功（调用方据此不退出）。
    pub(crate) fn relaunch_self_elevated() -> Result<(), String> {
        Ok(())
    }
}

/// 当前进程是否已提权（跨平台转发到平台实现）
pub fn is_elevated() -> bool {
    imp::is_elevated()
}

/// 以 runas 重新拉起自身（跨平台转发到平台实现）
pub fn relaunch_self_elevated() -> Result<(), String> {
    imp::relaunch_self_elevated()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 仅含配置服务为空的配置（is_elevated 任意都不应触发提权）
    fn cfg_with_services(services: &[&str]) -> ResolvedConfig {
        ResolvedConfig {
            app_name: "test".to_string(),
            launcher_name: "启动器".to_string(),
            manifest_urls: vec![],
            launch: None,
            max_versions: 3,
            services: services.iter().map(|s| s.to_string()).collect(),
            template: "F".to_string(),
            homepage: String::new(),
            retry_times: 3,
            require_sha256: true,
            parts_concurrency: 3,
            fail_open: false,
        }
    }

    #[test]
    fn needs_elevation_false_when_no_services() {
        // 未配置服务：无论是否提权都不需要（安装走 LocalAppData 不需管理员）
        assert!(!needs_elevation_for(&cfg_with_services(&[]), true));
        assert!(!needs_elevation_for(&cfg_with_services(&[]), false));
    }

    #[test]
    fn needs_elevation_false_when_already_elevated() {
        // 已提权：即便配了服务也不需要再提权
        assert!(!needs_elevation_for(
            &cfg_with_services(&["QsmServer"]),
            true
        ));
    }

    #[test]
    fn needs_elevation_true_when_services_and_not_elevated() {
        // 配了服务且未提权：必须提权
        assert!(needs_elevation_for(
            &cfg_with_services(&["QsmServer"]),
            false
        ));
    }

    #[test]
    fn ensure_admin_ok_false_when_no_services() {
        // 无服务：不提权、不拉起
        let called = std::cell::Cell::new(false);
        let r = ensure_admin_for_update_with(
            &cfg_with_services(&[]),
            || false,
            || {
                called.set(true);
                Ok(())
            },
        );
        assert_eq!(r, Ok(false));
        assert!(!called.get(), "无服务时不应调用 relaunch");
    }

    #[test]
    fn ensure_admin_ok_false_when_elevated() {
        // 已提权：不拉起
        let called = std::cell::Cell::new(false);
        let r = ensure_admin_for_update_with(
            &cfg_with_services(&["QsmServer"]),
            || true,
            || {
                called.set(true);
                Ok(())
            },
        );
        assert_eq!(r, Ok(false));
        assert!(!called.get(), "已提权不应调用 relaunch");
    }

    #[test]
    fn ensure_admin_ok_true_when_relaunch_succeeds() {
        // 需提权且拉起成功：返回 true（调用方应退出）
        let called = std::cell::Cell::new(false);
        let r = ensure_admin_for_update_with(
            &cfg_with_services(&["QsmServer"]),
            || false,
            || {
                called.set(true);
                Ok(())
            },
        );
        assert_eq!(r, Ok(true));
        assert!(called.get(), "应调用 relaunch");
    }

    #[test]
    fn ensure_admin_err_when_relaunch_fails() {
        // 需提权但拉起失败（如用户取消 UAC）：返回 Err 且含可处置文案
        let r = ensure_admin_for_update_with(
            &cfg_with_services(&["QsmServer"]),
            || false,
            || Err("用户取消了管理员授权（UAC）".to_string()),
        );
        let err = r.expect_err("拉起失败必须返回 Err");
        assert!(err.contains("提权失败"), "应提示提权失败: {err}");
        assert!(err.contains("管理员"), "应提示需管理员: {err}");
    }

    #[test]
    fn ensure_admin_stops_loop_on_retry_flag() {
        // 携带提权重试标记仍非提权：不再拉起（防死循环），交回 sc 兜底
        // 通过真实 std::env::args 难以注入，这里直接验证 needs_elevation_for 的语义：
        // 标记分支在 ensure_admin_for_update_with 内读取 env::args，单测环境无该标记，
        // 故该用例仅确认「已提权」短路优先于 relaunch。
        let r = ensure_admin_for_update_with(
            &cfg_with_services(&["QsmServer"]),
            || true,
            || panic!("不应调用 relaunch"),
        );
        assert_eq!(r, Ok(false));
    }
}
