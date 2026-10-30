//! 统一字体加载：默认使用项目内置的华为鸿蒙字体，缺失时逐字重回退系统字体。
//!
//! 逻辑复刻自 qsPlayer 的 `src/fonts.rs`，路径与字重按本项目的目录结构调整。
//!
//! # 字体来源与优先级
//!
//! 1. **内嵌资源**（launcherclient 默认）：`src-egui/assets/font/` 下的三个 ttf 由
//!    `include_bytes!` 编进 exe，单文件分发不依赖外部字体目录；调用方通过
//!    [`install_embedded`] 传入字节。woff2 是 Brotli 容器，egui 的 skrifa
//!    后端不认，必须用 ttf/otf/ttc。
//! 2. **磁盘字体目录**（pkgtool 等未内嵌的调用方；内嵌字节不可用时兜底）：
//!    exe 同级 `fonts/`、开发态 `src-egui/assets/font/`，探测顺序见 [`search_dirs`]。
//! 3. **客户端系统字体**（以上都缺失时兜底）：
//!    Windows 微软雅黑 / 黑体 / 宋体 / 等线 / Segoe UI / Arial；
//!    macOS 苹方 / 冬青黑体 / 华文黑体 / 宋体；
//!    Linux Noto Sans CJK / 思源黑体 / 文泉驿 / DejaVu / Liberation。
//!
//! # 字族
//!
//! egui 0.37 的 `FontId` 只有 size + family，没有字重字段。这里按
//! 「一个字重一个 FontFamily」实现：
//!
//! | 字族 | 来源 | 用途 |
//! |---|---|---|
//! | `Proportional` | 内置 Regular（Highest） | 正文、默认 UI 文本 |
//! | `harmony-medium` | 内置 Medium（Highest） | 次级强调 |
//! | `harmony-bold` | 内置 Bold（Highest） | 标题 |
//!
//! 每个字族的 Lowest 位置都挂了 Regular 做兜底，保证缺字时不出现豆腐块。
//! launcherclient 关闭了 eframe 的 `default_fonts`，因此字族完全由本模块建立；
//! pkgtool 仍启用默认字体，同一函数只是把字体插入既有字体链，行为不变。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Context, FontData, FontFamily, FontId, FontInsert, FontPriority, InsertFontFamily};

/// 次强调字族名（内置 Medium 字重）
pub const FAMILY_MEDIUM: &str = "harmony-medium";
/// 标题字族名（内置 Bold 字重）
pub const FAMILY_BOLD: &str = "harmony-bold";

/// 项目内置字重文件（相对 `assets/font/`）
const BUILTIN_REGULAR: &str = "HarmonyOSSansSC-Regular.ttf";
const BUILTIN_MEDIUM: &str = "HarmonyOSSansSC-Medium.ttf";
const BUILTIN_BOLD: &str = "HarmonyOSSansSC-Bold.ttf";

/// 系统字体兜底：常规。
///
/// 按 Windows → macOS → Linux 排列，每个平台先中文字体、后西文兜底。
/// launcherclient 关闭了 default_fonts；内置字体目录缺失时，若连一个候选都读不到，
/// 字族会为空并导致 epaint panic，因此各平台末尾都保留一个恒存在的西文字体
/// （Windows segoeui/arial、Linux DejaVu/Liberation）。
const SYSTEM_REGULAR: &[&str] = &[
    // Windows：微软雅黑 → 雅黑 Light → 黑体 → 宋体 → 等线
    "C:/Windows/Fonts/msyh.ttc",
    "C:/Windows/Fonts/msyhl.ttc",
    "C:/Windows/Fonts/simhei.ttf",
    "C:/Windows/Fonts/simsun.ttc",
    "C:/Windows/Fonts/Deng.ttf",
    // Windows 西文兜底
    "C:/Windows/Fonts/segoeui.ttf",
    "C:/Windows/Fonts/arial.ttf",
    // macOS：苹方 → 冬青黑体 → 华文黑体 → 宋体
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/Supplemental/Songti.ttc",
    // Linux：Noto CJK（Debian/Ubuntu、Arch、Fedora）
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-sans-cjk-ttc-fonts/NotoSansCJK-Regular.ttc",
    // Linux：思源黑体、文泉驿
    "/usr/share/fonts/adobe-source-han-sans/SourceHanSansCN-Regular.otf",
    "/usr/share/fonts/opentype/source-han-sans/SourceHanSansSC-Regular.otf",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    // Linux 西文兜底
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
];

