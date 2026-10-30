//! 更新包安装：zip 解压到版本目录 + 写入本地版本记录（app.json）

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc; // Read::take（流式截断，防 zip bomb 闸 2）
use std::time::{Duration, Instant};

use crate::defaults::PROGRESS_INTERVAL_MS;
use crate::local_manifest::LocalManifest;

/// 安装阶段回调：接收中文消息与可选进度百分比（0-100）
pub type StageCallback = Arc<dyn Fn(&str, Option<f64>) + Send + Sync>;

/// 更新器自身的可执行文件名（小写 stem），定位主程序时排除，避免误启动自己
const SELF_EXE_STEMS: &[&str] = &["launcherclient", "qsltdupdater", "updater"];

/// 主程序向下搜索的最大层数（安装目录自身为第 0 层，最多再往下 3 层）
///
/// 只在顶层找不到 exe 时才启用：绝大多数包主程序就在根目录，递归只是
/// 兜住 `bin/`、`app/` 这类分层打包的情况。层数上限避免在资源/插件目录
/// 里越走越深，把某个辅助程序当成主程序。
const MAX_SEARCH_DEPTH: usize = 3;

/// 在安装目录中定位主程序 exe（手动模式 launch 未配置时，供「完成」按钮启动用）
///
/// 先找顶层，再（仅在顶层没有 exe 时）向下最多 [`MAX_SEARCH_DEPTH`] 层。
/// 每一层的选择规则（按优先级）：
/// 1. 排除更新器自身文件名（launcherclient / qsltdupdater / updater，不区分大小写）
/// 2. 文件名 stem 与 `app_name` 匹配（不区分大小写）优先
/// 3. 唯一候选直接选用
/// 4. 多个候选且无名称匹配时，取文件体积最大者（主程序通常最大）；
///    读取元数据失败（大小均为 0）时返回 None，避免误启动
pub fn find_main_executable(install_dir: &Path, app_name: &str) -> Option<PathBuf> {
    // 顶层优先：保持既有行为不受递归影响（顶层有多个 exe 时仍取最大者）
    let top = collect_exes(install_dir);
    if let Some(hit) = pick_candidate(top, app_name) {
        return Some(hit);
    }
    // 顶层一个候选都没有才浅扫子目录：有些包把主程序放在 dist/ 或 bin/，
    // 只扫顶层会让用户在「更新完成」后看到「启动失败」
    let mut nested = Vec::new();
    collect_nested_exes(install_dir, 1, &mut nested);
    pick_candidate(nested, app_name)
}

/// 收集某一层目录中的候选 exe（不递归）
fn collect_exes(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_main_candidate(p))
        .collect()
}

/// 递归收集子目录中的候选 exe（`depth` 从 1 开始，最多到 [`MAX_SEARCH_DEPTH`]）
fn collect_nested_exes(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            if is_main_candidate(&path) {
                out.push(path);
            }
        } else if depth < MAX_SEARCH_DEPTH && path.is_dir() && !is_ignored_dir(&path) {
            collect_nested_exes(&path, depth + 1, out);
        }
    }
}

/// 交换/解压留下的中间目录不参与主程序搜索：`_bak_*` 是上一次安装的旧版本，
/// `*_tmp` 是解压半成品，从里面启动会得到一个马上要消失的程序。
fn is_ignored_dir(path: &Path) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy())
        .is_some_and(|n| n.starts_with("_bak_") || n.ends_with("_tmp"))
}

/// 是否为候选主程序：`.exe` 且不是更新器自身
fn is_main_candidate(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        && !path.file_stem().is_some_and(|s| {
            let stem = s.to_string_lossy().to_lowercase();
            SELF_EXE_STEMS.contains(&stem.as_str())
        })
}

