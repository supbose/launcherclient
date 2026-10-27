//! 更新程序内置默认值（编译期常量，两套实现共用）
//!
//! 这些值同时被以下位置引用，保证「回退值」与「生成的配置模板」始终一致：
//! - `config::generate_default_template()`：生成 update.json 模板时引用
//! - `src-tauri` 的 `updater.rs`（Tauri 版，已于 2026-10-26 删除）：命令行与 update.json 均未配置时的最终回退（历史上亦含已删除的 `src-zsui`）
//!
//! 如需调整默认值，只改本文件一处，两套实现与配置模板同步生效。

/// 默认服务器 manifest 地址（按序故障转移，第一个失败自动切换下一个）
pub const DEFAULT_MANIFEST_URLS: &[&str] = &[
    "https://qsc.cdn.qcsm.ltd/update/qsm/version.json",
    "https://qsw.us.ci/update/qsm/version.json",
];

/// 默认官网首页地址（下载地址全部失败时「前往官网」按钮跳这里）。
/// 官网具体域名随产品线走，通过 update.json 的 `homepage` 覆盖；
/// 保持内置值非空，避免用户点按钮时无事发生。
pub const DEFAULT_HOMEPAGE: &str = "https://www.qcsm.ltd";

/// 默认应用名称（决定配置/下载/安装目录）
pub const DEFAULT_APP_NAME: &str = "qsm";

/// 默认启动器界面模板（launcher template）
///
/// 可选值 A~H，对应 `src-egui/src/templates.rs` 下八套独立布局
/// （A 信息密度 · B 环形居中 · C 顶部步骤条 · D 极简留白 · E 日志为主 · F 环形步骤 · G 品牌沉浸 · H 极简轻量）。
/// 与 DEFAULT_MANIFEST_URLS 等一样：配置模板生成与两套实现的最终回退
/// 都引用此处，改一处即全局生效。
pub const DEFAULT_LAUNCHER_TEMPLATE: &str = "F";

/// 同一环节（下载 / 解压校验）自动重试次数
///
/// 弱网抖动、CDN 边缘节点临时 5xx 是常态，用户第一次看到错误页就点
/// 「重试」纯属把重试成本转嫁给人。次数含首次尝试：3 表示「失败后再
/// 自动重试 2 次」——保守取值，避免真挂掉时让用户干等两轮完整超时。
pub const DEFAULT_RETRY_TIMES: u32 = 3;

/// 进度事件推送间隔（毫秒）：UI 编排层的节流参数，与后端逻辑无关，
/// 仅两版 updater 的进度回调使用，故也集中到此处保持一致
pub const PROGRESS_INTERVAL_MS: u64 = 300;
