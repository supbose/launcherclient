//! 更新程序内置默认值（编译期常量，两套实现共用）
//!
//! 这些值同时被以下位置引用，保证「回退值」与「生成的配置模板」始终一致：
//! - `config::generate_default_template()`：生成 update.json 模板时引用
//! - 命令行与 update.json 均未配置时的最终回退
//!
//! 如需调整默认值，只改本文件一处，配置模板与运行时回退同步生效。
//!
//! **构建期覆盖**：`build.rs` 依据 `QSLTD_*` 环境变量生成 `BUILD_*` Option 常量
//! （`include!` 引入），传入即覆盖、未传回落此处 qsm 默认。
//! 入口：`python scripts/pybuild.py egui --app-name <名称> ...`。

include!(concat!(env!("OUT_DIR"), "/build_overrides.rs"));

/// 默认服务器 manifest 地址（按序故障转移，第一个失败自动切换下一个）
pub const DEFAULT_MANIFEST_URLS: &[&str] = match BUILD_MANIFEST_URLS {
    Some(urls) => urls,
    None => &[
        "https://qsc.cdn.qcsm.ltd/update/version.json",
        "https://launcherclient.qsw.us.ci/update/version.json",
    ],
};

/// 默认官网首页地址（下载地址全部失败时「前往官网」按钮跳这里）。
/// 官网具体域名随产品线走，通过 update.json 的 `homepage` 覆盖；
/// 保持内置值非空，避免用户点按钮时无事发生。
pub const DEFAULT_HOMEPAGE: &str = match BUILD_HOMEPAGE {
    Some(url) => url,
    None => "https://launcherclient.qsw.us.ci/",
};

/// 默认应用名称（决定配置/下载/安装目录）
pub const DEFAULT_APP_NAME: &str = match BUILD_APP_NAME {
    Some(name) => name,
    None => "qsm",
};

/// 默认启动器标题（标题栏左侧文字，同时作为系统窗口标题）
///
/// 构建期可经 `--name` / `QSLTD_NAME` 烘焙覆盖；未传参数时用内置默认「启动器」。
/// 入口：`python scripts/pybuild.py egui --name <标题>`。
pub const DEFAULT_LAUNCHER_NAME: &str = match BUILD_NAME {
    Some(name) => name,
    None => "启动器",
};

/// 默认启动器界面模板（launcher template）
///
/// 主模板 `F`（环形进度 + 步骤条），对应 `src-egui/src/templates.rs` 的 `template_f`。
/// 带参调用时由 `config::resolve_launcher_template` 自动改用副模板 `H`；
/// 与 DEFAULT_MANIFEST_URLS 等一样：配置模板生成与两套实现的最终回退
/// 都引用此处，改一处即全局生效。
pub const DEFAULT_LAUNCHER_TEMPLATE: &str = match BUILD_LAUNCHER_TEMPLATE {
    Some(t) => t,
    None => "F",
};

/// 默认更新前需卸载的 Windows 服务列表（仅用于生成 update.json 模板；
/// 运行时未配置 services 字段则不卸载任何服务）
pub const DEFAULT_SERVICES: &[&str] = match BUILD_SERVICES {
    Some(services) => services,
    None => &["QsmServer"],
};

/// 同一环节（下载 / 解压校验）自动重试次数
///
/// 弱网抖动、CDN 边缘节点临时 5xx 是常态，用户第一次看到错误页就点
/// 「重试」纯属把重试成本转嫁给人。次数含首次尝试：3 表示「失败后再
/// 自动重试 2 次」——保守取值，避免真挂掉时让用户干等两轮完整超时。
pub const DEFAULT_RETRY_TIMES: u32 = match BUILD_RETRY_TIMES {
    Some(n) => n,
    None => 3,
};

/// 解压产物总量上限（MiB）：防 zip bomb 的硬顶，可由 update.json 覆盖
///
/// 8 GiB 对常规启动器/桌面应用足够；游戏、离线素材包这类解压后超过此值的
/// 部署方在 update.json 里调大 `max_extract_total_mb` 即可，不必重新构建。
pub const DEFAULT_EXTRACT_TOTAL_MB: u64 = 8 * 1024;

/// 单个文件解压上限（MiB）：同样可由 update.json 覆盖。
///
/// 512 MiB 是「单个资源包/视频」与「误打包的整盘镜像」之间的保守分界；
/// 调大时注意它永远不会超过总量上限（resolve 层按总量截断）。
pub const DEFAULT_EXTRACT_ENTRY_MB: u64 = 512;

/// 进度事件推送间隔（毫秒）：UI 编排层的节流参数，与后端逻辑无关，
/// 仅两版 updater 的进度回调使用，故也集中到此处保持一致
pub const PROGRESS_INTERVAL_MS: u64 = 300;
