//! 更新包下载：流式写入 + 实时进度回调 + SHA256 校验

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

/// 下载失败错误信息的统一前缀
///
/// 单包/分卷下载与合并的所有失败都以此开头，编排层据此判断「是不是
/// 下载环节挂了」，从而在错误态给出「前往官网」入口。校验不匹配、磁盘
/// 写失败等本地问题不带这个前缀 —— 换个地址重试能解决，跳官网没用。
pub const DOWNLOAD_FAIL_PREFIX: &str = "下载失败: ";

/// 本地文件读写的分块大小（SHA256 校验与分卷合并共用）
const CHUNK_BYTES: usize = 256 * 1024;

/// 下载进度信息
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    /// 已下载字节数
    pub downloaded: u64,
    /// 总字节数（服务器未返回时为 None）
    pub total: Option<u64>,
}

impl DownloadProgress {
    /// 进度百分比（总大小未知时返回 None）
    pub fn percent(&self) -> Option<f64> {
        self.total
            .filter(|t| *t > 0)
            .map(|t| (self.downloaded as f64 / t as f64) * 100.0)
    }
}

/// 流式下载文件到指定路径，边下载边计算 SHA256
///
/// - `url`: 下载地址
/// - `dest`: 目标文件路径（父目录需已存在）
/// - `on_progress`: 进度回调（每收到数据块即触发，调用方自行节流）
///
/// 使用共享 Client（连接 10s、无总超时，见 [`crate::client::download_client`]）：
/// 连接池与 TLS 会话跨请求复用。大文件慢速下载可能合理超过任何固定
/// 上限，故不设整体超时——中途断流由下方读空闲超时兜底。
///
/// 返回：(文件路径, SHA256 十六进制值)
pub async fn download_file<F>(
    url: &str,
    dest: &Path,
    on_progress: F,
) -> Result<(PathBuf, String), String>
where
    F: Fn(DownloadProgress) + Send + Sync + 'static,
{
    let response = crate::client::download_client()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("{}请求失败: {}", DOWNLOAD_FAIL_PREFIX, e))?;

    if !response.status().is_success() {
        return Err(format!(
            "{}服务器返回错误状态码: {}",
            DOWNLOAD_FAIL_PREFIX,
            response.status()
        ));
    }

    let total = response.content_length();
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("创建下载文件失败 {}: {}", dest.display(), e))?;

    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;

    // 读空闲超时：60 秒未收到任何新数据视为断流，报错退出（防止无限挂死）
    while let Some(chunk) =
        tokio::time::timeout(std::time::Duration::from_secs(60), stream.next())
            .await
            .map_err(|_| "下载数据流超时（60 秒无新数据）".to_string())?
    {
        let chunk = chunk.map_err(|e| format!("下载数据流中断: {}", e))?;
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入下载文件失败: {}", e))?;
        downloaded += chunk.len() as u64;
        on_progress(DownloadProgress {
            downloaded,
            total,
        });
    }

    file.flush()
        .await
        .map_err(|e| format!("下载文件刷新失败: {}", e))?;
    file.sync_all()
        .await
        .map_err(|e| format!("下载文件落盘失败: {}", e))?;

    Ok((dest.to_path_buf(), hex::encode(hasher.finalize())))
}

/// 分块读取文件算 SHA256（阻塞函数，重活请放进 spawn_blocking）
///
/// 不用 `std::fs::read` 一次性读进内存：更新包动辄上百 MB 乃至 GB 级
/// （线上 qsm 包 2.3 GB），整读等于让更新器在校验那一刻多吃一整个文件的
/// RSS，还可能直接把低配机器打到 swap。这里固定 256 KB 缓冲流式累加。
pub fn sha256_of_file(file: &Path) -> Result<String, String> {
    use std::io::Read;

    let handle = std::fs::File::open(file).map_err(|e| format!("打开文件失败 {}: {}", file.display(), e))?;
    let mut reader = std::io::BufReader::with_capacity(CHUNK_BYTES, handle);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK_BYTES];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("读取文件失败 {}: {}", file.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 校验文件 SHA256 是否与期望值一致（期望值为空时跳过校验直接通过）
///
/// 「留空即跳过」是刻意保留的底层语义：调用方分两类——单包/分卷的逐卷校验
/// 只在给了校验值时才走到这里，而**是否允许「没给校验值」由编排层的
/// `require_sha256` 闸门决定**（见 [`crate::flow`]），校验工具本身不持政策。
/// 需要「无校验值就拒装」的策略时，请在闸门处拦，别在这里放宽。
pub fn verify_sha256(file: &Path, expected: &Option<String>) -> Result<(), String> {
    let Some(expected) = expected.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };

    let actual = sha256_of_file(file)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "SHA256 校验不匹配，期望: {}，实际: {}",
            expected, actual
        ))
    }
}

