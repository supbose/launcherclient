//! 模板预览模式（--preview）：复刻 preview.html / Preview.vue
//!
//! 五套模板共用同一份阶段数据，样式各自独立；切换模板与阶段对比渲染。
//! 仅本地验证用，不属于核心 CLI 参数。

use egui::{Align, Color32, Layout, RichText, Sense, Ui};

use qsltd_updater_core::phase::PhasePayload;

use crate::theme;

/// 更新日志预设（Preview.vue CHANGELOG）
const CHANGELOG: [&str; 4] = [
    "新增分卷下载支持，大包更新更稳定",
    "修复更新后启动旧版二进制的问题",
    "优化启动速度与安装速度",
    "修复了某些 bugs",
];

/// 阶段预设名（与 Preview.vue STAGE_PRESETS 顺序一致）
const STAGES: [&str; 8] = [
    "检测中",
    "下载中（分卷）",
    "校验中",
    "解压/安装中",
    "发现新版本",
    "已是最新",
    "更新完成",
    "错误",
];

/// 模板元信息（index.js TEMPLATE_META）
const TEMPLATE_META: [(&str, &str, &str); 5] = [
    ("A", "信息密度", "顶对齐 · 大百分比 · 日志卡片吸底"),
    ("B", "环形居中", "环形进度 · 视觉重心居中 · 日志贴底"),
    ("C", "顶部步骤条", "四步流程条 · 顶对齐 · 日志卡片吸底"),
    ("D", "极简留白", "细进度条 · 大留白 · 日志内联无卡片"),
    ("E", "日志为主", "进度压成状态条 · 日志占主体"),
];

/// 预览面板状态
#[derive(Debug, Clone)]
pub struct PreviewState {
    /// 界面模板键（A/B/C/D/E）
    pub template: String,
    /// 阶段预设下标（STAGES 顺序）
    pub stage: usize,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            template: "C".to_string(),
            stage: 1, // 下载中（分卷），与 Preview.vue 初始值一致
        }
    }
}

impl PreviewState {
    /// 指定初始模板与阶段下标构造
    pub fn with_stage(template: &str, stage: usize) -> Self {
        Self {
            template: template.to_string(),
            stage: stage.min(STAGES.len() - 1),
        }
    }

    /// 该预设是否视为「已进入过下载阶段」（步骤条判定）
    pub fn has_downloaded(&self) -> bool {
        self.stage != 0
    }
}

/// 阶段名/下标 → 下标（名称与 STAGES 一致；数字直接取用；未知回落 1）
pub fn stage_index(s: &str) -> Option<usize> {
    let t = s.trim();
    if let Ok(n) = t.parse::<usize>() {
        return Some(n.min(STAGES.len() - 1));
    }
    STAGES
        .iter()
        .position(|name| name.contains(t) || t.contains(name.trim_end_matches("（分卷）")))
}

/// 当前预设的阶段载荷（字段与后端 PhasePayload 一一对应）
pub fn payload(state: &PreviewState) -> PhasePayload {
    let log: Vec<String> = CHANGELOG.iter().map(|s| s.to_string()).collect();
    match state.stage {
        0 => PhasePayload::new(
            qsltd_updater_core::phase::Stage::Checking,
            "正在检查更新...",
            "",
            None,
        ),
        1 => {
            let mut p = PhasePayload::download(
                "正在下载更新包...",
                Some("分卷 1/28".to_string()),
                Some(3.4),
                (4.67 * 1024.0 * 1024.0) as u64,
                Some((138.39 * 1024.0 * 1024.0) as u64),
                Some(112 * 1024),
            );
            p.changelog = log.clone();
            p
        }
        2 => {
            let mut p = PhasePayload::new(
                qsltd_updater_core::phase::Stage::Checking,
                "正在校验更新包...",
                "校验 SHA256 完整性，确保更新包未被篡改",
                None,
            );
            p.changelog = log.clone();
            p
        }
        3 => {
            let mut p = PhasePayload::new(
                qsltd_updater_core::phase::Stage::Installing,
                "正在安装更新...",
                "正在解压更新文件...",
                Some(62.0),
            );
            p.changelog = log.clone();
            p
        }
        4 => {
            let mut p = PhasePayload::new(
                qsltd_updater_core::phase::Stage::Available,
                "发现新版本",
                "2.1.2 → 2.1.3",
                None,
            );
            p.changelog = log.clone();
            p
        }
        5 => PhasePayload::new(
            qsltd_updater_core::phase::Stage::UpToDate,
            "已是最新版本",
            "2.1.3",
            None,
        ),
        6 => {
            let mut p = PhasePayload::new(
                qsltd_updater_core::phase::Stage::Done,
                "更新完成",
                "已更新到版本 2.1.3",
                None,
            );
            p.changelog = log.clone();
            p
        }
        _ => PhasePayload::new(
            qsltd_updater_core::phase::Stage::Error,
            "更新包下载失败",
            "全部 3 个 manifest 地址均拉取失败",
            None,
        ),
    }
}

