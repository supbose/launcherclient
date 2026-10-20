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

/// 拉取并解析单个 manifest 地址
///
/// 使用共享 Client（连接 3s + 总超时 5s，见 [`crate::client::manifest_client`]）：
/// 故障转移的多个地址复用同一连接池，避免每地址一次完整 TLS 握手。
async fn fetch(url: &str) -> Result<FlatManifest, String> {
    let response = crate::client::manifest_client()
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

// 测试里构造失败即用例本身失败，按 clippy 惯例局部放行 expect/unwrap
#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个仅版本/更新开关可定的 manifest（其余字段用默认值）
    fn m(version: &str, update: bool) -> FlatManifest {
        FlatManifest {
            version: version.to_string(),
            update,
            size_bytes: None,
            sha256: None,
            url: String::new(),
            parts: None,
            part_sha256: None,
            release_date: None,
            changelog: Vec::new(),
        }
    }

    fn needs(cur: &str, latest: &str) -> bool {
        need_update(cur, &m(latest, true))
    }

    // ---------- update 开关 ----------

    #[test]
    fn update_false_never_updates_even_if_newer() {
        // 服务端强制关更新：即使版本更高也视为无更新
        assert!(!need_update("1.0.0", &m("9.9.9", false)));
    }

    // ---------- semver 可解析：标准比较 ----------

    #[test]
    fn semver_patch_newer_needs_update() {
        assert!(needs("1.0.0", "1.0.1"));
    }

    #[test]
    fn semver_minor_newer_needs_update() {
        assert!(needs("1.0.0", "1.1.0"));
    }

    #[test]
    fn semver_major_newer_needs_update() {
        assert!(needs("1.9.9", "2.0.0"));
    }

    #[test]
    fn semver_equal_no_update() {
        assert!(!needs("1.2.3", "1.2.3"));
    }

    #[test]
    fn semver_downgrade_no_update() {
        // 服务端版本更旧：不更新
        assert!(!needs("2.0.0", "1.0.0"));
    }

    #[test]
    fn semver_prerelease_is_older_than_release() {
        // 1.0.0-alpha < 1.0.0：装了正式版后不应被预发布版拽回去
        assert!(!needs("1.0.0", "1.0.0-alpha"));
        assert!(needs("1.0.0-alpha", "1.0.0"));
    }

    #[test]
    fn semver_surrounding_whitespace_is_trimmed() {
        assert!(!needs(" 1.0.0 ", "1.0.0"));
        assert!(needs(" 1.0.0", "1.0.1 "));
    }

    // ---------- semver 不可解析：字符串不等兜底 ----------

    #[test]
    fn unparsable_versions_fall_back_to_string_inequality() {
        assert!(needs("abc", "abd"));
        assert!(!needs("abc", "abc"));
    }

    #[test]
    fn one_side_unparsable_falls_back_to_string_inequality() {
        // 本地可解析、服务端不可解析：不相等即更新
        assert!(needs("1.0.0", "v2.0"));
        assert!(!needs("1.0.0", "1.0.0"));
    }

    #[test]
    fn empty_latest_means_no_update() {
        // manifest 缺 version 时反序列化会失败；此处覆盖空串边缘
        assert!(!needs("", ""));
        assert!(needs("", "1.0.0"));
    }

    // ---------- failover 入口 ----------

    #[tokio::test]
    async fn fetch_with_failover_empty_list_errors() {
        // 空列表直接报错，不发任何请求
        let err = fetch_with_failover(&[]).await.unwrap_err();
        assert!(err.contains("为空"), "应提示地址列表为空: {err}");
    }

    // ---------- FlatManifest 反序列化 ----------

    #[test]
    fn manifest_minimal_json_uses_defaults() {
        let m: FlatManifest =
            serde_json::from_str(r#"{"version": "1.0.0"}"#).expect("最小 manifest 应可解析");
        assert!(m.update, "update 缺省应为 true");
        assert_eq!(m.url, "");
        assert!(m.parts.is_none());
        assert!(m.part_sha256.is_none());
        assert!(m.size_bytes.is_none());
        assert!(m.sha256.is_none());
        assert!(m.changelog.is_empty());
    }

    #[test]
    fn manifest_full_json_parses_parts() {
        let m: FlatManifest = serde_json::from_str(
            r#"{
                "version": "2.0.0",
                "update": false,
                "size_bytes": 1024,
                "sha256": "abc",
                "url": "https://example.com/a.zip",
                "parts": ["https://example.com/p1", "https://example.com/p2"],
                "part_sha256": ["h1", "h2"],
                "changelog": ["修复", "优化"]
            }"#,
        )
        .expect("完整 manifest 应可解析");
        assert!(!m.update);
        assert_eq!(m.size_bytes, Some(1024));
        assert_eq!(m.sha256.as_deref(), Some("abc"));
        assert_eq!(m.parts.as_ref().map(Vec::len), Some(2));
        assert_eq!(m.part_sha256.as_ref().map(Vec::len), Some(2));
        assert_eq!(m.changelog, vec!["修复".to_string(), "优化".to_string()]);
    }

    #[test]
    fn manifest_unknown_fields_are_ignored() {
        // 服务端加字段不应阻塞老客户端
        let m: FlatManifest =
            serde_json::from_str(r#"{"version": "1.0.0", "future_field": 42}"#)
                .expect("未知字段应被忽略");
        assert_eq!(m.version, "1.0.0");
    }
}
