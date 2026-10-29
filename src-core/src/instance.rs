//! 单实例互斥 + 独占打开文件的公共原语
//!
//! 为什么需要互斥：更新流程会写 `<应用目录>/app/app_<版本>`、删旧版本目录、
//! 清理下载缓存。两个更新器同时跑（用户连点两次图标，或目标程序与计划任务
//! 各拉起一个）就会在同一批目录上互相踩——最坏的形态是 A 正在把 B 刚解压好的
//! 临时目录当残留删掉，装出一个半套版本。
//!
//! 实现选择：**独占文件句柄**，不引入 `windows-sys` 一类的 FFI 依赖，也不做
//! 命名内核对象。取 `<应用目录>/updater.lock`，以「不与任何进程共享」的
//! 共享模式打开并**持有句柄直到进程退出**：
//! - 第二实例打不开（共享冲突）→ 判定已有实例，直接退出
//! - 句柄由操作系统在进程终止时回收（包括被强杀、断电后的下次启动），
//!   所以不存在「陈旧锁挡住后续更新」这类需要 TTL 猜的情况
//! - 崩溃留下的 lock 文件本身无害：没人持有句柄时，下一次独占打开必然成功

use std::path::{Path, PathBuf};

/// 单实例锁文件名（放在应用数据目录下，随应用名天然隔离）
pub const LOCK_FILE_NAME: &str = "updater.lock";

/// 已持有的单实例锁。**句柄一释放，锁就解除**，因此调用方需让它活到进程结束
pub struct InstanceGuard {
    /// 独占持有的锁文件句柄（内容未被使用，只为占住句柄）。
    /// 用 `Option` 是为了能在 Drop 里**先关句柄再删文件**：Windows 上以
    /// `share_mode(0)` 打开的文件连删除都不允许（没给 FILE_SHARE_DELETE），
    /// 字段还在就删会失败，锁文件永远留着当垃圾。
    handle: Option<std::fs::File>,
    /// 锁文件路径：正常退出时顺手删除，不留垃圾
    path: PathBuf,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // 顺序要紧：先放掉独占句柄，再删文件（被强杀时两者都由 OS 兜底，
        // 下次启动照样能独占打开，所以不存在陈旧锁）
        drop(self.handle.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 取得 `app_dir` 的更新器单实例锁
///
/// - `wait`：拿不到时的最长等待（提权交接场景：父进程刚 spawn 完管理员副本就
///   自行退出，副本会短暂撞上父进程还没释放的句柄，等一手即可接管；
///   真正并存的另一实例在等待期内一直是冲突状态，超时后如实返回 Err）
///
/// 成功返回 [`InstanceGuard`]（调用方必须持有到进程结束）；已有实例时返回 Err，
/// 附带可读原因。目录建不出来（只读介质等）也返回 Err——这时更新流程本身也
/// 走不下去，但错误信息比后续莫名失败好定位。
pub fn acquire(app_dir: &Path, wait: std::time::Duration) -> Result<InstanceGuard, String> {
    std::fs::create_dir_all(app_dir)
        .map_err(|e| format!("创建应用数据目录失败 {}: {}", app_dir.display(), e))?;
    let path = app_dir.join(LOCK_FILE_NAME);

    let deadline = std::time::Instant::now() + wait;
    let mut first_err: Option<std::io::Error> = None;
    loop {
        match open_exclusive_write(&path) {
            Ok(handle) => {
                return Ok(InstanceGuard {
                    handle: Some(handle),
                    path,
                })
            }
            // 只记第一次的失败原因：后面每次都是同一个共享冲突，反复覆盖没意义
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    Err(format!(
        "另一个更新器实例正在运行（{}）: {}",
        path.display(),
        first_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "文件被占用".to_string())
    ))
}

/// 以「不与他人共享」的方式打开文件（不存在则创建）
///
/// Windows 下 `share_mode(0)` 让任何已持有该文件的进程都使本次打开失败，
/// 这正是「另一个实例还在」的判据；非 Windows 无强制文件锁，这里退化为
/// 普通的可写打开，互斥语义不成立（[`acquire`] 会照常放行，不阻塞更新）。
pub(crate) fn open_exclusive_write(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            // 锁文件不承载内容，显式声明 truncate 把「覆盖打开」写成确定语义：
            // clippy::suspicious_open_options 就是针对 `create` 而不交代 truncate 的组合，
            // 将来若改成写入 pid，残留旧字节会误导排查
            .truncate(true)
            // 0 = 不给任何共享模式：文件被别处打开时这里直接失败
            .share_mode(0)
            .open(path)
    }
    #[cfg(not(windows))]
    {
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(path)
    }
}

/// 只探测「文件是否没被别的进程占着」，不创建也不删除任何东西
///
/// 用于下载前的残留包体检：已被占用的包重下也写不进去，重试只是白等一轮。
pub(crate) fn is_unlocked(path: &Path) -> bool {
    if !path.exists() {
        return true;
    }
    // 探测时不能顺手创建，所以走不带 create 的独占打开
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(path)
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        std::fs::OpenOptions::new().write(true).open(path).is_ok()
    }
}

// 互斥行为可在这台机器上真实验证（不需要管理员权限，也不碰网络）
#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_app_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "qsltd_instance_test_{}_{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn acquire_creates_dir_and_lock_file() {
        let dir = temp_app_dir("first");
        let guard = acquire(&dir, std::time::Duration::from_secs(1)).expect("首次应取得锁");
        assert!(dir.exists(), "应用目录应被创建");
        assert!(dir.join(LOCK_FILE_NAME).exists(), "锁文件应落在应用目录里");

        // 释放后锁文件被顺手删掉，不留垃圾
        drop(guard);
        assert!(!dir.join(LOCK_FILE_NAME).exists(), "正常退出应清理锁文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_acquire_is_rejected_while_held() {
        let dir = temp_app_dir("contend");
        let guard = acquire(&dir, std::time::Duration::from_secs(1)).expect("首次应取得锁");

        // 不等待（wait=0）→ 立刻判定冲突；这正是「用户连点两次图标」的路径
        let err = acquire(&dir, std::time::Duration::ZERO)
            .err()
            .expect("持有期间第二个实例应失败");
        assert!(
            err.contains("另一个更新器实例"),
            "错误应说明并存实例: {err}"
        );

        drop(guard);
        let _ = acquire(&dir, std::time::Duration::from_secs(1)).expect("释放后应可再次取得");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 独占语义只有 Windows 有强制文件锁可验
    #[test]
    #[cfg(windows)]
    fn held_file_blocks_unlock_probe() {
        let dir = temp_app_dir("probe");
        std::fs::create_dir_all(&dir).expect("建目录");
        let f = dir.join("payload.bin");
        std::fs::write(&f, b"x").expect("写文件");

        let held = std::fs::File::open(&f).expect("持有句柄");
        assert!(!is_unlocked(&f), "被占用时探测应返回 false");
        assert!(f.exists(), "探测不得删除文件");
        drop(held);
        assert!(is_unlocked(&f), "句柄释放后应恢复");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_unlocked_true_for_absent_path() {
        let dir = temp_app_dir("absent");
        assert!(is_unlocked(&dir.join("nope.zip")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
