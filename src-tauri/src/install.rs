//! 更新包安装：zip 解压到版本目录 + 写入本地版本记录（app.json）

use std::path::{Path, PathBuf};

use qsltd_update::LocalManifest;

/// 安装更新包
///
/// - `zip_path`: 已下载并通过校验的更新包
/// - `install_dir`: 版本安装目录（如 .../app_2.1.2，已存在则先删除重装）
/// - `on_stage`: 安装阶段提示回调（中文消息）
///
/// 返回安装目录路径
pub async fn install<F>(zip_path: &Path, install_dir: &Path, on_stage: F) -> Result<PathBuf, String>
where
    F: Fn(String) + Send + Sync,
{
    // 安装目录已存在则先删除（同版本重装场景）
    if install_dir.exists() {
        on_stage(format!("清理旧目录: {}", install_dir.display()));
        tokio::fs::remove_dir_all(install_dir)
            .await
            .map_err(|e| format!("删除旧安装目录失败 {}: {}", install_dir.display(), e))?;
    }

    // 解压到临时目录，避免半成品直接进入安装目录
    let temp_dir = install_dir.with_extension("tmp");
    if temp_dir.exists() {
        tokio::fs::remove_dir_all(&temp_dir)
            .await
            .map_err(|e| format!("清理临时目录失败 {}: {}", temp_dir.display(), e))?;
    }
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| format!("创建临时目录失败 {}: {}", temp_dir.display(), e))?;

    on_stage("正在解压更新包...".to_string());
    let zip_path = zip_path.to_path_buf();
    let temp = temp_dir.clone();
    let extract_result = tokio::task::spawn_blocking(move || extract_zip(&zip_path, &temp))
        .await
        .map_err(|e| format!("解压任务异常: {}", e))?;

    if let Err(e) = extract_result {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        return Err(e);
    }

    // 临时目录重命名为正式安装目录
    on_stage("正在应用新版本...".to_string());
    if let Some(parent) = install_dir.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建安装父目录失败 {}: {}", parent.display(), e))?;
    }
    tokio::fs::rename(&temp_dir, install_dir)
        .await
        .map_err(|e| {
            format!(
                "应用新版本失败（{} -> {}）: {}",
                temp_dir.display(),
                install_dir.display(),
                e
            )
        })?;

    Ok(install_dir.to_path_buf())
}

/// 保存本地版本记录 app.json
///
/// - `path`: app.json 路径
/// - `app_name`: 应用名称
/// - `version`: 已安装版本
/// - `install_dir`: 安装目录
/// - `launch_app`: 目标程序路径（更新完成后由更新器拉起）
pub async fn save_local_manifest(
    path: &Path,
    app_name: &str,
    version: &str,
    install_dir: &Path,
    launch_app: Option<String>,
) -> Result<(), String> {
    let manifest = LocalManifest::new(
        app_name,
        version,
        &install_dir.to_string_lossy(),
        launch_app,
        true,
        None,
        None,
    );
    manifest
        .save(path)
        .await
        .map_err(|e| format!("保存本地版本记录失败 {}: {}", path.display(), e))
}

/// 解压 zip 到目标目录（阻塞函数，请在 spawn_blocking 中调用）
fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("打开更新包失败 {}: {}", zip_path.display(), e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("读取更新包结构失败: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取更新包条目 {} 失败: {}", i, e))?;

        // 跳过非法路径（防路径穿越）
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(name);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| format!("创建目录失败 {}: {}", out_path.display(), e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录失败 {}: {}", parent.display(), e))?;
            }
            let mut out_file = std::fs::File::create(&out_path)
                .map_err(|e| format!("创建文件失败 {}: {}", out_path.display(), e))?;
            std::io::copy(&mut entry, &mut out_file)
                .map_err(|e| format!("解压文件失败 {}: {}", out_path.display(), e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = entry.unix_mode() {
                    let _ = std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
                }
            }
        }
    }
    Ok(())
}
