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

// ---- 参数清单：文档的唯一来源 ----
//
// README §4.2 的用法行与参数表、使用手册 4.1 的同名两段，都由下面的
// `CLI_SPECS` 渲染出来：
//     cargo run -p qsltd-updater-core --example gen_cli_docs -- --write   # 重写文档
//     cargo run -p qsltd-updater-core --example gen_cli_docs -- --check   # 校验（CI 硬门禁）
// 新增/修改参数时改这里 + `parse_from`，再让脚本重写文档；手改 README/手册
// 会被一致性检查打回。

/// 一个命令行参数的文档元数据
pub struct CliSpec {
    /// 参数名（含 `--`）
    pub flag: &'static str,
    /// 值占位符；`None` = 布尔开关
    pub value: Option<&'static str>,
    /// 说明（支持 `反引号` 行内代码）
    pub desc: &'static str,
    /// 默认值说明（支持 `反引号` 行内代码）
    pub default: &'static str,
    /// 仅 src-egui 提供（本地预览）
    pub egui_only: bool,
}

impl CliSpec {
    /// 「--flag <值>」或「--flag」
    fn signature(&self) -> String {
        match self.value {
            Some(v) => format!("{} <{}>", self.flag, v),
            None => self.flag.to_string(),
        }
    }
}

/// 全部对外参数（内部提权标记 `--__elevated-retry` 不在此列）
pub const CLI_SPECS: &[CliSpec] = &[
    CliSpec {
        flag: "--app-name",
        value: Some("名称"),
        desc: "应用名，决定配置 / 下载 / 安装目录与拉取的清单",
        default: "未给时取 update.json 的 `app_name`，再回落内置默认 `qsm`",
        egui_only: false,
    },
    CliSpec {
        flag: "--name",
        value: Some("标题"),
        desc: "启动器标题：标题栏左侧文字与系统窗口标题。单行、最长 64 字符（换行剔除、超长截断，界面上再按可用宽度以省略号收尾）。只影响显示，不参与目录派生",
        default: "未给时取 update.json 的 `name`，再回落编译期烘焙标题（无烘焙即「启动器」）",
        egui_only: false,
    },
    CliSpec {
        flag: "--launch",
        value: Some("目标程序路径"),
        desc: "更新完成后拉起的目标程序；相对路径按安装目录解析",
        default: "空 = 自动定位安装目录中的主程序",
        egui_only: false,
    },
    CliSpec {
        flag: "--manifest",
        value: Some("manifest地址"),
        desc: "服务器 version.json 地址，覆盖 update.json 中的配置",
        default: "见 `defaults::DEFAULT_MANIFEST_URLS`",
        egui_only: false,
    },
    CliSpec {
        flag: "--current-version",
        value: Some("版本"),
        desc: "强制指定当前版本，覆盖本地 app.json 的探测结果",
        default: "自动探测",
        egui_only: false,
    },
    CliSpec {
        flag: "--template",
        value: Some("A~H"),
        desc: "界面模板 A~H（大小写不敏感）",
        default: "未给时取 update.json 的 `launcher_template`，再回落编译期烘焙值（无烘焙即 `F`）",
        egui_only: false,
    },
    CliSpec {
        flag: "--check-only",
        value: None,
        desc: "纯检测模式：不建窗、不下载、不拉起目标程序，按退出码结束（0 无更新 / 1 检测失败 / 2 有更新可跳过 / 3 必须更新）",
        default: "关闭",
        egui_only: false,
    },
    CliSpec {
        flag: "--allow-missing-sha256",
        value: None,
        desc: "放行「清单没给 SHA256」的更新包，关闭 `require_sha256` 校验闸。仅在服务端确实无法提供校验值时临时使用；只能关不能开",
        default: "关闭（即校验闸开启）",
        egui_only: false,
    },
    CliSpec {
        flag: "--preview",
        value: None,
        desc: "仅 src-egui：本地预览八套布局与各阶段画面，不联网",
        default: "关闭",
        egui_only: true,
    },
    CliSpec {
        flag: "--stage",
        value: Some("0-10|名称"),
        desc: "仅 src-egui：`--preview` 的阶段。0 检测中、1 下载中（分卷）、2 校验中、3 解压/安装中、4 发现新版本、5 已是最新、6 更新完成、7 错误、8 重试中、9 强制更新（阻断）、10 强制更新（可稍后升级）；也可传中文名称，未知值回落 1",
        default: "1",
        egui_only: true,
    },
];

