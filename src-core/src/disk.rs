//! 磁盘可用空间查询与更新峰值预检
//!
//! 更新一个 GB 级包的真实峰值远大于包本身：下载的包 + 合并/解压的产物
//! 会同时占盘（分卷模式在合并前是「分卷 + 整包」，解压后是「整包 +
//! 新版本目录」）。不预检的后果是：用户等了半小时下完 2.3 GB，开始解压
//! 才因为磁盘不够失败，还要再自动重试两轮。
//!
//! 查询走 Windows 原生 `GetDiskFreeSpaceExW`（不引第三方依赖）：
//! 其他平台返回 None，调用方按「无法判断 → 放行」处理——预检是体验优化，
//! 宁可不查也不能误拦。

use std::path::Path;

/// 预检余量：操作系统临时文件、日志、NTFS 元数据与正在增长的用户态缓冲
///
/// 取 256 MB：远大于一次更新的附加写入，又不至于让「刚好够」的机器被误拦。
pub const SPACE_MARGIN_BYTES: u64 = 256 * 1024 * 1024;

/// 下载前的峰值需求：包本身 + 合并/解压出的另一份等量数据 + 余量
///
/// 分卷模式下合并前是「分卷 + 整包」，单包模式是「整包 + 解压临时目录」，
/// 两种形态的峰值都约为 2 倍包大小。
pub fn required_for_download(package_size: u64) -> u64 {
    package_size
        .saturating_mul(2)
        .saturating_add(SPACE_MARGIN_BYTES)
}

/// 解压前的需求：中央目录声明的未压缩总量 + 余量
///
/// 此时更新包已经在盘上，解压产物是新增占用，不必再按 2 倍算。
pub fn required_for_extract(uncompressed_size: u64) -> u64 {
    uncompressed_size.saturating_add(SPACE_MARGIN_BYTES)
}

/// 预检指定路径所在卷是否有 `required` 字节可用
///
/// 路径不存在（下载目录尚未创建）时向上找最近的已存在祖先，
/// 保证检查落在真正会被写入的那块卷上。拿不到可用空间（非 Windows、
/// 路径异常）返回 Ok：不阻断更新。
pub fn ensure_space(dir: &Path, required: u64) -> Result<(), String> {
    ensure_space_with(available_bytes(dir), required)
}

/// 预检核心（可用空间由调用方给出，便于单测）
pub(crate) fn ensure_space_with(available: Option<u64>, required: u64) -> Result<(), String> {
    let Some(free) = available else {
        return Ok(());
    };
    if free >= required {
        return Ok(());
    }
    Err(format!(
        "磁盘空间不足：本次更新峰值约需 {}（下载与解压各占用一份安装包大小的空间），当前可用 {}，请清理磁盘后重试",
        crate::phase::format_bytes(required),
        crate::phase::format_bytes(free)
    ))
}

/// 路径所在卷的可用字节数（拿不到返回 None）
///
/// 路径不存在时向上找已存在的祖先：下载目录通常尚未创建，直接查询会失败。
pub fn available_bytes(path: &Path) -> Option<u64> {
    let mut probe = path;
    loop {
        if probe.exists() {
            return query_available(probe);
        }
        probe = probe.parent()?;
    }
}

#[cfg(windows)]
fn query_available(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;

    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_to_caller: u64 = 0;
    // SAFETY: 传入以 NUL 结尾的宽字符串指针与三个有效可写 u64 指针；
    // 失败（返回 0）时只读取未使用的返回值，不触碰输出缓冲。
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_to_caller,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free_to_caller)
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    /// 查询目录所在卷的可用空间（`lpTotalNumberOfBytes` / `lpTotalNumberOfFreeBytes`
    /// 传 NULL 表示不需要，本模块只关心调用方可用空间）
    fn GetDiskFreeSpaceExW(
        lpDirectoryName: *const u16,
        lpFreeBytesAvailableToCaller: *mut u64,
        lpTotalNumberOfBytes: *mut u64,
        lpTotalNumberOfFreeBytes: *mut u64,
    ) -> i32;
}

#[cfg(not(windows))]
fn query_available(_dir: &Path) -> Option<u64> {
    None
}

// 测试里构造失败即用例本身失败，按 clippy 惯例局部放行 expect_used
#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_peak_is_twice_package_plus_margin() {
        assert_eq!(
            required_for_download(1_000),
            2_000 + SPACE_MARGIN_BYTES,
            "下载/合并/解压峰值按两份包大小估算"
        );
    }

    #[test]
    fn extract_peak_is_uncompressed_plus_margin() {
        assert_eq!(
            required_for_extract(1_000),
            1_000 + SPACE_MARGIN_BYTES,
            "解压时包已在盘上，只需再留一份解压产物"
        );
    }

    #[test]
    fn huge_sizes_do_not_overflow() {
        // 服务端给个离谱的大数也不该 panic，只需得出一个「很大」的需求
        assert!(required_for_download(u64::MAX) > 0);
        assert!(required_for_extract(u64::MAX) > 0);
    }

    #[test]
    fn unknown_free_space_never_blocks() {
        // 非 Windows / 路径异常：不做判断，直接放行
        assert!(ensure_space_with(None, u64::MAX).is_ok());
    }

    #[test]
    fn enough_space_passes() {
        assert!(ensure_space_with(Some(10_000), 9_000).is_ok());
        // 恰好相等也算够用
        assert!(ensure_space_with(Some(9_000), 9_000).is_ok());
    }

    #[test]
    fn insufficient_space_errors_with_actionable_text() {
        let err = ensure_space_with(Some(100), 9_000).unwrap_err();
        assert!(err.contains("磁盘空间不足"), "错误应点明原因: {err}");
        assert!(err.contains("清理磁盘"), "错误应给出处理办法: {err}");
        // 这一类失败重试无意义，也不该把用户引到官网
        assert!(crate::download::is_local_io_error(&err));
        assert!(!crate::download::is_retryable(&err));
    }

    #[test]
    fn available_bytes_walks_up_to_existing_parent() {
        // 不存在的深层路径应回落到已存在的祖先而不是直接放弃
        let dir = std::env::temp_dir().join("qsltd_disk_test_missing_12345/deep/nested");
        let free = available_bytes(&dir);
        #[cfg(windows)]
        assert!(free.is_some(), "Windows 上应能查到临时目录所在卷");
        #[cfg(not(windows))]
        assert!(free.is_none());
    }
}
