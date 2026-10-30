//! 通用自绘控件：背景渐变、标题栏、spinner、进度条、状态点、徽标、按钮、
//! 居中文本、环形进度 —— 逐一对应 Vue 版的 CSS 类
//! （.updater-root / .title-bar / .tpl-x__spin / .tpl-x__track / .tpl-x__dot /
//!  .tpl-x__pill / .tpl-x__btn）。
//!
//! 启动器专用的控件（更新日志卡、动作行、错误提示）留在 src-egui，
//! 这里的每一个都不含业务文案，换一个应用也能直接用。

use egui::{
    emath::Pos2, epaint::PathShape, Align2, Color32, CursorIcon, FontId, Mesh, Painter, Rect,
    RichText, Sense, Shape, Stroke, Ui, Vec2,
};

use crate::theme;

/// 背景垂直渐变（linear-gradient(160deg, #1e2430 → #171c26) 的竖直近似）
///
/// egui 纯色矩形不支持渐变填充，用四顶点 gouraud 着色网格实现双过渡。
pub fn paint_bg(painter: &Painter, rect: Rect) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), theme::BG_TOP);
    mesh.colored_vertex(rect.right_top(), theme::BG_TOP);
    mesh.colored_vertex(rect.right_bottom(), theme::BG_BOTTOM);
    mesh.colored_vertex(rect.left_bottom(), theme::BG_BOTTOM);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// 标题栏按钮动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleBarAction {
    None,
    Minimize,
    Close,
}

/// 单行文本按可用宽度截断，超出部分以省略号收尾
///
/// egui 的 `Painter::text` 既不换行也不加省略号，溢出只会被裁剪区硬切一刀，
/// 中文标题就表现为「末尾露出半个字」。这里先整体测宽，装不下就从尾部逐字
/// 回退，直到「前缀 + …」完整放得下（标题最长 64 字符，逐字回退成本可忽略）。
fn ellipsize_to_width(painter: &Painter, text: &str, font: &FontId, max_width: f32) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    let width_of = |s: &str| {
        painter
            .layout_no_wrap(s.to_owned(), font.clone(), theme::TEXT)
            .size()
            .x
    };
    if width_of(text) <= max_width {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    loop {
        if chars.is_empty() {
            // 连一个省略号都放不下（标题栏被压到极窄）：宁可空着也不溢出到按钮上
            return String::new();
        }
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if width_of(&candidate) <= max_width {
            return candidate;
        }
    }
}

