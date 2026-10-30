//! 本地配置文件 update.json 读取
//!
//! 配置文件与更新器 exe 同目录（其次查找当前工作目录），flat 结构：
//!
//! ```json
//! {
//!   "manifest_url": "https://qsw.us.ci/update/qsm/version.json",
//!   "app_name": "qsm",
//!   "name": "启动器",
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
    /// 启动器标题：标题栏左侧文字与系统窗口标题。
    /// 留空（或整项缺失）用 exe 编译期烘焙的默认标题（`--name` 构建参数）
    #[serde(default)]
    pub name: Option<String>,
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
    /// 启动器界面模板：F 主模板 / H 副模板（大小写不敏感）。
    /// 缺省或空串 = 自动判定（无参直启 F，带参调用 H）；非法值回落到主模板 F。
    /// 取值校验见 [`UpdateSettings::launcher_template`]
    #[serde(default)]
    pub launcher_template: Option<String>,
    /// 官网首页地址：下载地址全挂时错误态按钮「前往官网」跳这里；
    /// 缺省或空串用 [`defaults::DEFAULT_HOMEPAGE`]
    #[serde(default)]
    pub homepage: Option<String>,
    /// 同一环节（下载 / 解压校验）的自动重试次数（含首次尝试），
    /// 缺省用 [`defaults::DEFAULT_RETRY_TIMES`]，0 或 1 视为只试一次
    #[serde(default)]
    pub retry_times: Option<u32>,
    /// 是否要求服务端清单提供包完整性校验值（缺省 = true = 要求）。
    /// 显式 `false` 才放行「没给 SHA256 的包」，命令行 `--allow-missing-sha256`
    /// 是同义出口；判定见 [`resolve_require_sha256`]。
    #[serde(default)]
    pub require_sha256: Option<bool>,
    /// 分卷并发下载数（同时下载的卷数，1~8）。缺省用
    /// [`DEFAULT_PARTS_CONCURRENCY`]；CDN 单连接限速时并发能明显提速，
    /// 服务端限制连接数时调小。归一规则见 [`resolve_parts_concurrency`]。
    #[serde(default)]
    pub parts_concurrency: Option<u32>,
    /// 检查更新失败（网络/服务端不可达）时是否直接启动已安装版本（缺省 false）。
    /// 仅当本地安装有效时生效，见 [`resolve_fail_open`]。
    #[serde(default)]
    pub fail_open: Option<bool>,
}

impl UpdateSettings {
    /// 生效的启动器界面模板
    ///
    /// 未配置（None）或空串 → 自动判定（见 [`resolve_launcher_template`]）；
    /// 配置了则大写归一（"f"/"F"/" f " 同义）。只做归一与去空，合法性由调用方
    /// （resolve 层）校验后回落，此处保留原值以便上层按「是否显式指定」判断优先级。
    pub fn launcher_template(&self) -> Option<String> {
        self.launcher_template
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_ascii_uppercase())
    }

    /// 生效的启动器标题原值（未归一长度）
    ///
    /// 只做去首尾空白与「空串视为未配置」，与 `launch`/`current_version` 的
    /// `""` 语义一致；换行剔除与长度上限由 [`resolve_launcher_name`] 负责。
    pub fn launcher_name(&self) -> Option<String> {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    }
}

/// 解析生效的自动重试次数（含首次尝试）
///
/// 未配置 → 默认值；显式 0 → 同 1（「试一次」而不是「不试」，配置笔误
/// 不该让下载变成空转）。上限 10：再多也只是让用户对着进度条等更久。
pub fn resolve_retry_times(specified: Option<u32>) -> u32 {
    match specified {
        None => defaults::DEFAULT_RETRY_TIMES,
        Some(0) => 1,
        Some(n) => n.min(10),
    }
}

/// 解析「清单必须提供包完整性校验值」开关的生效值
///
/// 默认**开启**：更新器是自替换、可能提权的程序，没有 SHA256 就对下载来的
/// 包毫无完整性证据，装坏包/装被动过手脚的包的代价远高于「服务器忘了填」。
/// 两条关得掉的路（都只用于自建/内网这类服务器与链路都可信的环境）：
/// - 命令行 `--allow-missing-sha256`
/// - update.json `"require_sha256": false`
///
/// 命令行只能**关**不能开（它本身就是「allow-missing」），配置项缺省按 true 处理。
pub fn resolve_require_sha256(cli_allows_missing: bool, settings: Option<bool>) -> bool {
    if cli_allows_missing {
        return false;
    }
    settings.unwrap_or(true)
}

