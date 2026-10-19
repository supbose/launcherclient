//! 更新程序内置默认值（编译期常量，两套实现共用）
//!
//! 这些值同时被以下位置引用，保证「回退值」与「生成的配置模板」始终一致：
//! - `config::generate_default_template()`：生成 update.json 模板时引用
//! - `src-tauri` / `src-zsui` 的 `updater.rs`：命令行与 update.json 均未配置时的最终回退
//!
//! 如需调整默认值，只改本文件一处，两套实现与配置模板同步生效。

/// 默认服务器 manifest 地址（按序故障转移，第一个失败自动切换下一个）
pub const DEFAULT_MANIFEST_URLS: &[&str] = &[
    "https://qsc.cdn.qcsm.ltd/update/qsm/version.json",
    "https://qsw.us.ci/update/qsm/version.json",
];

/// 默认应用名称（决定配置/下载/安装目录）
pub const DEFAULT_APP_NAME: &str = "qsm";

/// 进度事件推送间隔（毫秒）：UI 编排层的节流参数，与后端逻辑无关，
/// 仅两版 updater 的进度回调使用，故也集中到此处保持一致
pub const PROGRESS_INTERVAL_MS: u64 = 300;