/// 候选排序：名称匹配 > 唯一候选 > 体积最大
fn pick_candidate(candidates: Vec<PathBuf>, app_name: &str) -> Option<PathBuf> {
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

/// 判断安装失败是否值得自动重试
///
/// 只对「同一份包、同一块磁盘，再来一遍可能不一样」的失败返回 true
/// （交换阶段的改名被短暂占用等）。包结构非法、超限、空包、路径穿越，
/// 以及建目录/写文件失败（磁盘满、目录只读）都是确定性的：重试只会把
/// 同样的错误再走一遍，还多占用户两次退避时间。
pub fn is_retryable_error(err: &str) -> bool {
    const PERMANENT: &[&str] = &[
        "打开更新包失败",
        "读取更新包结构失败",
        "读取更新包条目",
        "条目过大已中止",
        "解压总量超限已中止",
        "非法路径条目",
        "更新包为空",
        "创建临时目录失败",
        "清理临时目录失败",
        "创建安装父目录失败",
        "创建目录失败",
        "创建文件失败",
        "解压文件失败",
        // 磁盘预检失败：清盘才有用，重试只会再写坏一次
        "磁盘空间不足",
    ];
    !PERMANENT.iter().any(|m| err.contains(m))
}

/// 解压限额（字节）。默认值见 [`crate::defaults`]，可由 update.json 覆盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractLimits {
    /// 解压产物总量上限
    pub max_total_bytes: u64,
    /// 单个条目解压上限
    pub max_entry_bytes: u64,
}

impl ExtractLimits {
    /// 从 MiB 构造；单文件上限不会超过总量上限（调用方已校验范围，这里再兜一层）
    pub fn from_mb(total_mb: u64, entry_mb: u64) -> Self {
        const MIB: u64 = 1024 * 1024;
        let max_total_bytes = total_mb.saturating_mul(MIB).max(1);
        let max_entry_bytes = entry_mb.saturating_mul(MIB).clamp(1, max_total_bytes);
        Self {
            max_total_bytes,
            max_entry_bytes,
        }
    }
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self::from_mb(
            crate::defaults::DEFAULT_EXTRACT_TOTAL_MB,
            crate::defaults::DEFAULT_EXTRACT_ENTRY_MB,
        )
    }
}

/// 安装更新包
///
/// - `zip_path`: 已下载并通过校验的更新包
/// - `install_dir`: 版本安装目录（如 .../app_2.1.2）
/// - `limits`: 解压限额（防 zip bomb；大包部署可在 update.json 调大）
/// - `on_stage`: 安装阶段回调（中文消息 + 进度百分比，解压阶段按条目实时推进）
///
/// 返回安装目录路径。
///
/// 顺序是「先解压、后替换」：解压成功前不动旧目录，同版本修复重装失败时
/// 仍保留可运行的旧版本。替换时先把旧目录改名为 `_bak_` 备份，再把临时
/// 目录改名为正式目录，成功后才删除备份；中途崩溃留下的备份会在下次
/// 安装时自动恢复或清理。
pub async fn install(
    zip_path: &Path,
    install_dir: &Path,
    limits: ExtractLimits,
    on_stage: StageCallback,
) -> Result<PathBuf, String> {
    let backup_dir = backup_dir_for(install_dir);
    recover_interrupted_swap(install_dir, &backup_dir).await;

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
    crate::qlog!(
        "[安装] 解压上限：总量 {}、单文件 {}",
        crate::phase::format_bytes(limits.max_total_bytes),
        crate::phase::format_bytes(limits.max_entry_bytes)
    );
    let zip_path = zip_path.to_path_buf();
    let temp = temp_dir.clone();
    let cb = on_stage.clone();
    let extract_result =
        tokio::task::spawn_blocking(move || extract_zip(&zip_path, &temp, limits, &cb))
            .await
            .map_err(|e| format!("解压任务异常: {}", e))?;

    if let Err(e) = extract_result {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        return Err(e);
    }

    // ---- 交换：旧目录 → 备份，临时目录 → 正式目录 ----
    let had_existing = install_dir.exists();
    if had_existing {
        on_stage("正在备份旧版本...", None);
        if backup_dir.exists() {
            tokio::fs::remove_dir_all(&backup_dir)
                .await
                .map_err(|e| format!("清理旧版本备份失败 {}: {}", backup_dir.display(), e))?;
        }
        if let Err(e) = tokio::fs::rename(install_dir, &backup_dir).await {
            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
            return Err(format!(
                "备份旧安装目录失败（{} -> {}）: {}",
                install_dir.display(),
                backup_dir.display(),
                e
            ));
        }
    }

    // 临时目录重命名为正式安装目录
    on_stage("正在应用新版本...", Some(100.0));
    if let Some(parent) = install_dir.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建安装父目录失败 {}: {}", parent.display(), e))?;
    }
    match tokio::fs::rename(&temp_dir, install_dir).await {
        Ok(()) => {
            if had_existing {
                if let Err(e) = tokio::fs::remove_dir_all(&backup_dir).await {
                    // 备份清不掉不影响新版本可用，下次安装/启动会再处理
                    crate::qlog!(
                        "[更新器] 清理旧版本备份失败（忽略）: {}（{}）",
                        backup_dir.display(),
                        e
                    );
                }
            }
            Ok(install_dir.to_path_buf())
        }
        Err(e) => {
            // 回滚：把备份改回正式目录，尽量恢复可运行的旧版本
            if had_existing {
                if let Err(re) = tokio::fs::rename(&backup_dir, install_dir).await {
                    crate::qlog!(
                        "[更新器] 回滚旧版本失败（{} -> {}）: {}",
                        backup_dir.display(),
                        install_dir.display(),
                        re
                    );
                }
            }
            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
            Err(format!(
                "应用新版本失败（{} -> {}）: {}",
                temp_dir.display(),
                install_dir.display(),
                e
            ))
        }
    }
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