/// 分卷并发下载的默认卷数
///
/// 3 路是「限速 CDN 上明显提速」与「不把服务端连接数打满」之间的折中：
/// 单连接已跑满带宽时并发只会更慢一点，不会明显变差。
pub const DEFAULT_PARTS_CONCURRENCY: usize = 3;

/// 分卷并发上限：再多也只是让磁盘/带宽排队，还容易触发服务端限流
pub const MAX_PARTS_CONCURRENCY: usize = 8;

/// 解析生效的分卷并发数
///
/// 未配置 → 默认 3；显式 0 → 1（「不并发」而不是「不下载」，与
/// [`resolve_retry_times`] 的 0 语义一致）；超过上限夹到 8。
pub fn resolve_parts_concurrency(specified: Option<u32>) -> usize {
    match specified {
        None => DEFAULT_PARTS_CONCURRENCY,
        Some(n) => (n as usize).clamp(1, MAX_PARTS_CONCURRENCY),
    }
}

/// 解析「检查失败时放行启动」开关
///
/// 默认关闭：更新器的主职责是把版本更新到位，静默跳过检查属于产品决策，
/// 必须由部署方显式打开。真正的「本地安装有效」判断在编排层
/// （[`crate::flow`]），这里只做配置归一。
pub fn resolve_fail_open(settings: Option<bool>) -> bool {
    settings.unwrap_or(false)
}

/// max_versions 默认值：当前版本 + 1 个旧版本（回滚资产）；构建期可经 QSLTD_MAX_VERSIONS 覆盖
pub const DEFAULT_MAX_VERSIONS: usize = match defaults::BUILD_MAX_VERSIONS {
    Some(n) => n as usize,
    None => 2,
};

/// 合法启动器界面模板键（与 `src-egui/src/templates.rs` 的两套布局一一对应）
///
/// F = 主模板（环形进度 + 步骤条），H = 副模板（极简单行状态）。
/// 旧版 A~E/G 已删除，配置里残留这些值会告警并回落到主模板 F。
pub const VALID_LAUNCHER_TEMPLATES: &[&str] = &["F", "H"];

/// 副模板键：启动时带命令行参数（视作被其他应用调用）时的自动选择
pub const SECONDARY_LAUNCHER_TEMPLATE: &str = "H";

/// 启动器标题最长字符数（按字符计，中文算 1 个）
///
/// 与 `scripts/pybuild.py` 的 `LAUNCHER_NAME_MAX` 同值：标题栏单行可容纳上限，
/// 超出即截断（界面侧再按可用宽度以省略号收尾，不挤压右侧按钮）。
pub const LAUNCHER_NAME_MAX: usize = 64;

/// 解析生效的启动器标题：命令行 `--name` > update.json `name` > 编译期烘焙默认
///
/// - 两者优先级在此集中（命令行显式给了非空值才压过配置文件），
///   归一规则见下；两者都没给 → [`defaults::DEFAULT_LAUNCHER_NAME`]
///   （构建期 `pybuild.py --name` 烘焙值，未烘焙即「启动器」）
/// - 换行与制表符剔除（标题是单行文本，配置手滑不该让标题栏断行）
/// - 超过 [`LAUNCHER_NAME_MAX`] 字符 → 截断并告警（写错长度不该顶掉关闭按钮）
pub fn resolve_launcher_name(cli: Option<&str>, file: Option<&str>) -> String {
    let from_cli = cli.map(str::trim).filter(|v| !v.is_empty());
    let Some(raw) = from_cli.or(file).map(str::trim).filter(|v| !v.is_empty()) else {
        return defaults::DEFAULT_LAUNCHER_NAME.to_string();
    };
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '\r' | '\n' | '\t'))
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        eprintln!(
            "[配置] 启动器标题只有空白字符，回落到内置默认「{}」",
            defaults::DEFAULT_LAUNCHER_NAME
        );
        return defaults::DEFAULT_LAUNCHER_NAME.to_string();
    }
    if cleaned.chars().count() > LAUNCHER_NAME_MAX {
        let truncated: String = cleaned.chars().take(LAUNCHER_NAME_MAX).collect();
        eprintln!(
            "[配置] 启动器标题超过 {} 字符，已截断为 {:?}",
            LAUNCHER_NAME_MAX, truncated
        );
        return truncated;
    }
    cleaned.to_string()
}

