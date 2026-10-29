//! 应用数据目录解析（`<用户配置目录>\<app_name>`）
//!
//! 从自研能力库 `qingshu` 的 `update::get_myapp_dir` 复刻而来（2026-10-30 起
//! 本仓库不再依赖该外部 crate，能力内聚到此模块）。行为与旧实现**逐条对齐**，
//! 因为老用户机器上的 `app.json`、已装版本目录、manifest 缓存全都落在这个路径下——
//! 换目录等于让所有人的历史安装凭空消失。对齐点：
//!
//! 1. `app_name` 为 `None` 或空串 → 用固定名 `qsltd-update`
//! 2. 可在 exe 旁（`config/config.toml`、`config.toml`）或当前工作目录的
//!    `config.toml` 里用 `[appdata] dir = "..."` **整盘接管**根目录：
//!    命中后**直接返回该目录，不再拼 `app_name`**（旧行为如此，保留）；
//!    指向 SYSTEM 账户目录的配置视为无效，继续走默认解析
//! 3. 基目录取平台配置目录（Windows 为 `%APPDATA%`）。若它落在
//!    `C:\Windows\System32|SysWOW64\...`（服务/SYSTEM 上下文里的典型形态），
//!    依次退回 `%LOCALAPPDATA%\<name>` → `%PROGRAMDATA%\QsLtdService\<name>` → 当前工作目录
//!
//! 与旧实现的两处**有意**差异：不引入 `dirs` crate（直接读环境变量，Windows 上
//! 与 `dirs::config_dir()` 同值）；日志从 `tracing` 换成本仓库统一的 `eprintln!`。

use std::path::{Path, PathBuf};

/// `app_name` 缺省时使用的固定目录名（与旧 `qingshu` 实现一致）
pub const DEFAULT_APP_DIR_NAME: &str = "qsltd-update";

/// 应用数据根目录：`<配置目录>\<app_name>`
///
/// 见模块头的三条规则。返回值不保证存在，调用方按需 `create_dir_all`。
pub fn app_dir(app_name: Option<&str>) -> PathBuf {
    let name = match app_name {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => DEFAULT_APP_DIR_NAME.to_string(),
    };

    // 规则 2：config.toml 显式接管（命中即用，不拼 name）
    if let Some(config_path) = config_file_path() {
        if let Some(override_dir) = read_appdata_dir(&config_path) {
            if is_valid_appdata_path(&override_dir) {
                eprintln!(
                    "[路径] 使用 config.toml 中的 appdata.dir: {:?}",
                    override_dir
                );
                return override_dir;
            }
            eprintln!(
                "[路径] config.toml 的 appdata.dir 指向系统账户目录，忽略: {:?}",
                override_dir
            );
        }
    }

    let base = base_config_dir();

    // 规则 3：SYSTEM 上下文里的配置目录不能用来放用户数据
    if let Some(dir) = base.as_ref() {
        if is_system_profile(dir) {
            eprintln!("[路径] 检测到 SYSTEM 用户目录 {:?}，改用备用路径", dir);
            return system_fallback_dir(&name);
        }
    }

    base.unwrap_or_else(current_dir_or_dot).join(name)
}

/// 平台配置目录（相当于 `dirs::config_dir()`，不引额外依赖）
///
/// Windows `%APPDATA%` · macOS `~/Library/Application Support` ·
/// Linux `$XDG_CONFIG_HOME` 或 `~/.config`
fn base_config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library").join("Application Support"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(x));
        }
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
    }
}

/// SYSTEM/服务上下文的备用落点，按可用性依次尝试（与旧实现同序）
fn system_fallback_dir(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let dir = PathBuf::from(local).join(name);
            eprintln!("[路径] 使用 LOCALAPPDATA: {:?}", dir);
            return dir;
        }
        if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
            let dir = PathBuf::from(program_data).join("QsLtdService").join(name);
            eprintln!("[路径] 使用 PROGRAMDATA: {:?}", dir);
            return dir;
        }
    }
    let dir = current_dir_or_dot().join(name);
    eprintln!("[路径] 找不到合适的备用路径，使用当前工作目录: {:?}", dir);
    dir
}

fn current_dir_or_dot() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// 查找配置文件：exe 同目录 `config/config.toml` > exe 同目录 `config.toml` > cwd `config.toml`
fn config_file_path() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let nested = exe_dir.join("config").join("config.toml");
            if nested.exists() {
                return Some(nested);
            }
            let side_by_side = exe_dir.join("config.toml");
            if side_by_side.exists() {
                return Some(side_by_side);
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let in_cwd = cwd.join("config.toml");
        if in_cwd.exists() {
            return Some(in_cwd);
        }
    }
    None
}

/// 从 config.toml 里读 `[appdata]` 段的 `dir`
///
/// 手写行解析（不引 toml 依赖，与旧实现一致）：只认 `[appdata]` 段内的
/// `dir = "..."`，单/双引号都剥，段外的同名键不生效。
fn read_appdata_dir(config_path: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(config_path).ok()?;
    let mut in_appdata_section = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_appdata_section = line.eq("[appdata]");
            continue;
        }
        if in_appdata_section && line.starts_with("dir") && line.contains('=') {
            if let Some(value) = line.split('=').nth(1) {
                let path = value.trim().trim_matches('"').trim_matches('\'');
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }
    }
    None
}

