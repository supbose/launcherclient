//! QsLtd 更新程序共享核心（无 UI 层）
//!
//! 由各 UI 实现共同依赖：
//! - `src-egui`：egui/eframe 原生 UI 版（共享快照轮询 + F/H 两套模板，当前唯一生产壳）
//!
//! 注：`src-tauri`（Tauri 2 + Vue 3 版）与 `src-zsui`（zsui 原生 UI 版）均已于 2026-10-26 删除，
//! 不再维护，也不再依赖本 crate；其阶段推送逻辑（事件 emit）已并入本共享编排层。
//!
//! 本 crate 承载与界面无关的全部后端模块；UI 编排层统一收敛到
//! [`crate::flow`]（启动决策 + 更新全流程，阶段经 [`crate::phase::PhaseHandle`]
//! 推给各 UI），各 UI crate 只保留视图层装配。

// 单元测试里 `unwrap()/expect()` 是想要的写法：断言失败应当直接 panic 并带上原因，
// 换成 Result 传播只会让测试更难读。生产代码仍受 [lints.clippy] 的 deny/warn 约束。
#![cfg_attr(
    test,
    allow(clippy::expect_used, clippy::unwrap_used, clippy::err_expect)
)]

pub mod appdir;
pub mod cleanup;
pub mod cli;
pub mod client;
pub mod config;
pub mod defaults;
pub mod disk;
pub mod download;
pub mod elevation;
pub mod flow;
pub mod install;
pub mod instance;
pub mod local_manifest;
pub mod log;
pub mod manifest;
pub mod phase;
pub mod services;
pub mod shell;

/// 分卷拼接读取器（内部模块：供安装阶段从已校验分卷直接解压）
mod parts;

/// 写一行诊断日志（同时落盘到 `<app_dir>/logs/updater.log`）。
///
/// 用法与 `eprintln!` 完全一致；未初始化文件日志时回落到 stderr。
/// release 的 GUI 进程没有控制台，落盘是唯一可事后追溯的通道。
#[macro_export]
macro_rules! qlog {
    ($($arg:tt)*) => {
        $crate::log::write_line(&format!($($arg)*))
    };
}