/// 分卷下载结果
pub struct PartsDownload {
    /// 已下载（或复用）的分卷文件路径，按序排列
    pub part_paths: Vec<PathBuf>,
}

/// 逐卷下载分卷文件（已存在且校验通过的卷直接复用）
///
/// - `urls`: 分卷地址列表（按序）
/// - `sha256s`: 各分卷期望 SHA256（可选，None 或与 urls 不等长时跳过逐卷校验）
/// - `dir`: 分卷保存目录（不存在会自动创建）
/// - `total_size`: 整体总字节数（可选，来自 manifest.size_bytes，用于整体进度计算）
/// - `on_part`: 进度回调 (当前卷索引, 总卷数, 整体字节进度)
pub async fn download_parts<F>(
    urls: &[String],
    sha256s: Option<&[String]>,
    dir: &Path,
    total_size: Option<u64>,
    on_part: F,
) -> Result<PartsDownload, String>
where
    F: Fn(usize, usize, DownloadProgress) + Send + Sync + 'static,
{
    if urls.is_empty() {
        return Err(format!("{}分卷地址列表为空", DOWNLOAD_FAIL_PREFIX));
    }
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| format!("创建分卷目录失败 {}: {}", dir.display(), e))?;

    let total = urls.len();
    let on_part = std::sync::Arc::new(on_part);
    let mut part_paths = Vec::with_capacity(total);
    let mut completed_bytes: u64 = 0;

    for (i, url) in urls.iter().enumerate() {
        let part_path = dir.join(format!("part_{:03}", i + 1));
        let expected = sha256s.and_then(|s| s.get(i).cloned());

        // 已存在且校验通过的卷直接复用（断点续传思想）。
        // 前提：manifest 提供了该卷的非空 SHA256（part_sha256）——
        // 无校验值时 verify_sha256 会直接放行，等于把上次中断的半包当
        // 有效卷复用；整体 sha256 也要等合并完才能发现损坏，代价更大。
        // 故此处缺校验值宁可重下，也不复用未校验文件。
        let reusable = expected
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        if part_path.exists() && reusable {
            let cached = part_path.clone();
            let exp = expected.clone();
            let ok = tokio::task::spawn_blocking(move || verify_sha256(&cached, &exp))
                .await
                .unwrap_or_else(|e| Err(format!("校验任务异常: {}", e)))
                .is_ok();
            if ok {
                let part_len = tokio::fs::metadata(&part_path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                completed_bytes += part_len;
                on_part(
                    i,
                    total,
                    DownloadProgress {
                        downloaded: completed_bytes,
                        total: total_size,
                    },
                );
                eprintln!(
                    "[下载] 复用分卷 {}/{}: {}",
                    i + 1,
                    total,
                    part_path.display()
                );
                part_paths.push(part_path);
                continue;
            }
            eprintln!("[下载] 分卷 {}/{} 校验不通过，重新下载", i + 1, total);
            let _ = tokio::fs::remove_file(&part_path).await;
        }

        // 下载该卷，字节进度透传累加为整体进度
        let base = completed_bytes;
        on_part(
            i,
            total,
            DownloadProgress {
                downloaded: base,
                total: total_size,
            },
        );
        let cb = {
            let on_part = on_part.clone();
            move |p: DownloadProgress| {
                on_part(
                    i,
                    total,
                    DownloadProgress {
                        downloaded: base + p.downloaded,
                        total: total_size,
                    },
                );
            }
        };
        download_file(url, &part_path, cb)
            .await
            .map_err(|e| {
                // e 已带统一前缀，这里只补卷号定位
                let detail = e.strip_prefix(DOWNLOAD_FAIL_PREFIX).unwrap_or(&e);
                format!(
                    "{}分卷 {}/{} {}",
                    DOWNLOAD_FAIL_PREFIX,
                    i + 1,
                    total,
                    detail
                )
            })?;

        // 逐卷校验
        if let Some(exp) = &expected {
            let cached = part_path.clone();
            let exp = exp.clone();
            let check = tokio::task::spawn_blocking(move || verify_sha256(&cached, &Some(exp)))
                .await
                .unwrap_or_else(|e| Err(format!("校验任务异常: {}", e)));
            if let Err(e) = check {
                return Err(format!("分卷 {}/{} 校验失败: {}", i + 1, total, e));
            }
        }

        let part_len = tokio::fs::metadata(&part_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        completed_bytes += part_len;
        eprintln!("[下载] 分卷 {}/{} 完成: {}", i + 1, total, url);
        part_paths.push(part_path);
    }

    Ok(PartsDownload { part_paths })
}

/// 按序拼接分卷为完整 zip，边拼接边计算 SHA256 并与期望值比对
///
/// - `part_paths`: 分卷路径列表（按序）
/// - `dest`: 合并输出路径
/// - `expected_sha256`: 完整 zip 的期望 SHA256（空则跳过整体校验）
/// - `on_progress`: 合并进度回调（已合并字节, 总字节），供 UI 展示实时进度
pub async fn merge_parts<F>(
    part_paths: &[PathBuf],
    dest: &Path,
    expected_sha256: &Option<String>,
    on_progress: F,
) -> Result<(), String>
where
    F: Fn(u64, u64),
{
    use tokio::io::AsyncReadExt;

    let total_bytes: u64 = {
        let mut sum = 0u64;
        for part in part_paths {
            sum += tokio::fs::metadata(part)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
        }
        sum
    };

    let mut out = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("创建合并文件失败 {}: {}", dest.display(), e))?;
    let mut hasher = Sha256::new();
    let mut merged: u64 = 0;
    on_progress(0, total_bytes);

    for (idx, part) in part_paths.iter().enumerate() {
        let mut file = tokio::fs::File::open(part)
            .await
            .map_err(|e| format!("打开分卷失败 {}: {}", part.display(), e))?;
        let mut buf = vec![0u8; CHUNK_BYTES];
        loop {
            let n = file
                .read(&mut buf)
                .await
                .map_err(|e| format!("读取分卷失败 {}: {}", part.display(), e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n])
                .await
                .map_err(|e| format!("写入合并文件失败: {}", e))?;
            merged += n as u64;
            on_progress(merged, total_bytes);
        }
        eprintln!("[合并] 卷 {}/{} 完成", idx + 1, part_paths.len());
    }
    eprintln!("[合并] 读取循环全部完成，开始 flush");
    out.flush()
        .await
        .map_err(|e| format!("合并文件刷新失败: {}", e))?;
    eprintln!("[合并] flush 完成，开始 sync_all");
    out.sync_all()
        .await
        .map_err(|e| format!("合并文件落盘失败: {}", e))?;
    eprintln!("[合并] sync_all 完成，开始 SHA256 校验");

    // 整体 SHA256 校验
    if let Some(expected) = expected_sha256.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "合并后整体 SHA256 校验不匹配，期望: {}，实际: {}",
                expected, actual
            ));
        }
    }

    Ok(())
}