/// 标题栏：32px 高、可拖拽、左侧标题文字、右侧最小化/关闭按钮（MinTitleBar.vue 复刻）
///
/// `title` 为生效标题（命令行 `--name` > update.json `name` > 编译期烘焙默认，
/// 见 `qsltd_updater_core::config::resolve_launcher_name`），左侧内边距处垂直居中
/// 显示；宽度受拖拽区限制，超长以省略号收尾（不露半个字、不遮挡右侧按钮）。
/// 按钮悬停：白色 10% 底 + 图标变白；关闭悬停：#e81123 底 + 白图标。
pub fn draw_titlebar(ui: &mut Ui, title: &str) -> TitleBarAction {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), theme::TITLEBAR_HEIGHT),
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);

    // 拖拽区（按钮区域除外）
    let drag_rect = Rect::from_min_max(
        rect.left_top(),
        Pos2::new(
            rect.right() - theme::TITLEBAR_BTN_WIDTH * 2.0,
            rect.bottom(),
        ),
    );
    let drag = ui.interact(
        drag_rect,
        ui.scope_id().with("tb-drag"),
        Sense::click_and_drag(),
    );
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    // 标题文字（左侧内边距、垂直居中；裁在拖拽区内，不遮按钮）
    //
    // CJK 字形墨迹在行框内偏下约 1px：锚点上移补偿，让标题墨迹与右侧按钮图标
    // 落在同一条视觉中线上，避免标题看上去"比按钮低一点"。
    const TITLE_INSET: f32 = 12.0;
    const TITLE_V_NUDGE: f32 = 1.0;
    let text_rect = Rect::from_min_max(
        Pos2::new(rect.left() + TITLE_INSET, rect.top()),
        Pos2::new(drag_rect.right() - TITLE_INSET, rect.bottom()),
    );
    if !title.is_empty() && text_rect.width() > 0.0 {
        let font = FontId::proportional(13.0);
        let text_painter = ui.painter_at(text_rect);
        // 预留 1px 余量：字体度量与光栅化有零点几像素误差，贴着边界会削到笔画
        let shown = ellipsize_to_width(&text_painter, title, &font, text_rect.width() - 1.0);
        text_painter.text(
            Pos2::new(text_rect.left(), rect.center().y - TITLE_V_NUDGE),
            Align2::LEFT_CENTER,
            shown,
            font,
            theme::text_alpha(220),
        );
    }

    let mut action = TitleBarAction::None;
    let y_center = rect.center().y;
    let right_edge = rect.right();
    let close_rect = Rect::from_min_size(
        Pos2::new(right_edge - theme::TITLEBAR_BTN_WIDTH, rect.top()),
        Vec2::new(theme::TITLEBAR_BTN_WIDTH, theme::TITLEBAR_HEIGHT),
    );
    let min_rect = Rect::from_min_size(
        Pos2::new(close_rect.left() - theme::TITLEBAR_BTN_WIDTH, rect.top()),
        Vec2::new(theme::TITLEBAR_BTN_WIDTH, theme::TITLEBAR_HEIGHT),
    );

    // 最小化按钮
    let min_resp = ui.interact(min_rect, ui.scope_id().with("tb-min"), Sense::click());
    if min_resp.hovered() {
        painter.rect_filled(min_rect, 0.0, theme::W10_HOVER);
    }
    let min_color = if min_resp.hovered() {
        Color32::WHITE
    } else {
        theme::text_alpha(178) // rgba(255,255,255,0.7)
    };
    let cy = y_center;
    painter.line_segment(
        [
            Pos2::new(min_rect.center().x - 7.0, cy),
            Pos2::new(min_rect.center().x + 7.0, cy),
        ],
        Stroke::new(2.0, min_color),
    );
    min_resp.clone().on_hover_cursor(CursorIcon::PointingHand);
    if min_resp.clicked() {
        action = TitleBarAction::Minimize;
    }

    // 关闭按钮
    let close_resp = ui.interact(close_rect, ui.scope_id().with("tb-close"), Sense::click());
    if close_resp.hovered() {
        painter.rect_filled(close_rect, 0.0, theme::DANGER);
    }
    let close_color = if close_resp.hovered() {
        Color32::WHITE
    } else {
        theme::text_alpha(178)
    };
    let cx = close_rect.center().x;
    painter.line_segment(
        [Pos2::new(cx - 6.0, cy - 6.0), Pos2::new(cx + 6.0, cy + 6.0)],
        Stroke::new(2.0, close_color),
    );
    painter.line_segment(
        [Pos2::new(cx - 6.0, cy + 6.0), Pos2::new(cx + 6.0, cy - 6.0)],
        Stroke::new(2.0, close_color),
    );
    close_resp.clone().on_hover_cursor(CursorIcon::PointingHand);
    if close_resp.clicked() {
        action = TitleBarAction::Close;
    }

    action
}

/// 加载 spinner（.tpl-x__spin：灰环 + 彩色 90° 弧，0.9s/圈）
pub fn spinner(painter: &Painter, center: Pos2, radius: f32, width: f32, color: Color32) {
    let t = painter.ctx().input(|i| i.time);
    let track = Stroke::new(width, theme::text_alpha(38)); // rgba(255,255,255,0.15)
                                                           // 底圈
    let circle: Vec<Pos2> = (0..=48)
        .map(|i| {
            let a = (i as f64 / 48.0) * std::f64::consts::TAU;
            Pos2::new(
                center.x + (radius as f64 * a.cos()) as f32,
                center.y + (radius as f64 * a.sin()) as f32,
            )
        })
        .collect();
    painter.add(PathShape::line(circle, track));
    // 彩色 90° 弧，随时间旋转
    let start = (t / 0.9) * std::f64::consts::TAU;
    let arc: Vec<Pos2> = (0..=16)
        .map(|i| {
            let a = start + (i as f64 / 16.0) * std::f64::consts::FRAC_PI_2;
            Pos2::new(
                center.x + (radius as f64 * a.cos()) as f32,
                center.y + (radius as f64 * a.sin()) as f32,
            )
        })
        .collect();
    painter.add(PathShape::line(arc, Stroke::new(width, color)));
}

