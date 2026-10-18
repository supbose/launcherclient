//! 服务器 manifest（version.json）解析与更新判断
//!
//! manifest 为平铺单包结构，示例：
//!
//! ```json
//! {
//!   "version": "2.1.2",
//!   "update": true,
//!   "size_bytes": 2374852351,
//!   "sha256": "419d...",
//!   "url": "https://down.cdn.qcsm.ltd/download/xxx.zip",
//!   "release_date": "2026-06-09T18:23:00.000Z",
//!   "changelog": ["修复了某些bugs"]
//! }
//! ```

use serde::Deserialize;

/// 平铺 manifest 结构
#[derive(Debug, Clone, Deserialize)]
pub struct FlatManifest {
    /// 最新版本号
    pub version: String,
    /// 是否开放更新（false 时强制视为无更新）
    #[serde(default = "default_true")]
    pub update: bool,
    /// 更新包大小（字节）
    #[serde(default)]
    pub size_bytes: Option<u64>,
    /// 更新包 SHA256 校验值
    #[serde(default)]
    pub sha256: Option<String>,
    /// 更新包下载地址（单包模式必填；分卷模式下可缺省，parts 存在时忽略此字段）
    #[serde(default)]
    pub url: String,
    /// 分卷下载地址列表（可选，分卷模式；按序拼接后为完整 zip）
    #[serde(default)]
    pub parts: Option<Vec<String>>,
    /// 各分卷的 SHA256 校验值（可选，与 parts 等长，逐卷校验）
    #[serde(default)]
    pub part_sha256: Option<Vec<String>>,
    /// 发布时间
    #[serde(default)]
    pub release_date: Option<String>,
    /// 更新日志
    #[serde(default)]
    pub changelog: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// 拉取并解析服务器 manifest（带故障转移：按序尝试多个地址，第一个成功的生效）
///
/// 全部失败时返回最后一个错误
pub async fn fetch_with_failover(urls: &[String]) -> Result<FlatManifest, String> {
    if urls.is_empty() {
        return Err("manifest 地址列表为空".to_string());
    }

    let mut last_err = String::new();
    for (i, url) in urls.iter().enumerate() {
        if urls.len() > 1 {
            eprintln!("[更新器] 尝试 manifest 地址 {}/{}: {}", i + 1, urls.len(), url);
        }
        match fetch(url).await {
            Ok(m) => {
                if i > 0 {
                    eprintln!("[更新器] 地址 {} 拉取成功", url);
                }
                return Ok(m);
            }
            Err(e) => {
                eprintln!("[更新器] 地址 {} 拉取失败: {}", url, e);
                last_err = e;
            }
        }
    }
    Err(format!(
        "全部 {} 个 manifest 地址均拉取失败，最后错误: {}",
        urls.len(),
        last_err
    ))
}

/// 拉取并解析单个 manifest 地址（连接超时 3s / 总超时 5s，防弱网挂死拖慢启动）
async fn fetch(url: &str) -> Result<FlatManifest, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("请求 manifest 失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("服务器返回错误状态码: {}", response.status()));
    }

    response
        .json::<FlatManifest>()
        .await
        .map_err(|e| format!("manifest 解析失败（结构不符合规范）: {}", e))
}

/// 判断是否需要更新：manifest.update 为 false 时直接返回 false；
/// 双方版本均可按 semver 解析时用 semver 比较，否则以版本号字符串不等作为更新依据
pub fn need_update(current_version: &str, manifest: &FlatManifest) -> bool {
    if !manifest.update {
        return false;
    }
    match (
        semver::Version::parse(current_version.trim()),
        semver::Version::parse(manifest.version.trim()),
    ) {
        (Ok(cur), Ok(latest)) => cur < latest,
        _ => current_version.trim() != manifest.version.trim(),
    }
}
