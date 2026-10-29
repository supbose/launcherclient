//! 更新包安装：zip 解压到版本目录 + 写入本地版本记录（app.json）

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::io::Read as _; // Read::take（流式截断，防 zip bomb 闸 2）

use crate::local_manifest::LocalManifest;

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
    let temp_dir = temp_dir_for(install_dir);
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

/// 安装临时目录：`<安装目录名>_tmp`（如 `app_2.1.2` → `app_2.1.2_tmp`）
///
/// 不用 `with_extension("tmp")`：那会把 `app_2.1.2` 变成 `app_2.1.tmp`，
/// 导致 `2.1.2` 与 `2.1.10` 两个版本的临时目录共名，某次解压失败留下的
/// 残留会被另一版本的安装误清理/误复用。
fn temp_dir_for(install_dir: &Path) -> PathBuf {
    let name = install_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    install_dir.with_file_name(format!("{}_tmp", name))
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
    // 错误信息已由 [`LocalManifest::save`] 带上目标路径，此处不再重复拼接
    manifest.save(path).await
}

/// 解压总量硬顶：8GB。
///
/// 超出即判定为异常包（zip bomb 或 manifest 指错地址），立即中止，
/// 防止解压写满用户磁盘。正常更新包（主 exe + 资源）远低于此限。
const MAX_EXTRACT_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// 单条目硬顶：512MB。
///
/// 流式截断的兜底额度：条目头部声明的未压缩大小可造假，按声明值放行后
/// 实际最多写出「上限 + 1」字节即被截断判定。
const MAX_EXTRACT_ENTRY_BYTES: u64 = 512 * 1024 * 1024;

/// 解压 zip 到目标目录（阻塞函数，请在 spawn_blocking 中调用）
///
/// 通过 `on_stage` 按条目推送解压进度（第 N/M 项 + 百分比）
fn extract_zip(zip_path: &Path, dest: &Path, on_stage: &StageCallback) -> Result<(), String> {
    extract_zip_with_limits(
        zip_path,
        dest,
        on_stage,
        MAX_EXTRACT_TOTAL_BYTES,
        MAX_EXTRACT_ENTRY_BYTES,
    )
}

