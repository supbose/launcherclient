//! 本地版本记录（`app.json`）
//!
//! 从自研能力库 `qingshu` 的 `update::LocalManifest` 复刻而来（2026-10-30 起本仓库
//! 不再依赖该外部 crate）。**字段名与序列化形态必须逐字保持**：用户机器上已经存在
//! 的 `app.json` 是老版本更新器写下的，任何改名、增加必填字段或改掉 `skip_serializing_if`
//! 都会让历史记录读不出来——读不出来等于「未安装」，会触发一次全量重下。
//!
//! 保持兼容的几个细节：
//! - `name` 带 `#[serde(default)]`：早期包没写这个字段，缺失时按旧格式处理
//! - `api_address` / `ws_address` 为 `None` 时**不出现**在 JSON 里（而不是输出 null）
//! - 时间戳用 UTC、秒级精度、`Z` 结尾（`chrono` 的 `to_rfc3339_opts(SecondsFormat::Secs, true)`）
//!
//! 与旧实现的两处**有意**差异：错误类型从库内 `UpdateError` 换成本仓库统一的
//! `String`（调用方直接 `format!` 上抛 UI）；`save` 顺带创建父目录——旧实现由外层
//! `UpdateManager` 负责，本仓库的调用点没有那层封装，不建目录会在首次安装时偶发失败。

use std::path::Path;

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

/// 本地版本记录，落盘为 `<应用目录>\app.json`
///
/// `Default` 用 derive：所有字段都是 `Option` / `String` / `bool`，派生结果即
/// 「全 None + 全空串 + false」，与旧手写实现一致，且新增字段不会漏初始化。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalManifest {
    /// 应用名称（向前兼容：旧版本 manifest 可能没有此字段）
    #[serde(default)]
    pub name: Option<String>,
    pub version: String,
    pub installed_at: String,
    pub installed_path: String,
    pub last_check: String,
    pub launch_app: Option<String>,
    pub use_version_dir: bool,
    /// API 服务完整 TCP 地址（格式：127.0.0.1:8888）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_address: Option<String>,
    /// WebSocket 服务完整地址（格式：ws://127.0.0.1:8889）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ws_address: Option<String>,
}

/// 当前 UTC 时刻的秒级 RFC3339 字符串（如 `2026-10-30T07:12:56Z`）
///
/// 与旧实现同参数调用，保证新旧更新器写出的时间戳格式一致。
fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

