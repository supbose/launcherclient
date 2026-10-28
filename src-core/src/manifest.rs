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

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 平铺 manifest 结构
#[derive(Debug, Clone, Deserialize, Serialize)]
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
    /// 服务端要求的最低运行版本（可选）。当前版本低于它即视为不满足最低版本
    /// 要求，配合 update.json 的 `force_update` 开关决定是否阻断启动。
    #[serde(default)]
    pub min_version: Option<String>,
    /// 当前 version.json 对应的版本是否被标记为「强制更新」（默认 false）。
    /// 为 true 时，即便当前版本已等于/高于最新版，也强制要求用户升级到本版本。
    #[serde(default)]
    pub force_update: Option<bool>,
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
pub fn need_update(current_version: &str, manifest: &FlatManifest) -> bool {    if !manifest.update {
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

/// 当前版本是否低于服务端要求的最低版本
///
/// - `min_version` 缺失或为空视为「无最低版本约束」→ 返回 false
/// - 双方均可按 semver 解析时按 semver 比较；否则以版本号字符串不等兜底
pub fn is_below_min_version(current_version: &str, manifest: &FlatManifest) -> bool {
    let Some(min) = manifest
        .min_version
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    else {
        return false;
    };
    match (
        semver::Version::parse(current_version.trim()),
        semver::Version::parse(min),
    ) {
        (Ok(cur), Ok(mv)) => cur < mv,
        _ => current_version.trim() != min,
    }
}

/// 版本是否被服务端标记为强制更新（缺省/显式 false 视为否）
pub fn is_forced_update(manifest: &FlatManifest) -> bool {
    manifest.force_update.unwrap_or(false)
}

/// 是否触发「强制更新」条件：版本被标记强制更新，或当前版本低于最低版本要求
pub fn requires_force_update(current_version: &str, manifest: &FlatManifest) -> bool {
    is_forced_update(manifest) || is_below_min_version(current_version, manifest)
}

/// 计算「要求版本号」用于界面展示：优先 `min_version`，
/// 否则用 `manifest.version`（强制更新标记指向的版本）
pub fn required_version(manifest: &FlatManifest) -> String {
    manifest
        .min_version
        .clone()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| manifest.version.clone())
}

// ---------- manifest TTL 缓存 ----------

/// manifest 缓存 TTL：5 分钟。
///
/// 更新器每次启动跑一次检测，「已是最新」是最高频路径。无缓存时
/// 该路径每次必联网（弱网/服务端故障时最坏 5s×N 才进入无更新态）。
/// TTL 内重复启动直接用缓存判定；版本发布节奏远大于 5 分钟，
/// 新版本延迟可见性可接受。`--check-only` 模式不读缓存（准确优先）。
const MANIFEST_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// 缓存文件条目：拉取时刻 + manifest 原文
#[derive(Debug, Clone, Deserialize, Serialize)]
struct CacheEntry {
    /// 拉取时的 Unix 时间戳（秒）
    fetched_at: u64,
    /// manifest 原文
    manifest: FlatManifest,
}

/// 带 TTL 缓存的 manifest 拉取（缓存按 app_name 隔离）
///
/// - TTL 内命中：直接用缓存，不联网
/// - 未命中/过期：走 [`fetch_with_failover`]，成功后刷新缓存
///   （失败不写，保留旧缓存——下次仍在 TTL 外，重新联网）
///
/// 与 `--check-only` 的分工：check_only 要最新判定，保持全量联网；
/// 本函数供 UI 版 run_check 使用（双击运行/手动启动场景真正受益，
/// 且通常刚被 check_only 预热过，命中即免一次往返）。
pub async fn fetch_with_cache(urls: &[String], app_name: &str) -> Result<FlatManifest, String> {
    let cache_path = manifest_cache_path(app_name);
    if let Some(cached) = load_fresh_cache(&cache_path) {
        eprintln!(
            "[更新器] 使用 manifest 缓存（{} 秒内刚检测过，跳过联网）",
            MANIFEST_CACHE_TTL.as_secs()
        );
        return Ok(cached);
    }
    let m = fetch_with_failover(urls).await?;
    save_cache(&cache_path, &m);
    Ok(m)
}

/// manifest 缓存文件路径（按 app_name 隔离，避免不同应用的 manifest 互相覆盖）
fn manifest_cache_path(app_name: &str) -> PathBuf {
    qingshu::update::get_myapp_dir(Some(app_name)).join("manifest_cache.json")
}

/// 读取未过期的缓存（过期/损坏/不存在均返回 None）
fn load_fresh_cache(path: &Path) -> Option<FlatManifest> {
    let data = std::fs::read(path).ok()?;
    let entry: CacheEntry = serde_json::from_slice(&data).ok()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // saturating_sub 防系统时钟回拨导致 age 下溢
    let age = now.saturating_sub(entry.fetched_at);
    if age <= MANIFEST_CACHE_TTL.as_secs() {
        Some(entry.manifest)
    } else {
        None
    }
}

/// 写入缓存（失败静默：缓存只是加速，不影响主流程）
fn save_cache(path: &Path, m: &FlatManifest) {
    let fetched_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = CacheEntry {
        fetched_at,
        manifest: m.clone(),
    };
    let Ok(json) = serde_json::to_vec(&entry) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(path, json) {
        eprintln!("[更新器] manifest 缓存写入失败（忽略）: {}", e);
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
            min_version: None,
            force_update: None,
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

    // ---------- TTL 缓存 ----------

    fn temp_cache_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "qsltd_manifest_cache_test_{}_{}.json",
            tag,
            std::process::id()
        ))
    }

    fn write_entry(path: &Path, fetched_at: u64, manifest: &FlatManifest) {
        let entry = CacheEntry {
            fetched_at,
            manifest: manifest.clone(),
        };
        std::fs::write(path, serde_json::to_vec(&entry).expect("序列化测试数据"))
            .expect("写测试缓存");
    }

    #[test]
    fn cache_roundtrip_fresh() {
        let path = temp_cache_path("fresh");
        let _ = std::fs::remove_file(&path);
        save_cache(&path, &m("2.1.2", true));
        let loaded = load_fresh_cache(&path).expect("刚写入的缓存应立即命中");
        assert_eq!(loaded.version, "2.1.2");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cache_stale_rejected() {
        let path = temp_cache_path("stale");
        let _ = std::fs::remove_file(&path);
        write_entry(&path, 1, &m("9.9.9", true)); // 1970 年，远超 TTL
        assert!(load_fresh_cache(&path).is_none(), "远古缓存必须判过期");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cache_at_ttl_boundary_passes() {
        // age 恰等于 TTL：判定为 <= 放行（边界含端点）
        let path = temp_cache_path("boundary");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = std::fs::remove_file(&path);
        write_entry(&path, now.saturating_sub(MANIFEST_CACHE_TTL.as_secs()), &m("1.0.0", true));
        assert!(load_fresh_cache(&path).is_some(), "age == TTL 应仍命中");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cache_corrupt_file_returns_none() {
        let path = temp_cache_path("corrupt");
        std::fs::write(&path, b"not a json").expect("写损坏缓存");
        assert!(load_fresh_cache(&path).is_none(), "损坏缓存必须静默视为未命中");
        let _ = std::fs::remove_file(&path);
    }

    // ---------- 强制更新判定 ----------

    /// 构造带 min_version / force_update 的 manifest（其余字段用默认值）
    fn fm_with(version: &str, min: Option<&str>, forced: bool) -> FlatManifest {
        FlatManifest {
            version: version.to_string(),
            update: true,
            size_bytes: None,
            sha256: None,
            url: String::new(),
            parts: None,
            part_sha256: None,
            release_date: None,
            changelog: Vec::new(),
            min_version: min.map(|s| s.to_string()),
            force_update: Some(forced),
        }
    }

    #[test]
    fn below_min_version_semver() {
        assert!(!is_below_min_version("2.1.3", &fm_with("2.1.3", Some("2.1.0"), false)));
        assert!(is_below_min_version("2.0.0", &fm_with("2.1.3", Some("2.1.0"), false)));
    }

    #[test]
    fn below_min_version_none_means_no_constraint() {
        // min_version 缺失：任何当前版本都不算「低于最低版本」
        assert!(!is_below_min_version("0.0.1", &fm_with("1.0.0", None, false)));
    }

    #[test]
    fn forced_update_flag_triggers_regardless_of_version() {
        // 标记强制更新：即便当前版本等于最新版也要升级
        assert!(requires_force_update("2.1.3", &fm_with("2.1.3", None, true)));
    }

    #[test]
    fn requires_force_update_combines_min_and_flag() {
        let m = fm_with("3.0.0", Some("2.0.0"), false);
        assert!(!requires_force_update("3.0.0", &m));
        assert!(requires_force_update("1.9.0", &m));
        let f = fm_with("3.0.0", Some("2.0.0"), true);
        assert!(requires_force_update("3.0.0", &f), "标记强制更新时高于 min 仍触发");
    }

    #[test]
    fn required_version_prefers_min_then_manifest_version() {
        assert_eq!(required_version(&fm_with("2.1.3", Some("2.0.0"), false)), "2.0.0");
        assert_eq!(required_version(&fm_with("2.1.3", None, true)), "2.1.3");
    }

    #[test]
    fn manifest_min_and_force_fields_parse_optional() {
        // 缺字段时反序列化应成功并回落默认（None / false）
        let m: FlatManifest = serde_json::from_str(r#"{"version": "1.0.0"}"#).expect("应可解析");
        assert!(m.min_version.is_none());
        assert!(!is_forced_update(&m));
        // 显式字段应被正确读取
        let m: FlatManifest = serde_json::from_str(
            r#"{"version": "2.0.0", "min_version": "1.5.0", "force_update": true}"#,
        )
        .expect("应可解析");
        assert_eq!(m.min_version.as_deref(), Some("1.5.0"));
        assert!(is_forced_update(&m));
    }
}