// 测试里构造失败即用例本身失败，按 clippy 惯例局部放行 expect/unwrap
#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 造一个唯一的临时文件路径（不创建文件）
    fn temp_path(tag: &str, ext: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "qsltd_dl_test_{}_{}_{}.{}",
            tag,
            std::process::id(),
            n,
            ext
        ))
    }

    /// 造一个指定内容的临时文件
    fn write_temp(tag: &str, content: &[u8]) -> std::io::Result<PathBuf> {
        let p = temp_path(tag, "bin");
        std::fs::write(&p, content)?;
        Ok(p)
    }

    /// 文件内容的 SHA256 十六进制
    fn sha_of(content: &[u8]) -> String {
        hex::encode(Sha256::digest(content))
    }

    // ---------- DownloadProgress::percent ----------

    #[test]
    fn percent_none_when_total_unknown() {
        let p = DownloadProgress { downloaded: 100, total: None };
        assert_eq!(p.percent(), None);
    }

    #[test]
    fn percent_none_when_total_zero() {
        // 服务器谎报 Content-Length: 0 时不应除零
        let p = DownloadProgress { downloaded: 100, total: Some(0) };
        assert_eq!(p.percent(), None);
    }

    #[test]
    fn percent_computes_ratio() {
        let p = DownloadProgress { downloaded: 25, total: Some(100) };
        assert_eq!(p.percent(), Some(25.0));
    }

    // ---------- verify_sha256 ----------

    #[test]
    fn verify_passes_on_match() {
        let p = write_temp("match", b"hello").expect("写临时文件");
        assert!(verify_sha256(&p, &Some(sha_of(b"hello"))).is_ok());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn verify_passes_case_insensitive() {
        let p = write_temp("case", b"hello").expect("写临时文件");
        let upper = sha_of(b"hello").to_uppercase();
        assert!(verify_sha256(&p, &Some(upper)).is_ok());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn verify_skipped_when_expected_empty_or_blank() {
        // 期望值为空/空白：跳过校验直接通过（底层工具语义；「缺校验值要不要
        // 拒装」是编排层 require_sha256 闸门的事，不在这里定政策）
        let p = write_temp("empty", b"hello").expect("写临时文件");
        assert!(verify_sha256(&p, &None).is_ok());
        assert!(verify_sha256(&p, &Some(String::new())).is_ok());
        assert!(verify_sha256(&p, &Some("   ".to_string())).is_ok());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn verify_fails_on_mismatch() {
        let p = write_temp("mismatch", b"hello").expect("写临时文件");
        let err = verify_sha256(&p, &Some(sha_of(b"world"))).unwrap_err();
        assert!(err.contains("校验不匹配"), "错误应说明不匹配: {err}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn verify_fails_on_missing_file() {
        let p = temp_path("missing", "bin");
        assert!(verify_sha256(&p, &Some(sha_of(b"x"))).is_err());
    }

    #[test]
    fn verify_matches_across_chunk_boundaries() {
        // 流式分块（256 KB）的正确性回归：文件大小刻意取「整数倍 + 跨块尾巴」，
        // 分块边界算错就不会命中期望值。整读实现能过这个用例纯属巧合，
        // 换成分块后它是唯一的守门人。
        let n = CHUNK_BYTES * 2 + CHUNK_BYTES / 3;
        let payload: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        let p = write_temp("chunked", &payload).expect("写大临时文件");
        assert!(verify_sha256(&p, &Some(sha_of(&payload))).is_ok());
        // 末尾改一个字节必须露头（校验不是只看长度）
        let mut tail = std::fs::read(&p).expect("读回");
        let last = tail.len() - 1;
        tail[last] ^= 0xFF;
        std::fs::write(&p, &tail).expect("改写末字节");
        assert!(verify_sha256(&p, &Some(sha_of(&payload))).is_err());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn sha256_of_file_empty_file() {
        // 空文件的 SHA256 是固定值，顺手验一下分块循环不会 panic
        let p = write_temp("zero_len", b"").expect("写空文件");
        assert_eq!(
            sha256_of_file(&p).expect("空文件应可校验"),
            sha_of(b"")
        );
        let _ = std::fs::remove_file(&p);
    }

    // ---------- merge_parts ----------

    #[tokio::test]
    async fn merge_concatenates_in_order_and_verifies() {
        let p1 = write_temp("part1", b"AAA").expect("写分卷1");
        let p2 = write_temp("part2", b"BBBB").expect("写分卷2");
        let out = temp_path("merged", "zip");
        let expected = sha_of(b"AAABBBB");

        merge_parts(&[p1.clone(), p2.clone()], &out, &Some(expected), |_, _| {})
            .await
            .expect("合并并校验应成功");

        let merged = std::fs::read(&out).expect("读合并产物");
        assert_eq!(merged, b"AAABBBB");
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
        let _ = std::fs::remove_file(&out);
    }

    #[tokio::test]
    async fn merge_fails_when_overall_hash_mismatch() {
        let p1 = write_temp("bad1", b"AAA").expect("写分卷");
        let out = temp_path("badmerged", "zip");
        // 期望值对应别的内容 → 合并成功但整体校验失败
        let err = merge_parts(std::slice::from_ref(&p1), &out, &Some(sha_of(b"ZZZ")), |_, _| {})
            .await
            .unwrap_err();
        assert!(err.contains("整体 SHA256 校验不匹配"), "错误应说明整体不匹配: {err}");
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&out);
    }

    #[tokio::test]
    async fn merge_skips_hash_when_expected_absent() {
        let p1 = write_temp("nohash", b"AAA").expect("写分卷");
        let out = temp_path("nohashmerged", "zip");
        merge_parts(std::slice::from_ref(&p1), &out, &None, |_, _| {})
            .await
            .expect("无期望值时应跳过整体校验");
        assert_eq!(std::fs::read(&out).expect("读合并产物"), b"AAA");
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&out);
    }

    #[tokio::test]
    async fn merge_reports_progress_monotonic() {
        let p1 = write_temp("prog1", b"AAA").expect("写分卷1");
        let p2 = write_temp("prog2", b"BBB").expect("写分卷2");
        let out = temp_path("progmerged", "zip");
        let seen = std::sync::Mutex::new(Vec::new());
        merge_parts(&[p1.clone(), p2.clone()], &out, &None, |done, total| {
            seen.lock().unwrap_or_else(|e| e.into_inner()).push((done, total));
        })
        .await
        .expect("合并应成功");
        let seen = seen.into_inner().unwrap_or_else(|e| e.into_inner());
        assert_eq!(seen.first().copied(), Some((0, 6)), "应从 0 开始");
        assert_eq!(seen.last().copied(), Some((6, 6)), "应到总量");
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "进度应单调不减");
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
        let _ = std::fs::remove_file(&out);
    }

    #[tokio::test]
    async fn merge_fails_on_missing_part() {
        let missing = temp_path("ghostpart", "bin");
        let out = temp_path("ghostmerged", "zip");
        assert!(merge_parts(&[missing], &out, &None, |_, _| {}).await.is_err());
    }
}
