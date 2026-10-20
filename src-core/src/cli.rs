//! 命令行参数解析
//!
//! 用法：
//! `QsLtdUpdater.exe --app-name <名称> [--launch <目标程序路径>] [--manifest <manifest地址>]`

/// 解析后的命令行参数
pub struct CliArgs {
    /// 应用名称，决定配置/下载/安装目录（默认 QsLtd）
    pub app_name: String,
    /// 更新完成后要拉起的目标程序路径（可选，缺省表示手动运行模式）
    pub launch: Option<String>,
    /// 服务器 manifest 地址（可选，覆盖 update.json 与默认地址）
    pub manifest_url: Option<String>,
    /// 当前已安装版本（可选，覆盖本地 app.json 记录）
    pub current_version: Option<String>,
    /// 纯检测模式：只检查是否有更新，不建窗、不下载、不拉回目标程序。
    /// 退出码：0=无更新 1=检测失败 2=有更新（供客户端快速分支）
    pub check_only: bool,
}

/// 解析命令行参数，未知参数仅打印警告不中断
pub fn parse() -> CliArgs {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    parse_from(&argv)
}

/// 从给定参数列表解析（不含程序名本身），供测试注入
pub fn parse_from(argv: &[String]) -> CliArgs {
    let mut app_name = String::from("QsLtd");
    let mut launch: Option<String> = None;
    let mut manifest_url: Option<String> = None;
    let mut current_version: Option<String> = None;
    let mut check_only = false;

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
            other => eprintln!("[参数] 未知参数: {}，已忽略", other),
        }
        i += 1;
    }

    CliArgs {
        app_name,
        launch,
        manifest_url,
        current_version,
        check_only,
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
        assert!(c.launch.is_none());
        assert!(c.manifest_url.is_none());
        assert!(c.current_version.is_none());
        assert!(!c.check_only);
    }

    #[test]
    fn parse_all_flags() {
        let c = parse_from(&args(&[
            "--app-name", "qsm",
            "--launch", "D:/app/qsm.exe",
            "--manifest", "https://example.com/version.json",
            "--current-version", "1.2.3",
            "--check-only",
        ]));
        assert_eq!(c.app_name, "qsm");
        assert_eq!(c.launch.as_deref(), Some("D:/app/qsm.exe"));
        assert_eq!(c.manifest_url.as_deref(), Some("https://example.com/version.json"));
        assert_eq!(c.current_version.as_deref(), Some("1.2.3"));
        assert!(c.check_only);
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
