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
//! 优先级：命令行参数 > update.json > 内置默认值

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
}

/// max_versions 默认值：当前版本 + 1 个旧版本（回滚资产）
pub const DEFAULT_MAX_VERSIONS: usize = 2;

/// 依次在 exe 同目录、当前工作目录查找 update.json，找不到返回默认配置
pub fn load() -> UpdateSettings {
    for dir in candidate_dirs() {
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
    UpdateSettings::default()
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