/// 交换安装时旧目录的备份前缀。
///
/// 刻意不用 `app_` 开头：`cleanup_old_versions` 只扫 `app_*` 版本目录，
/// `cleanup_pending` 只清 `_del_*`，备份不会被误当成版本或待删残留。
const SWAP_BACKUP_PREFIX: &str = "_bak_";

/// 旧目录备份路径：`app_2.1.2` → `_bak_app_2.1.2`
fn backup_dir_for(install_dir: &Path) -> PathBuf {
    let name = install_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    install_dir.with_file_name(format!("{}{}", SWAP_BACKUP_PREFIX, name))
}

/// 处理上次交换中断留下的备份：
/// - 正式目录不在（崩溃发生在改名之后、新目录就位之前）→ 还原备份
/// - 正式目录在（崩溃发生在删除备份之前）→ 备份是残留，删除
async fn recover_interrupted_swap(install_dir: &Path, backup_dir: &Path) {
    if !backup_dir.exists() {
        return;
    }
    if install_dir.exists() {
        if let Err(e) = tokio::fs::remove_dir_all(backup_dir).await {
            crate::qlog!(
                "[更新器] 清理上次安装残留备份失败（忽略）: {}（{}）",
                backup_dir.display(),
                e
            );
        }
    } else {
        match tokio::fs::rename(backup_dir, install_dir).await {
            Ok(()) => crate::qlog!(
                "[更新器] 已恢复上次中断安装的旧版本: {}",
                install_dir.display()
            ),
            Err(e) => crate::qlog!(
                "[更新器] 恢复上次中断安装的旧版本失败（{} -> {}）: {}",
                backup_dir.display(),
                install_dir.display(),
                e
            ),
        }
    }
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

/// 解压分块大小：与下载/合并一致，兼顾进度粒度与系统调用次数
const EXTRACT_CHUNK_BYTES: usize = 256 * 1024;

/// 解压 zip 到目标目录（阻塞函数，请在 spawn_blocking 中调用）
///
/// 通过 `on_stage` 按条目推送解压进度（第 N/M 项 + 百分比）
fn extract_zip(
    zip_path: &Path,
    dest: &Path,
    limits: ExtractLimits,
    on_stage: &StageCallback,
) -> Result<(), String> {
    extract_zip_with_limits(
        zip_path,
        dest,
        on_stage,
        limits.max_total_bytes,
        limits.max_entry_bytes,
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
/// 路径穿越防护：`enclosed_name` 拒绝 `../` 等非法条目，命中即整包失败
/// （不再静默跳过，避免装出缺文件却标记成功的版本）。
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
    let entry_count = archive.len();
    let mut written: u64 = 0;
    let mut file_count: u64 = 0;

    // 进度分母取中央目录里声明的未压缩总大小（只读条目头，不解压）。
    // 按条目数算进度时，一个 2 GB 的主 exe 就会让进度条卡在「1/28」不动，
    // 用户以为程序挂了；按字节算才能持续前进。
    let declared_total: u64 = (0..entry_count)
        .filter_map(|i| archive.by_index(i).ok().map(|e| e.size()))
        .sum();

    // 声明总量已超上限：整包必然装不下，在写盘/磁盘预检前直接拒绝，
    // 既不用按虚高的声明值去要求磁盘空间，也让错误指向「上限」而不是「磁盘不足」。
    if declared_total > max_total {
        return Err(format!(
            "更新包解压总量超限已中止: 包内声明 {} 字节，上限 {} 字节（可调大 update.json 的 max_extract_total_mb）",
            declared_total, max_total
        ));
    }
    // 解压前的磁盘预检：更新包已经在盘上，额外需要的是解压产物 + 余量。
    // 在写第一个文件前拒绝，比写到一半磁盘满好得多（后者还会被重试两轮）。
    if declared_total > 0 {
        crate::disk::ensure_space(dest, crate::disk::required_for_extract(declared_total))
            .map_err(|e| {
                crate::qlog!("[安装] 解压前预检未通过: {}", e);
                e
            })?;
    }
    // 每 300ms 才推一次进度：几千个小文件时逐文件 set 会把 UI 线程刷爆。
    // 闭包按「已写出字节」算百分比，最后一项/最后一帧由调用方强制推送。
    let mut last_report = Instant::now() - Duration::from_millis(PROGRESS_INTERVAL_MS);
    let mut emit = |written: u64, index: usize, brief: &str, done: bool, force: bool| {
        if !force && last_report.elapsed() < Duration::from_millis(PROGRESS_INTERVAL_MS) {
            return;
        }
        last_report = Instant::now();
        let pct = extract_percent(written, declared_total, index, entry_count, done);
        on_stage(
            &format!(
                "正在解压更新包... {} / {}（{}）",
                index + 1,
                entry_count,
                brief
            ),
            Some(pct),
        );
    };

    for i in 0..entry_count {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("读取更新包条目 {} 失败: {}", i, e))?;

        // 非法路径（防路径穿越）直接判整包失败：静默跳过会装出「缺文件但成功」
        // 的半套版本，启动时才暴露，比当场报错更难排查
        let Some(name) = entry.enclosed_name() else {
            return Err(format!(
                "更新包包含非法路径条目，已中止安装: {}",
                entry.name()
            ));
        };
        let out_path = dest.join(name);
        let brief = out_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let is_dir = entry.is_dir();
        if is_dir {
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

            // 闸 2：流式截断（头部声明可能小于实际数据量）。
            // 分块复制而非 std::io::copy：一个几百 MB 的大文件也要能持续推
            // 字节进度，否则进度条会停在「第 N/M 项」直到整个文件写完。
            let mut limited = entry.take(max_entry + 1);
            let mut buf = vec![0u8; EXTRACT_CHUNK_BYTES];
            let mut copied: u64 = 0;
            loop {
                let n = limited
                    .read(&mut buf)
                    .map_err(|e| format!("解压文件失败 {}: {}", out_path.display(), e))?;
                if n == 0 {
                    break;
                }
                out_file
                    .write_all(&buf[..n])
                    .map_err(|e| format!("解压文件失败 {}: {}", out_path.display(), e))?;
                copied += n as u64;
                written += n as u64;
                // 多写出 1 字节即可判定超限（take 的上限是 max_entry + 1）
                if copied > max_entry {
                    break;
                }
                emit(written, i, &brief, false, false);
            }
            file_count += 1;

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
                    let _ =
                        std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
                }
            }
        }

        // 条目收尾进度：最后一个条目强制推 100%。目录条目不动字节数，
        // 不必单独推（只有它恰好是最后一项时才推）。
        let done = i + 1 == entry_count;
        if !is_dir || done {
            emit(written, i, &brief, done, done);
        }
    }

    // 空包/只有目录的包不能算安装成功：否则会得到一个空版本目录，
    // 更新器随后找不到主程序，用户看到的是「更新完成但启动失败」
    if file_count == 0 {
        return Err("更新包为空或没有可解压的文件，已中止安装".to_string());
    }
    Ok(())
}

