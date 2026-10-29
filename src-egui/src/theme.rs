//! 视觉主题：颜色、字体、尺寸常量
//!
//! 数值一一对应 Tauri + Vue 版的 CSS（`src/App.vue`、`src/components/
//! MinTitleBar.vue`、`src/components/templates/UpdateTemplateA~E.vue`），
//! 改动时保持两侧同步。

use egui::{Color32, Context, FontDefinitions, Vec2};

// ---- 窗口 ----

/// 窗口客户区尺寸（tauri.conf.json main: 440×420，逻辑像素）
pub const WINDOW_SIZE: (f32, f32) = (440.0, 420.0);
/// 标题栏高度（App.vue .updater-root padding-top / MinTitleBar .title-bar height）
pub const TITLEBAR_HEIGHT: f32 = 32.0;
/// 标题栏按钮宽度（.title-btn width 46px）
pub const TITLEBAR_BTN_WIDTH: f32 = 46.0;

// ---- 背景与文字 ----

/// 根背景渐变起色（linear-gradient(160deg, #1e2430 0%, ...)）
pub const BG_TOP: Color32 = Color32::from_rgb(0x1e, 0x24, 0x30);
/// 根背景渐变止色（..., #171c26 100%）
pub const BG_BOTTOM: Color32 = Color32::from_rgb(0x17, 0x1c, 0x26);
/// 主文字色（#e8eaf0）
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xea, 0xf0);
/// 深色环内底色（B 模板 .tpl-b__ring-in background #1a2029）
pub const RING_INNER: Color32 = Color32::from_rgb(0x1a, 0x20, 0x29);

// ---- 语义色 ----

/// 强调蓝（#4a9eff）：进度、当前步骤、主按钮、徽标、日志圆点
pub const ACCENT: Color32 = Color32::from_rgb(0x4a, 0x9e, 0xff);
/// 成功绿（#35c26e）：已完成步骤、安装 spinner、成功状态点
pub const SUCCESS: Color32 = Color32::from_rgb(0x35, 0xc2, 0x6e);
/// 错误红（#e81123）：错误状态点、关闭按钮 hover
pub const DANGER: Color32 = Color32::from_rgb(0xe8, 0x11, 0x23);
/// 错误标题色（#ff5b66）
pub const ERROR_TEXT: Color32 = Color32::from_rgb(0xff, 0x5b, 0x66);
/// 警告/强制更新色（#f0a020 琥珀橙，阻断提示用）
pub const WARNING: Color32 = Color32::from_rgb(0xf0, 0xa0, 0x20);

// ---- 透明度层级（白色/主文字色叠加，rgba(255,255,255,x) 与 rgba(232,234,240,x)）----

/// 白色叠加：步骤点底（0.12）
pub const W12: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 31);
/// 白色叠加：步骤点描边（0.18）
pub const W18: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 46);
/// 白色叠加：连接线（0.14）
pub const W14: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 36);
/// 白色叠加：进度条轨道 / 空环（0.10）
pub const W10: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 26);
/// 白色叠加：日志卡片边框（0.08）
pub const W08: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 20);
/// 白色叠加：日志卡片底（0.04）
pub const W04: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 10);
/// 白色叠加：spinner 底圈 / 按钮边框（0.15 / 0.2）
pub const W20: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 51);
/// 白色叠加：标题栏按钮 hover 底（0.1）
pub const W10_HOVER: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 26);

/// 主文字色叠加（rgba(232,234,240,x)）
#[inline]
pub fn text_alpha(a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(0xe8, 0xea, 0xf0, a)
}

/// 语义色低透明度叠层（当前步骤光晕 0.18 / 连接线 0.5 / 状态点光晕 0.5 等）
#[inline]
pub fn tint(mut c: Color32, a: u8) -> Color32 {
    c = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
    c
}

// ---- 字体 ----

/// Windows 系统中文字体候选（按序取第一个存在的；微软雅黑优先）
const FONT_CANDIDATES: [&str; 5] = [
    "C:/Windows/Fonts/msyh.ttc",
    "C:/Windows/Fonts/msyhl.ttc",
    "C:/Windows/Fonts/simhei.ttf",
    "C:/Windows/Fonts/Deng.ttf",
    "C:/Windows/Fonts/simsun.ttc",
];

/// 安装中文字体：egui 默认字体无 CJK 字形（会显示豆腐块），
/// 从系统字体目录加载微软雅黑（TTC 第 0 面 = Regular）。
///
/// 找不到任何候选字体时保留 egui 默认字体（界面仍可用，仅中文显示
/// 退化为方块 —— 非常规 Windows 环境才会发生）。
pub fn install_fonts(ctx: &Context) {
    let Some(path) = FONT_CANDIDATES
        .iter()
        .find(|p| std::path::Path::new(p).exists())
    else {
        eprintln!("[更新器] 未找到系统中文字体，中文将无法显示");
        return;
    };
    let Ok(data) = std::fs::read(path) else {
        eprintln!("[更新器] 读取系统字体失败: {}", path);
        return;
    };
    eprintln!("[更新器] 使用系统字体: {}", path);

    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".to_owned(), egui::FontData::from_owned(data).into());
    // 插到最前：中英文字形全部优先走雅黑（与 Vue 版 font-family 一致的观感）
    if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        prop.insert(0, "cjk".to_owned());
    }
    if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        mono.insert(0, "cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// 关闭全局部件间距：布局间距全部由模板代码显式控制
/// （对应 CSS flex gap / padding 的逐项复刻）
pub fn zero_item_spacing(ctx: &Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::ZERO;
    });
}