/// 解压 zip 到目标目录，带写入量上限（阻塞函数，请在 spawn_blocking 中调用）
///
/// 防 zip bomb 三道闸（上限可由测试注入小值验证）：
/// 1. **写前预检**：条目头部声明的未压缩大小超单条目上限、或累计超总量
///    上限时，在创建文件前直接拒绝（声明值可信时零磁盘浪费）
/// 2. **流式截断**：读取用 `take(max_entry + 1)` 限制——头部声明可造假，
///    实际数据超出上限即被截断，写出量判定后报错
/// 3. **写后累计**：每条目写完后复核总量，超出立即中止
///
/// 路径穿越防护不变：`enclosed_name` 拒绝 `../` 等非法条目（跳过）。
fn extract_zip_with_limits(
    zip_path: &Path,
    dest: &Path,
    on_stage: &StageCallback,
    max_total: u64,
    max_entry: u64,
) -> Result<(), String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("打开更新包失败 {}: {}", zip_path.display(), e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("读取更新包结构失败: {}", e))?;
    let total = archive.len() as f64;
    let mut written: u64 = 0;

    for i in 0..archive.len() {
        let entry = archive
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
            // 闸 1a：单条目声明大小预检（创建文件前拒绝）
            let declared = entry.size();
            if declared > max_entry {
                return Err(format!(
                    "更新包条目过大已中止: {}（声明 {} 字节，上限 {} 字节）",
                    out_path.display(),
                    declared,
                    max_entry
                ));
            }
            // 闸 1b：累计预检
            if written + declared > max_total {
                return Err(format!(
                    "更新包解压总量超限已中止: 第 {} 项后累计将达 {} 字节（上限 {} 字节）",
                    i + 1,
                    written + declared,
                    max_total
                ));
            }

            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录失败 {}: {}", out_path.display(), e))?;
            }
            let mut out_file = std::fs::File::create(&out_path)
                .map_err(|e| format!("创建文件失败 {}: {}", out_path.display(), e))?;

            // unix 权限位需在 take() 移动 entry 前取出
            #[cfg(unix)]
            let unix_mode = entry.unix_mode();

            // 闸 2：流式截断（头部声明可能小于实际数据量）
            let mut limited = entry.take(max_entry + 1);
            let copied = std::io::copy(&mut limited, &mut out_file)
                .map_err(|e| format!("解压文件失败 {}: {}", out_path.display(), e))?;
            written += copied;

            // 闸 2 判定：读满 max_entry+1 说明实际数据超出单条目上限
            if copied > max_entry {
                return Err(format!(
                    "更新包条目过大已中止: {}（实际超过 {} 字节）",
                    out_path.display(),
                    max_entry
                ));
            }
            // 闸 3：写后累计复核
            if written > max_total {
                return Err(format!(
                    "更新包解压总量超限已中止: 已写出 {} 字节（上限 {} 字节）",
                    written, max_total
                ));
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = unix_mode {
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

    // ---------- temp_dir_for ----------

    #[test]
    fn temp_dir_appends_tmp_suffix() {
        let d = Path::new("/apps/qsm/app/app_2.1.2");
        assert_eq!(
            temp_dir_for(d),
            Path::new("/apps/qsm/app/app_2.1.2_tmp")
        );
    }

    #[test]
    fn temp_dir_unique_across_versions() {
        // 历史上 with_extension("tmp") 会让 2.1.2 与 2.1.10 共名 app_2.1.tmp
        let a = temp_dir_for(Path::new("/apps/qsm/app/app_2.1.2"));
        let b = temp_dir_for(Path::new("/apps/qsm/app/app_2.1.10"));
        assert_ne!(a, b);
        assert!(a.ends_with("app_2.1.2_tmp"));
        assert!(b.ends_with("app_2.1.10_tmp"));
    }

    #[test]
    fn temp_dir_stays_in_same_parent() {
        let d = Path::new("/apps/qsm/app/app_1.0.0");
        assert_eq!(temp_dir_for(d).parent(), d.parent());
    }

    // ---------- extract_zip_with_limits ----------

    use std::io::Write as _;

    /// 造一个测试用 zip（zip crate 是正式依赖，测试模块可直接用）
    fn make_test_zip(path: &Path, entries: &[(&str, &[u8])]) -> io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut writer = zip::ZipWriter::new(file);
        for (name, data) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .map_err(|e| io::Error::other(e.to_string()))?;
            writer.write_all(data)?;
        }
        writer
            .finish()
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(())
    }

    /// 无操作进度回调
    fn noop_stage() -> StageCallback {
        Arc::new(|_msg: &str, _pct: Option<f64>| {})
    }

    #[test]
    fn extract_normal_within_limits() -> io::Result<()> {
        let dir = temp_install_dir("extract_ok")?;
        let zip = dir.join("update.zip");
        make_test_zip(&zip, &[("a.txt", b"hello"), ("sub/b.bin", &[0u8; 64])])?;
        let out = dir.join("out");
        std::fs::create_dir_all(&out)?;
        extract_zip(&zip, &out, &noop_stage())
            .map_err(io::Error::other)?;
        assert_eq!(std::fs::read(out.join("a.txt"))?, b"hello");
        assert_eq!(std::fs::read(out.join("sub/b.bin"))?.len(), 64);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn extract_entry_at_limit_boundary_passes() -> io::Result<()> {
        // 声明大小恰等于上限：100 > 100 为假，应放行
        let dir = temp_install_dir("extract_boundary")?;
        let zip = dir.join("update.zip");
        make_test_zip(&zip, &[("exact.txt", &[b'a'; 100])])?;
        let out = dir.join("out");
        std::fs::create_dir_all(&out)?;
        extract_zip_with_limits(&zip, &out, &noop_stage(), 1_000, 100)
            .map_err(io::Error::other)?;
        assert_eq!(std::fs::read(out.join("exact.txt"))?.len(), 100);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn extract_rejects_entry_over_limit() -> io::Result<()> {
        // 闸 1a：声明 200 字节 > 上限 100，创建文件前即拒绝
        let dir = temp_install_dir("extract_entry_limit")?;
        let zip = dir.join("update.zip");
        make_test_zip(&zip, &[("big.txt", &[b'a'; 200])])?;
        let out = dir.join("out");
        std::fs::create_dir_all(&out)?;
        match extract_zip_with_limits(&zip, &out, &noop_stage(), 1_000, 100) {
            Err(e) => {
                assert!(e.contains("条目过大"), "错误信息应说明条目过大: {e}");
                assert!(!out.join("big.txt").exists(), "超限条目不应落盘");
            }
            Ok(()) => panic!("超限条目必须解压失败"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn extract_rejects_total_over_limit() -> io::Result<()> {
        // 闸 1b：两个 80 字节条目，总量上限 100，第二个条目累计 160 超限
        let dir = temp_install_dir("extract_total_limit")?;
        let zip = dir.join("update.zip");
        make_test_zip(&zip, &[("p1.bin", &[b'a'; 80]), ("p2.bin", &[b'b'; 80])])?;
        let out = dir.join("out");
        std::fs::create_dir_all(&out)?;
        match extract_zip_with_limits(&zip, &out, &noop_stage(), 100, 1_000) {
            Err(e) => assert!(e.contains("总量超限"), "错误信息应说明总量超限: {e}"),
            Ok(()) => panic!("总量超限必须解压失败"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn extract_skips_path_traversal_entry() -> io::Result<()> {
        // 路径穿越条目被 enclosed_name 拒绝（跳过），dest 外不落盘
        let dir = temp_install_dir("extract_traversal")?;
        let zip = dir.join("update.zip");
        make_test_zip(&zip, &[("../evil.txt", b"evil"), ("ok.txt", b"fine")])?;
        let out = dir.join("out");
        std::fs::create_dir_all(&out)?;
        extract_zip(&zip, &out, &noop_stage())
            .map_err(io::Error::other)?;
        assert!(out.join("ok.txt").exists(), "正常条目应解压");
        assert!(!out.join("evil.txt").exists(), "穿越条目不应写入 dest");
        assert!(!dir.join("evil.txt").exists(), "穿越条目不应写入 dest 之外");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