/// 解压进度百分比：优先按已写字节 / 声明总量，总量未知时退回按条目数。
///
/// 最后一条强制 100%（声明值可能比实际大，按字节算会停在 99.x）。
fn extract_percent(
    written: u64,
    declared_total: u64,
    index: usize,
    total: usize,
    done: bool,
) -> f64 {
    if done {
        return 100.0;
    }
    if declared_total > 0 {
        (written as f64 / declared_total as f64 * 100.0).min(100.0)
    } else {
        ((index + 1) as f64 / total.max(1) as f64 * 100.0).min(100.0)
    }
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
    fn find_main_falls_back_to_nested_exe() -> io::Result<()> {
        // 分层打包（主程序在 bin/）时，顶层没有 exe 就向下找
        let dir = temp_install_dir("nested")?;
        let sub = dir.join("bin");
        std::fs::create_dir_all(&sub)?;
        make_exe(&sub, "qsm.exe", 100)?;
        let hit = find_main_executable(&dir, "qsm");
        assert_eq!(hit.as_deref().map(stem_of).as_deref(), Some("qsm.exe"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_prefers_top_level_over_nested() -> io::Result<()> {
        // 顶层有候选时不再往下找：顶层的 helper.exe 也比子目录的 qsm.exe 优先
        let dir = temp_install_dir("top_wins")?;
        make_exe(&dir, "helper.exe", 100)?;
        let sub = dir.join("dist");
        std::fs::create_dir_all(&sub)?;
        make_exe(&sub, "qsm.exe", 10_000)?;
        let hit = find_main_executable(&dir, "qsm");
        assert_eq!(hit.as_deref().map(stem_of).as_deref(), Some("helper.exe"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_skips_backup_and_temp_dirs() -> io::Result<()> {
        // `_bak_` 旧版本与 `*_tmp` 解压半成品不参与搜索
        let dir = temp_install_dir("skip_backup")?;
        let bak = dir.join("_bak_app_1.0.0");
        let tmp = dir.join("app_2.0.0_tmp");
        std::fs::create_dir_all(&bak)?;
        std::fs::create_dir_all(&tmp)?;
        make_exe(&bak, "qsm.exe", 10_000)?;
        make_exe(&tmp, "qsm.exe", 20_000)?;
        assert!(find_main_executable(&dir, "qsm").is_none());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn find_main_respects_depth_limit() -> io::Result<()> {
        // 超过 3 层的 exe 不再搜索，避免在资源目录里越走越深
        let dir = temp_install_dir("depth_limit")?;
        let deep = dir.join("a").join("b").join("c").join("d");
        std::fs::create_dir_all(&deep)?;
        make_exe(&deep, "qsm.exe", 100)?;
        assert!(find_main_executable(&dir, "qsm").is_none());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    // ---------- is_retryable_error ----------

    #[test]
    fn retryable_errors_are_exchange_stage_failures() {
        assert!(is_retryable_error("备份旧安装目录失败（a -> b）: 拒绝访问"));
        assert!(is_retryable_error(
            "应用新版本失败（a -> b）: 另一个程序正在使用"
        ));
    }

    #[test]
    fn permanent_install_errors_are_not_retried() {
        // 包非法 / 超限 / 本地写盘失败：重试只会重复同一个结果
        for e in [
            "读取更新包结构失败: invalid Zip archive",
            "更新包条目过大已中止: big.dll（声明 999 字节，上限 100 字节）",
            "更新包解压总量超限已中止: 已写出 200 字节（上限 100 字节）",
            "更新包包含非法路径条目，已中止安装: ../evil.txt",
            "更新包为空或没有可解压的文件，已中止安装",
            "创建临时目录失败 C:\\app\\tmp: 磁盘已满",
            "创建文件失败 C:\\app\\x.exe: 磁盘已满",
            "解压文件失败 C:\\app\\x.exe: 拒绝访问",
        ] {
            assert!(!is_retryable_error(e), "不应重试: {e}");
        }
    }

    // ---------- extract_percent ----------

    #[test]
    fn extract_percent_uses_bytes_not_entry_count() {
        // 1 字节 / 共 100 字节：按条目数会显示 50%，按字节应是 1%
        let pct = extract_percent(1, 100, 0, 2, false);
        assert!((pct - 1.0).abs() < f64::EPSILON, "按字节推进: {pct}");
    }

    #[test]
    fn extract_percent_falls_back_to_entry_count() {
        // 声明总量未知（0）：退回按条目数
        let pct = extract_percent(0, 0, 2, 4, false);
        assert!((pct - 75.0).abs() < f64::EPSILON, "按条目数推进: {pct}");
    }

    #[test]
    fn extract_percent_final_is_always_100() {
        // 声明值偏大导致按字节只到 99.x 时，最后一条强制 100
        assert!((extract_percent(90, 100, 9, 10, true) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn extract_progress_reaches_100() -> io::Result<()> {
        // 端到端：回调至少收到一次，且最后一次为 100%
        let dir = temp_install_dir("progress")?;
        let zip = dir.join("update.zip");
        make_test_zip(&zip, &[("a.txt", b"hello"), ("b.txt", b"world")])?;
        let out = dir.join("out");
        std::fs::create_dir_all(&out)?;
        let seen = Arc::new(std::sync::Mutex::new(Vec::<f64>::new()));
        let sink = seen.clone();
        let cb: StageCallback = Arc::new(move |_msg: &str, pct: Option<f64>| {
            if let Some(p) = pct {
                sink.lock().unwrap_or_else(|e| e.into_inner()).push(p);
            }
        });
        extract_zip(&zip, &out, ExtractLimits::default(), &cb).map_err(io::Error::other)?;
        let seen = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(
            seen.last().copied(),
            Some(100.0),
            "最后一次应为 100%: {seen:?}"
        );
        assert!(
            seen.iter().all(|p| (0.0..=100.0).contains(p)),
            "百分比越界: {seen:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    // ---------- temp_dir_for ----------

    #[test]
    fn temp_dir_appends_tmp_suffix() {
        let d = Path::new("/apps/qsm/app/app_2.1.2");
        assert_eq!(temp_dir_for(d), Path::new("/apps/qsm/app/app_2.1.2_tmp"));
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
    fn extract_limits_from_mb_clamps_entry_to_total() {
        let limits = ExtractLimits::from_mb(1024, 4096);
        assert_eq!(limits.max_total_bytes, 1024 * 1024 * 1024);
        assert_eq!(
            limits.max_entry_bytes, limits.max_total_bytes,
            "单文件上限不得超过总量上限"
        );
        // 溢出防护：极大配置不得回绕成小值
        assert!(ExtractLimits::from_mb(u64::MAX, u64::MAX).max_total_bytes > 0);
    }

    #[test]
    fn extract_normal_within_limits() -> io::Result<()> {
        let dir = temp_install_dir("extract_ok")?;
        let zip = dir.join("update.zip");
        make_test_zip(&zip, &[("a.txt", b"hello"), ("sub/b.bin", &[0u8; 64])])?;
        let out = dir.join("out");
        std::fs::create_dir_all(&out)?;
        extract_zip(&zip, &out, ExtractLimits::default(), &noop_stage())
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
        extract_zip_with_limits(&zip, &out, &noop_stage(), 1_000, 100).map_err(io::Error::other)?;
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
    fn extract_rejects_path_traversal_entry() -> io::Result<()> {
        // 路径穿越条目被 enclosed_name 拒绝时必须整包失败，而不是跳过后续条目
        let dir = temp_install_dir("extract_traversal")?;
        let zip = dir.join("update.zip");
        make_test_zip(&zip, &[("../evil.txt", b"evil"), ("ok.txt", b"fine")])?;
        let out = dir.join("out");
        std::fs::create_dir_all(&out)?;
        let err = extract_zip(&zip, &out, ExtractLimits::default(), &noop_stage())
            .expect_err("含穿越条目的包必须解压失败");
        assert!(
            err.contains("非法路径条目"),
            "错误信息应说明非法路径: {err}"
        );
        assert!(!out.join("evil.txt").exists(), "穿越条目不应写入 dest");
        assert!(!dir.join("evil.txt").exists(), "穿越条目不应写入 dest 之外");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn extract_rejects_empty_archive() -> io::Result<()> {
        // 空包不能算安装成功：否则会留下一个没有主程序的版本目录
        let dir = temp_install_dir("extract_empty")?;
        let zip = dir.join("update.zip");
        make_test_zip(&zip, &[])?;
        let out = dir.join("out");
        std::fs::create_dir_all(&out)?;
        let err = extract_zip(&zip, &out, ExtractLimits::default(), &noop_stage())
            .expect_err("空包必须解压失败");
        assert!(err.contains("为空"), "错误信息应说明空包: {err}");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    // ---------- 交换安装 ----------

    #[test]
    fn backup_dir_uses_non_version_prefix() {
        // 备份目录不能以 app_ 开头，否则会被 cleanup_old_versions 当成版本目录
        let b = backup_dir_for(Path::new("/apps/qsm/app/app_2.1.2"));
        assert_eq!(b, Path::new("/apps/qsm/app/_bak_app_2.1.2"));
    }

    #[tokio::test]
    async fn install_failure_keeps_existing_version() -> io::Result<()> {
        // 回归：解压失败时不得动旧目录（旧实现会先删掉同版本目录）
        let dir = temp_install_dir("install_keep_old")?;
        let install_dir = dir.join("app_9.9.9");
        std::fs::create_dir_all(&install_dir)?;
        std::fs::write(install_dir.join("old.exe"), b"old")?;
        let bad_zip = dir.join("bad.zip");
        std::fs::write(&bad_zip, b"not a zip")?;

        let err = install(
            &bad_zip,
            &install_dir,
            ExtractLimits::default(),
            noop_stage(),
        )
        .await
        .expect_err("损坏包必须安装失败");
        assert!(!err.is_empty());
        assert_eq!(
            std::fs::read(install_dir.join("old.exe"))?,
            b"old",
            "解压失败后旧版本必须原样保留"
        );
        assert!(!temp_dir_for(&install_dir).exists(), "临时目录应清理");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[tokio::test]
    async fn install_replaces_existing_version_and_removes_backup() -> io::Result<()> {
        let dir = temp_install_dir("install_swap")?;
        let install_dir = dir.join("app_9.9.9");
        std::fs::create_dir_all(&install_dir)?;
        std::fs::write(install_dir.join("old.exe"), b"old")?;
        let zip = dir.join("update.zip");
        make_test_zip(&zip, &[("new.exe", b"new")])?;

        install(&zip, &install_dir, ExtractLimits::default(), noop_stage())
            .await
            .expect("正常包应安装成功");
        assert_eq!(std::fs::read(install_dir.join("new.exe"))?, b"new");
        assert!(!install_dir.join("old.exe").exists(), "旧目录内容不应残留");
        assert!(!backup_dir_for(&install_dir).exists(), "成功后备份应删除");
        assert!(
            !temp_dir_for(&install_dir).exists(),
            "临时目录应改名为正式目录"
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[tokio::test]
    async fn recover_interrupted_swap_restores_missing_install() -> io::Result<()> {
        // 模拟「旧目录已改名、新目录尚未就位」时崩溃：下次安装前应还原
        let dir = temp_install_dir("recover_swap")?;
        let install_dir = dir.join("app_9.9.9");
        let backup = backup_dir_for(&install_dir);
        std::fs::create_dir_all(&backup)?;
        std::fs::write(backup.join("old.exe"), b"old")?;

        recover_interrupted_swap(&install_dir, &backup).await;
        assert_eq!(std::fs::read(install_dir.join("old.exe"))?, b"old");
        assert!(!backup.exists(), "还原后备份目录应消失");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[tokio::test]
    async fn recover_interrupted_swap_removes_stale_backup() -> io::Result<()> {
        // 正式目录已就位、备份残留：直接清理备份，不影响新版本
        let dir = temp_install_dir("recover_stale")?;
        let install_dir = dir.join("app_9.9.9");
        std::fs::create_dir_all(&install_dir)?;
        std::fs::write(install_dir.join("new.exe"), b"new")?;
        let backup = backup_dir_for(&install_dir);
        std::fs::create_dir_all(&backup)?;
        std::fs::write(backup.join("old.exe"), b"old")?;

        recover_interrupted_swap(&install_dir, &backup).await;
        assert_eq!(std::fs::read(install_dir.join("new.exe"))?, b"new");
        assert!(!backup.exists(), "残留备份应被删除");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
