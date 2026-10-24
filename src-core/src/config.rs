//! 本地配置文件 update.json 读取
//!
//! 配置文件与更新器 exe 同目录（其次查找当前工作目录），flat 结构：
//!
//! ```json
//! {
//!   "manifest_url": "https://qsw.us.ci/update/qsm/version.json",
//!   "app_name": "qsm",
//!   "launch": "目标程序.exe",
//!   "current_version": "2.1.1"
//! }
//! ```
//!
//! 优先级：命令行参数 > update.json > 内置默认值（见 [`crate::defaults`]）

use crate::defaults;
use serde::Deserialize;

/// manifest 地址配置项（兼容单地址字符串或多地址数组，多地址按序故障转移）
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    /// 单地址
    One(String),
    /// 多地址（按序尝试）
    Many(Vec<String>),
}

impl OneOrMany {
    /// 展开为地址列表
    pub fn to_urls(&self) -> Vec<String> {
        match self {
            OneOrMany::One(u) => vec![u.clone()],
            OneOrMany::Many(us) => us.clone(),
        }
    }
}

/// update.json 配置项（全部可选，未配置的项回退默认值）
#[derive(Debug, Default, Deserialize)]
pub struct UpdateSettings {
    /// 服务器 manifest（version.json）地址，单个字符串或地址数组（按序故障转移）
    #[serde(default)]
    pub manifest_url: Option<OneOrMany>,
    /// 应用名称，决定配置/下载/安装目录
    #[serde(default)]
    pub app_name: Option<String>,
    /// 更新完成后要拉起的目标程序路径
    #[serde(default)]
    pub launch: Option<String>,
    /// 当前已安装版本（备用，一般从本地 app.json 读取）
    #[serde(default)]
    pub current_version: Option<String>,
    /// 保留的版本目录数量（含当前版本，旧目录即回滚资产），0 或缺省用默认值
    #[serde(default)]
    pub max_versions: Option<usize>,
    /// 更新前需卸载的 Windows 服务名列表（运行中先停止；卸载后不恢复不重启；空或缺省不处理）
    #[serde(default)]
    pub services: Option<Vec<String>>,
    /// 启动器界面模板：A/B/C/D/E（大小写不敏感），缺省或非法值用
    /// [`defaults::DEFAULT_LAUNCHER_TEMPLATE`]；取值校验见 [`UpdateSettings::launcher_template`]
    #[serde(default)]
    pub launcher_template: Option<String>,
}

impl UpdateSettings {
    /// 生效的启动器界面模板
    ///
    /// 未配置（None）或空串 → 默认模板；配置了则大写归一（"a"/"A"/" a " 同义）。
    /// 只做归一与去空，合法性由调用方（resolve 层）校验后回落，此处保留原值
    /// 以便上层按「是否显式指定」判断优先级。
    pub fn launcher_template(&self) -> Option<String> {
        self.launcher_template
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_ascii_uppercase())
    }
}

/// max_versions 默认值：当前版本 + 1 个旧版本（回滚资产）
pub const DEFAULT_MAX_VERSIONS: usize = 2;

/// 合法启动器界面模板键（与 `src/components/templates/` 下的组件一一对应）
pub const VALID_LAUNCHER_TEMPLATES: &[&str] = &["A", "B", "C", "D", "E"];

/// 解析生效的启动器界面模板：命令行 > update.json > 内置默认
///
/// - 未指定 / 空串 → [`defaults::DEFAULT_LAUNCHER_TEMPLATE`]
/// - 指定了 → 去空白 + 大写归一（"a" 与 "A" 同义），合法则采用
/// - 未知值 → 打印告警并回落到默认模板（配置写错不该让界面白屏）
pub fn resolve_launcher_template(specified: Option<&str>) -> String {
    let Some(raw) = specified.map(str::trim).filter(|v| !v.is_empty()) else {
        return defaults::DEFAULT_LAUNCHER_TEMPLATE.to_string();
    };
    let normalized = raw.to_ascii_uppercase();
    if VALID_LAUNCHER_TEMPLATES.contains(&normalized.as_str()) {
        return normalized;
    }
    eprintln!(
        "[配置] 未知启动器模板 '{}'（可选 {}），回落到默认 {}",
        raw,
        VALID_LAUNCHER_TEMPLATES.join("/"),
        defaults::DEFAULT_LAUNCHER_TEMPLATE
    );
    defaults::DEFAULT_LAUNCHER_TEMPLATE.to_string()
}

