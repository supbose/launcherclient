//! 更新包下载：流式写入 + 实时进度回调 + SHA256 校验

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use reqwest::header::{CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};

/// 下载失败错误信息的统一前缀
///
/// 单包/分卷下载与合并的所有失败都以此开头，编排层据此判断「是不是
/// 下载环节挂了」，从而在错误态给出「前往官网」入口。校验不匹配、磁盘
/// 写失败等本地问题不带这个前缀 —— 换个地址重试能解决，跳官网没用。
pub const DOWNLOAD_FAIL_PREFIX: &str = "下载失败: ";

/// 本地文件读写的分块大小（SHA256 校验与分卷合并共用）
const CHUNK_BYTES: usize = 256 * 1024;

/// 断点续传的半成品后缀：目标文件旁边 `xxx.zip.part`
///
/// 下载过程只写这个文件，完整收尾后才改名为正式目标。这样任何时刻
/// 正式目标要么不存在、要么是完整数据，`try_reuse_zip` 不会拿到半包。
const PART_SUFFIX: &str = ".part";

/// 半成品的续传元数据后缀（记录校验器与总长）
const PART_META_SUFFIX: &str = ".part.meta";

/// 半包续传元数据
///
/// 只有拿到**强校验器**（ETag 非弱、或 Last-Modified）才允许续传：
/// `If-Range` 用它确认「远端还是同一份内容」，否则续上的前缀可能属于
/// 旧版本包，最终 SHA256 必然失败，还会在每次重试里重复同一个错误。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PartialMeta {
    /// 下载地址：换了地址（故障转移/镜像切换）不复用旧半包
    url: String,
    /// `If-Range` 的取值（强 ETag 或 Last-Modified）
    validator: String,
    /// 服务器声明的总字节数（能拿到时填，用于判断是否已下完）
    total: Option<u64>,
}

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
        percent_of(self.downloaded, self.total)
    }
}

/// 已下载字节 / 总量 → 百分比（总量未知、为 0 或越界时封顶 100）
///
/// 服务器不给 `Content-Length` 时，调用方可以把自己的已知总长
/// （manifest `size_bytes`）传进来，界面不会只剩字节数、没有百分比。
pub fn percent_of(downloaded: u64, total: Option<u64>) -> Option<f64> {
    total
        .filter(|t| *t > 0)
        .map(|t| (downloaded as f64 / t as f64 * 100.0).min(100.0))
}

