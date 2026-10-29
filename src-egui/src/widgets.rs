//! 自绘控件集合：背景渐变、标题栏、spinner、进度条、状态点、徽标、
//! 日志卡片、按钮 —— 逐一对应 Vue 版的 CSS 类
//! （.updater-root / .title-bar / .tpl-x__spin / .tpl-x__track / .tpl-x__dot /
//!  .tpl-x__pill / .tpl-x__log / .tpl-x__btn）。

use egui::{
    emath::Pos2, epaint::PathShape, Align2, Color32, CursorIcon, FontId, Mesh, Painter, Rect,
    RichText, Sense, Shape, Stroke, Vec2, Ui,
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

/// 标题栏：32px 高、可拖拽、左侧标题文字、右侧最小化/关闭按钮（MinTitleBar.vue 复刻）
///
/// `title` 为构建期烘焙的启动器标题（见 `app::WINDOW_TITLE`），左侧内边距处
/// 垂直居中显示；宽度受拖拽区限制，超长文字裁切，不遮挡右侧按钮。
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
        Pos2::new(rect.right() - theme::TITLEBAR_BTN_WIDTH * 2.0, rect.bottom()),
    );
    let drag = ui.interact(drag_rect, ui.scope_id().with("tb-drag"), Sense::click_and_drag());
    if drag.drag_started() {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::StartDrag);
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
        ui.painter_at(text_rect).text(
            Pos2::new(text_rect.left(), rect.center().y - TITLE_V_NUDGE),
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(13.0),
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
        [Pos2::new(min_rect.center().x - 7.0, cy), Pos2::new(min_rect.center().x + 7.0, cy)],
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

/// 进度条（.tpl-x__track 8px 圆角轨道 + .tpl-x__fill 蓝→绿渐变）
pub fn progress_track(ui: &mut Ui, width: f32, percent: f32) {
    let height = 8.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::W10);
    let fill_w = (rect.width() * percent.clamp(0.0, 100.0) / 100.0).max(6.0);
    let fill = Rect::from_min_size(rect.left_top(), Vec2::new(fill_w, height));
    // linear-gradient(90deg, #4a9eff → #35c26e) 的 gouraud 近似
    let mut mesh = Mesh::default();
    mesh.colored_vertex(fill.left_top(), theme::ACCENT);
    mesh.colored_vertex(fill.right_top(), theme::SUCCESS);
    mesh.colored_vertex(fill.right_bottom(), theme::SUCCESS);
    mesh.colored_vertex(fill.left_bottom(), theme::ACCENT);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
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
    painter.rect_stroke(rect, size.y / 2.0, Stroke::new(1.0, theme::tint(theme::ACCENT, 89)), egui::StrokeKind::Inside); // 0.35
    painter.galley(
        rect.left_top() + Vec2::new(pad_x, pad_y),
        galley,
        theme::ACCENT,
    );
    rect
}

/// 更新日志卡片（.tpl-x__log + .tpl-x__log-list）
///
/// - `body_h`: 正文区固定高度（卡片总高 = body_h + 42）。条目超出时在
///   卡片内部出竖向滚动条，卡片自身高度恒定，不会撑动整体布局
/// - D 模板走 [`inline_log`]（无卡片容器，同样固定高度内部滚动）
pub fn log_card(ui: &mut Ui, items: &[&str], body_h: f32) {
    let full_width = ui.available_width();
    // 固定高度：标题+内边距约 42px，正文区恒为 body_h，多行内部滚动
    let (rect, _) = ui.allocate_exact_size(Vec2::new(full_width, body_h + 42.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::W04);
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0, theme::W08), egui::StrokeKind::Inside);

    // 卡片内部布局
    let inner = Rect::from_min_max(
        rect.left_top() + Vec2::new(14.0, 12.0),
        rect.right_bottom() - Vec2::new(14.0, 12.0),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );
    child.label(
        RichText::new("更新日志")
            .size(12.0)
            .color(theme::text_alpha(128)), // 0.5
    );
    child.add_space(8.0);
    log_list(&mut child, items, body_h);
}

/// 日志条目列表（蓝点 + 12.5px 文本，行高 1.9，固定高度内部滚动）
fn log_list(ui: &mut Ui, items: &[&str], body_h: f32) {
    let line_h = 12.5 * 1.9;
    let body = |ui: &mut Ui| {
        for line in items {
            ui.horizontal(|ui| {
                // 蓝点（4px，左侧 2px，垂直约 10px 处）
                let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(13.0, line_h), Sense::hover());
                ui.painter_at(dot_rect).circle_filled(
                    Pos2::new(dot_rect.left() + 4.0, dot_rect.top() + 10.0),
                    2.0,
                    theme::ACCENT,
                );
                ui.label(
                    RichText::new(*line)
                        .size(12.5)
                        .color(theme::text_alpha(217)), // 0.85
                );
            });
        }
    };
    egui::ScrollArea::vertical()
        .max_height(body_h)
        .auto_shrink(false)
        .show(ui, body);
}

