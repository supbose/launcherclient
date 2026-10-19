//! 更新包安装：zip 解压到版本目录 + 写入本地版本记录（app.json）

use std::path::{Path, PathBuf};
use std::sync::Arc;

use qingshu::update::LocalManifest;

/// 安装阶段回调：接收中文消息与可选进度百分比（0-100）
pub type StageCallback = Arc<dyn Fn(&str, Option<f64>) + Send + Sync>;

/// 安装更新包
///
/// - `zip_path`: 已下载并通过校验的更新包
/// - `install_dir`: 版本安装目录（如 .../app_2.1.2，已存在则先删除重装）
/// - `on_stage`: 安装阶段回调（中文消息 + 进度百分比，解压阶段按条目实时推进）
///
/// 返回安装目录路径
pub async fn install(zip_path: &Path, install_dir: &Path, on_stage: StageCallback) -> Result<PathBuf, String> {
    // 安装目录已存在则先删除（同版本重装场景）
    if install_dir.exists() {
        on_stage(&format!("清理旧目录: {}", install_dir.display()), None);
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

    on_stage("正在解压更新包...", Some(0.0));
    let zip_path = zip_path.to_path_buf();
    let temp = temp_dir.clone();
    let cb = on_stage.clone();
    let extract_result = tokio::task::spawn_blocking(move || extract_zip(&zip_path, &temp, &cb))
        .await
        .map_err(|e| format!("解压任务异常: {}", e))?;

    if let Err(e) = extract_result {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        return Err(e);
    }

    // 临时目录重命名为正式安装目录
    on_stage("正在应用新版本...", Some(100.0));
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
///
/// 通过 `on_stage` 按条目推送解压进度（第 N/M 项 + 百分比）
fn extract_zip(zip_path: &Path, dest: &Path, on_stage: &StageCallback) -> Result<(), String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("打开更新包失败 {}: {}", zip_path.display(), e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("读取更新包结构失败: {}", e))?;
    let total = archive.len() as f64;

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
                    .map_err(|e| format!("创建目录失败 {}: {}", out_path.display(), e))?;
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

        // 每完成一个条目推送一次进度（含文件名，取最后一段避免长路径）
        let pct = ((i + 1) as f64 / total * 100.0).min(100.0);
        let brief = out_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        on_stage(
            &format!("正在解压更新包... {} / {}（{}）", i + 1, total as u64, brief),
            Some(pct),
        );
    }
    Ok(())
}
