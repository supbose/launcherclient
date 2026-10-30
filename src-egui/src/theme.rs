//! 主题（色板 / 字体 / 尺寸）已迁到共享 crate `qsltd-ui`。
//!
//! 这里只做转出，让八套模板与各界面继续使用 `crate::theme::*` 的短路径；
//! 真正改色值请到 `src-ui/src/theme.rs`（pkgtool 用的是同一份）。
//! 新代码也可以直接 `use qsltd_ui::theme;`。

pub use qsltd_ui::theme::*;
