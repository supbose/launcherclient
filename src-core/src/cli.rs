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
    let mut app_name = String::from("QsLtd");
    let mut launch: Option<String> = None;
    let mut manifest_url: Option<String> = None;
    let mut current_version: Option<String> = None;
    let mut check_only = false;

    let argv: Vec<String> = std::env::args().skip(1).collect();
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
