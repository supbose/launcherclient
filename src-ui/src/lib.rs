//! egui 原生 UI 原语：主题色板/字体/标题栏/spinner/进度条等，
//! 由 `launcherclient`（src-egui）与 `pkgtool`（src-pkgtool）共用。
//!
//! 只依赖 `egui`，不依赖业务 crate：这里放的是「画什么、用什么颜色」，
//! 「什么时候画、写了什么文案」留在各自的应用里。

pub mod fonts;
pub mod theme;
pub mod widgets;