/// 流式下载文件到指定路径，边下载边计算 SHA256，支持断点续传
///
/// - `url`: 下载地址
/// - `dest`: 目标文件路径（父目录需已存在）
/// - `on_progress`: 进度回调（每收到数据块即触发，调用方自行节流）
///
/// 数据先写 `dest.part`，完整收尾后才改名为 `dest`；中断时半包与
/// `dest.part.meta` 保留，下一次调用用 `Range` + `If-Range` 从断点续传
/// （见 [`PartialMeta`]）。服务端不支持 Range 时自动回落全量下载。
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
    let part = with_suffix(dest, PART_SUFFIX);
    let meta_path = with_suffix(dest, PART_META_SUFFIX);

    let mut resume = load_resume(url, &part, &meta_path).await;
    // 上次已下完、只差改名（改名失败/崩溃窗口）：直接算哈希交付，不再联网
    if let Some((meta, len)) = &resume {
        if meta.total == Some(*len) {
            let (hasher, hashed) = hash_existing(&part).await?;
            if hashed == *len {
                crate::qlog!("[下载] 发现已下完的半包，直接收尾: {}", part.display());
                finish_part(&part, &meta_path, dest).await?;
                return Ok((dest.to_path_buf(), hex::encode(hasher.finalize())));
            }
            // 读取期间长度又变了：交给下面的正常续传/全量流程重新对齐
        }
    }

    // Range 请求被 416 拒绝时只允许「丢弃半包重来」一次，防异常服务端死循环
    let mut retried_without_range = false;

    loop {
        let offset = resume.as_ref().map(|(_, n)| *n).unwrap_or(0);
        let mut request = crate::client::download_client().get(url);
        if let Some((meta, _)) = &resume {
            request = request
                .header(RANGE, format!("bytes={}-", offset))
                .header(IF_RANGE, meta.validator.clone());
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("{}请求失败: {}", DOWNLOAD_FAIL_PREFIX, e))?;
        let status = response.status();

        // 416：远端内容比半包短（或校验器已失效），旧半包没有续传价值
        if offset > 0 && status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            if retried_without_range {
                return Err(format!(
                    "{}服务器拒绝续传位置（416），已重试全量下载仍失败",
                    DOWNLOAD_FAIL_PREFIX
                ));
            }
            crate::qlog!("[下载] 续传位置被服务器拒绝（416），丢弃半包重新下载");
            remove_quietly(&part).await;
            remove_quietly(&meta_path).await;
            resume = None;
            retried_without_range = true;
            continue;
        }

        if !status.is_success() {
            return Err(format!(
                "{}服务器返回错误状态码: {}",
                DOWNLOAD_FAIL_PREFIX, status
            ));
        }

        // 只有 206 才代表服务器按 Range 续传；200 表示忽略 Range 或校验器
        // 不匹配——按全量从头写。206 的起点必须等于请求的偏移，否则同样全量。
        let mut is_resume = offset > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
        let mut range_total = None;
        if is_resume {
            match response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range)
            {
                Some((start, total)) if start == offset => range_total = total,
                _ => {
                    crate::qlog!("[下载] Content-Range 不可用，改为全量下载");
                    is_resume = false;
                }
            }
        }

        // 续传前重读已存前缀喂哈希：最终 SHA256 必须覆盖整个文件。
        // 长度与请求时不一致（文件被改动）则放弃本次续传，按全量重来。
        let (offset, mut hasher) = if is_resume {
            let (hasher, len) = hash_existing(&part).await?;
            if len == offset {
                (offset, hasher)
            } else if retried_without_range {
                return Err(format!(
                    "{}续传半包长度异常（期望 {}，实际 {}）",
                    DOWNLOAD_FAIL_PREFIX, offset, len
                ));
            } else {
                crate::qlog!(
                    "[下载] 续传半包长度变化（{} → {}），丢弃后全量下载",
                    offset,
                    len
                );
                remove_quietly(&part).await;
                remove_quietly(&meta_path).await;
                resume = None;
                retried_without_range = true;
                continue;
            }
        } else {
            (0, Sha256::new())
        };

        let content_len = response.content_length();
        let total = if is_resume {
            range_total
                .or_else(|| resume.as_ref().and_then(|(m, _)| m.total))
                .or_else(|| content_len.map(|len| offset + len))
        } else {
            content_len
        };

        // 记录校验器，供下一次中断后续传。没有强校验器时不写 meta，
        // 下次只会全量重下（宁多重下，不拿错前缀去赌）。
        match validator_from(response.headers()) {
            Some(validator) => {
                let meta = PartialMeta {
                    url: url.to_string(),
                    validator,
                    total,
                };
                save_meta(&meta_path, &meta).await;
            }
            None => remove_quietly(&meta_path).await,
        }

        let file = if is_resume {
            tokio::fs::OpenOptions::new().append(true).open(&part).await
        } else {
            tokio::fs::File::create(&part).await
        }
        .map_err(|e| format!("创建下载文件失败 {}: {}", part.display(), e))?;
        // reqwest 的数据块通常只有几十 KB，逐块 write_all 会产生大量系统调用；
        // 256 KB 缓冲把写盘次数降一个数量级，进度与哈希仍按原始块计算。
        let mut file = BufWriter::with_capacity(CHUNK_BYTES, file);
        let mut stream = response.bytes_stream();

        let mut written: u64 = offset;
        // 续传时先报一次当前位置：界面不会从 0% 重新爬
        on_progress(DownloadProgress {
            downloaded: written,
            total,
        });

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
            written += chunk.len() as u64;
            on_progress(DownloadProgress {
                downloaded: written,
                total,
            });
        }

        // 只做用户态缓冲落盘，不再 fsync：数据完整性由 SHA256 关卡保证——
        // 崩溃后残留的包要么在复用时整读校验（不匹配就重下），要么根本不被复用
        // （无校验值时禁用复用）。GB 级文件的 FlushFileBuffers 在机械盘上要十几秒，
        // 却换不来任何安装正确性。
        file.flush()
            .await
            .map_err(|e| format!("下载文件刷新失败: {}", e))?;

        // 服务器给了总量却少给字节，说明连接被中途掐断（否则流会报错）。
        // 半包保留，下一次调用从断点续传。
        if let Some(total) = total {
            if written < total {
                return Err(format!(
                    "{}下载数据不完整（已收 {} / {} 字节）",
                    DOWNLOAD_FAIL_PREFIX, written, total
                ));
            }
        }

        finish_part(&part, &meta_path, dest).await?;
        return Ok((dest.to_path_buf(), hex::encode(hasher.finalize())));
    }
}