impl LocalManifest {
    /// 构造一条「刚装好」的记录：`installed_at` 与 `last_check` 均取当前时刻
    ///
    /// `name` 为空串时记为 `None`（与旧实现一致，区分「未提供」与「提供了空值」）。
    pub fn new(
        name: &str,
        version: &str,
        installed_path: &str,
        launch_app: Option<String>,
        use_version_dir: bool,
        api_address: Option<String>,
        ws_address: Option<String>,
    ) -> Self {
        let now = now_rfc3339();
        Self {
            name: if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            },
            version: version.to_string(),
            installed_at: now.clone(),
            installed_path: installed_path.to_string(),
            last_check: now,
            launch_app,
            use_version_dir,
            api_address,
            ws_address,
        }
    }

    /// 以 pretty JSON 写入 `path`（父目录不存在时自动创建）
    pub async fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("创建记录目录失败 {}: {}", parent.display(), e))?;
            }
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化本地版本记录失败: {}", e))?;
        tokio::fs::write(path, json)
            .await
            .map_err(|e| format!("写入本地版本记录失败 {}: {}", path.display(), e))
    }

    /// 读取 `path`；文件不存在返回 `Ok(None)`（正常首装），解析失败返回 `Err`
    ///
    /// 旧格式迁移：`name` 缺失时补时间戳并原地重写一次，让后续读取走新格式分支。
    /// 不回填 `name` 值本身——调用方传不传 app_name 是它的选择，这里只保证文件被
    /// 规范化重写一次（与旧实现一致）。
    pub async fn load(path: &Path) -> Result<Option<Self>, String> {
        if !path.exists() {
            return Ok(None);
        }

        let raw = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("读取本地版本记录失败 {}: {}", path.display(), e))?;

        match serde_json::from_str::<Self>(&raw) {
            Ok(mut manifest) => {
                if manifest.name.is_none() {
                    eprintln!(
                        "[版本记录] 检测到旧格式 app.json（缺少 name 字段），自动迁移: {}",
                        path.display()
                    );
                    manifest.last_check = now_rfc3339();
                    manifest.save(path).await?;
                }
                Ok(Some(manifest))
            }
            Err(e) => Err(format!("解析本地版本记录失败 {}: {}", path.display(), e)),
        }
    }
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qsltd-lm-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("app.json")
    }

    #[test]
    fn json_field_names_match_legacy_app_json() {
        // 锁死序列化形态：字段名、顺序无关但内容必须齐；api/ws 为 None 时不出现
        let m = LocalManifest {
            name: Some("qsm".to_string()),
            version: "1.2.3".to_string(),
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            installed_path: "C:\\app\\app_1.2.3".to_string(),
            last_check: "2026-01-02T00:00:00Z".to_string(),
            launch_app: Some("qsm.exe".to_string()),
            use_version_dir: true,
            api_address: None,
            ws_address: None,
        };
        let json = serde_json::to_string(&m).unwrap();
        for key in [
            "\"name\"",
            "\"version\"",
            "\"installed_at\"",
            "\"installed_path\"",
            "\"last_check\"",
            "\"launch_app\"",
            "\"use_version_dir\"",
        ] {
            assert!(json.contains(key), "序列化缺少字段 {key}: {json}");
        }
        assert!(
            !json.contains("api_address"),
            "None 的 api_address 不应序列化: {json}"
        );
    }

    #[test]
    fn reads_legacy_app_json_without_name_field() {
        // 早期 app.json 没有 name —— 必须能按 default 读出来，不能报缺字段
        let legacy = r#"{"version":"1.0.0","installed_at":"2025-01-01T00:00:00Z",
            "installed_path":"C:\\app\\app_1.0.0","last_check":"2025-01-01T00:00:00Z",
            "launch_app":null,"use_version_dir":true}"#;
        let m: LocalManifest = serde_json::from_str(legacy).expect("旧格式应可解析");
        assert!(m.name.is_none());
        assert_eq!(m.version, "1.0.0");
        assert!(m.use_version_dir);
    }

    #[test]
    fn keeps_api_and_ws_address_when_present() {
        // 带地址的历史记录（旧 UpdateManager 写过）必须原样读回
        let json = r#"{"name":"qsm","version":"1.0.0","installed_at":"x","installed_path":"y",
            "last_check":"z","launch_app":null,"use_version_dir":false,
            "api_address":"127.0.0.1:8888","ws_address":"ws://127.0.0.1:8889"}"#;
        let m: LocalManifest = serde_json::from_str(json).expect("应可解析");
        assert_eq!(m.api_address.as_deref(), Some("127.0.0.1:8888"));
        assert_eq!(m.ws_address.as_deref(), Some("ws://127.0.0.1:8889"));

        let back = serde_json::to_string(&m).unwrap();
        assert!(back.contains("api_address"));
    }

    #[test]
    fn new_stamps_both_timestamps_and_blank_name_becomes_none() {
        let m = LocalManifest::new("", "2.0.0", "C:\\app\\app_2.0.0", None, true, None, None);
        assert!(m.name.is_none(), "空 name 应记为 None");
        assert_eq!(m.version, "2.0.0");
        assert_eq!(m.installed_at, m.last_check);
        // 秒级 UTC、Z 结尾：20 字符 + 无小数点
        assert!(m.installed_at.ends_with('Z'), "{}", m.installed_at);
        assert!(!m.installed_at.contains('.'), "{}", m.installed_at);
        let digits: usize = m
            .installed_at
            .chars()
            .filter(|c| c.is_ascii_digit())
            .count();
        assert_eq!(digits, 14, "应为 YYYYMMDDHHMMSS 秒级精度");

        let named = LocalManifest::new("qsm", "2.0.0", "p", None, true, None, None);
        assert_eq!(named.name.as_deref(), Some("qsm"));
    }

    #[tokio::test]
    async fn save_creates_missing_parent_dir_and_round_trips() {
        let path = tmp_path("roundtrip");
        let m = LocalManifest::new(
            "qsm",
            "3.1.4",
            &path.parent().unwrap().display().to_string(),
            Some("qsm.exe".to_string()),
            true,
            None,
            None,
        );
        // 父目录尚不存在：save 需自建
        assert!(!path.parent().unwrap().exists());
        m.save(&path).await.expect("父目录缺失也应写入成功");

        let loaded = LocalManifest::load(&path).await.expect("应可读取");
        assert_eq!(loaded.as_ref(), Some(&m), "读写应完全一致");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn load_returns_none_when_absent_and_err_on_corrupted() {
        let path = tmp_path("absent");
        assert_eq!(LocalManifest::load(&path).await.unwrap(), None);

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ 这不是 json").unwrap();
        let err = LocalManifest::load(&path).await.unwrap_err();
        assert!(err.contains("解析本地版本记录失败"), "{err}");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn load_migrates_legacy_file_in_place() {
        let path = tmp_path("migrate");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"version":"1.0.0","installed_at":"2025-01-01T00:00:00Z",
               "installed_path":"C:\\app\\app_1.0.0","last_check":"2025-01-01T00:00:00Z",
               "launch_app":null,"use_version_dir":true}"#,
        )
        .unwrap();

        let loaded = LocalManifest::load(&path)
            .await
            .unwrap()
            .expect("旧格式也应读到记录");
        assert!(loaded.name.is_none(), "迁移不回填 name");
        assert_ne!(
            loaded.last_check, "2025-01-01T00:00:00Z",
            "迁移应刷新 last_check"
        );

        let rewritten = std::fs::read_to_string(&path).unwrap();
        assert!(rewritten.contains("\"name\": null"), "{rewritten}");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