/// 底部切换条：模板 chips + 阶段 chips + 重试开关（Preview.vue 面板复刻）
pub fn draw_strip(ui: &mut Ui, state: &mut PreviewState, last_event: &mut String) {
    let (rect, _) = ui.allocate_exact_size(
        egui::Vec2::new(ui.available_width(), ui.available_height()),
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    // 面板底色（稍亮于根背景，区隔内容区）
    painter.rect_filled(rect, 0.0, theme::tint(theme::BG_BOTTOM, 255));
    painter.rect_filled(
        egui::Rect::from_min_max(rect.left_top(), egui::pos2(rect.right(), rect.top() + 1.0)),
        0.0,
        theme::W08,
    );

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(12.0, 10.0)))
            .layout(Layout::top_down(Align::LEFT)),
    );
    child.label(
        RichText::new(format!(
            "界面模板（launcher_template）· {} · {}",
            TEMPLATE_META.iter().find(|m| m.0 == state.template).map(|m| m.1).unwrap_or(""),
            TEMPLATE_META.iter().find(|m| m.0 == state.template).map(|m| m.2).unwrap_or(""),
        ))
        .size(11.0)
        .color(theme::text_alpha(140)),
    );
    child.add_space(6.0);
    child.horizontal_wrapped(|ui| {
        for (key, name, _) in TEMPLATE_META {
            if chip(ui, &format!("{} · {}", key, name), state.template == key) {
                state.template = key.to_string();
                *last_event = format!("切换模板 {}", key);
            }
            ui.add_space(6.0);
        }
    });
    child.add_space(8.0);
    child.label(RichText::new("阶段").size(11.0).color(theme::text_alpha(140)));
    child.add_space(6.0);
    child.horizontal(|ui| {
        ui.horizontal_wrapped(|ui| {
            for (i, name) in STAGES.iter().enumerate() {
                if chip(ui, name, state.stage == i) {
                    state.stage = i;
                    *last_event = format!("切换阶段 {}", name);
                }
                ui.add_space(6.0);
            }
        });
    });
    if !last_event.is_empty() {
        child.add_space(6.0);
        child.label(RichText::new(format!("最近操作：{}", last_event)).size(11.0).color(theme::text_alpha(102)));
    }
}

/// chip 按钮（预览面板用，蓝底选中态）
fn chip(ui: &mut Ui, label: &str, on: bool) -> bool {
    let font = egui::FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        font,
        if on { Color32::WHITE } else { theme::text_alpha(178) },
    );
    let size = galley.size() + egui::vec2(16.0, 8.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);
    if on {
        painter.rect_filled(rect, size.y / 2.0, theme::ACCENT);
    } else {
        if resp.hovered() {
            painter.rect_filled(rect, size.y / 2.0, theme::W10_HOVER);
        }
        painter.rect_stroke(rect, size.y / 2.0, egui::Stroke::new(1.0, theme::W18), egui::StrokeKind::Inside);
    }
    let offset = ((size - galley.size()) * 0.5).round();
    painter.galley(rect.left_top() + offset, galley, Color32::WHITE);
    let clicked = resp.clicked();
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    clicked
}