/// 追加后缀（`update.zip` → `update.zip.part`），不用 with_extension：
/// 那会把 `2.1.2` 之类的点号吃掉
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// 读取可用的续传半包；不可用时（无强校验器、地址不符、半包损坏）返回 None
///
/// 顺带清掉孤儿半包：没有 meta 的 `.part` 无法确认来源，留着只会占盘。
async fn load_resume(url: &str, part: &Path, meta_path: &Path) -> Option<(PartialMeta, u64)> {
    if !part.exists() {
        remove_quietly(meta_path).await;
        return None;
    }
    let meta = match tokio::fs::read(meta_path).await {
        Ok(bytes) => serde_json::from_slice::<PartialMeta>(&bytes).ok(),
        Err(_) => None,
    };
    let Some(meta) = meta.filter(|m| m.url == url && !m.validator.trim().is_empty()) else {
        crate::qlog!(
            "[下载] 半包缺少可用续传信息，丢弃后重新下载: {}",
            part.display()
        );
        remove_quietly(part).await;
        remove_quietly(meta_path).await;
        return None;
    };
    let len = tokio::fs::metadata(part).await.map(|m| m.len()).ok()?;
    if len == 0 {
        remove_quietly(part).await;
        remove_quietly(meta_path).await;
        return None;
    }
    if let Some(total) = meta.total {
        if len > total {
            crate::qlog!(
                "[下载] 半包大于声明的总大小（{} > {}），丢弃后重新下载",
                len,
                total
            );
            remove_quietly(part).await;
            remove_quietly(meta_path).await;
            return None;
        }
    }
    Some((meta, len))
}

/// 流式重读已有半包并返回 SHA256 状态（续传时把前缀喂给哈希）
async fn hash_existing(path: &Path) -> Result<(Sha256, u64), String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("打开半包失败 {}: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut read: u64 = 0;
    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| format!("读取半包失败 {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        read += n as u64;
    }
    Ok((hasher, read))
}

/// 写入续传元数据（失败仅告警：最坏只是下次全量重下）
async fn save_meta(path: &Path, meta: &PartialMeta) {
    let Ok(json) = serde_json::to_vec(meta) else {
        return;
    };
    if let Err(e) = tokio::fs::write(path, json).await {
        crate::qlog!("[下载] 续传元数据写入失败（忽略）: {}", e);
    }
}

/// 半包收尾：改名为正式目标并删除元数据
///
/// 失败时半包与 meta 原样保留——下次调用会识别「长度已够」直接改名，
/// 不必重下整包。改名失败多为目标被占用，属于本地问题。
async fn finish_part(part: &Path, meta_path: &Path, dest: &Path) -> Result<(), String> {
    // Windows 的 rename 不覆盖已存在文件，先删掉旧目标
    if dest.exists() {
        tokio::fs::remove_file(dest)
            .await
            .map_err(|e| format!("删除旧下载文件失败 {}: {}", dest.display(), e))?;
    }
    tokio::fs::rename(part, dest)
        .await
        .map_err(|e| format!("重命名下载文件失败 {}: {}", dest.display(), e))?;
    remove_quietly(meta_path).await;
    Ok(())
}

/// 删除文件，失败静默（清理只是尽力而为）
async fn remove_quietly(path: &Path) {
    let _ = tokio::fs::remove_file(path).await;
}

/// 解析 `Content-Range: bytes 100-499/1234` → (起点, 总长)
///
/// 总长可以是 `*`（未知），起点必须能解析；格式不对返回 None，
/// 调用方据此退回全量下载。
fn parse_content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end: u64 = end.trim().parse().ok()?;
    if end < start {
        return None;
    }
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().ok()?),
    };
    Some((start, total))
}

/// 取强校验器：非弱 ETag 优先，其次 Last-Modified
///
/// 弱 ETag（`W/...`）按 RFC 不能用于 `If-Range`（它只保证语义等价，
/// 不保证字节一致），续传拿到错前缀最终只会卡在 SHA256。
fn validator_from(headers: &reqwest::header::HeaderMap) -> Option<String> {
    if let Some(etag) = headers.get(ETAG).and_then(|v| v.to_str().ok()) {
        let etag = etag.trim();
        if !etag.is_empty() && !etag.starts_with("W/") {
            return Some(etag.to_string());
        }
    }
    headers
        .get(LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 分块读取文件算 SHA256（阻塞函数，重活请放进 spawn_blocking）
///
/// 不用 `std::fs::read` 一次性读进内存：更新包动辄上百 MB 乃至 GB 级
/// （线上 qsm 包 2.3 GB），整读等于让更新器在校验那一刻多吃一整个文件的
/// RSS，还可能直接把低配机器打到 swap。这里固定 256 KB 缓冲流式累加。
pub fn sha256_of_file(file: &Path) -> Result<String, String> {
    use std::io::Read;

    let handle =
        std::fs::File::open(file).map_err(|e| format!("打开文件失败 {}: {}", file.display(), e))?;
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
    compare_sha256(expected, &actual)
}

/// 比对已经算好的 SHA256 与期望值（大小写不敏感，期望值前后空白先 trim）。
///
/// 供「下载/合并时已流式算出哈希」的调用方复用，避免为了校验再把 GB 级
/// 文件整读一遍。不匹配属于下载链路问题，错误带 [`DOWNLOAD_FAIL_PREFIX`]，
/// 错误态因此能给出「前往官网重新下载」出口。
pub fn compare_sha256(expected: &str, actual: &str) -> Result<(), String> {
    let expected = expected.trim();
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "{}SHA256 校验不匹配，期望: {}，实际: {}",
            DOWNLOAD_FAIL_PREFIX, expected, actual
        ))
    }
}

