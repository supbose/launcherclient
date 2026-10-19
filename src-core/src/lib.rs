//! QsLtd 更新程序共享核心（无 UI 层）
//!
//! 由两套实现共同依赖：
//! - `src-tauri`：Tauri 2 + Vue 3 版（事件驱动推送阶段状态）
//! - `src-zsui`：zsui 原生 UI 版（共享快照驱动视图刷新）
//!
//! 本 crate 承载与界面无关的全部后端模块；UI 编排层（updater.rs）留在各自 crate 中，
//! 通过调用这里的模块完成「配置 → 检查 → 下载 → 安装 → 清理」全流程。

pub mod cli;
pub mod cleanup;
pub mod config;
pub mod defaults;
pub mod download;
pub mod install;
pub mod manifest;
pub mod services;
