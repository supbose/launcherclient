//! 命令行参数解析
//!
//! 用法：
//! `QsLtdUpdater.exe --app-name <名称> [--name <标题>] [--launch <目标程序路径>] [--manifest <manifest地址>] [--template <A~H>] [--check-only] [--allow-missing-sha256]`
//!
//! `--app-name` 与 `--name` 是两件事：前者决定配置/下载/安装目录，
//! 后者只是界面标题栏（含系统窗口标题）显示的文字。
//!
//! `--allow-missing-sha256` 关掉「清单必须给校验值」这条保护：默认情况下，
//! 服务端 manifest 既没给整包 `sha256`、分卷也没给齐 `part_sha256` 时，更新器
//! 拒绝自动安装（包完整性没有任何证据）。只有确认服务器与链路都可信的
//! 内网/自建环境才需要它；等价配置项是 update.json 的 `require_sha256: false`。

/// 解析后的命令行参数
pub struct CliArgs {
    /// 应用名称，决定配置/下载/安装目录（默认取 `defaults::DEFAULT_APP_NAME`）
    pub app_name: String,
    /// 启动器标题（标题栏左侧文字 + 系统窗口标题，可选，覆盖 update.json 与烘焙默认）
    pub launcher_name: Option<String>,
    /// 更新完成后要拉起的目标程序路径（可选，缺省时自动定位安装目录中的主程序）
    pub launch: Option<String>,
    /// 服务器 manifest 地址（可选，覆盖 update.json 与默认地址）
    pub manifest_url: Option<String>,
    /// 当前已安装版本（可选，覆盖本地 app.json 记录）
    pub current_version: Option<String>,
    /// 启动器界面模板（可选，覆盖 update.json 与默认模板；A~H）
    pub template: Option<String>,
    /// 纯检测模式：只检查是否有更新，不建窗、不下载、不拉回目标程序。
    /// 退出码：0=无更新 1=检测失败 2=有更新（可跳过） 3=必须更新（供客户端快速分支）
    pub check_only: bool,
    /// 显式允许「清单没给 SHA256」的包继续自动安装（默认 false = 拒绝）。
    /// 见模块头的用法说明；与 update.json 的 `require_sha256` 是同一条开关的两面。
    pub allow_missing_sha256: bool,
}

/// 解析命令行参数，未知参数仅打印警告不中断
pub fn parse() -> CliArgs {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    parse_from(&argv)
}

