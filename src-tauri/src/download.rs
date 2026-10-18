//! 更新包下载：流式写入 + 实时进度回调 + SHA256 校验

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

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
/// 返回：(文件路径, SHA256 十六进制值)
pub async fn download_file<F>(
    url: &str,
    dest: &Path,
    on_progress: F,
) -> Result<(PathBuf, String), String>
where
    F: Fn(DownloadProgress) + Send + Sync + 'static,
{
    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("下载请求失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("下载服务器返回错误状态码: {}", response.status()));
    }

    let total = response.content_length();
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("创建下载文件失败 {}: {}", dest.display(), e))?;

    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
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

/// 校验文件 SHA256 是否与期望值一致（期望值为空时跳过校验直接通过）
pub fn verify_sha256(file: &Path, expected: &Option<String>) -> Result<(), String> {
    let Some(expected) = expected.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };

    let data = std::fs::read(file).map_err(|e| format!("读取文件校验失败: {}", e))?;
    let actual = hex::encode(Sha256::digest(&data));
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "SHA256 校验不匹配，期望: {}，实际: {}",
            expected, actual
        ))
    }
}