/// 用法行（多行、按宽度折行；`indent` 用于续行对齐）
pub fn usage_text() -> String {
    /// 终端显示宽度：ASCII 记 1，CJK 等宽字符记 2（避免中文占位符把行撑长）
    fn width(s: &str) -> usize {
        s.chars()
            .map(|c| if (c as u32) < 0x1100 { 1 } else { 2 })
            .sum()
    }

    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::from("launcherclient.exe");
    let mut cur_w = width(&cur);
    const INDENT: &str = "                   ";
    for spec in CLI_SPECS {
        let token = format!("[{}]", spec.signature());
        let token_w = width(&token);
        // 约 78 列折行：一行放不下就换到缩进续行
        if cur_w + 1 + token_w > 78 && cur != "launcherclient.exe" {
            lines.push(cur);
            cur = String::from(INDENT);
            cur_w = INDENT.len();
        }
        if cur == INDENT {
            cur.push_str(&token);
        } else {
            cur.push(' ');
            cur.push_str(&token);
        }
        cur_w += token_w + 1;
    }
    lines.push(cur);
    lines.join("\n")
}

/// Markdown 表格（README §4.2）
pub fn table_markdown() -> String {
    let mut out = String::from("| 参数 | 说明 | 默认 |\n| --- | --- | --- |\n");
    for spec in CLI_SPECS {
        out.push_str(&format!(
            "| `{}` | {} | {} |\n",
            md_cell(&spec.signature()),
            md_cell(spec.desc),
            md_cell(spec.default)
        ));
    }
    out
}

/// HTML 表格（使用手册 4.1，与站内样式一致）
pub fn table_html() -> String {
    let mut out = String::from(
        "<div class=\"table-wrap\">\n      <table>\n        <thead><tr><th>参数</th><th>说明</th><th>默认</th></tr></thead>\n        <tbody>\n",
    );
    for spec in CLI_SPECS {
        out.push_str(&format!(
            "          <tr><td class=\"nowrap\"><code>{}</code></td><td>{}</td><td>{}</td></tr>\n",
            html_inline(&spec.signature()),
            html_inline(spec.desc),
            html_inline(spec.default)
        ));
    }
    out.push_str("        </tbody>\n      </table>\n    </div>\n");
    out
}

/// Markdown 表格单元格转义（竖线会破坏表格结构）
fn md_cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// HTML 行内转义 + `反引号` → `<code>`
fn html_inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_code = false;
    for ch in s.chars() {
        match ch {
            '`' => {
                out.push_str(if in_code { "</code>" } else { "<code>" });
                in_code = !in_code;
            }
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    if in_code {
        out.push_str("</code>");
    }
    out
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

    #[test]
    fn cli_specs_have_unique_flags_and_cover_parse_targets() {
        // 文档表里每个参数都必须是解析器认识的那个名字，且不能重复
        let mut seen = std::collections::HashSet::new();
        for spec in CLI_SPECS {
            assert!(
                spec.flag.starts_with("--"),
                "参数名必须带 --：{}",
                spec.flag
            );
            assert!(seen.insert(spec.flag), "参数名重复：{}", spec.flag);
        }
        // 内部提权标记不进文档
        assert!(!seen.contains("--__elevated-retry"));
        for expect in [
            "--app-name",
            "--name",
            "--launch",
            "--manifest",
            "--current-version",
            "--template",
            "--check-only",
            "--allow-missing-sha256",
            "--preview",
            "--stage",
        ] {
            assert!(seen.contains(expect), "CLI_SPECS 缺少 {expect}");
        }
    }

    #[test]
    fn rendered_docs_mention_every_spec() {
        let usage = usage_text();
        let md = table_markdown();
        let html = table_html();
        for spec in CLI_SPECS {
            assert!(usage.contains(spec.flag), "用法行缺少 {}", spec.flag);
            assert!(md.contains(spec.flag), "Markdown 表缺少 {}", spec.flag);
            assert!(html.contains(spec.flag), "HTML 表缺少 {}", spec.flag);
            // 一行一个参数：表格行数必须与清单一致
        }
        assert_eq!(md.lines().count(), CLI_SPECS.len() + 2);
    }

    #[test]
    fn html_escapes_angle_brackets_and_markdown_escapes_pipes() {
        // 占位符里的 <名称> 在 HTML 里必须转义，否则被当成标签吞掉
        let html = table_html();
        assert!(html.contains("&lt;名称&gt;"));
        assert!(!html.contains("--app-name <"));
        // 竖线在 Markdown 表格里会切列，必须转义
        let md = table_markdown();
        assert!(md.contains("0-10\\|名称"));
    }
}