/// 从给定参数列表解析（不含程序名本身），供测试注入
pub fn parse_from(argv: &[String]) -> CliArgs {
    let mut app_name = String::from("QsLtd");
    let mut launcher_name: Option<String> = None;
    let mut launch: Option<String> = None;
    let mut manifest_url: Option<String> = None;
    let mut current_version: Option<String> = None;
    let mut template: Option<String> = None;
    let mut check_only = false;
    let mut allow_missing_sha256 = false;

    let mut i = 0;
    while i < argv.len() {
        let flag = argv[i].as_str();
        let value = argv.get(i + 1).map(|s| s.as_str());
        match flag {
            "--app-name" => match value {
                Some(v) => {
                    app_name = v.to_string();
                    i += 1;
                }
                None => eprintln!("[参数] --app-name 缺少值，使用默认值: {}", app_name),
            },
            "--name" => match value {
                Some(v) => {
                    launcher_name = Some(v.to_string());
                    i += 1;
                }
                None => eprintln!("[参数] --name 缺少值，使用默认标题"),
            },
            "--launch" => match value {
                Some(v) => {
                    launch = Some(v.to_string());
                    i += 1;
                }
                None => eprintln!("[参数] --launch 缺少值，忽略该参数"),
            },
            "--manifest" => match value {
                Some(v) => {
                    manifest_url = Some(v.to_string());
                    i += 1;
                }
                None => eprintln!("[参数] --manifest 缺少值，使用默认地址"),
            },
            "--current-version" => match value {
                Some(v) => {
                    current_version = Some(v.to_string());
                    i += 1;
                }
                None => eprintln!("[参数] --current-version 缺少值，忽略该参数"),
            },
            "--check-only" => check_only = true,
            "--allow-missing-sha256" => allow_missing_sha256 = true,
            // 内部提权标记（runas 重新拉起的副本携带），非用户参数，静默忽略
            "--__elevated-retry" => {}
            "--template" => match value {
                Some(v) => {
                    template = Some(v.to_string());
                    i += 1;
                }
                None => eprintln!("[参数] --template 缺少值，使用默认模板"),
            },
            other => eprintln!("[参数] 未知参数: {}，已忽略", other),
        }
        i += 1;
    }

    CliArgs {
        app_name,
        launcher_name,
        launch,
        manifest_url,
        current_version,
        template,
        check_only,
        allow_missing_sha256,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_empty_uses_defaults() {
        let c = parse_from(&[]);
        assert_eq!(c.app_name, "QsLtd");
        assert!(c.launcher_name.is_none());
        assert!(c.launch.is_none());
        assert!(c.manifest_url.is_none());
        assert!(c.current_version.is_none());
        assert!(c.template.is_none());
        assert!(!c.check_only);
        assert!(!c.allow_missing_sha256);
    }

    #[test]
    fn parse_all_flags() {
        let c = parse_from(&args(&[
            "--app-name",
            "qsm",
            "--name",
            "青杉应用中心",
            "--launch",
            "D:/app/qsm.exe",
            "--manifest",
            "https://example.com/version.json",
            "--current-version",
            "1.2.3",
            "--template",
            "B",
            "--check-only",
        ]));
        assert_eq!(c.app_name, "qsm");
        assert_eq!(c.launcher_name.as_deref(), Some("青杉应用中心"));
        assert_eq!(c.launch.as_deref(), Some("D:/app/qsm.exe"));
        assert_eq!(
            c.manifest_url.as_deref(),
            Some("https://example.com/version.json")
        );
        assert_eq!(c.current_version.as_deref(), Some("1.2.3"));
        assert_eq!(c.template.as_deref(), Some("B"));
        assert!(c.check_only);
    }

    #[test]
    fn parse_name_without_value_is_none() {
        // 缺值不得把标题变成空串：保持 None，由 resolve 层回落默认标题
        assert!(parse_from(&args(&["--name"])).launcher_name.is_none());
    }

    #[test]
    fn parse_name_takes_last_and_keeps_spaces() {
        // 带空格的标题按一个值消费；重复传取最后一次
        let c = parse_from(&args(&[
            "--name",
            "A Launcher",
            "--app-name",
            "qsm",
            "--name",
            "青杉 应用中心",
        ]));
        assert_eq!(c.launcher_name.as_deref(), Some("青杉 应用中心"));
        assert_eq!(c.app_name, "qsm");
    }

    #[test]
    fn parse_name_is_independent_of_app_name() {
        // --name 只影响标题，不参与目录派生（--app-name 才是目录名）
        let c = parse_from(&args(&["--name", "My App"]));
        assert_eq!(c.app_name, "QsLtd");
        assert_eq!(c.launcher_name.as_deref(), Some("My App"));
    }

    #[test]
    fn parse_template_without_value_is_none() {
        assert!(parse_from(&args(&["--template"])).template.is_none());
    }

    #[test]
    fn parse_ignores_unknown_flag() {
        let c = parse_from(&args(&["--whatever", "x", "--app-name", "qsm"]));
        assert_eq!(c.app_name, "qsm");
    }

    #[test]
    fn parse_launch_without_value_is_none() {
        let c = parse_from(&args(&["--launch"]));
        assert!(c.launch.is_none());
    }

    #[test]
    fn parse_manifest_without_value_keeps_none() {
        let c = parse_from(&args(&["--manifest"]));
        assert!(c.manifest_url.is_none());
    }

    #[test]
    fn parse_current_version_without_value_is_none() {
        let c = parse_from(&args(&["--current-version"]));
        assert!(c.current_version.is_none());
    }

    #[test]
    fn parse_app_name_without_value_keeps_default() {
        let c = parse_from(&args(&["--app-name"]));
        assert_eq!(c.app_name, "QsLtd");
    }

    #[test]
    fn parse_check_only_stands_alone() {
        assert!(parse_from(&args(&["--check-only"])).check_only);
        assert!(!parse_from(&args(&["--app-name", "qsm"])).check_only);
    }

    #[test]
    fn parse_allow_missing_sha256_defaults_off() {
        // 默认必须关着：安全闸门的开关不靠用户记得传参
        assert!(!parse_from(&args(&["--app-name", "qsm"])).allow_missing_sha256);
        assert!(parse_from(&args(&["--allow-missing-sha256"])).allow_missing_sha256);
        // 独立开关，不吃后面的值
        let c = parse_from(&args(&["--allow-missing-sha256", "--app-name", "qsm"]));
        assert!(c.allow_missing_sha256);
        assert_eq!(c.app_name, "qsm");
    }

    #[test]
    fn parse_value_looking_like_flag_is_consumed_as_value() {
        // --launch 的值即使以 -- 开头也按值消费（简单顺序解析的既定行为）
        let c = parse_from(&args(&["--launch", "--weird"]));
        assert_eq!(c.launch.as_deref(), Some("--weird"));
    }

    #[test]
    fn parse_repeated_flag_takes_last() {
        let c = parse_from(&args(&["--app-name", "a", "--app-name", "b"]));
        assert_eq!(c.app_name, "b");
    }
}