/// 系统字体兜底：粗体（排列顺序与 [`SYSTEM_REGULAR`] 相同）
const SYSTEM_BOLD: &[&str] = &[
    // Windows
    "C:/Windows/Fonts/msyhbd.ttc",
    "C:/Windows/Fonts/simhei.ttf",
    "C:/Windows/Fonts/segoeuib.ttf",
    "C:/Windows/Fonts/arialbd.ttf",
    // macOS
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Medium.ttc",
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/google-noto-sans-cjk-ttc-fonts/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/adobe-source-han-sans/SourceHanSansCN-Bold.otf",
    "/usr/share/fonts/opentype/source-han-sans/SourceHanSansSC-Bold.otf",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
];

/// 已安装标记：`add_font` 同名重复会重复注册字体，这里做进程级幂等
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// 一个已加载的字体文件
#[derive(Clone)]
struct LoadedFont {
    /// 来源描述（日志用）
    source: String,
    /// 字体字节
    data: FontData,
}

/// 内嵌字体字节：调用方用 `include_bytes!` 提供，编进 exe。
///
/// launcherclient 在 `src-egui/src/app.rs` 里定义静态实例；pkgtool 不内嵌，
/// 继续走 [`install`] 的磁盘/系统字体路径，产物不受影响。
#[derive(Clone, Copy)]
pub struct EmbeddedFonts {
    /// Regular 字重（正文）
    pub regular: &'static [u8],
    /// Medium 字重（次级强调）
    pub medium: &'static [u8],
    /// Bold 字重（标题）
    pub bold: &'static [u8],
}

/// 安装内嵌字体（launcherclient 用）：内嵌资源优先，缺失时回退磁盘与系统字体。
pub fn install_embedded(ctx: &Context, fonts: &EmbeddedFonts) -> bool {
    install_with(ctx, Some(fonts))
}

/// 安装字体到 egui 上下文（幂等，可重复调用）
///
/// 按「磁盘内置 → 系统字体」的顺序加载，供没有内嵌字体的调用方（pkgtool）使用。
/// 返回是否成功装上了正文用字体；探测顺序见 [`search_dirs`]。
pub fn install(ctx: &Context) -> bool {
    install_with(ctx, None)
}

