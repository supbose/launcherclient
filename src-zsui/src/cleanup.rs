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