/// 依次在 exe 同目录、当前工作目录查找 update.json，找不到时自动生成默认模板
pub fn load() -> UpdateSettings {
    let dirs = candidate_dirs();
    for dir in &dirs {
        let path = dir.join("update.json");
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<UpdateSettings>(&content) {
                    Ok(settings) => {
                        eprintln!("[配置] 已加载配置文件: {}", path.display());
                        return settings;
                    }
                    Err(e) => eprintln!("[配置] 配置文件解析失败 {}: {}", path.display(), e),
                },
                Err(e) => eprintln!("[配置] 配置文件读取失败 {}: {}", path.display(), e),
            }
        }
    }
    // 未找到配置文件：生成默认模板（exe 目录写失败时落到 cwd），不影响本次默认值运行
    for dir in &dirs {
        if generate_default_template(dir) {
            break;
        }
    }
    UpdateSettings::default()
}

/// 生成默认配置模板到指定目录（已存在时不覆盖）
///
/// 返回是否生成成功；JSON 不支持注释，字段说明放在 `_说明` 字段（读取时会忽略未知字段）
fn generate_default_template(dir: &std::path::Path) -> bool {
    let path = dir.join("update.json");
    if path.exists() {
        return true;
    }
    let template = serde_json::json!({
        "_说明": {
            "manifest_url": "服务器 version.json 地址；数组表示按序故障转移，第一个失败自动切换下一个",
            "app_name": "应用名称，决定配置/下载/安装目录（%APPDATA%\\<app_name>）",
            "launch": "更新完成后拉起的目标程序；相对路径按安装目录解析；留空则自动定位安装目录中的主程序拉起（文件名与应用名匹配者优先）",
            "current_version": "强制指定当前版本；留空自动从本地 app.json 探测",
            "max_versions": "保留的版本目录数量（含当前版本，旧目录即回滚资产），默认 2",
            "services": "更新前需卸载的 Windows 服务名数组；运行中的先停止后卸载，卸载后不恢复不重启",
            "launcher_template": "启动器界面模板，可选 A/B/C/D/E（大小写不敏感）：A 信息密度 · B 环形居中 · C 顶部步骤条 · D 极简留白 · E 日志为主；留空或未知值用默认 C"
        },
        "manifest_url": defaults::DEFAULT_MANIFEST_URLS,
        "app_name": defaults::DEFAULT_APP_NAME,
        "launch": "qsm.exe",
        "current_version": "",
        "max_versions": DEFAULT_MAX_VERSIONS,
        "services": ["QsmServer"],
        "launcher_template": defaults::DEFAULT_LAUNCHER_TEMPLATE
    });
    let Ok(content) = serde_json::to_string_pretty(&template) else {
        eprintln!("[配置] 默认模板序列化失败");
        return false;
    };
    match std::fs::write(&path, content) {
        Ok(_) => {
            eprintln!("[配置] 未找到配置文件，已生成默认模板: {}", path.display());
            true
        }
        Err(e) => {
            eprintln!("[配置] 默认模板生成失败 {}: {}", path.display(), e);
            false
        }
    }
}

/// 候选查找目录：exe 所在目录优先，其次当前工作目录
fn candidate_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd);
    }
    dirs
}