/// 配置里给的目录是否可用：SYSTEM 账户目录一律不可用
fn is_valid_appdata_path(path: &Path) -> bool {
    !is_system_profile(path) && !lowercase(path).contains("systemprofile")
}

/// 是否落在 Windows 系统账户的配置目录里（服务进程以 SYSTEM 运行时的典型形态）
fn is_system_profile(path: &Path) -> bool {
    let p = lowercase(path);
    p.contains(r"c:\windows\system32\config\systemprofile")
        || p.contains(r"c:\windows\syswow64\config\systemprofile")
        || p.starts_with(r"c:\windows\system32")
        || p.starts_with(r"c:\windows\syswow64")
}

/// 小写化的路径字符串：Windows 路径大小写不敏感，判定前先归一
///
/// 旧实现对所有平台都直接小写；非 Windows 路径大小写敏感，故只在 Windows
/// 上小写，其余原样返回（避免把 `/Users/X` 这类真实路径判成别处）。
fn lowercase(path: &Path) -> String {
    #[cfg(windows)]
    {
        path.to_string_lossy().to_lowercase()
    }
    #[cfg(not(windows))]
    {
        path.to_string_lossy().into_owned()
    }
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个唯一临时目录
    fn temp_dir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("qsltd_appdir_test_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
    }

    // ---------- app_name 归一 ----------

    #[test]
    fn app_dir_appends_name_to_base_config_dir() {
        let dir = app_dir(Some("qsm"));
        assert!(dir.ends_with("qsm"), "应以应用名为末级目录: {:?}", dir);
        // 基目录取不到时退到 cwd，但末级名不变
        assert!(
            dir.is_absolute() || dir.starts_with("."),
            "路径应可派生: {:?}",
            dir
        );
    }

    #[test]
    fn app_dir_uses_default_name_for_none_and_empty() {
        assert!(app_dir(None).ends_with(DEFAULT_APP_DIR_NAME));
        assert!(app_dir(Some("")).ends_with(DEFAULT_APP_DIR_NAME));
    }

    // ---------- 配置文件解析 ----------

    #[test]
    fn reads_dir_only_from_appdata_section() {
        let dir = temp_dir("toml");
        let cfg = dir.join("config.toml");
        std::fs::write(
            &cfg,
            "[other]\ndir = \"/should/be/ignored\"\n\n[appdata]\ndir = \"D:/LauncherData\"\n",
        )
        .expect("写配置");
        assert_eq!(
            read_appdata_dir(&cfg),
            Some(PathBuf::from("D:/LauncherData"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strips_quotes_and_accepts_single_quotes() {
        let dir = temp_dir("toml_quote");
        let cfg = dir.join("config.toml");
        std::fs::write(&cfg, "[appdata]\ndir = 'E:/data'\n").expect("写配置");
        assert_eq!(read_appdata_dir(&cfg), Some(PathBuf::from("E:/data")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_appdata_section_yields_none() {
        let dir = temp_dir("toml_missing");
        let cfg = dir.join("config.toml");
        std::fs::write(&cfg, "[service]\nport = 8888\n").expect("写配置");
        assert_eq!(read_appdata_dir(&cfg), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------- SYSTEM 目录判定 ----------

    #[cfg(windows)]
    #[test]
    fn detects_system_profile_paths_case_insensitively() {
        for bad in [
            r"C:\Windows\System32\config\systemprofile\AppData\Roaming",
            r"c:\windows\syswow64\config\systemprofile\AppData\Roaming",
            r"C:\Windows\System32\winebuild\thing",
        ] {
            let p = Path::new(bad);
            assert!(is_system_profile(p), "应识别为系统目录: {bad}");
            assert!(!is_valid_appdata_path(p), "系统目录不得作为接管目录: {bad}");
        }
        let ok = Path::new(r"C:\Users\admin\AppData\Roaming");
        assert!(!is_system_profile(ok));
        assert!(is_valid_appdata_path(ok));
    }

    #[cfg(windows)]
    #[test]
    fn system_fallback_prefers_localappdata() {
        // 服务上下文里 APPDATA 指向 systemprofile，数据应落到 LOCALAPPDATA 下
        let dir = system_fallback_dir("qsm");
        let expected = std::env::var_os("LOCALAPPDATA")
            .map(|l| PathBuf::from(l).join("qsm"))
            .unwrap_or_else(|| PathBuf::from(".").join("qsm"));
        assert_eq!(dir, expected, "备用路径应落在 LOCALAPPDATA: {:?}", dir);
        assert!(dir.ends_with("qsm"));
    }

    #[test]
    fn valid_appdata_path_rejects_any_systemprofile_mention() {
        // 与旧实现一致：即便不是 System32 前缀，路径里出现 systemprofile 也判无效
        let p = Path::new(r"D:\somewhere\systemprofile\data");
        #[cfg(windows)]
        assert!(!is_valid_appdata_path(p));
        // 非 Windows 上大小写敏感，这条只验函数不炸
        #[cfg(not(windows))]
        let _ = p;
    }
}
