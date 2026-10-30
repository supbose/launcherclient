//! 旧版本目录清理：安装成功后删除多余的历史版本目录。
//!
//! 版本目录隔离模式下，旧版本目录本身就是回滚资产（保留最近 max_versions 个），
//! 因此不需要额外的文件级备份。Windows 上目标程序可能正在运行导致目录被占用，
//! 删除失败的目录重命名为 `_del_` 前缀留待下次启动再清（参考 qcsm 延迟删除思想）。

use std::path::{Path, PathBuf};

/// 删除失败的目录前缀（下次启动时由 cleanup_pending 再次清理）
const PENDING_DELETE_PREFIX: &str = "_del_";

/// 交换安装的备份前缀（与 install.rs 的 `SWAP_BACKUP_PREFIX` 保持一致）
const SWAP_BACKUP_PREFIX: &str = "_bak_";

/// 下载暂存文件/目录的统一前缀（`update_*.zip` / `update_*_parts`）
const DOWNLOAD_STALE_PREFIX: &str = "update_";

/// 下载暂存区里的包超过这个天数未改动即视为废弃
const STALE_DOWNLOAD_DAYS: u64 = 7;

/// 启动时的一次性残留清理（在拉 manifest 前调用，开销只与目录条目数相关）
///
/// 覆盖四类「上次没走完 / 走完没删掉」的痕迹：
/// - `_del_*`：删除失败、等下次启动的旧版本（原有行为）
/// - `_bak_<name>`：交换安装中断的备份——正式目录在则删备份，不在则还原
/// - `app_*_tmp`：解压中断留下的半成品（正常流程会自己删，崩溃才残留）
/// - `downloads/` 里 7 天前的 `update_*`：装好了却没清掉的包/分卷
///
/// 陈旧包按 mtime 判定而不是一律删除：提权交接、异常退出的窗口里，
/// 另一个进程可能刚写好包正准备安装，删掉会迫使它重新下载完整包。
pub async fn cleanup_startup(app_dir: &Path, downloads_dir: &Path) {
    cleanup_pending(app_dir).await;
    recover_backups(app_dir).await;
    cleanup_temp_dirs(app_dir).await;
    cleanup_manifest_temps(app_dir).await;
    cleanup_stale_downloads(downloads_dir).await;
}

/// 恢复/清理交换安装留下的 `_bak_*` 备份
///
/// 正式目录在（崩溃发生在删备份之前）→ 备份已无用途，删除；
/// 正式目录不在（崩溃发生在改名之后、新目录就位之前）→ 把备份还原，
/// 至少先恢复出一个能跑的旧版本。
async fn recover_backups(app_dir: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(app_dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(original) = name.strip_prefix(SWAP_BACKUP_PREFIX) else {
            continue;
        };
        let target = app_dir.join(original);
        if target.exists() {
            remove_dir_smart(&path).await;
        } else {
            match tokio::fs::rename(&path, &target).await {
                Ok(()) => crate::qlog!("[清理] 已还原上次中断安装的版本: {}", original),
                Err(e) => crate::qlog!("[清理] 还原中断安装的备份失败 {}（{}）", name, e),
            }
        }
    }
}

/// 清理解压中断留下的 `app_*_tmp` 半成品目录
async fn cleanup_temp_dirs(app_dir: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(app_dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("app_") && name.ends_with("_tmp") {
            remove_dir_smart(&path).await;
        }
    }
}

/// 清理原子写 app.json 时崩溃留下的临时文件（`app.json.<pid>.tmp`）
async fn cleanup_manifest_temps(app_dir: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(app_dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("app.json.") && name.ends_with(".tmp") {
            if let Err(e) = tokio::fs::remove_file(&path).await {
                crate::qlog!("[清理] 版本记录临时文件删除失败 {}: {}", name, e);
            }
        }
    }
}

/// 清理下载暂存区里长期未改动的 `update_*`（完整包与分卷目录）
async fn cleanup_stale_downloads(downloads_dir: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(downloads_dir).await else {
        return;
    };
    let now = std::time::SystemTime::now();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(DOWNLOAD_STALE_PREFIX) {
            continue;
        }
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        // 读不到 mtime（异常卷/未来时间戳）就放过，宁可多留也不误删
        let Ok(modified) = meta.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age.as_secs() < STALE_DOWNLOAD_DAYS * 24 * 60 * 60 {
            continue;
        }
        if meta.is_dir() {
            remove_dir_smart(&path).await;
        } else {
            match tokio::fs::remove_file(&path).await {
                Ok(()) => crate::qlog!("[清理] 已删除过期更新包: {}", name),
                Err(e) => crate::qlog!("[清理] 过期更新包删除失败 {}: {}", name, e),
            }
        }
    }
}