/// 从「服务器返回错误状态码: 404 Not Found」里取回状态码（仅供重试判定）
fn http_status_code(err: &str) -> Option<u16> {
    const TAG: &str = "服务器返回错误状态码: ";
    err.split_once(TAG)?
        .1
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// 这个下载错误换同一个地址再试一次是否有意义
///
/// - HTTP 4xx（除 408 请求超时、429 限流）是确定性拒绝：404/403/410 重试同址
///   必然复现，只会白等退避时间。
/// - 本地磁盘/权限类失败（见 [`is_local_io_error`]）自动重试解决不了。
/// - 其余（连接失败、5xx、数据流中断/超时、SHA 不匹配等）按可重试处理。
pub fn is_retryable(err: &str) -> bool {
    if let Some(code) = http_status_code(err) {
        return !(400..500).contains(&code) || code == 408 || code == 429;
    }
    !is_local_io_error(err)
}

/// 是否本地磁盘/权限类失败（下载写入、合并输出、读取本地分卷）
///
/// 这类失败既不该自动重试，也不该在界面引导用户「前往官网重新下载」——
/// 换了地址照样写不进同一块磁盘。按包含匹配：分卷模式会在错误里插入
/// 「分卷 x/y」中缀，前缀判断会漏。
pub fn is_local_io_error(err: &str) -> bool {
    const LOCAL: &[&str] = &[
        "创建下载文件失败",
        "写入下载文件失败",
        "下载文件刷新失败",
        "创建分卷目录失败",
        "创建合并文件失败",
        "写入合并文件失败",
        "合并文件刷新失败",
        "打开分卷失败",
        "读取分卷失败",
        // 断点续传的本地读写与收尾改名
        "打开半包失败",
        "读取半包失败",
        "重命名下载文件失败",
        "删除旧下载文件失败",
        // 磁盘空间预检（重试/换地址都解决不了）
        "磁盘空间不足",
    ];
    LOCAL.iter().any(|m| err.contains(m))
}

/// 分卷下载结果
pub struct PartsDownload {
    /// 已下载（或复用）的分卷文件路径，按序排列
    pub part_paths: Vec<PathBuf>,
}

/// 分卷并发下载的进度聚合器
///
/// 并发下载时每卷只知道自己下到哪：整体进度 = 所有卷当前字节之和，
/// 已完成卷数单独计数给界面做「分卷 x/y」徽标。单卷续传/回落全量会让
/// 自己的字节数回退，这里用 `fetch_max` 保持整体进度单调，避免进度条
/// 来回跳。
struct PartsProgress<F>
where
    F: Fn(usize, usize, DownloadProgress) + Send + Sync,
{
    total_parts: usize,
    total_bytes: Option<u64>,
    bytes: Vec<AtomicU64>,
    done: AtomicUsize,
    on_part: Arc<F>,
}

impl<F> PartsProgress<F>
where
    F: Fn(usize, usize, DownloadProgress) + Send + Sync,
{
    fn new(total_parts: usize, total_bytes: Option<u64>, on_part: Arc<F>) -> Self {
        let bytes = (0..total_parts).map(|_| AtomicU64::new(0)).collect();
        Self {
            total_parts,
            total_bytes,
            bytes,
            done: AtomicUsize::new(0),
            on_part,
        }
    }

    fn report(&self) {
        let downloaded: u64 = self.bytes.iter().map(|b| b.load(Ordering::Relaxed)).sum();
        (self.on_part)(
            self.done.load(Ordering::Relaxed),
            self.total_parts,
            DownloadProgress {
                downloaded,
                total: self.total_bytes,
            },
        );
    }

    /// 单卷收到新数据（`downloaded` 已含该卷的续传偏移）
    fn update(&self, index: usize, downloaded: u64) {
        if let Some(slot) = self.bytes.get(index) {
            slot.fetch_max(downloaded, Ordering::Relaxed);
        }
        self.report();
    }

    /// 单卷完成：以落盘长度为准并递增完成计数
    fn complete(&self, index: usize, len: u64) {
        if let Some(slot) = self.bytes.get(index) {
            slot.store(len, Ordering::Relaxed);
        }
        self.done.fetch_add(1, Ordering::Relaxed);
        self.report();
    }
}

/// 并发下载分卷文件（已存在且校验通过的卷直接复用）
///
/// - `urls`: 分卷地址列表（按序）
/// - `sha256s`: 各分卷期望 SHA256（可选，None 或与 urls 不等长时跳过逐卷校验）
/// - `dir`: 分卷保存目录（不存在会自动创建）
/// - `total_size`: 整体总字节数（可选，来自 manifest.size_bytes，用于整体进度计算）
/// - `concurrency`: 同时下载的卷数（调用方已归一，本函数再夹一次 1~8 兜底）
/// - `on_part`: 进度回调 (已完成卷数, 总卷数, 整体字节进度)
///
/// 并发只影响「同时在飞」的卷数：每卷仍各自落盘到 `part_XXX`，按索引回填，
/// 合并顺序不受影响。单卷失败会保留 `.part`，外层重试时从断点续传。
pub async fn download_parts<F>(
    urls: &[String],
    sha256s: Option<&[String]>,
    dir: &Path,
    total_size: Option<u64>,
    concurrency: usize,
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
    let progress = Arc::new(PartsProgress::new(total, total_size, Arc::new(on_part)));
    let mut part_paths: Vec<Option<PathBuf>> = vec![None; total];
    let mut pending: Vec<usize> = Vec::new();

    // 第一轮：复用已存在且校验通过的卷（逐卷校验是磁盘读，串行更稳，
    // 也不会和后续下载抢带宽/磁头）
    for (i, _url) in urls.iter().enumerate() {
        let part_path = dir.join(format!("part_{:03}", i + 1));
        let expected = sha256s.and_then(|s| s.get(i).cloned());

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
                progress.complete(i, part_len);
                crate::qlog!(
                    "[下载] 复用分卷 {}/{}: {}",
                    i + 1,
                    total,
                    part_path.display()
                );
                part_paths[i] = Some(part_path);
                continue;
            }
            crate::qlog!("[下载] 分卷 {}/{} 校验不通过，重新下载", i + 1, total);
            let _ = tokio::fs::remove_file(&part_path).await;
        }
        pending.push(i);
    }

    // 第二轮：剩余卷按配置并发下载
    let concurrency = concurrency.clamp(1, 8).min(total.max(1));
    progress.report();

    let tasks = futures_util::stream::iter(pending)
        .map(|i| {
            let progress = progress.clone();
            let part_path = dir.join(format!("part_{:03}", i + 1));
            let expected = sha256s.and_then(|s| s.get(i).cloned());
            let url = urls[i].clone();
            async move {
                let (_, part_sha) = {
                    let progress = progress.clone();
                    let cb = move |p: DownloadProgress| progress.update(i, p.downloaded);
                    download_file(&url, &part_path, cb).await.map_err(|e| {
                        // 本地落盘失败不带下载前缀：编排层据此既不重试、也不把用户
                        // 往官网引（换地址写不进同一块磁盘），这里只补卷号定位。
                        let detail = e.strip_prefix(DOWNLOAD_FAIL_PREFIX).unwrap_or(&e);
                        if is_local_io_error(&e) {
                            format!("分卷 {}/{} {}", i + 1, total, detail)
                        } else {
                            format!(
                                "{}分卷 {}/{} {}",
                                DOWNLOAD_FAIL_PREFIX,
                                i + 1,
                                total,
                                detail
                            )
                        }
                    })?
                };

                // 逐卷校验：直接用下载时流式算出的哈希比对，不再把整卷读第二遍
                // （2.3 GB 包按 28 卷算，省掉的就是一次全量读盘）。
                if let Some(exp) = expected.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    if let Err(e) = compare_sha256(exp, &part_sha) {
                        let detail = e.strip_prefix(DOWNLOAD_FAIL_PREFIX).unwrap_or(&e);
                        return Err(format!(
                            "{}分卷 {}/{} 校验失败: {}",
                            DOWNLOAD_FAIL_PREFIX,
                            i + 1,
                            total,
                            detail
                        ));
                    }
                }

                let part_len = tokio::fs::metadata(&part_path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                progress.complete(i, part_len);
                crate::qlog!("[下载] 分卷 {}/{} 完成: {}", i + 1, total, url);
                Ok::<(usize, PathBuf), String>((i, part_path))
            }
        })
        .buffer_unordered(concurrency);

    futures_util::pin_mut!(tasks);
    while let Some(done) = tasks.next().await {
        // 一卷失败就收手：其余在飞的卷会在 drop 时停下，已落盘的字节保留，
        // 外层重试从断点续传，不必从头再来
        let (i, path) = done?;
        part_paths[i] = Some(path);
    }

    let part_paths = part_paths
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| format!("{}有分卷未能完成下载，请重试", DOWNLOAD_FAIL_PREFIX))?;
    Ok(PartsDownload { part_paths })
}

