//! QsLtd 更新程序共享核心（无 UI 层）
//!
//! 由各 UI 实现共同依赖：
//! - `src-egui`：egui/eframe 原生 UI 版（共享快照轮询 + 八套模板复刻，当前唯一生产壳）
//!
//! 注：`src-tauri`（Tauri 2 + Vue 3 版）与 `src-zsui`（zsui 原生 UI 版）均已于 2026-10-26 删除，
//! 不再维护，也不再依赖本 crate；其阶段推送逻辑（事件 emit）已并入本共享编排层。
//!
//! 本 crate 承载与界面无关的全部后端模块；UI 编排层统一收敛到
//! [`crate::flow`]（启动决策 + 更新全流程，阶段经 [`crate::phase::PhaseHandle`]
//! 推给各 UI），各 UI crate 只保留视图层装配。

pub mod appdir;
pub mod cli;
pub mod cleanup;
pub mod client;
pub mod config;
pub mod defaults;
pub mod download;
pub mod elevation;
pub mod flow;
pub mod install;
pub mod instance;
pub mod local_manifest;
pub mod manifest;
pub mod phase;
pub mod services;
pub mod shell;