/// 进度条样式（八套模板的进度差异全部收敛到这四个数）
///
/// `gradient` 为 true 时填充走「蓝→绿」gouraud 近似，false 时是纯蓝实色。
#[derive(Debug, Clone, Copy)]
pub struct TrackStyle {
    /// 轨道高度（同时是填充高度）
    pub height: f32,
    /// 轨道与填充的圆角半径
    pub radius: f32,
    /// 最小填充宽度：0% 时也留一点可见头部，避免「看起来没画」
    pub min_fill: f32,
    /// 填充是否使用蓝→绿渐变
    pub gradient: bool,
}

impl TrackStyle {
    /// 标准条：8px 圆角轨道 + 渐变填充（A/C 模板）
    pub const STD: TrackStyle = TrackStyle {
        height: 8.0,
        radius: 4.0,
        min_fill: 6.0,
        gradient: true,
    };
    /// E 模板：6px 渐变
    pub const E: TrackStyle = TrackStyle {
        height: 6.0,
        radius: 3.0,
        min_fill: 4.0,
        gradient: true,
    };
    /// D 模板：4px 纯蓝
    pub const D: TrackStyle = TrackStyle {
        height: 4.0,
        radius: 2.0,
        min_fill: 3.0,
        gradient: false,
    };
    /// G/H 模板：3px 纯蓝，比 D 更收敛
    pub const THIN: TrackStyle = TrackStyle {
        height: 3.0,
        radius: 1.5,
        min_fill: 2.0,
        gradient: false,
    };
}

/// 参数化进度条：轨道 + 填充（渐变或实色）
pub fn track(ui: &mut Ui, width: f32, percent: f32, style: &TrackStyle) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, style.height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style.radius, theme::W10);
    let fill_w = (rect.width() * percent.clamp(0.0, 100.0) / 100.0).max(style.min_fill);
    let fill = Rect::from_min_size(rect.left_top(), Vec2::new(fill_w, style.height));
    if style.gradient {
        // linear-gradient(90deg, #4a9eff → #35c26e) 的 gouraud 近似
        let mut mesh = Mesh::default();
        mesh.colored_vertex(fill.left_top(), theme::ACCENT);
        mesh.colored_vertex(fill.right_top(), theme::SUCCESS);
        mesh.colored_vertex(fill.right_bottom(), theme::SUCCESS);
        mesh.colored_vertex(fill.left_bottom(), theme::ACCENT);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(Shape::mesh(mesh));
    } else {
        painter.rect_filled(fill, style.radius, theme::ACCENT);
    }
}

/// 进度条（.tpl-x__track 8px 圆角轨道 + .tpl-x__fill 蓝→绿渐变）
pub fn progress_track(ui: &mut Ui, width: f32, percent: f32) {
    track(ui, width, percent, &TrackStyle::STD);
}

/// 状态圆点（.tpl-x__dot / .tpl-c__node-dot；`halo` 为光晕半径，0 表示无）
pub fn status_dot(painter: &Painter, center: Pos2, radius: f32, color: Color32, halo: f32) {
    if halo > 0.0 {
        painter.circle_filled(center, radius + halo, theme::tint(color, 46)); // 0.18*255
    }
    painter.circle_filled(center, radius, color);
}

/// 徽标 pill（.tpl-x__pill：蓝字蓝底描边全圆角）
///
/// 返回实际占用的矩形，供调用方排布。
pub fn pill(ui: &mut Ui, text: &str) -> Rect {
    let font = FontId::proportional(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, theme::ACCENT);
    let pad_x = 10.0;
    let pad_y = 3.0;
    let size = Vec2::new(galley.size().x + pad_x * 2.0, galley.size().y + pad_y * 2.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, size.y / 2.0, theme::tint(theme::ACCENT, 36)); // 0.14
    painter.rect_stroke(
        rect,
        size.y / 2.0,
        Stroke::new(1.0, theme::tint(theme::ACCENT, 89)),
        egui::StrokeKind::Inside,
    ); // 0.35
    painter.galley(
        rect.left_top() + Vec2::new(pad_x, pad_y),
        galley,
        theme::ACCENT,
    );
    rect
}