/// 两种入口共用的安装逻辑；`embedded` 为 None 时只走磁盘/系统字体。
fn install_with(ctx: &Context, embedded: Option<&EmbeddedFonts>) -> bool {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return true;
    }

    let dirs = search_dirs();
    let mut regular = embedded
        .and_then(|f| load_embedded(f.regular))
        .or_else(|| load_builtin(&dirs, BUILTIN_REGULAR))
        .or_else(|| load_system(SYSTEM_REGULAR));
    let mut medium = embedded
        .and_then(|f| load_embedded(f.medium))
        .or_else(|| load_builtin(&dirs, BUILTIN_MEDIUM));
    let mut bold = embedded
        .and_then(|f| load_embedded(f.bold))
        .or_else(|| load_builtin(&dirs, BUILTIN_BOLD))
        .or_else(|| load_system(SYSTEM_BOLD));

    // 极端兜底：只剩粗体/中等文件时也拿来当正文，避免字族为空
    if regular.is_none() {
        regular = medium.clone().or_else(|| bold.clone());
    }
    if medium.is_none() {
        medium = regular.clone();
    }
    if bold.is_none() {
        bold = regular.clone();
    }

    let has_regular = regular.is_some();
    if let Some(f) = regular {
        ctx.add_font(FontInsert {
            name: "app-regular".to_owned(),
            data: f.data,
            families: vec![
                InsertFontFamily {
                    family: FontFamily::Proportional,
                    priority: FontPriority::Highest,
                },
                InsertFontFamily {
                    family: FontFamily::Monospace,
                    priority: FontPriority::Lowest,
                },
                InsertFontFamily {
                    family: FontFamily::Name(FAMILY_MEDIUM.into()),
                    priority: FontPriority::Lowest,
                },
                InsertFontFamily {
                    family: FontFamily::Name(FAMILY_BOLD.into()),
                    priority: FontPriority::Lowest,
                },
            ],
        });
        eprintln!("[ui] 正文字体: {}", f.source);
    }

    if let Some(f) = medium {
        ctx.add_font(FontInsert {
            name: "app-medium".to_owned(),
            data: f.data,
            families: vec![InsertFontFamily {
                family: FontFamily::Name(FAMILY_MEDIUM.into()),
                priority: FontPriority::Highest,
            }],
        });
        eprintln!("[ui] 中等字重: {}", f.source);
    }

    if let Some(f) = bold {
        ctx.add_font(FontInsert {
            name: "app-bold".to_owned(),
            data: f.data,
            families: vec![InsertFontFamily {
                family: FontFamily::Name(FAMILY_BOLD.into()),
                priority: FontPriority::Highest,
            }],
        });
        eprintln!("[ui] 粗体字重: {}", f.source);
    }

    if !has_regular {
        eprintln!("[ui] 未找到任何可用字体（内置缺失且系统字体不可读），文字将无法渲染");
    }
    has_regular
}

/// 正文字体（Regular，即 `Proportional`）
pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

/// 次强调字体（Medium）
pub fn medium(size: f32) -> FontId {
    FontId {
        size,
        family: FontFamily::Name(FAMILY_MEDIUM.into()),
    }
}

/// 标题字体（Bold）
pub fn bold(size: f32) -> FontId {
    FontId {
        size,
        family: FontFamily::Name(FAMILY_BOLD.into()),
    }
}

/// 字体探测目录（按优先级）：
///
/// 1. `<exe 同级>/fonts/`：内嵌字节不可用时兜底（pkgtool 等未内嵌调用方）
/// 2. `<exe 上两级>/src-egui/assets/font/`：开发态 `target/<profile>/launcherclient.exe`
/// 3. `<exe 上三级>/src-egui/assets/font/`：开发态 `src-pkgtool/target/<profile>/pkgtool.exe`
/// 4. `<exe 上级>/src-egui/assets/font/`
/// 5. `<cwd>/src-egui/assets/font/`、`<cwd>/fonts/`
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.join("fonts"));
            let parent = dir.join("..");
            let grand = parent.join("..");
            let great = grand.join("..");
            for base in [parent, grand, great] {
                dirs.push(base.join("src-egui").join("assets").join("font"));
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("src-egui").join("assets").join("font"));
        dirs.push(cwd.join("fonts"));
    }

    dirs
}

/// 从内置字体目录里找字体
fn load_builtin(dirs: &[PathBuf], file: &str) -> Option<LoadedFont> {
    dirs.iter().find_map(|dir| {
        read_font(&dir.join(file)).map(|data| LoadedFont {
            source: dir.join(file).display().to_string(),
            data,
        })
    })
}

/// 校验并包装内嵌字节；签名不对时返回 None，交给后续回退
fn load_embedded(bytes: &'static [u8]) -> Option<LoadedFont> {
    if bytes.len() < 12 || !looks_like_sfnt(bytes) {
        return None;
    }
    Some(LoadedFont {
        source: "内嵌资源".to_owned(),
        data: FontData::from_static(bytes),
    })
}

