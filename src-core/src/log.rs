//! 进程内文件日志（`<app_dir>/logs/updater.log`）
//!
//! 为什么需要它：release 构建使用 `windows_subsystem = "windows"`，双击启动时
//! stderr 没有接收方，而本仓库的诊断输出全部是 `eprintln!`——用户报障时只能看到
//! 界面最后一行文案，下载重试、manifest 失败、提权交接这些过程信息全部丢失。
//! 这里提供一个极小的落盘通道，让「更新为什么失败/花了多久」事后可查。
//!
//! 设计取舍：
//! - 单文件、1 MiB 滚动、保留 3 份（当前 + `.1` + `.2`），不引入日志框架；
//! - 每行写完立即 flush：崩溃时最后一行必须留在盘上，日志量很小，开销可忽略；
//! - 初始化失败不影响主流程（记 stderr 后继续），日志永远不能成为新的故障点；
//! - 未初始化时只写 stderr（debug 下方便开发；release 下等于静默），避免库被
//!   测试或其他入口直接调用时 panic。
//!
//! 调用方式：入口处 [`init`]，其余位置用 [`crate::qlog!`] 宏。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 日志子目录（位于 `app_dir` 下）
const LOG_DIR: &str = "logs";
/// 日志文件名
const LOG_FILE: &str = "updater.log";
/// 单文件上限：1 MiB。按当前输出量足够覆盖多次更新，又不会长期占盘
const MAX_BYTES: u64 = 1024 * 1024;
/// 保留文件总数（当前 + `.1` + `.2`）
const KEEP_FILES: usize = 3;

/// 进程级日志句柄。`OnceLock` 保证只初始化一次；写入频率极低，`Mutex` 足够。
static LOGGER: OnceLock<Mutex<LogFile>> = OnceLock::new();

struct LogFile {
    path: PathBuf,
    /// `None` 表示当前无法写入（重开失败）；后续写入静默丢弃，不 panic
    file: Option<File>,
    size: u64,
    max_bytes: u64,
    keep_files: usize,
}

impl LogFile {
    fn open(path: PathBuf, max_bytes: u64, keep_files: usize) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // 启动时若上一轮已顶到上限，先滚动；失败不致命，继续追加即可
        let _ = rotate_if_needed(&path, max_bytes, keep_files);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path,
            file: Some(file),
            size,
            max_bytes,
            keep_files,
        })
    }

    /// 写入一行（不含时间戳前缀；调用方负责格式化）
    fn write_line(&mut self, line: &str) {
        let need = line.len() as u64 + 1;
        if self.size > 0 && self.size + need > self.max_bytes {
            self.rotate();
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if file.write_all(line.as_bytes()).is_err() || file.write_all(b"\n").is_err() {
            return;
        }
        // 崩溃时最后一行必须可读：日志量小，逐行 flush 的代价可以接受
        let _ = file.flush();
        self.size += need;
    }

    /// 关闭当前文件 → 滚动 → 重新追加。任何一步失败都不 panic
    fn rotate(&mut self) {
        // 先关闭句柄：Windows 上打开中的文件可能无法改名
        self.file = None;
        // 这里已经确定「再写这一行会超限」，不能再走 rotate_if_needed 的
        // 「当前文件还没到上限」判断，否则单行较长时会持续超限
        let _ = rotate_files(&self.path, self.keep_files);
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(file) => {
                self.size = file.metadata().map(|m| m.len()).unwrap_or(0);
                self.file = Some(file);
            }
            Err(e) => {
                let _ = writeln!(
                    io::stderr(),
                    "[日志] 滚动后重新打开失败（{}），本次日志停止落盘: {}",
                    self.path.display(),
                    e
                );
                self.size = 0;
            }
        }
    }
}

/// 把 `updater.log` 滚成 `updater.log.1`，旧的依次后移，最老的删除
fn rotate_if_needed(path: &Path, max_bytes: u64, keep_files: usize) -> io::Result<()> {
    let Ok(meta) = fs::metadata(path) else {
        return Ok(());
    };
    if meta.len() < max_bytes {
        return Ok(());
    }
    rotate_files(path, keep_files)
}

/// 无条件滚动一次（调用方已确认需要滚动）
fn rotate_files(path: &Path, keep_files: usize) -> io::Result<()> {
    let backups = keep_files.saturating_sub(1);
    if backups == 0 {
        // 只保留当前文件：清空内容但保留文件本身，便于外部 tail -f
        fs::write(path, b"")?;
        return Ok(());
    }
    // 最老的先删，再从大到小后移，最后把当前文件变成 .1
    let _ = fs::remove_file(backup_path(path, backups));
    for i in (1..backups).rev() {
        let from = backup_path(path, i);
        if from.exists() {
            let _ = fs::rename(&from, backup_path(path, i + 1));
        }
    }
    let _ = fs::rename(path, backup_path(path, 1));
    Ok(())
}