/// 按钮（.tpl-x__btn：padding 7/26 圆角 6；primary 蓝底白字）
///
/// 返回是否被点击（disabled 时不响应）。
pub fn button(ui: &mut Ui, text: &str, primary: bool, enabled: bool) -> bool {
    let font = FontId::proportional(13.0);
    let pad = Vec2::new(26.0, 7.0);
    let text_color = if primary { Color32::WHITE } else { theme::TEXT };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, text_color);
    let size = galley.size() + pad * 2.0;
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);

    if enabled {
        resp.clone().on_hover_cursor(CursorIcon::PointingHand);
    }
    let hovered = resp.hovered() && enabled;
    if primary {
        let fill = if enabled {
            if hovered {
                theme::tint(theme::ACCENT, 217) // hover 提亮（原版无 hover 变色，轻微提亮增强反馈）
            } else {
                theme::ACCENT
            }
        } else {
            theme::tint(theme::ACCENT, 153) // disabled opacity 0.6
        };
        painter.rect_filled(rect, 6.0, fill);
    } else {
        if hovered {
            painter.rect_filled(rect, 6.0, theme::W10_HOVER);
        }
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, theme::W20),
            egui::StrokeKind::Inside,
        );
    }

    let tc = if enabled {
        text_color
    } else {
        theme::text_alpha(153)
    };
    let tc = if primary && !enabled {
        Color32::from_white_alpha(153)
    } else {
        tc
    };
    let offset = ((size - galley.size()) * 0.5).round();
    painter.galley(rect.left_top() + offset, galley, tc);

    enabled && resp.clicked()
}

/// 居中文本标签（模板标题等；带 wrap，超宽自动换行）
pub fn centered_label(ui: &mut Ui, text: &str, size: f32, color: Color32) {
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.label(RichText::new(text).size(size).color(color));
    });
}

/// 环形进度（B 模板 .tpl-b__ring：外环进度弧 + 深色内盘 + 居中百分比）
pub fn progress_ring(ui: &mut Ui, radius: f32, percent: f32, pct_color: Color32, sub_text: &str) {
    let size = radius * 2.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(size, size), Sense::hover());
    let painter = ui.painter_at(rect);
    let center = rect.center();

    // 外环：满圈灰 + 进度蓝弧（conic-gradient 近似为等宽弧线）
    let ring_w = 9.0;
    let ring_r = radius - ring_w / 2.0;
    let track: Vec<Pos2> = (0..=64)
        .map(|i| {
            let a = (i as f64 / 64.0) * std::f64::consts::TAU;
            Pos2::new(
                center.x + (ring_r as f64 * a.cos()) as f32,
                center.y + (ring_r as f64 * a.sin()) as f32,
            )
        })
        .collect();
    painter.add(PathShape::line(track, Stroke::new(ring_w, theme::W10)));
    let frac = (percent.clamp(0.0, 100.0) / 100.0) as f64;
    if frac > 0.0 {
        let steps = ((64.0 * frac) as usize).max(1);
        let arc: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let a = -std::f64::consts::FRAC_PI_2 + (i as f64 / 64.0) * std::f64::consts::TAU;
                Pos2::new(
                    center.x + (ring_r as f64 * a.cos()) as f32,
                    center.y + (ring_r as f64 * a.sin()) as f32,
                )
            })
            .collect();
        painter.add(PathShape::line(arc, Stroke::new(ring_w, theme::ACCENT)));
    }

    // 内盘 + 居中文字
    painter.circle_filled(center, radius - ring_w - 0.5, theme::RING_INNER);
    let pct_text = format!("{:.0}%", percent);
    painter.text(
        center + Vec2::new(0.0, if sub_text.is_empty() { 0.0 } else { -8.0 }),
        Align2::CENTER_CENTER,
        pct_text,
        FontId::proportional(22.0),
        pct_color,
    );
    if !sub_text.is_empty() {
        painter.text(
            center + Vec2::new(0.0, 14.0),
            Align2::CENTER_CENTER,
            sub_text,
            FontId::proportional(11.0),
            theme::text_alpha(128),
        );
    }
}