// 测试里构造失败即用例本身失败，按 clippy 惯例局部放行 expect_used
#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_or_many_single_string() {
        let v: OneOrMany =
            serde_json::from_str(r#""https://example.com/version.json""#).expect("单地址应可解析");
        assert_eq!(v.to_urls(), vec!["https://example.com/version.json".to_string()]);
    }

    #[test]
    fn one_or_many_array_preserves_order() {
        // 多地址按序故障转移，顺序即优先级，不得被打乱
        let v: OneOrMany = serde_json::from_str(r#"["https://a/1", "https://b/2", "https://c/3"]"#)
            .expect("地址数组应可解析");
        assert_eq!(
            v.to_urls(),
            vec![
                "https://a/1".to_string(),
                "https://b/2".to_string(),
                "https://c/3".to_string()
            ]
        );
    }

    #[test]
    fn settings_empty_json_all_none() {
        let s: UpdateSettings = serde_json::from_str("{}").expect("空配置应可解析");
        assert!(s.manifest_url.is_none());
        assert!(s.app_name.is_none());
        assert!(s.launch.is_none());
        assert!(s.current_version.is_none());
        assert!(s.max_versions.is_none());
        assert!(s.services.is_none());
        assert!(s.launcher_template.is_none());
    }

    #[test]
    fn settings_full_json() {
        let s: UpdateSettings = serde_json::from_str(
            r#"{
                "manifest_url": ["https://a/1", "https://b/2"],
                "app_name": "qsm",
                "launch": "qsm.exe",
                "current_version": "1.2.3",
                "max_versions": 3,
                "services": ["QsmServer", "QsmHelper"],
                "launcher_template": "B"
            }"#,
        )
        .expect("完整配置应可解析");
        assert_eq!(s.app_name.as_deref(), Some("qsm"));
        assert_eq!(s.launch.as_deref(), Some("qsm.exe"));
        assert_eq!(s.current_version.as_deref(), Some("1.2.3"));
        assert_eq!(s.max_versions, Some(3));
        assert_eq!(s.launcher_template.as_deref(), Some("B"));
        assert_eq!(
            s.services,
            Some(vec!["QsmServer".to_string(), "QsmHelper".to_string()])
        );
        assert_eq!(s.manifest_url.map(|m| m.to_urls()).map(|u| u.len()), Some(2));
    }

    #[test]
    fn settings_unknown_fields_ignored() {
        // 生成的模板带 _说明 文档字段（对象），读取时必须忽略
        let s: UpdateSettings = serde_json::from_str(
            r#"{"_说明": {"manifest_url": "..."}, "app_name": "qsm"}"#,
        )
        .expect("未知字段应被忽略");
        assert_eq!(s.app_name.as_deref(), Some("qsm"));
    }

    #[test]
    fn settings_max_versions_zero_is_parsed_not_defaulted() {
        // 0 是合法解析值（是否回退默认由 resolve 层过滤，此处只验证解析）
        let s: UpdateSettings = serde_json::from_str(r#"{"max_versions": 0}"#).expect("应可解析");
        assert_eq!(s.max_versions, Some(0));
    }

    #[test]
    fn launcher_template_absent_falls_back_to_default() {
        // 未配置 → 默认模板 C
        let s: UpdateSettings = serde_json::from_str("{}").expect("空配置应可解析");
        assert!(s.launcher_template().is_none());
        assert_eq!(resolve_launcher_template(s.launcher_template().as_deref()), "C");
    }

    #[test]
    fn launcher_template_explicit_overrides_default() {
        // 显式指定优先于默认：五套模板都应能被正确选中
        for key in ["A", "B", "C", "D", "E"] {
            let json = format!(r#"{{"launcher_template": "{key}"}}"#);
            let s: UpdateSettings = serde_json::from_str(&json).expect("配置应可解析");
            assert_eq!(
                resolve_launcher_template(s.launcher_template().as_deref()),
                key
            );
        }
    }

    #[test]
    fn launcher_template_case_and_space_insensitive() {
        // 大小写与首尾空白归一："a" / " A " 都视为 A
        let s: UpdateSettings =
            serde_json::from_str(r#"{"launcher_template": " a "}"#).expect("应可解析");
        assert_eq!(s.launcher_template().as_deref(), Some("A"));
        assert_eq!(resolve_launcher_template(Some("d")), "D");
    }

    #[test]
    fn launcher_template_empty_string_uses_default() {
        // 配置留空视为未指定（与 launch/current_version 的 "" 语义一致）
        let s: UpdateSettings =
            serde_json::from_str(r#"{"launcher_template": ""}"#).expect("应可解析");
        assert!(s.launcher_template().is_none());
        assert_eq!(resolve_launcher_template(s.launcher_template().as_deref()), "C");
    }

    #[test]
    fn launcher_template_unknown_value_falls_back_to_default() {
        // 未知模板不得让界面白屏：回落到默认 C
        assert_eq!(resolve_launcher_template(Some("Z")), "C");
        assert_eq!(resolve_launcher_template(Some("")), "C");
        assert_eq!(resolve_launcher_template(None), "C");
    }

    #[test]
    fn launcher_template_default_constant_is_valid() {
        assert!(
            VALID_LAUNCHER_TEMPLATES.contains(&defaults::DEFAULT_LAUNCHER_TEMPLATE),
            "默认模板必须是合法键之一"
        );
    }

    #[test]
    fn repo_update_json_is_parseable() {
        // 仓库根目录的 update.json 必须能被当前结构解析（字段名/类型对不上会直接失败）
        // CARGO_MANIFEST_DIR = <仓库>/src-core，故上级目录即仓库根
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../update.json")
            .canonicalize()
            .expect("仓库根 update.json 应存在");
        let content = std::fs::read_to_string(&path).expect("读取 update.json");
        let s: UpdateSettings = serde_json::from_str(&content).expect("update.json 应可解析");
        assert_eq!(
            resolve_launcher_template(s.launcher_template().as_deref()),
            "C",
            "仓库默认配置应落到默认模板 C"
        );
    }

    #[test]
    fn default_max_versions_constant() {
        assert_eq!(DEFAULT_MAX_VERSIONS, 2, "当前版本 + 1 个回滚资产");
    }
}