/// 按序拼接分卷为完整 zip，边拼接边计算 SHA256 并与期望值比对
///
/// - `part_paths`: 分卷路径列表（按序）
/// - `dest`: 合并输出路径
/// - `expected_sha256`: 完整 zip 的期望 SHA256（空则跳过整体校验）
/// - `on_progress`: 合并进度回调（已合并字节, 总字节），供 UI 展示实时进度
///
/// 返回合并产物的实际 SHA256（十六进制）。调用方可直接拿它与清单比对，
/// 不必再读一遍 GB 级文件。
pub async fn merge_parts<F>(
    part_paths: &[PathBuf],
    dest: &Path,
    expected_sha256: &Option<String>,
    on_progress: F,
) -> Result<String, String>
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

    let out = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("创建合并文件失败 {}: {}", dest.display(), e))?;
    // 输出侧同样走 256 KB 缓冲：合并循环本身以 CHUNK_BYTES 读写，
    // 缓冲主要兜住 BufWriter 内部可能的小块写。
    let mut out = BufWriter::with_capacity(CHUNK_BYTES, out);
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
        crate::qlog!("[合并] 卷 {}/{} 完成", idx + 1, part_paths.len());
    }
    crate::qlog!("[合并] 读取循环全部完成，开始 flush");
    // 与下载一致：只 flush 用户态缓冲，不 fsync。合并产物紧接着会被解压读取，
    // 崩溃后完整性由整包 SHA256 把关，机械盘上省掉一次 GB 级 FlushFileBuffers。
    out.flush()
        .await
        .map_err(|e| format!("合并文件刷新失败: {}", e))?;
    crate::qlog!("[合并] flush 完成，开始 SHA256 校验");

    // 整体 SHA256 校验
    let actual = hex::encode(hasher.finalize());
    if let Some(expected) = expected_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "{}合并后整体 SHA256 校验不匹配，期望: {}，实际: {}",
                DOWNLOAD_FAIL_PREFIX, expected, actual
            ));
        }
    }

    Ok(actual)
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
        let p = DownloadProgress {
            downloaded: 100,
            total: None,
        };
        assert_eq!(p.percent(), None);
    }

    #[test]
    fn percent_none_when_total_zero() {
        // 服务器谎报 Content-Length: 0 时不应除零
        let p = DownloadProgress {
            downloaded: 100,
            total: Some(0),
        };
        assert_eq!(p.percent(), None);
    }

    #[test]
    fn percent_computes_ratio() {
        let p = DownloadProgress {
            downloaded: 25,
            total: Some(100),
        };
        assert_eq!(p.percent(), Some(25.0));
    }

    #[test]
    fn percent_of_accepts_manifest_fallback_total() {
        // 服务器没给 Content-Length 时，编排层用 manifest.size_bytes 兜底
        assert_eq!(percent_of(25, Some(100)), Some(25.0));
        // 越界（服务器谎报或续传偏移超出）封顶 100，不显示 120%
        assert_eq!(percent_of(120, Some(100)), Some(100.0));
        assert_eq!(percent_of(25, None), None);
        assert_eq!(percent_of(25, Some(0)), None);
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
        assert!(
            err.starts_with(DOWNLOAD_FAIL_PREFIX),
            "校验不匹配属于下载链路问题，需带统一前缀: {err}"
        );
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
        assert_eq!(sha256_of_file(&p).expect("空文件应可校验"), sha_of(b""));
        let _ = std::fs::remove_file(&p);
    }

    // ---------- 重试分类 ----------

    #[test]
    fn http_4xx_is_not_retryable_except_timeout_and_rate_limit() {
        for permanent in [
            "下载失败: 服务器返回错误状态码: 404 Not Found",
            "下载失败: 服务器返回错误状态码: 403 Forbidden",
            "下载失败: 服务器返回错误状态码: 410 Gone",
        ] {
            assert!(
                !is_retryable(permanent),
                "4xx 重试同址不会成功: {permanent}"
            );
        }
        for transient in [
            "下载失败: 服务器返回错误状态码: 408 Request Timeout",
            "下载失败: 服务器返回错误状态码: 429 Too Many Requests",
        ] {
            assert!(is_retryable(transient), "408/429 应该继续重试: {transient}");
        }
    }

    #[test]
    fn http_5xx_and_stream_errors_are_retryable() {
        for e in [
            "下载失败: 服务器返回错误状态码: 503 Service Unavailable",
            "下载失败: 请求失败: connection reset",
            "下载数据流超时（60 秒无新数据）",
            "下载数据流中断: error decoding response body",
        ] {
            assert!(is_retryable(e), "网络抖动应允许重试: {e}");
        }
    }

    #[test]
    fn local_write_failures_are_not_retryable() {
        // 分卷模式下错误会带「分卷 x/y」中缀，因此按包含匹配而非前缀
        for e in [
            "创建下载文件失败 C:\\x\\update.zip: 磁盘空间不足",
            "下载失败: 分卷 3/28 写入下载文件失败: 设备无空间",
            "下载文件刷新失败: 句柄无效",
            "创建分卷目录失败 C:\\x\\parts: 拒绝访问",
            "创建合并文件失败 C:\\x\\update.zip: 磁盘空间不足",
            "分卷合并失败: 写入合并文件失败: 设备无空间",
            "分卷合并失败: 打开分卷失败 C:\\x\\part_002: 拒绝访问",
        ] {
            assert!(is_local_io_error(e), "应识别为本地 IO 失败: {e}");
            assert!(!is_retryable(e), "本地落盘失败重试无意义: {e}");
        }
    }

    #[test]
    fn network_errors_are_not_local_io() {
        // 网络类错误不能被误判成本地问题：它们需要重试/官网出口
        for e in [
            "下载失败: 请求失败: connection reset",
            "下载失败: 服务器返回错误状态码: 404 Not Found",
            "下载失败: SHA256 校验不匹配，期望: a，实际: b",
        ] {
            assert!(!is_local_io_error(e), "不应识别为本地 IO: {e}");
        }
    }

    // ---------- 断点续传 ----------

    #[test]
    fn part_suffix_is_appended_not_replaced() {
        // with_extension 会把 update_2.1.2.zip 变成 update_2.1.part，毁掉版本号
        assert_eq!(
            with_suffix(Path::new("C:/x/update_2.1.2.zip"), PART_SUFFIX),
            PathBuf::from("C:/x/update_2.1.2.zip.part")
        );
        assert_eq!(
            with_suffix(Path::new("C:/x/part_007"), PART_META_SUFFIX),
            PathBuf::from("C:/x/part_007.part.meta")
        );
    }

    #[test]
    fn parse_content_range_reads_start_and_total() {
        assert_eq!(
            parse_content_range("bytes 100-499/1234"),
            Some((100, Some(1234)))
        );
        // 总量未知用 *
        assert_eq!(parse_content_range(" bytes 0-9/* "), Some((0, None)));
    }

    #[test]
    fn parse_content_range_rejects_malformed_values() {
        for bad in [
            "",
            "bytes",
            "bytes 1-",
            "bytes -5/10",
            "items 1-2/3",
            "bytes 9-3/10",
        ] {
            assert_eq!(parse_content_range(bad), None, "应拒绝: {bad:?}");
        }
    }

    fn headers_with(pairs: &[(&'static str, &str)]) -> reqwest::header::HeaderMap {
        let mut map = reqwest::header::HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, value.parse().expect("构造响应头"));
        }
        map
    }

    #[test]
    fn strong_etag_is_preferred_validator() {
        let h = headers_with(&[
            ("etag", "\"abc\""),
            ("last-modified", "Wed, 21 Oct 2015 07:28:00 GMT"),
        ]);
        assert_eq!(validator_from(&h).as_deref(), Some("\"abc\""));
    }

    #[test]
    fn weak_etag_falls_back_to_last_modified() {
        // 弱校验器不能用于 If-Range，应退到 Last-Modified
        let h = headers_with(&[
            ("etag", "W/\"weak\""),
            ("last-modified", "Wed, 21 Oct 2015 07:28:00 GMT"),
        ]);
        assert_eq!(
            validator_from(&h).as_deref(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT")
        );
    }

    #[test]
    fn no_usable_validator_means_no_resume_meta() {
        let empty = reqwest::header::HeaderMap::new();
        assert!(validator_from(&empty).is_none());
        let weak_only = headers_with(&[("etag", "W/\"weak\"")]);
        assert!(validator_from(&weak_only).is_none());
    }

    #[tokio::test]
    async fn load_resume_accepts_matching_meta() {
        let dir = temp_path("resume_ok", "dir");
        std::fs::create_dir_all(&dir).expect("建目录");
        let part = dir.join("update.zip.part");
        let meta_path = dir.join("update.zip.part.meta");
        std::fs::write(&part, b"hello").expect("写半包");
        save_meta(
            &meta_path,
            &PartialMeta {
                url: "https://a/x.zip".to_string(),
                validator: "\"v1\"".to_string(),
                total: Some(10),
            },
        )
        .await;

        let (meta, len) = load_resume("https://a/x.zip", &part, &meta_path)
            .await
            .expect("同地址半包应可续传");
        assert_eq!(len, 5);
        assert_eq!(meta.validator, "\"v1\"");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn load_resume_discards_meta_for_other_url() {
        // 故障转移换地址后不能拿旧地址的半包接着下
        let dir = temp_path("resume_url", "dir");
        std::fs::create_dir_all(&dir).expect("建目录");
        let part = dir.join("update.zip.part");
        let meta_path = dir.join("update.zip.part.meta");
        std::fs::write(&part, b"hello").expect("写半包");
        save_meta(
            &meta_path,
            &PartialMeta {
                url: "https://old/x.zip".to_string(),
                validator: "\"v1\"".to_string(),
                total: Some(10),
            },
        )
        .await;

        assert!(load_resume("https://new/x.zip", &part, &meta_path)
            .await
            .is_none());
        assert!(!part.exists(), "来源不明的半包应被清掉，别长期占盘");
        assert!(!meta_path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn load_resume_drops_part_larger_than_declared_total() {
        let dir = temp_path("resume_oversize", "dir");
        std::fs::create_dir_all(&dir).expect("建目录");
        let part = dir.join("update.zip.part");
        let meta_path = dir.join("update.zip.part.meta");
        std::fs::write(&part, b"0123456789").expect("写半包");
        save_meta(
            &meta_path,
            &PartialMeta {
                url: "https://a/x.zip".to_string(),
                validator: "\"v1\"".to_string(),
                total: Some(4),
            },
        )
        .await;

        assert!(load_resume("https://a/x.zip", &part, &meta_path)
            .await
            .is_none());
        assert!(!part.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn download_file_finishes_complete_part_without_network() {
        // 崩溃窗口：数据已收齐、只差改名。再联网毫无意义，应直接校验收尾。
        // 地址用不可达端口：只要走到请求就一定失败，返回 Ok 即证明未联网。
        let dir = temp_path("complete_part", "dir");
        std::fs::create_dir_all(&dir).expect("建目录");
        let dest = dir.join("update.zip");
        let part = with_suffix(&dest, PART_SUFFIX);
        let meta_path = with_suffix(&dest, PART_META_SUFFIX);
        std::fs::write(&part, b"hello").expect("写完整半包");
        save_meta(
            &meta_path,
            &PartialMeta {
                url: "http://127.0.0.1:9/x.zip".to_string(),
                validator: "\"v1\"".to_string(),
                total: Some(5),
            },
        )
        .await;

        let (path, sha) = download_file("http://127.0.0.1:9/x.zip", &dest, |_| {})
            .await
            .expect("已下完的半包应直接收尾");
        assert_eq!(path, dest);
        assert_eq!(sha, sha_of(b"hello"));
        assert!(dest.exists());
        assert!(!part.exists());
        assert!(!meta_path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn finish_part_replaces_stale_destination() {
        let dir = temp_path("finish_part", "dir");
        std::fs::create_dir_all(&dir).expect("建目录");
        let part = dir.join("update.zip.part");
        let meta_path = dir.join("update.zip.part.meta");
        let dest = dir.join("update.zip");
        std::fs::write(&part, b"new").expect("写半包");
        std::fs::write(&meta_path, b"{}").expect("写元数据");
        std::fs::write(&dest, b"old").expect("写旧包");

        finish_part(&part, &meta_path, &dest)
            .await
            .expect("应改名成功");
        assert_eq!(std::fs::read(&dest).expect("读回"), b"new");
        assert!(!part.exists(), "半包应已被消费");
        assert!(!meta_path.exists(), "元数据应被清掉");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------- 并发分卷进度聚合 ----------

    #[test]
    fn parts_progress_aggregates_bytes_and_done_count() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<(usize, usize, u64)>::new()));
        let sink = seen.clone();
        let progress = PartsProgress::new(
            3,
            Some(30),
            Arc::new(move |done: usize, total: usize, p: DownloadProgress| {
                sink.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((done, total, p.downloaded));
            }),
        );

        progress.update(0, 5);
        progress.update(1, 7);
        progress.complete(1, 10);
        progress.update(2, 6);
        progress.complete(0, 10);

        let seen = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let last = seen.last().copied().expect("应至少推送一次进度");
        assert_eq!(last.0, 2, "已完成两卷");
        assert_eq!(last.1, 3, "总卷数");
        assert_eq!(last.2, 26, "整体字节应为各卷之和");
    }

    #[test]
    fn parts_progress_never_goes_backwards() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<u64>::new()));
        let sink = seen.clone();
        let progress = PartsProgress::new(
            1,
            Some(100),
            Arc::new(move |_: usize, _: usize, p: DownloadProgress| {
                sink.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(p.downloaded);
            }),
        );
        progress.update(0, 50);
        progress.update(0, 4); // 回落全量：不应把已显示进度拉回去
        progress.update(0, 60);
        let seen = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert!(
            seen.windows(2).all(|w| w[0] <= w[1]),
            "进度应单调: {seen:?}"
        );
        assert_eq!(seen.last().copied(), Some(60));
    }

    // ---------- merge_parts ----------

    #[tokio::test]
    async fn merge_concatenates_in_order_and_verifies() {
        let p1 = write_temp("part1", b"AAA").expect("写分卷1");
        let p2 = write_temp("part2", b"BBBB").expect("写分卷2");
        let out = temp_path("merged", "zip");
        let expected = sha_of(b"AAABBBB");

        let actual = merge_parts(
            &[p1.clone(), p2.clone()],
            &out,
            &Some(expected.clone()),
            |_, _| {},
        )
        .await
        .expect("合并并校验应成功");
        assert_eq!(actual, expected, "应返回合并产物的实际 SHA256");

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
        let err = merge_parts(
            std::slice::from_ref(&p1),
            &out,
            &Some(sha_of(b"ZZZ")),
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert!(
            err.contains("整体 SHA256 校验不匹配"),
            "错误应说明整体不匹配: {err}"
        );
        assert!(
            err.starts_with(DOWNLOAD_FAIL_PREFIX),
            "合并后哈希不匹配属于下载链路问题，需带统一前缀: {err}"
        );
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
            seen.lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((done, total));
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
        assert!(merge_parts(&[missing], &out, &None, |_, _| {})
            .await
            .is_err());
    }
}