/// 清理删除失败的残留目录（`_del_*`）
///
/// - `app_dir`: 版本根目录（如 .../qsm/app）
pub async fn cleanup_pending(app_dir: &Path) {
    let mut entries = match tokio::fs::read_dir(app_dir).await {
        Ok(entries) => entries,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with(PENDING_DELETE_PREFIX) {
                    match tokio::fs::remove_dir_all(&path).await {
                        Ok(_) => crate::qlog!("[清理] 已清理残留目录: {}", name),
                        Err(e) => crate::qlog!("[清理] 残留目录清理失败 {}: {}", name, e),
                    }
                }
            }
        }
    }
}

/// 清理旧版本目录，按版本从旧到新删除，保留最近 `keep` 个（当前版本始终保留）
///
/// - `app_dir`: 版本根目录（如 .../qsm/app）
/// - `current_version`: 当前已安装版本
/// - `keep`: 保留的版本目录总数（含当前版本；keep=1 表示只留当前版本）
pub async fn cleanup_old_versions(app_dir: &Path, current_version: &str, keep: usize) {
    if keep == 0 {
        return;
    }

    let mut versioned_dirs = collect_version_dirs(app_dir).await;
    if versioned_dirs.len() <= keep {
        return;
    }

    // 按版本从旧到新排序（解析失败的排最后兜底按目录名）
    versioned_dirs.sort_by(|a, b| {
        let va = semver::Version::parse(&a.0);
        let vb = semver::Version::parse(&b.0);
        match (va, vb) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            _ => a.0.cmp(&b.0),
        }
    });

    let to_delete = versioned_dirs.len() - keep;
    for (version, path) in versioned_dirs.iter().take(to_delete) {
        if version == current_version {
            // 当前版本目录不清理（防御 keep 配置过小）
            continue;
        }
        remove_dir_smart(path).await;
    }
}

/// 收集版本目录（app_{version} 形式），返回 (版本号, 路径) 列表
async fn collect_version_dirs(app_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut dirs = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(app_dir).await else {
        return dirs;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(version) = name.strip_prefix("app_") {
                if !version.is_empty() && !version.starts_with(PENDING_DELETE_PREFIX) {
                    dirs.push((version.to_string(), path));
                }
            }
        }
    }
    dirs
}

/// 智能删除目录：直接删除；失败（文件被占用）则重命名为 `_del_` 前缀留待下次
async fn remove_dir_smart(dir: &Path) {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    match tokio::fs::remove_dir_all(dir).await {
        Ok(_) => crate::qlog!("[清理] 已删除旧版本目录: {}", name),
        Err(e) => {
            // 重命名为待删除前缀，目录占用解除后下次启动清理
            let renamed = dir.with_file_name(format!("{}{}", PENDING_DELETE_PREFIX, name));
            match tokio::fs::rename(dir, &renamed).await {
                Ok(_) => crate::qlog!("[清理] 目录被占用，已重命名待下次清理: {}（{}）", name, e),
                Err(re) => crate::qlog!(
                    "[清理] 旧版本目录删除失败且重命名失败: {}（删除: {}，重命名: {}）",
                    name,
                    e,
                    re
                ),
            }
        }
    }
}

