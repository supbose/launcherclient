//! 旧版本目录清理：安装成功后删除多余的历史版本目录。
//!
//! 版本目录隔离模式下，旧版本目录本身就是回滚资产（保留最近 max_versions 个），
//! 因此不需要额外的文件级备份。Windows 上目标程序可能正在运行导致目录被占用，
//! 删除失败的目录重命名为 `_del_` 前缀留待下次启动再清（参考 qcsm 延迟删除思想）。

use std::path::{Path, PathBuf};

/// 删除失败的目录前缀（下次启动时由 cleanup_pending 再次清理）
const PENDING_DELETE_PREFIX: &str = "_del_";

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
                        Ok(_) => eprintln!("[清理] 已清理残留目录: {}", name),
                        Err(e) => eprintln!("[清理] 残留目录清理失败 {}: {}", name, e),
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
        Ok(_) => eprintln!("[清理] 已删除旧版本目录: {}", name),
        Err(e) => {
            // 重命名为待删除前缀，目录占用解除后下次启动清理
            let renamed = dir.with_file_name(format!("{}{}", PENDING_DELETE_PREFIX, name));
            match tokio::fs::rename(dir, &renamed).await {
                Ok(_) => eprintln!(
                    "[清理] 目录被占用，已重命名待下次清理: {}（{}）",
                    name, e
                ),
                Err(re) => eprintln!(
                    "[清理] 旧版本目录删除失败且重命名失败: {}（删除: {}，重命名: {}）",
                    name, e, re
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
}