/// 从系统字体候选里找字体
fn load_system(candidates: &[&str]) -> Option<LoadedFont> {
    candidates.iter().find_map(|path| {
        read_font(Path::new(path)).map(|data| LoadedFont {
            source: (*path).to_owned(),
            data,
        })
    })
}

/// 读字体文件，并做一次 sfnt 容器头校验（挡掉坏文件，避免 egui 解析时 panic）
fn read_font(path: &Path) -> Option<FontData> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 12 || !looks_like_sfnt(&bytes) {
        return None;
    }
    Some(FontData::from_owned(bytes))
}

/// 判断是不是 sfnt 容器（ttf / otf / ttc）；woff2 不是 sfnt，不能被 egui 加载
fn looks_like_sfnt(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some(
            [0x00, 0x01, 0x00, 0x00] // TrueType
                | [b't', b'r', b'u', b'e'] // TrueType（Apple）
                | [b'O', b'T', b'T', b'O'] // CFF
                | [b't', b't', b'c', b'f'] // TrueType Collection
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sfnt_signatures_are_accepted() {
        for head in [&[0u8, 1, 0, 0][..], b"true", b"OTTO", b"ttcf"] {
            assert!(looks_like_sfnt(head));
        }
    }

    #[test]
    fn woff2_and_truncated_data_are_rejected() {
        // woff2 是 Brotli 容器，egui（skrifa）不认，必须在读取时挡掉
        assert!(!looks_like_sfnt(b"wOF2\x00\x01\x00\x00"));
        assert!(!looks_like_sfnt(b""));
        assert!(!looks_like_sfnt(b"ttf"));
    }

    #[test]
    fn search_dirs_cover_deploy_and_dev_layouts() {
        let dirs = search_dirs();
        // 兜底：exe 同级 fonts/（内嵌缺失时才会用到）
        assert!(dirs.iter().any(|p| p.ends_with("fonts")));
        // 开发态：target/<profile>/ 向上回溯到仓库的 src-egui/assets/font
        assert!(dirs.iter().any(|p| p.ends_with("src-egui/assets/font")));
    }

    #[test]
    fn embedded_fonts_require_sfnt_signature() {
        // 合法 sfnt 头：接受并标记来源
        let Some(font) = load_embedded(b"\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00") else {
            panic!("合法 sfnt 头应被接受");
        };
        assert_eq!(font.source, "内嵌资源");
        // woff2 / 截断数据：拒绝，交给磁盘/系统字体回退
        assert!(load_embedded(b"wOF2\x00\x01\x00\x00\x00\x00\x00\x00").is_none());
        assert!(load_embedded(b"ttf").is_none());
    }

    #[test]
    fn system_fallbacks_cover_all_platforms() {
        // 中文字体优先；末尾必须有西文兜底，避免关闭 default_fonts 后无字体可用
        assert!(SYSTEM_REGULAR[0].ends_with("msyh.ttc"));
        assert!(SYSTEM_REGULAR.iter().any(|p| p.ends_with("segoeui.ttf")));
        assert!(SYSTEM_REGULAR.iter().any(|p| p.ends_with("arial.ttf")));
        // macOS
        assert!(SYSTEM_REGULAR.iter().any(|p| p.contains("PingFang")));
        assert!(SYSTEM_REGULAR.iter().any(|p| p.contains("STHeiti")));
        // Linux
        assert!(SYSTEM_REGULAR.iter().any(|p| p.contains("NotoSansCJK")));
        assert!(SYSTEM_REGULAR.iter().any(|p| p.contains("wqy-microhei")));
        assert!(SYSTEM_REGULAR.iter().any(|p| p.ends_with("DejaVuSans.ttf")));

        // 粗体：Windows/macOS/Linux 都要有候选
        assert!(SYSTEM_BOLD.iter().any(|p| p.ends_with("msyhbd.ttc")));
        assert!(SYSTEM_BOLD.iter().any(|p| p.contains("PingFang")));
        assert!(SYSTEM_BOLD.iter().any(|p| p.contains("NotoSansCJK-Bold")));
    }
}
