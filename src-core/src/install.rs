//! 更新包安装：zip 解压到版本目录 + 写入本地版本记录（app.json）

use std::path::{Path, PathBuf};
use std::sync::Arc;

use qingshu::update::LocalManifest;

/// 安装阶段回调：接收中文消息与可选进度百分比（0-100）
pub type StageCallback = Arc<dyn Fn(&str, Option<f64>) + Send + Sync>;

/// 更新器自身的可执行文件名（小写 stem），定位主程序时排除，避免误启动自己
const SELF_EXE_STEMS: &[&str] = &["launcherclient", "qsltdupdater", "updater"];

/// 在安装目录中定位主程序 exe（手动模式 launch 未配置时，供「完成」按钮启动用）
///
/// 选择规则（按优先级）：
/// 1. 排除更新器自身文件名（launcherclient / qsltdupdater / updater，不区分大小写）
/// 2. 文件名 stem 与 `app_name` 匹配（不区分大小写）优先
/// 3. 唯一候选直接选用
/// 4. 多个候选且无名称匹配时，取文件体积最大者（主程序通常最大）；
///    读取元数据失败（大小均为 0）时返回 None，避免误启动
pub fn find_main_executable(install_dir: &Path, app_name: &str) -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = std::fs::read_dir(install_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                && !p.file_stem().is_some_and(|s| {
                    let stem = s.to_string_lossy().to_lowercase();
                    SELF_EXE_STEMS.contains(&stem.as_str())
                })
        })
        .collect();
    if candidates.is_empty() {
        return None;
    }
    // 名称匹配优先（qsm.exe 之于 app_name=qsm）
    let app_lower = app_name.to_lowercase();
    if let Some(hit) = candidates
        .iter()
        .find(|p| {
            p.file_stem()
                .is_some_and(|s| s.to_string_lossy().to_lowercase() == app_lower)
        })
        .cloned()
    {
        return Some(hit);
    }
    if candidates.len() == 1 {
        return candidates.into_iter().next();
    }
    // 多个候选：取体积最大者；全部读取失败时不猜
    let mut best: Option<(PathBuf, u64)> = None;
    for c in candidates {
        let size = std::fs::metadata(&c).map(|m| m.len()).unwrap_or(0);
        match &best {
            Some((_, s)) if *s >= size => {}
            _ => best = Some((c, size)),
        }
    }
    best.filter(|(_, size)| *size > 0).map(|(p, _)| p)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 造一个临时安装目录（tag 用于区分用例）
    fn temp_install_dir(tag: &str) -> io::Result<PathBuf> {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "qsltd_install_test_{}_{}_{}",
            tag,
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// 在目录里造一个指定大小的假 exe
    fn make_exe(dir: &Path, name: &str, size: u64) -> io::Result<()> {
        let buf = vec![b'x'; size as usize];
        std::fs::write(dir.join(name), &buf)
    }

    fn stem_of(p: &Path) -> String {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }

    #[test]
    fn find_main_prefers_app_name_match() -> io::Result<()> {
        // qsm.exe 与 app_name 匹配，即使另一个文件更大也优先选 qsm.exe
        let dir = temp_install_dir("name_match")?;
        make_exe(&dir, "bigger.exe", 10_000)?;
        make_exe(&dir, "qsm.exe", 100)?;
        let hit = find_main_executable(&dir, "qsm");
        assert_eq!(hit.as_deref().map(stem_of).as_deref(), Some("qsm.exe"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_app_name_match_case_insensitive() -> io::Result<()> {
        let dir = temp_install_dir("case_ins")?;
        make_exe(&dir, "QSM.EXE", 100)?;
        make_exe(&dir, "other.exe", 5_000)?;
        let hit = find_main_executable(&dir, "qsm");
        assert_eq!(hit.as_deref().map(stem_of).as_deref(), Some("qsm.exe"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_single_exe_any_name() -> io::Result<()> {
        // 唯一 exe 直接选用，不要求名称匹配
        let dir = temp_install_dir("single")?;
        make_exe(&dir, "whatever.exe", 100)?;
        let hit = find_main_executable(&dir, "qsm");
        assert_eq!(hit.as_deref().map(stem_of).as_deref(), Some("whatever.exe"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_largest_when_ambiguous() -> io::Result<()> {
        // 多个候选且无名称匹配：取体积最大者
        let dir = temp_install_dir("largest")?;
        make_exe(&dir, "small.exe", 100)?;
        make_exe(&dir, "medium.exe", 5_000)?;
        make_exe(&dir, "big.exe", 20_000)?;
        let hit = find_main_executable(&dir, "qsm");
        assert_eq!(hit.as_deref().map(stem_of).as_deref(), Some("big.exe"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_excludes_updater_self() -> io::Result<()> {
        // 更新器自身被排除；排除后唯一候选胜出
        let dir = temp_install_dir("exclude_self")?;
        make_exe(&dir, "launcherclient.exe", 99_000)?;
        make_exe(&dir, "qsltdupdater.exe", 99_000)?;
        make_exe(&dir, "qsm.exe", 100)?;
        let hit = find_main_executable(&dir, "qsm");
        assert_eq!(hit.as_deref().map(stem_of).as_deref(), Some("qsm.exe"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_excludes_updater_self_case_insensitive() -> io::Result<()> {
        let dir = temp_install_dir("exclude_self_ci")?;
        make_exe(&dir, "QsLtdUpdater.exe", 50)?;
        make_exe(&dir, "client.exe", 10)?;
        let hit = find_main_executable(&dir, "qsm");
        assert_eq!(hit.as_deref().map(stem_of).as_deref(), Some("client.exe"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_none_when_no_exe() -> io::Result<()> {
        let dir = temp_install_dir("no_exe")?;
        make_exe(&dir, "readme.txt", 100)?;
        make_exe(&dir, "data.dll", 100)?;
        assert!(find_main_executable(&dir, "qsm").is_none());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_none_when_only_updater_self() -> io::Result<()> {
        // 排除更新器自身后无候选
        let dir = temp_install_dir("only_self")?;
        make_exe(&dir, "launcherclient.exe", 100)?;
        assert!(find_main_executable(&dir, "qsm").is_none());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_none_when_all_zero_size() -> io::Result<()> {
        // 全部候选大小为 0（元数据异常/空文件）时不猜，返回 None
        let dir = temp_install_dir("zero_size")?;
        make_exe(&dir, "a.exe", 0)?;
        make_exe(&dir, "b.exe", 0)?;
        assert!(find_main_executable(&dir, "qsm").is_none());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_none_for_missing_dir() {
        let dir = std::env::temp_dir().join("qsltd_install_test_not_exist_12345");
        assert!(find_main_executable(&dir, "qsm").is_none());
    }

    #[test]
    fn find_main_ignores_subdirectory_exes() -> io::Result<()> {
        // 只扫顶层，不递归子目录
        let dir = temp_install_dir("no_recurse")?;
        let sub = dir.join("bin");
        std::fs::create_dir_all(&sub)?;
        make_exe(&sub, "qsm.exe", 100)?;
        assert!(find_main_executable(&dir, "qsm").is_none());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
