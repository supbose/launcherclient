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
    /// 更新包下载地址
    pub url: String,
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

/// 拉取并解析服务器 manifest
pub async fn fetch(url: &str) -> Result<FlatManifest, String> {
    let response = reqwest::get(url)
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