/// D 模板内联日志（.tpl-d__log：无卡片，细分割线分隔）
///
/// 正文固定高度 96px，多行时内部滚动，避免撑破窗口布局。
pub fn inline_log(ui: &mut Ui, items: &[&str]) {
    ui.label(
        RichText::new("更新日志")
            .size(12.0)
            .color(theme::text_alpha(115)), // 0.45
    );
    egui::ScrollArea::vertical()
        .max_height(96.0)
        .auto_shrink(false)
        .show(ui, |ui| {
            for (i, line) in items.iter().enumerate() {
                if i > 0 {
                    let w = ui.available_width();
                    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), Sense::hover());
                    ui.painter_at(r)
                        .rect_filled(r, 0.0, theme::W08);
                }
                ui.label(
                    RichText::new(*line)
                        .size(13.0)
                        .color(theme::text_alpha(204)), // 0.8
                );
            }
        });
}

/// 按钮（.tpl-x__btn：padding 7/26 圆角 6；primary 蓝底白字）
///
/// 返回是否被点击（disabled 时不响应）。
pub fn button(ui: &mut Ui, text: &str, primary: bool, enabled: bool) -> bool {
    let font = FontId::proportional(13.0);
    let pad = Vec2::new(26.0, 7.0);
    let text_color = if primary {
        Color32::WHITE
    } else {
        theme::TEXT
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, text_color);
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

    let tc = if enabled { text_color } else { theme::text_alpha(153) };
    let tc = if primary && !enabled { Color32::from_white_alpha(153) } else { tc };
    let offset = ((size - galley.size()) * 0.5).round();
    painter.galley(rect.left_top() + offset, galley, tc);

    enabled && resp.clicked()
}

/// 底部动作行（.tpl-x__actions：gap 10）
///
/// - 错误态：重试 + 关闭；若自动重试已用尽且失败出在下载链路
///   （`show_homepage`），再加一枚「前往官网」——地址全挂时重试是徒劳，
///   得给条能拿到包的路
/// - 完成态：完成
pub fn actions_row(
    ui: &mut Ui,
    retrying: bool,
    show_retry: bool,
    show_finish: bool,
    show_homepage: bool,
) -> Option<Action> {
    let mut act = None;
    ui.horizontal(|ui| {
        ui.add_space(0.0);
        if show_retry {
            if button(ui, if retrying { "正在重试..." } else { "重试" }, true, !retrying) {
                act = Some(Action::Retry);
            }
            ui.add_space(10.0);
            if show_homepage && button(ui, "前往官网重新下载", false, true) {
                act = Some(Action::Homepage);
            }
            if show_homepage {
                ui.add_space(10.0);
            }
            if button(ui, "关闭", false, true) {
                act = Some(Action::Close);
            }
        } else if show_finish && button(ui, "完成", true, true) {
            act = Some(Action::Finish);
        }
    });
    act
}

/// 错误态一句话说明（接在错误详情下方）
///
/// 说清两件事：这次失败在哪个环节试了几次、接下来该点哪个按钮。
/// 只有下载链路的重试用尽才把用户往官网引——解压失败让他去官网重下，
/// 相当于让他重走一遍一模一样的坑。
pub fn error_hint(attempt_text: &str, stage_name: &str, needs_homepage: bool) -> String {
    // 环节名 + 已试次数：「下载已自动重试 2 次（共 3 次尝试）」
    let tried = if attempt_text.is_empty() {
        format!("{}失败", if stage_name.is_empty() { "更新" } else { stage_name })
    } else if stage_name.is_empty() {
        attempt_text.to_string()
    } else {
        format!("{}{}", stage_name, attempt_text)
    };
    if needs_homepage {
        return format!("{}。可点击「前往官网重新下载」获取完整安装包", tried);
    }
    if attempt_text.is_empty() {
        // 没走自动重试的失败（本地错误）：别劝他去官网，也别假装还有兜底
        format!("{}，请处理后点「重试」", tried)
    } else {
        format!("{}，请检查网络或磁盘后点「重试」", tried)
    }
}

/// 底部动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Retry,
    /// 打开官网首页（下载地址全挂时的兜底出口）
    Homepage,
    Close,
    Finish,
    /// 打开应用商店（强制更新界面）
    Store,
    /// 下载升级包（强制更新界面：走更新流程下载并安装最新版）
    ForceDownload,
    /// 稍后升级（非阻断强制更新：启动当前已安装应用并退出更新器）
    ForceLater,
}

/// 居中文本标签（模板标题等；带 wrap，超宽自动换行）
pub fn centered_label(ui: &mut Ui, text: &str, size: f32, color: Color32) {
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.label(RichText::new(text).size(size).color(color));
    });
}

/// 强制更新界面底部动作行
///
/// - 有商店地址（`has_store`）：展示「前往应用商店」
/// - 始终展示「下载升级包」（primary，走更新流程）
/// - 非阻断（`blocking=false`）：额外展示「稍后升级」，允许应用照常启动
pub fn force_update_actions_row(
    ui: &mut Ui,
    blocking: bool,
    has_store: bool,
) -> Option<Action> {
    let mut act = None;
    ui.horizontal(|ui| {
        ui.add_space(0.0);
        if has_store {
            if button(ui, "前往应用商店", false, true) {
                act = Some(Action::Store);
            }
            ui.add_space(10.0);
        }
        if button(ui, "下载升级包", true, true) {
            act = Some(Action::ForceDownload);
        }
        if !blocking {
            ui.add_space(10.0);
            if button(ui, "稍后升级", false, true) {
                act = Some(Action::ForceLater);
            }
        }
    });
    act
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