/// 解析生效的启动器界面模板：命令行 > update.json > 自动判定
///
/// - 未指定 / 空串 → 自动判定：`invoked_with_args` 为真（被其他应用带参调用）用副模板 H，
///   否则用主模板 F（[`defaults::DEFAULT_LAUNCHER_TEMPLATE`]）
/// - 指定了 → 去空白 + 大写归一（"f" 与 "F" 同义），F/H 合法则采用
/// - 旧版键（A~E/G）或未知值 → 打印告警并回落到主模板 F（配置写错不该让界面白屏）
pub fn resolve_launcher_template(specified: Option<&str>, invoked_with_args: bool) -> String {
    let Some(raw) = specified.map(str::trim).filter(|v| !v.is_empty()) else {
        return if invoked_with_args {
            SECONDARY_LAUNCHER_TEMPLATE.to_string()
        } else {
            defaults::DEFAULT_LAUNCHER_TEMPLATE.to_string()
        };
    };
    let normalized = raw.to_ascii_uppercase();
    if VALID_LAUNCHER_TEMPLATES.contains(&normalized.as_str()) {
        return normalized;
    }
    eprintln!(
        "[配置] 不支持的启动器模板 '{}'（可选 {}），回落到主模板 {}",
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
            "name": "启动器标题（标题栏左侧文字与系统窗口标题）；留空用 exe 内置标题（构建期 --name 烘焙，默认「启动器」），单行、最长 64 字符",
            "launch": "更新完成后拉起的目标程序；相对路径按安装目录解析；留空则自动定位安装目录中的主程序拉起（文件名与应用名匹配者优先）",
            "current_version": "强制指定当前版本；留空自动从本地 app.json 探测",
            "max_versions": "保留的版本目录数量（含当前版本，旧目录即回滚资产），默认 2",
            "services": "更新前需卸载的 Windows 服务名数组；运行中的先停止后卸载，卸载后不恢复不重启",
            "launcher_template": "启动器界面模板：留空 = 自动（双击直启用主模板 F，被其他应用带参数调用用副模板 H）；也可填 F/H 强制指定，旧版 A~E/G 等值回落到 F",
            "homepage": "官网首页地址；更新检查（manifest 全部地址失效）或下载地址全部失败时，界面上的「前往官网重新下载」按钮跳这里，留空用内置默认",
            "retry_times": "同一环节（下载 / 解压校验）的自动重试次数（含首次尝试），默认 3：失败后自动再试 2 次，仍失败才提示前往官网重新下载",
            "require_sha256": "是否要求服务器清单提供包完整性校验值，默认 true：整包 sha256 缺失且分卷 part_sha256 也没给齐时拒绝自动安装。自建/内网等可信环境确需放行时设为 false（等价于命令行 --allow-missing-sha256）",
            "parts_concurrency": "分卷并发下载数（同时下载的卷数），默认 3，可选 1~8：CDN 对单连接限速时并发能明显提速；服务端限制连接数或小带宽环境可调小到 1",
            "fail_open": "检查更新失败（网络/服务器不可达）时是否直接启动已安装版本，默认 false：仅当本地安装目录有效时生效，适合弱网下不希望启动被检查卡住的启动器；强制更新语义不受影响（能拿到清单时仍按清单执行）"
        },
        "manifest_url": defaults::DEFAULT_MANIFEST_URLS,
        "app_name": defaults::DEFAULT_APP_NAME,
        // 标题留空 = 跟随 exe 内置值：换一版烘焙了 --name 的 exe 覆盖安装后，
        // 老目录里的 update.json 不会把旧标题钉死（与 current_version 的留空语义一致）
        "name": "",
        "launch": format!("{}.exe", defaults::DEFAULT_APP_NAME),
        "current_version": "",
        "max_versions": DEFAULT_MAX_VERSIONS,
        "services": defaults::DEFAULT_SERVICES,
        // 留空 = 自动判定：双击直启 F、被其他应用带参调用 H；
        // 想固定某一套再显式填 F/H（与 name 的留空跟随语义一致）
        "launcher_template": "",
        "homepage": defaults::DEFAULT_HOMEPAGE,
        "retry_times": defaults::DEFAULT_RETRY_TIMES,
        // 显式写出默认值：这条开关关掉就是放弃包完整性校验，写在配置里比
        // 藏在「缺省即 true」里更容易被接手的人看见
        "require_sha256": true,
        "parts_concurrency": DEFAULT_PARTS_CONCURRENCY,
        // 放行启动会跳过更新检查，必须显式开启；默认关闭才是更新器的本分
        "fail_open": false
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
        assert_eq!(
            v.to_urls(),
            vec!["https://example.com/version.json".to_string()]
        );
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
        assert!(s.parts_concurrency.is_none());
        assert!(s.fail_open.is_none());
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
                "launcher_template": "H"
            }"#,
        )
        .expect("完整配置应可解析");
        assert_eq!(s.app_name.as_deref(), Some("qsm"));
        assert_eq!(s.launch.as_deref(), Some("qsm.exe"));
        assert_eq!(s.current_version.as_deref(), Some("1.2.3"));
        assert_eq!(s.max_versions, Some(3));
        assert_eq!(s.launcher_template.as_deref(), Some("H"));
        assert_eq!(
            s.services,
            Some(vec!["QsmServer".to_string(), "QsmHelper".to_string()])
        );
        assert_eq!(
            s.manifest_url.map(|m| m.to_urls()).map(|u| u.len()),
            Some(2)
        );
    }

    #[test]
    fn settings_unknown_fields_ignored() {
        // 生成的模板带 _说明 文档字段（对象），读取时必须忽略
        let s: UpdateSettings =
            serde_json::from_str(r#"{"_说明": {"manifest_url": "..."}, "app_name": "qsm"}"#)
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
    fn launcher_template_absent_uses_auto_detection() {
        // 未配置 → 自动：无参直启 F，带参（被其他应用调用）H
        let s: UpdateSettings = serde_json::from_str("{}").expect("空配置应可解析");
        assert!(s.launcher_template().is_none());
        assert_eq!(
            resolve_launcher_template(s.launcher_template().as_deref(), false),
            "F"
        );
        assert_eq!(
            resolve_launcher_template(s.launcher_template().as_deref(), true),
            "H"
        );
    }

    // ---------- require_sha256（包完整性闸门） ----------

    #[test]
    fn require_sha256_absent_means_on() {
        // 缺省即要求：老 update.json 一个字没改，也自动获得这层保护
        let s: UpdateSettings = serde_json::from_str("{}").expect("空配置应可解析");
        assert!(s.require_sha256.is_none());
        assert!(resolve_require_sha256(false, s.require_sha256));
    }

    #[test]
    fn require_sha256_explicit_false_disables() {
        let s: UpdateSettings =
            serde_json::from_str(r#"{"require_sha256": false}"#).expect("应可解析");
        assert_eq!(s.require_sha256, Some(false));
        assert!(!resolve_require_sha256(false, s.require_sha256));
    }

    #[test]
    fn require_sha256_cli_escape_hatch_overrides_config_true() {
        // 命令行只用来「放行」：写了 --allow-missing-sha256 就关，配置写 true 也不再强开
        assert!(!resolve_require_sha256(true, Some(true)));
        assert!(!resolve_require_sha256(true, None));
        // 反过来不行：命令行没有强开语义
        assert!(resolve_require_sha256(false, None));
    }

    // ---------- 分卷并发 / fail-open ----------

    #[test]
    fn parts_concurrency_defaults_and_clamps() {
        assert_eq!(
            resolve_parts_concurrency(None),
            DEFAULT_PARTS_CONCURRENCY,
            "未配置用默认 3 路"
        );
        // 0 是配置笔误：按「不并发」处理，而不是「不下载」
        assert_eq!(resolve_parts_concurrency(Some(0)), 1);
        assert_eq!(resolve_parts_concurrency(Some(1)), 1);
        assert_eq!(resolve_parts_concurrency(Some(4)), 4);
        // 超过上限夹到 8：再多只会触发服务端限流
        assert_eq!(resolve_parts_concurrency(Some(99)), MAX_PARTS_CONCURRENCY);
    }

    #[test]
    fn parts_concurrency_field_parses() {
        let s: UpdateSettings =
            serde_json::from_str(r#"{"parts_concurrency": 4}"#).expect("应可解析");
        assert_eq!(s.parts_concurrency, Some(4));
        assert_eq!(resolve_parts_concurrency(s.parts_concurrency), 4);
    }

    #[test]
    fn fail_open_defaults_off_and_requires_explicit_opt_in() {
        // 默认关闭：跳过更新检查必须是部署方的显式决定
        assert!(!resolve_fail_open(None));
        assert!(!resolve_fail_open(Some(false)));
        assert!(resolve_fail_open(Some(true)));
        let s: UpdateSettings = serde_json::from_str(r#"{"fail_open": true}"#).expect("应可解析");
        assert!(resolve_fail_open(s.fail_open));
    }

    #[test]
    fn launcher_template_explicit_overrides_auto_detection() {
        // 显式指定优先于自动判定：即使带参调用，配置 H/F 也照用
        for key in ["F", "H"] {
            let json = format!(r#"{{"launcher_template": "{key}"}}"#);
            let s: UpdateSettings = serde_json::from_str(&json).expect("配置应可解析");
            for invoked in [false, true] {
                assert_eq!(
                    resolve_launcher_template(s.launcher_template().as_deref(), invoked),
                    key
                );
            }
        }
    }

    #[test]
    fn launcher_template_case_and_space_insensitive() {
        // 大小写与首尾空白归一：" f " / "h" 都按 F/H 处理
        let s: UpdateSettings =
            serde_json::from_str(r#"{"launcher_template": " f "}"#).expect("应可解析");
        assert_eq!(s.launcher_template().as_deref(), Some("F"));
        assert_eq!(resolve_launcher_template(Some("h"), false), "H");
    }

    #[test]
    fn launcher_template_empty_string_uses_auto_detection() {
        // 配置留空视为未指定（与 launch/current_version 的 "" 语义一致）
        let s: UpdateSettings =
            serde_json::from_str(r#"{"launcher_template": ""}"#).expect("应可解析");
        assert!(s.launcher_template().is_none());
        assert_eq!(
            resolve_launcher_template(s.launcher_template().as_deref(), false),
            "F"
        );
        assert_eq!(
            resolve_launcher_template(s.launcher_template().as_deref(), true),
            "H"
        );
    }

    #[test]
    fn legacy_or_unknown_template_falls_back_to_main() {
        // 旧版 A~E/G 与未知值不得让界面白屏：一律回落到主模板 F，
        // 且显式旧值不再参与「带参用 H」的自动判定
        for old in ["A", "B", "C", "D", "E", "G", "Z"] {
            assert_eq!(resolve_launcher_template(Some(old), false), "F");
            assert_eq!(resolve_launcher_template(Some(old), true), "F");
        }
        assert_eq!(resolve_launcher_template(Some(""), true), "H");
        assert_eq!(resolve_launcher_template(None, true), "H");
    }

    #[test]
    fn launcher_template_default_constant_is_valid() {
        assert!(
            VALID_LAUNCHER_TEMPLATES.contains(&defaults::DEFAULT_LAUNCHER_TEMPLATE),
            "默认模板必须是合法键之一"
        );
        assert!(
            VALID_LAUNCHER_TEMPLATES.contains(&SECONDARY_LAUNCHER_TEMPLATE),
            "副模板必须是合法键之一"
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
            resolve_launcher_template(s.launcher_template().as_deref(), false),
            "F",
            "仓库默认配置应落到主模板 F"
        );
        // 仓库配置留空模板，带参调用应自动切副模板
        assert_eq!(
            resolve_launcher_template(s.launcher_template().as_deref(), true),
            "H"
        );
    }

    #[test]
    fn homepage_field_parsed_and_defaulted() {
        // 配了就用配的
        let s: UpdateSettings =
            serde_json::from_str(r#"{"homepage": "https://example.com"}"#).expect("解析");
        assert_eq!(s.homepage.as_deref(), Some("https://example.com"));
        // 没配就是 None，由 resolve_config 回落到 DEFAULT_HOMEPAGE
        let s: UpdateSettings = serde_json::from_str("{}").expect("解析");
        assert!(s.homepage.is_none());
    }

    #[test]
    fn launcher_name_field_parsed() {
        let s: UpdateSettings = serde_json::from_str(r#"{"name": "青杉应用中心"}"#).expect("解析");
        assert_eq!(s.launcher_name().as_deref(), Some("青杉应用中心"));
        // 未配置 / 空串 / 全空白都视为「跟随 exe 内置标题」
        assert!(UpdateSettings::default().launcher_name().is_none());
        let s: UpdateSettings = serde_json::from_str(r#"{"name": ""}"#).expect("解析");
        assert!(s.launcher_name().is_none());
        let s: UpdateSettings = serde_json::from_str(r#"{"name": "   "}"#).expect("解析");
        assert!(s.launcher_name().is_none());
    }

    #[test]
    fn launcher_name_absent_falls_back_to_baked_default() {
        assert_eq!(
            resolve_launcher_name(None, None),
            defaults::DEFAULT_LAUNCHER_NAME.to_string()
        );
        assert_eq!(
            resolve_launcher_name(Some(""), Some("")),
            defaults::DEFAULT_LAUNCHER_NAME.to_string()
        );
    }

    #[test]
    fn launcher_name_explicit_wins_and_trims() {
        assert_eq!(resolve_launcher_name(Some("My App"), None), "My App");
        assert_eq!(
            resolve_launcher_name(None, Some("  青杉启动器 \n")),
            "青杉启动器"
        );
    }

    #[test]
    fn launcher_name_priority_command_line_over_update_json() {
        // 命令行 > update.json > 烘焙默认（与 --template / manifest_url 同构）
        assert_eq!(
            resolve_launcher_name(Some("命令行标题"), Some("配置文件标题")),
            "命令行标题"
        );
        assert_eq!(
            resolve_launcher_name(None, Some("配置文件标题")),
            "配置文件标题"
        );
        // 命令行只给了空白 = 没给，仍取配置文件值
        assert_eq!(
            resolve_launcher_name(Some("   "), Some("配置文件标题")),
            "配置文件标题"
        );
    }

    #[test]
    fn launcher_name_strips_line_breaks_keeps_single_line() {
        // 标题只有一行：配置里混进换行/制表符要剔除，不能让标题栏断行
        assert_eq!(
            resolve_launcher_name(None, Some("青杉\r\n应用中心")),
            "青杉应用中心"
        );
        assert_eq!(resolve_launcher_name(Some("A\tB"), None), "AB");
        // 剔完只剩空白 → 回落内置默认
        assert_eq!(
            resolve_launcher_name(None, Some("\r\n\t ")),
            defaults::DEFAULT_LAUNCHER_NAME.to_string()
        );
    }

    #[test]
    fn launcher_name_over_length_is_truncated_not_rejected() {
        // 超长截断到上限（保留可用标题），而不是整项丢弃
        let long = "标".repeat(LAUNCHER_NAME_MAX + 20);
        let got = resolve_launcher_name(Some(&long), None);
        assert_eq!(got.chars().count(), LAUNCHER_NAME_MAX);
        assert!(got.chars().all(|c| c == '标'));
    }

    #[test]
    fn generated_template_keeps_name_empty() {
        // 生成的模板里 name 留空：跟随 exe 烘焙值，避免把旧标题钉进部署目录
        let s: UpdateSettings = serde_json::from_str(r#"{"name": ""}"#).expect("解析");
        assert!(s.launcher_name().is_none());
        assert_eq!(
            resolve_launcher_name(s.launcher_name().as_deref(), None),
            defaults::DEFAULT_LAUNCHER_NAME.to_string()
        );
    }

    #[test]
    fn retry_times_resolution() {
        // 未配置用默认 3；0 视为「只试一次」而不是「不试」
        assert_eq!(resolve_retry_times(None), defaults::DEFAULT_RETRY_TIMES);
        assert_eq!(resolve_retry_times(Some(0)), 1);
        assert_eq!(resolve_retry_times(Some(1)), 1);
        assert_eq!(resolve_retry_times(Some(5)), 5);
        // 上限 10：避免配置手滑写个 1000 让用户干等
        assert_eq!(resolve_retry_times(Some(1000)), 10);
    }

    #[test]
    fn default_max_versions_constant() {
        assert_eq!(DEFAULT_MAX_VERSIONS, 2, "当前版本 + 1 个回滚资产");
    }

    #[test]
    fn repo_update_json_has_no_removed_fields() {
        // store_url / force_update 已迁到服务端 manifest，本地配置不应再出现
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../update.json")
            .canonicalize()
            .expect("仓库根 update.json 应存在");
        let content = std::fs::read_to_string(&path).expect("读取 update.json");
        let s: UpdateSettings = serde_json::from_str(&content).expect("update.json 应可解析");
        assert!(s.manifest_url.is_some(), "仓库示例应带 manifest_url");
        assert!(
            !content.contains("store_url"),
            "update.json 不应再含 store_url"
        );
        assert!(
            !content.contains("force_update"),
            "update.json 不应再含 force_update"
        );
    }
}