// 测试里构造失败即用例本身失败，按 clippy 惯例局部放行 expect_used
#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 造一个唯一的临时版本根目录
    fn temp_app_dir(tag: &str) -> std::io::Result<PathBuf> {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "qsltd_cleanup_test_{}_{}_{}",
            tag,
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// 在版本根目录下造一个版本目录（带一个标记文件）
    fn make_version_dir(root: &Path, name: &str) -> std::io::Result<()> {
        let d = root.join(name);
        std::fs::create_dir_all(&d)?;
        std::fs::write(d.join("marker.txt"), name.as_bytes())?;
        Ok(())
    }

    fn exists(root: &Path, name: &str) -> bool {
        root.join(name).exists()
    }

    /// 测试用：把文件/目录的修改时间改到过去（验证按 mtime 判定陈旧包）
    fn backdate(path: &Path, t: std::time::SystemTime) {
        // Windows 上需要 FILE_WRITE_ATTRIBUTES 才能改时间戳，目录还必须
        // 带 FILE_FLAG_BACKUP_SEMANTICS 才允许 OpenOptions 打开
        #[cfg(windows)]
        let opened = {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
            const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
            std::fs::OpenOptions::new()
                .access_mode(FILE_WRITE_ATTRIBUTES)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(path)
        };
        #[cfg(not(windows))]
        let opened = std::fs::File::open(path);
        opened
            .expect("打开待回填时间的路径")
            .set_times(std::fs::FileTimes::new().set_modified(t))
            .expect("回填修改时间");
    }

    #[tokio::test]
    async fn keep_zero_deletes_nothing() {
        // keep=0 防御性不删（避免配置错误清空所有版本）
        let root = temp_app_dir("keep0").expect("建目录");
        make_version_dir(&root, "app_1.0.0").expect("造版本目录");
        cleanup_old_versions(&root, "1.0.0", 0).await;
        assert!(exists(&root, "app_1.0.0"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn within_keep_deletes_nothing() {
        let root = temp_app_dir("within").expect("建目录");
        make_version_dir(&root, "app_1.0.0").expect("造版本目录");
        make_version_dir(&root, "app_1.1.0").expect("造版本目录");
        cleanup_old_versions(&root, "1.1.0", 2).await;
        assert!(exists(&root, "app_1.0.0"));
        assert!(exists(&root, "app_1.1.0"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn deletes_oldest_beyond_keep() {
        let root = temp_app_dir("oldest").expect("建目录");
        for v in ["1.0.0", "1.1.0", "1.2.0"] {
            make_version_dir(&root, &format!("app_{v}")).expect("造版本目录");
        }
        cleanup_old_versions(&root, "1.2.0", 2).await;
        assert!(!exists(&root, "app_1.0.0"), "最旧版本应被删");
        assert!(exists(&root, "app_1.1.0"));
        assert!(exists(&root, "app_1.2.0"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn current_version_never_deleted_even_if_oldest() {
        // keep 配置过小时的防御：当前版本目录不清理
        let root = temp_app_dir("current").expect("建目录");
        make_version_dir(&root, "app_1.0.0").expect("造版本目录"); // 当前版本，且最旧
        make_version_dir(&root, "app_2.0.0").expect("造版本目录");
        cleanup_old_versions(&root, "1.0.0", 1).await;
        assert!(exists(&root, "app_1.0.0"), "当前版本不删");
        assert!(exists(&root, "app_2.0.0"), "无更旧可删时不动其他");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn semver_ordering_not_string_ordering() {
        // 字符串排序会判 "1.10.0" < "1.9.0"；semver 排序必须删 1.9.0
        let root = temp_app_dir("semver").expect("建目录");
        for v in ["1.9.0", "1.10.0", "1.11.0"] {
            make_version_dir(&root, &format!("app_{v}")).expect("造版本目录");
        }
        cleanup_old_versions(&root, "1.11.0", 2).await;
        assert!(!exists(&root, "app_1.9.0"), "最旧应按 semver 判定");
        assert!(exists(&root, "app_1.10.0"), "1.10.0 比 1.9.0 新，应保留");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn unparsable_version_sorts_last_and_survives() {
        // 版本号无法按 semver 解析时按目录名字符串排最后，不被优先删
        let root = temp_app_dir("unparsable").expect("建目录");
        make_version_dir(&root, "app_abc").expect("造版本目录");
        make_version_dir(&root, "app_1.0.0").expect("造版本目录");
        make_version_dir(&root, "app_1.1.0").expect("造版本目录");
        cleanup_old_versions(&root, "1.1.0", 2).await;
        assert!(!exists(&root, "app_1.0.0"), "最旧可解析版本被删");
        assert!(exists(&root, "app_abc"), "不可解析版本排最后，保留");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn non_version_dirs_ignored() {
        let root = temp_app_dir("nonver").expect("建目录");
        make_version_dir(&root, "downloads").expect("造目录");
        make_version_dir(&root, "app_").expect("造目录"); // 空前缀版本号，不算版本目录
        make_version_dir(&root, "app_1.0.0").expect("造版本目录");
        make_version_dir(&root, "app_1.1.0").expect("造版本目录");
        cleanup_old_versions(&root, "1.1.0", 2).await;
        assert!(exists(&root, "downloads"));
        assert!(exists(&root, "app_"));
        assert!(exists(&root, "app_1.0.0"), "有效版本目录仅 2 个，不超 keep");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn cleanup_pending_removes_del_prefixed_dirs_only() {
        let root = temp_app_dir("pending").expect("建目录");
        make_version_dir(&root, "_del_app_1.0.0").expect("造残留目录");
        make_version_dir(&root, "app_1.1.0").expect("造版本目录");
        cleanup_pending(&root).await;
        assert!(!exists(&root, "_del_app_1.0.0"), "残留目录应被清");
        assert!(exists(&root, "app_1.1.0"), "正常目录不动");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn cleanup_pending_tolerates_missing_root() {
        let root = std::env::temp_dir().join("qsltd_cleanup_test_not_exist_98765");
        cleanup_pending(&root).await; // 不 panic 即通过
    }

    #[tokio::test]
    async fn remove_dir_smart_does_not_panic_on_failure() {
        // 目录不存在：删除与重命名双失败，只记日志不 panic
        let ghost = std::env::temp_dir().join("qsltd_cleanup_test_ghost_dir_12345");
        remove_dir_smart(&ghost).await;
    }

    #[tokio::test]
    async fn remove_dir_smart_deletes_existing_dir() {
        let root = temp_app_dir("smart").expect("建目录");
        let d = root.join("app_0.0.1");
        make_version_dir(&root, "app_0.0.1").expect("造目录");
        remove_dir_smart(&d).await;
        assert!(!d.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---------- cleanup_startup ----------

    #[tokio::test]
    async fn startup_gc_removes_backup_when_version_dir_exists() {
        // 交换完成后崩溃：正式目录在，`_bak_` 只是残留
        let root = temp_app_dir("gc_bak_stale").expect("建目录");
        make_version_dir(&root, "app_1.0.0").expect("造版本目录");
        make_version_dir(&root, "_bak_app_1.0.0").expect("造备份目录");
        cleanup_startup(&root, &root.join("downloads")).await;
        assert!(!exists(&root, "_bak_app_1.0.0"), "残留备份应删除");
        assert!(exists(&root, "app_1.0.0"), "正式目录不动");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn startup_gc_restores_backup_when_version_dir_missing() {
        // 改名后、新目录就位前崩溃：备份就是唯一能跑的版本，必须还原
        let root = temp_app_dir("gc_bak_recover").expect("建目录");
        make_version_dir(&root, "_bak_app_1.0.0").expect("造备份目录");
        cleanup_startup(&root, &root.join("downloads")).await;
        assert!(exists(&root, "app_1.0.0"), "备份应还原为正式目录");
        assert!(!exists(&root, "_bak_app_1.0.0"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn startup_gc_removes_install_temp_dirs() {
        let root = temp_app_dir("gc_tmp").expect("建目录");
        make_version_dir(&root, "app_2.0.0_tmp").expect("造解压半成品");
        make_version_dir(&root, "app_2.0.0").expect("造版本目录");
        cleanup_startup(&root, &root.join("downloads")).await;
        assert!(!exists(&root, "app_2.0.0_tmp"), "解压半成品应删除");
        assert!(exists(&root, "app_2.0.0"), "正常版本目录不动");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn startup_gc_removes_manifest_temp_files() {
        // 原子写 app.json 时崩溃留下的临时文件
        let root = temp_app_dir("gc_manifest_tmp").expect("建目录");
        std::fs::write(root.join("app.json"), b"{}").expect("造 app.json");
        std::fs::write(root.join("app.json.1234.tmp"), b"partial").expect("造临时文件");
        cleanup_startup(&root, &root.join("downloads")).await;
        assert!(!exists(&root, "app.json.1234.tmp"), "临时文件应删除");
        assert!(exists(&root, "app.json"), "正式记录不动");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn startup_gc_only_removes_old_downloads() {
        // 新建的包可能是另一个进程刚下好的，按 mtime 保留；8 天前的废弃包才删
        let root = temp_app_dir("gc_downloads").expect("建目录");
        let downloads = root.join("downloads");
        std::fs::create_dir_all(&downloads).expect("建下载目录");
        let old_zip = downloads.join("update_1.0.0.zip");
        let old_parts = downloads.join("update_1.0.0_parts");
        let fresh_zip = downloads.join("update_2.0.0.zip");
        std::fs::write(&old_zip, b"old").expect("造旧包");
        std::fs::create_dir_all(&old_parts).expect("造旧分卷目录");
        std::fs::write(&fresh_zip, b"fresh").expect("造新包");
        std::fs::write(downloads.join("notes.txt"), b"keep").expect("造无关文件");

        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 24 * 3600);
        backdate(&old_zip, old);
        backdate(&old_parts, old);
        backdate(&downloads.join("notes.txt"), old);

        cleanup_startup(&root, &downloads).await;
        assert!(!old_zip.exists(), "8 天前的更新包应删除");
        assert!(!old_parts.exists(), "8 天前的分卷目录应删除");
        assert!(fresh_zip.exists(), "新包不能误删（可能正在用）");
        assert!(downloads.join("notes.txt").exists(), "非 update_ 文件不动");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn startup_gc_tolerates_missing_dirs() {
        // 首次启动：app 与 downloads 都不存在，不能 panic
        let root = std::env::temp_dir().join("qsltd_cleanup_test_gc_missing_98765");
        cleanup_startup(&root, &root.join("downloads")).await;
    }
}