fn backup_path(path: &Path, index: usize) -> PathBuf {
    let mut raw = path.as_os_str().to_os_string();
    raw.push(format!(".{}", index));
    PathBuf::from(raw)
}

/// 初始化文件日志。可重复调用（后续调用直接忽略）。
///
/// `app_dir` 为应用数据根目录（见 [`crate::appdir::app_dir`]），日志写到
/// `<app_dir>/logs/updater.log`。初始化失败只提示、不影响启动。
pub fn init(app_dir: &Path) {
    if LOGGER.get().is_some() {
        return;
    }
    let path = app_dir.join(LOG_DIR).join(LOG_FILE);
    match LogFile::open(path, MAX_BYTES, KEEP_FILES) {
        Ok(log) => {
            let _ = LOGGER.set(Mutex::new(log));
        }
        Err(e) => {
            let _ = writeln!(
                io::stderr(),
                "[日志] 初始化失败（{}），本次仅输出到 stderr: {}",
                app_dir.join(LOG_DIR).join(LOG_FILE).display(),
                e
            );
        }
    }
}

/// 写一行带时间戳的日志（[`crate::qlog!`] 的底层实现，也可直接调用）
pub fn write_line(msg: &str) {
    // debug 构建保留控制台输出（开发体验与替换 eprintln! 前一致）；
    // release 的 GUI 进程没有控制台，只落盘。
    #[cfg(debug_assertions)]
    {
        let _ = writeln!(io::stderr(), "{}", msg);
    }
    // 未初始化时仍保留 stderr 出口：命令行重定向/开发期还能看到输出
    if LOGGER.get().is_none() {
        #[cfg(not(debug_assertions))]
        {
            let _ = writeln!(io::stderr(), "{}", msg);
        }
        return;
    }
    let Some(lock) = LOGGER.get() else {
        return;
    };
    let Ok(mut logger) = lock.lock() else {
        return;
    };
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    // 多行消息逐行加前缀，避免日志里出现没有时间戳的裸行
    for line in msg.lines() {
        logger.write_line(&format!("{} {}", stamp, line));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "qsltd_log_test_{}_{}_{}",
            tag,
            std::process::id(),
            n
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("创建测试目录");
        dir
    }

    #[test]
    fn open_creates_parent_dirs_and_appends() {
        let dir = temp_dir("append");
        let path = dir.join("nested").join("logs").join("updater.log");
        let mut log = LogFile::open(path.clone(), MAX_BYTES, KEEP_FILES).expect("打开日志");
        log.write_line("first line");
        log.write_line("second line");
        drop(log);

        let content = fs::read_to_string(&path).expect("读取日志");
        assert!(content.contains("first line"));
        assert!(content.contains("second line"));
        assert_eq!(content.lines().count(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotates_when_file_reaches_limit() {
        let dir = temp_dir("rotate");
        let path = dir.join("updater.log");
        let mut log = LogFile::open(path.clone(), 64, KEEP_FILES).expect("打开日志");
        // 每行 ~20 字节，多写几行必然触发滚动
        for i in 0..10 {
            log.write_line(&format!("line-{}-0123456789", i));
        }
        drop(log);

        assert!(path.exists(), "滚动后当前文件必须仍存在");
        assert!(backup_path(&path, 1).exists(), "必须至少产生一个备份");
        let current = fs::metadata(&path).expect("读取当前文件元信息").len();
        assert!(current <= 64, "当前文件不应超过上限: {}", current);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn keeps_only_configured_number_of_files() {
        let dir = temp_dir("keep");
        let path = dir.join("updater.log");
        let mut log = LogFile::open(path.clone(), 32, 3).expect("打开日志");
        for i in 0..40 {
            log.write_line(&format!("line-{}", i));
        }
        drop(log);

        assert!(path.exists());
        assert!(backup_path(&path, 1).exists());
        assert!(backup_path(&path, 2).exists());
        assert!(
            !backup_path(&path, 3).exists(),
            "keep=3 时最多保留当前 + 两个备份"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_without_backups_truncates() {
        let dir = temp_dir("truncate");
        let path = dir.join("updater.log");
        fs::write(&path, "x".repeat(100)).expect("写入初始内容");
        rotate_if_needed(&path, 32, 1).expect("滚动");
        assert!(path.exists());
        assert_eq!(fs::metadata(&path).expect("元信息").len(), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
