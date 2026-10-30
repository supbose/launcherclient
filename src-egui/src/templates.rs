//! 两套启动器界面模板：F 主模板 / H 副模板
//!
//! 选哪套由启动方式决定（判定在 core 的 `resolve_launcher_template`）：
//! - 双击直启、没有任何命令行参数 → 主模板 F（环形进度 + 步骤条，信息最全）
//! - 被其他应用带参调用 → 副模板 H（单行状态 + 3px 进度，尽量轻不抢戏）
//! - `launcher_template` / `--template` 显式给 F/H 时以配置为准
//!
//! 两套模板各自持有布局与配色，展示值统一来自 `PhaseView`（usePhase.js 的
//! Rust 对应物），文案与显示条件共用同一套 helper，避免画面随布局漂移。

use egui::{
    emath::Pos2, Align, Align2, Color32, FontId, Layout, RichText, Sense, Stroke, Ui, UiBuilder,
    Vec2,
};

use qsltd_updater_core::phase::{PhaseView, Stage, STEP_LABELS};

use crate::theme;
use crate::widgets::{self, Action};

/// 绘制当前模板（按 template 键分发；未知键回落主模板 F）
pub fn draw(
    ui: &mut Ui,
    view: &PhaseView,
    template: &str,
    retrying: bool,
    last_step: i32,
    store_error: Option<&str>,
) -> Option<Action> {
    match template {
        "H" => template_h(ui, view, retrying, store_error),
        _ => template_f(ui, view, retrying, last_step, store_error),
    }
}

/// 强制更新界面（阻断 / 非阻断共用布局）
///
/// 展示层与业务判定解耦：`blocking` / `has_store` 已编码进 `view`（来自
/// `PhasePayload::force_update`），界面只负责呈现：
/// - 警示图标 + 标题：`view.message()`（阻断="必须升级后才能继续使用" /
///   非阻断="建议升级到新版本"）
/// - 版本对比卡：当前版本 → 要求版本，清晰展示两者
/// - 商店打开失败提示（`store_error` 非空时，红字一行）
/// - 底部动作行：前往应用商店（服务端 manifest 有 store_url）/ 下载升级包（primary）/ 稍后升级（非阻断）
///
/// 约定：调用方已处于 `content()` 内容区内（已带模板内边距），本函数不再
/// 二次包裹；各模板在 `match` 中以 `return draw_force_update(...)` 提前返回，
/// 避免后续日志卡片/动作行误渲染到强制更新界面。
pub fn draw_force_update(
    ui: &mut Ui,
    view: &PhaseView,
    store_error: Option<&str>,
) -> Option<Action> {
    let blocking = view.is_blocking();
    let accent = if blocking {
        theme::WARNING
    } else {
        theme::ACCENT
    };
    let mut act: Option<Action> = None;

    // 警示图标 + 标题（居中）
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        let (ir, _) = ui.allocate_exact_size(Vec2::new(52.0, 52.0), Sense::hover());
        let ip = ui.painter_at(ir);
        let c = ir.center();
        ip.circle_filled(c, 26.0, theme::tint(theme::WARNING, 38));
        ip.text(
            c,
            Align2::CENTER_CENTER,
            "!",
            FontId::proportional(30.0),
            theme::WARNING,
        );
        ui.add_space(14.0);
        widgets::centered_label(ui, view.message(), 17.0, accent);
    });

    // 商店打开失败提示（仅失败时显示；成功打开后用户视线已转移）
    if let Some(err) = store_error {
        ui.add_space(12.0);
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.label(RichText::new(err).size(11.5).color(theme::text_alpha(140)));
        });
    }

    // 版本对比卡：当前版本 → 要求版本
    ui.add_space(18.0);
    let width = ui.available_width();
    let (card, _) = ui.allocate_exact_size(Vec2::new(width, 60.0), Sense::hover());
    let painter = ui.painter_at(card);
    painter.rect_filled(card, 8.0, theme::W04);
    painter.rect_stroke(
        card,
        8.0,
        Stroke::new(1.0, theme::W08),
        egui::StrokeKind::Inside,
    );
    let col_w = card.width() / 2.0;
    painter.text(
        Pos2::new(card.left() + 14.0, card.top() + 12.0),
        Align2::LEFT_TOP,
        "当前版本",
        FontId::proportional(11.0),
        theme::text_alpha(140),
    );
    painter.text(
        Pos2::new(card.left() + 14.0, card.top() + 30.0),
        Align2::LEFT_TOP,
        view.force_current(),
        FontId::proportional(16.0),
        theme::TEXT,
    );
    painter.text(
        Pos2::new(card.left() + col_w + 14.0, card.top() + 12.0),
        Align2::LEFT_TOP,
        "要求版本",
        FontId::proportional(11.0),
        theme::text_alpha(140),
    );
    painter.text(
        Pos2::new(card.left() + col_w + 14.0, card.top() + 30.0),
        Align2::LEFT_TOP,
        view.force_required(),
        FontId::proportional(16.0),
        accent,
    );
    painter.text(
        Pos2::new(card.left() + col_w - 6.0, card.center().y),
        Align2::CENTER_CENTER,
        "→",
        FontId::proportional(18.0),
        theme::text_alpha(153),
    );

    // 更新日志：用户此刻正要决策「下载升级包 / 稍后升级」，日志是决策依据；
    // manifest 未带日志时整块不占位（与其余界面日志卡约定一致），
    // 固定高度内部滚动，不随条目数撑破 420 高窗口。
    let log = view.changelog();
    if !log.is_empty() {
        ui.add_space(16.0);
        let log_h = (ui.available_height() - 89.0).max(72.0);
        widgets::log_card(ui, &log, log_h);
    }

    // 动作行（推到内容区底部居中）
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(18.0);
        let size = widgets::force_update_actions_row_size(ui, blocking, view.has_store());
        ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
            act = widgets::force_update_actions_row(ui, blocking, view.has_store());
        });
    });

    act
}

/// 结束态（已是最新 / 发现新版本 / 完成 / 错误）的主文本兜底
/// （`message || (up-to-date ? '已是最新版本' : '更新完成')`）
fn end_title<'a>(view: &'a PhaseView<'a>) -> &'a str {
    let msg = view.message();
    if !msg.is_empty() {
        return msg;
    }
    if view.stage() == Stage::UpToDate {
        "已是最新版本"
    } else {
        "更新完成"
    }
}

/// 结束态状态点颜色（ok 绿 / info 蓝 / err 红）
fn end_dot_color(view: &PhaseView) -> (Color32, f32) {
    match view.stage() {
        Stage::UpToDate | Stage::Done => (theme::SUCCESS, 5.0),
        Stage::Available => (theme::ACCENT, 5.0),
        Stage::Error => (theme::DANGER, 5.0),
        _ => (theme::ACCENT, 0.0),
    }
}

/// 「检测中」极简屏文案
const CHECKING_TEXT: &str = "正在检查更新...";

/// 启动决策前的「检测中」极简屏
///
/// 仅 spinner + 主文案，**不渲染步骤条/更新日志**。对应「检测到更新才展示
/// 完整界面」的诉求：弱网慢 manifest（>300ms）弹窗时用户有加载反馈，但
/// 不会在还没确定要不要更新时就先铺开一大片完整 UI；当且仅当后台
/// `run_startup` 判定有更新（Show 决策）后，`app.rs` 才切到完整模板。
///
/// 加载指示器按整个窗口区域居中（含标题栏以下的可视区），不随文案高度
/// 漂移；配置的应用名属于装饰信息，检测阶段不展示（用户看的是「在转」，
/// 而不是「在为谁转」）。
pub fn draw_checking_minimal(ui: &mut Ui) {
    let rect = ui.max_rect();
    let painter = ui.painter_at(rect);
    let center = rect.center();

    // 大号 spinner：36px 半径，重心压在中心略偏上（视觉居中）
    let spinner_r = 36.0;
    let spinner_c = Pos2::new(center.x, center.y - 12.0);
    widgets::spinner(&painter, spinner_c, spinner_r, 4.0, theme::ACCENT);

    // 文案排在 spinner 下方，水平居中
    let font = egui::FontId::proportional(17.0);
    let galley = painter.layout_no_wrap(CHECKING_TEXT.to_owned(), font, theme::TEXT);
    // 图标 + 文案整块的重心落在中心附近
    let text_pos = Pos2::new(
        center.x - galley.size().x / 2.0,
        spinner_c.y + spinner_r + 26.0,
    );
    painter.galley(text_pos, galley, theme::TEXT);
}

/// 在 padding 区内开一层垂直布局
fn content<R>(ui: &mut Ui, pad: (f32, f32, f32, f32), add: impl FnOnce(&mut Ui) -> R) -> R {
    let max = ui.max_rect();
    let inner_max = egui::Rect::from_min_max(
        Pos2::new(max.left() + pad.1, max.top() + pad.0),
        Pos2::new(max.right() - pad.3, max.bottom() - pad.2),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner_max)
            .layout(Layout::top_down(Align::LEFT)),
    );
    add(&mut child)
}

// ---------------------------------------------------------------------------
// 公共骨架：两套模板的差异只留在布局函数里，逻辑收敛到下面的 helper。
// ---------------------------------------------------------------------------

/// 各阶段的兜底标题（后端没带 message 时用；与 `PhaseView::stage` 一一对应）
///
/// 结束态的兜底沿用紧凑条原有的文案表：`Available` 是「发现新版本」，
/// 其余（含 `Error`）落「更新完成」——错误态实际总带 message，这里只是防止空串。
fn default_title(stage: Stage) -> &'static str {
    match stage {
        Stage::Checking => "正在检查更新...",
        Stage::Downloading => "正在下载更新包...",
        Stage::Installing => "正在安装更新...",
        Stage::UpToDate => "已是最新版本",
        Stage::Available => "发现新版本",
        _ => "更新完成",
    }
}

/// 主标题：后端 message 优先，空则用阶段兜底
fn stage_title<'a>(view: &'a PhaseView<'a>, fallback: &'a str) -> &'a str {
    let msg = view.message();
    if msg.is_empty() {
        fallback
    } else {
        msg
    }
}

/// 进行中阶段的标题（最常见的形态：按 stage 取兜底）
fn stage_title_by_stage<'a>(view: &'a PhaseView<'a>) -> &'a str {
    stage_title(view, default_title(view.stage()))
}

/// 居中大标题（与结束态同一套「message 优先、阶段兜底」规则）
fn centered_title(ui: &mut Ui, view: &PhaseView, gap: f32, size: f32) {
    if gap > 0.0 {
        ui.add_space(gap);
    }
    widgets::centered_label(ui, stage_title_by_stage(view), size, theme::TEXT);
}

/// 副文本行：为空则完全不占位（含前置 add_space）
fn sub_line(ui: &mut Ui, view: &PhaseView, gap: f32, size: f32, alpha: u8, centered: bool) {
    let sub = view.sub();
    if sub.is_empty() {
        return;
    }
    ui.add_space(gap);
    if centered {
        widgets::centered_label(ui, sub, size, theme::text_alpha(alpha));
    } else {
        ui.label(
            RichText::new(sub)
                .size(size)
                .color(theme::text_alpha(alpha)),
        );
    }
}

/// 错误态提示行（「下载失败，已自动重试 2 次（共 3 次尝试）…」）
///
/// 只在错误态出现：正常阶段用户不需要知道自己被重试了几次。
/// `centered` 区分主模板的居中版与副模板的左对齐版。
fn error_hint_line(ui: &mut Ui, view: &PhaseView, gap: f32, centered: bool) {
    if view.stage() != Stage::Error {
        return;
    }
    let hint = widgets::error_hint(
        &view.attempt_text(),
        view.failed_stage_name(),
        view.needs_homepage(),
    );
    if hint.is_empty() {
        return;
    }
    ui.add_space(gap);
    if centered {
        widgets::centered_label(ui, &hint, 11.5, theme::text_alpha(153));
    } else {
        ui.label(RichText::new(hint).size(11.5).color(theme::text_alpha(153)));
    }
}

/// 左对齐错误提示行（副模板自绘布局用；居中版由 [`end_block`] 内部处理）
fn error_hint_row(ui: &mut Ui, view: &PhaseView, gap: f32) {
    error_hint_line(ui, view, gap, false);
}

/// 结束态整块：状态点 + 标题（+ 副文本 + 错误提示）
///
/// 居中式必须在 `top_down(Align::Center)` 里直接分配方形，不能包一层 horizontal
/// 子容器——子容器会吃掉剩余高度，把圆点顶到左侧、标题挤到底部（实测踩过）。
/// 28×28 的方形是给 8px 状态点 + halo 留的余量，否则会被 painter 裁掉边缘。
fn end_block(ui: &mut Ui, view: &PhaseView) {
    let is_err = view.stage() == Stage::Error;
    let title_color = if is_err {
        theme::ERROR_TEXT
    } else {
        theme::TEXT
    };
    let sub_color = if is_err {
        theme::tint(theme::ERROR_TEXT, 191)
    } else {
        theme::text_alpha(140)
    };

    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
        let (color, halo) = end_dot_color(view);
        widgets::status_dot(&ui.painter_at(r), r.center(), 8.0, color, halo);
        ui.add_space(2.0);
        ui.label(RichText::new(end_title(view)).size(15.0).color(title_color));
    });

    if !view.sub().is_empty() {
        ui.add_space(8.0);
        widgets::centered_label(ui, view.sub(), 12.0, sub_color);
    }
    error_hint_line(ui, view, 8.0, true);
}

/// 是否需要在底部显示动作行（重试 / 前往官网 / 立即启动）
fn shows_actions(view: &PhaseView) -> bool {
    let stage = view.stage();
    stage == Stage::Error || stage == Stage::Done
}

/// 动作行：左对齐直接排布，居中版按实测行宽居中
///
/// `ui.horizontal` 会先占满父容器可用宽度，光把它套进 `top_down(Center)`
/// 并不能让按钮动起来（行起点仍在左边）。居中要先量出按钮行自身的尺寸，
/// 再用同尺寸子区域排布，父容器才有东西可以居中。
fn actions_line(ui: &mut Ui, view: &PhaseView, retrying: bool, centered: bool) -> Option<Action> {
    let stage = view.stage();
    let show_retry = stage == Stage::Error;
    let show_finish = stage == Stage::Done;
    let show_homepage = view.needs_homepage();

    if !centered {
        return widgets::actions_row(ui, retrying, show_retry, show_finish, show_homepage);
    }

    let mut act = None;
    let size = widgets::actions_row_size(ui, retrying, show_retry, show_finish, show_homepage);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
            act = widgets::actions_row(ui, retrying, show_retry, show_finish, show_homepage);
        });
    });
    act
}

/// 主模板尾部：固定 96px 日志卡 + 居中动作行
fn tail_block(ui: &mut Ui, view: &PhaseView, retrying: bool) -> Option<Action> {
    let log = view.changelog();
    if !log.is_empty() {
        ui.add_space(16.0);
        widgets::log_card(ui, &log, 96.0);
    }
    if shows_actions(view) {
        ui.add_space(16.0);
        actions_line(ui, view, retrying, true)
    } else {
        None
    }
}

/// 步骤条（.tpl-c__steps：4 节点 + 3 连接线）
///
/// 整条步骤条占一个区域，用绝对 painter 绘制：先画连接线（贯穿相邻圆点
/// 边缘，不再留空隙），再画节点与标签。已完成段连接线为绿色半透明。
fn draw_steps(ui: &mut Ui, view: &PhaseView, width: f32, last_step: i32) {
    // 错误态保留最后到达的步骤（view.step() 返回 -2 表示保持不变）
    let step = if view.step() == -2 {
        last_step
    } else {
        view.step()
    };
    let n = STEP_LABELS.len() as f32; // 4.0
    let dot_r = 8.0;
    let row_h = 40.0;
    // 首尾对称内缩：首节点距左 margin、末节点距右 margin，中间等分。
    // 若按旧式 (2*i+1)*seg 公式，第 4 个节点会落在 area.right()（区域右边界），
    // 而 painter_at(area) 会按区域矩形裁剪，圆点与标签超出右边界被裁掉（"完成"只露半截）。
    let margin = dot_r + 8.0; // 16.0：大于圆点半径且留出标签半宽余量，保证两端不贴边、不被裁剪

    let (area, _) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::hover());
    let painter = ui.painter_at(area);
    // 间距改用 area.width()（实际分配到的宽度），避免父容器夹紧导致末节点越过右界被裁。
    let aw = area.width();
    let spacing = (aw - 2.0 * margin) / (n - 1.0);
    // 圆点中心下移 12.5px：当前步外环半径 dot_r+3、halo 半径 dot_r+4=12，
    // 旧值 11 会让环顶部越过区域上边界被 painter 裁掉（圆点看起来没画满）。
    let dot_y = area.top() + dot_r + 4.5;

    // 连接线（位于圆点中心高度，从节点 i 右缘到节点 i+1 左缘，不压圆点）
    for i in 0..(STEP_LABELS.len() - 1) {
        let xi = margin + i as f32 * spacing;
        let xj = margin + (i + 1) as f32 * spacing;
        let x0 = area.left() + xi + dot_r;
        let x1 = area.left() + xj - dot_r;
        let done_seg = step >= 0 && (i as i32) < step;
        painter.rect_filled(
            egui::Rect::from_min_max(Pos2::new(x0, dot_y - 1.0), Pos2::new(x1, dot_y + 1.0)),
            1.0,
            if done_seg {
                theme::tint(theme::SUCCESS, 128)
            } else {
                theme::W14
            },
        );
    }

    // 节点 + 标签
    for (i, label) in STEP_LABELS.iter().enumerate() {
        let i = i as f32;
        let cx = area.left() + margin + i * spacing;
        let dot_c = Pos2::new(cx, dot_y);
        let done = step >= 0 && (i as i32) < step;
        let on = (i as i32) == step;
        let err = on && view.stage() == Stage::Error;
        if err {
            widgets::status_dot(&painter, dot_c, dot_r, theme::DANGER, 4.0);
        } else if on {
            // 当前步：外环描边增强焦点（在 halo 之外再加一圈蓝边）
            painter.circle_stroke(
                dot_c,
                dot_r + 3.0,
                Stroke::new(1.0, theme::tint(theme::ACCENT, 128)),
            );
            widgets::status_dot(&painter, dot_c, dot_r, theme::ACCENT, 4.0);
        } else if done {
            widgets::status_dot(&painter, dot_c, dot_r, theme::SUCCESS, 0.0);
        } else {
            // 未到达（todo）：实心可见灰填充 + 更亮的环，避免深色背景上显得"空心/没画满"。
            painter.circle_filled(dot_c, dot_r, theme::W18);
            painter.circle_stroke(dot_c, dot_r, Stroke::new(1.25, theme::text_alpha(80)));
        }
        painter.text(
            Pos2::new(cx, area.top() + 32.0),
            egui::Align2::CENTER_CENTER,
            *label,
            egui::FontId::proportional(11.0),
            if on {
                theme::ACCENT
            } else {
                theme::text_alpha(115)
            },
        );
    }
}

// ---------------------------------------------------------------------------
// 主模板 F：深色工具风 · 环形进度 + 步骤条 · 间距统一 8 的倍数
// （JetBrains Toolbox / Discord 参考：视觉重心 + 流程感合一；双击直启用这套）
// ---------------------------------------------------------------------------

fn template_f(
    ui: &mut Ui,
    view: &PhaseView,
    retrying: bool,
    last_step: i32,
    store_error: Option<&str>,
) -> Option<Action> {
    content(ui, (16.0, 28.0, 20.0, 28.0), |ui| {
        let stage = view.stage();
        let width = ui.available_width();

        match stage {
            Stage::Checking => {
                ring_placeholder(ui);
                centered_title(ui, view, 16.0, 15.0);
                sub_line(ui, view, 8.0, 12.0, 140, true);
            }
            Stage::Downloading | Stage::Installing => {
                let installing = stage == Stage::Installing;
                let inner_sub = if installing {
                    "解压中".to_string()
                } else {
                    view.speed_text()
                };
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    widgets::progress_ring(
                        ui,
                        48.0,
                        view.percent() as f32,
                        theme::TEXT,
                        &inner_sub,
                    );
                });
                centered_title(ui, view, 16.0, 15.0);
                sub_line(ui, view, 8.0, 12.0, 140, true);
            }
            Stage::ForceUpdate => {
                return draw_force_update(ui, view, store_error);
            }
            _ => end_block(ui, view),
        }

        // 步骤条：环形给「当下进度」，步骤条给「流程位置」（错误态保留最后步骤）
        ui.add_space(16.0);
        draw_steps(ui, view, width, last_step);

        tail_block(ui, view, retrying)
    })
}

/// 主模板的「检测中」空环：灰盘 + 内盘 + 居中 spinner，96×96
fn ring_placeholder(ui: &mut Ui) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::new(96.0, 96.0), Sense::hover());
        let p = ui.painter_at(r);
        let c = r.center();
        p.circle_filled(c, 48.0, theme::W10);
        p.circle_filled(c, 39.0, theme::RING_INNER);
        widgets::spinner(&p, c, 13.0, 3.0, theme::ACCENT);
    });
}

// ---------------------------------------------------------------------------
// 副模板 H：极简轻量风 · 单行状态 + 3px 进度 + 折叠日志
// （VS Code / macOS 软件更新参考：进度即界面，信息压到最少；带参调用用这套）
// ---------------------------------------------------------------------------

fn template_h(
    ui: &mut Ui,
    view: &PhaseView,
    retrying: bool,
    store_error: Option<&str>,
) -> Option<Action> {
    content(ui, (14.0, 24.0, 14.0, 24.0), |ui| {
        let stage = view.stage();
        let width = ui.available_width();

        // 强制更新：提前返回专属界面，避免走下面单行状态/进度条布局
        if stage == Stage::ForceUpdate {
            return draw_force_update(ui, view, store_error);
        }

        // 单行状态：前置指示（进行中 spinner / 结束态状态点）+ 文案 + 尾部百分比
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(14.0, 14.0), Sense::hover());
            match stage {
                Stage::Checking | Stage::Downloading | Stage::Installing => {
                    widgets::spinner(
                        &ui.painter_at(r),
                        r.center(),
                        6.0,
                        2.0,
                        if stage == Stage::Installing {
                            theme::SUCCESS
                        } else {
                            theme::ACCENT
                        },
                    );
                }
                _ => {
                    let (color, halo) = end_dot_color(view);
                    widgets::status_dot(&ui.painter_at(r), r.center(), 5.0, color, halo);
                }
            }
            ui.add_space(8.0);
            ui.label(RichText::new(stage_title_by_stage(view)).size(13.5).color(
                if stage == Stage::Error {
                    theme::ERROR_TEXT
                } else {
                    theme::TEXT
                },
            ));
            if show_percent(view) {
                let pct = format!("{:.0}%", view.percent());
                let w = ui.available_width() - 56.0;
                if w > 0.0 {
                    ui.add_space(w.min(ui.available_width()));
                }
                ui.label(RichText::new(pct).size(13.0).color(theme::ACCENT));
            }
        });

        ui.add_space(10.0);
        if view.show_progress() {
            widgets::track(ui, width, view.percent() as f32, &widgets::TrackStyle::THIN);
        }

        // 单行明细：size · speed · eta，空项过滤；安装中补「请勿关闭窗口」
        let info = [view.size_text(), view.speed_text(), view.eta_text()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        if !info.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new(info).size(11.5).color(theme::text_alpha(128)));
        }
        if stage == Stage::Installing {
            ui.add_space(8.0);
            ui.label(
                RichText::new("请勿关闭窗口")
                    .size(11.5)
                    .color(theme::text_alpha(128)),
            );
        }
        error_hint_row(ui, view, 8.0);

        // 日志：无卡片，标题行带条数，列表限高 64px 内滚动
        let log = view.changelog();
        if !log.is_empty() {
            ui.add_space(12.0);
            ui.label(
                RichText::new(format!("更新日志 · 共 {} 项", log.len()))
                    .size(11.0)
                    .color(theme::text_alpha(102)),
            );
            ui.add_space(4.0);
            bullet_log(ui, &log);
        }

        if shows_actions(view) {
            ui.add_space(12.0);
            actions_line(ui, view, retrying, false)
        } else {
            None
        }
    })
}

/// 副模板百分比显示条件（结束态不显示 0%）
fn show_percent(view: &PhaseView) -> bool {
    match view.stage() {
        Stage::Downloading | Stage::Installing => true,
        Stage::Checking => view.percent() > 0.0,
        _ => false,
    }
}

/// 副模板的灰点日志列表：12px、行高 1.7、限高 64px 内滚动
fn bullet_log(ui: &mut Ui, items: &[&str]) {
    const ROW_W: f32 = 12.0;
    const FONT: f32 = 12.0;
    let line_h = FONT * 1.7;
    egui::ScrollArea::vertical()
        .max_height(64.0)
        .auto_shrink(false)
        .show(ui, |ui| {
            for line in items {
                ui.horizontal(|ui| {
                    let (dot_rect, _) =
                        ui.allocate_exact_size(Vec2::new(ROW_W, line_h), Sense::hover());
                    ui.painter_at(dot_rect).circle_filled(
                        Pos2::new(dot_rect.left() + 3.0, dot_rect.top() + 9.0),
                        2.0,
                        theme::text_alpha(128),
                    );
                    ui.label(
                        RichText::new(*line)
                            .size(FONT)
                            .color(theme::text_alpha(204)),
                    );
                });
            }
        });
}

// ---------------------------------------------------------------------------
// 纯逻辑 helper 单测
//
// 模板重构只动「差异怎么排布」，不动「文案与显示条件怎么算」。这一组测试
// 锁住抽出来的公共规则（阶段兜底文案、message 优先级、动作行/百分比的显示
// 条件、进度条规格），使两套画面的文案与状态判定不会随布局改动漂移。
// egui 的绘制部分需要窗口上下文，不在单测范围内。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use qsltd_updater_core::phase::PhasePayload;

    /// 全部阶段（与 Stage 变体一一对应，新增阶段时这里会漏，测试负责提醒）
    const ALL_STAGES: [Stage; 8] = [
        Stage::Checking,
        Stage::Downloading,
        Stage::Installing,
        Stage::UpToDate,
        Stage::Available,
        Stage::Done,
        Stage::Error,
        Stage::ForceUpdate,
    ];

    #[test]
    fn default_title_covers_every_stage() {
        let cases = [
            (Stage::Checking, "正在检查更新..."),
            (Stage::Downloading, "正在下载更新包..."),
            (Stage::Installing, "正在安装更新..."),
            (Stage::UpToDate, "已是最新版本"),
            (Stage::Available, "发现新版本"),
            (Stage::Done, "更新完成"),
            (Stage::Error, "更新完成"),
            (Stage::ForceUpdate, "更新完成"),
        ];
        assert_eq!(cases.len(), ALL_STAGES.len(), "兜底文案表必须覆盖全部阶段");
        for (stage, want) in cases {
            assert_eq!(default_title(stage), want, "阶段 {stage:?} 的兜底文案变了");
        }
    }

    #[test]
    fn backend_message_wins_over_fallback() {
        let payload = PhasePayload::new(Stage::Downloading, "正在拉取 12.3 MB", "v1", None);
        let view = PhaseView::new(&payload, true);
        assert_eq!(stage_title_by_stage(&view), "正在拉取 12.3 MB");

        let blank = PhasePayload::new(Stage::Downloading, "", "v1", None);
        let view = PhaseView::new(&blank, true);
        assert_eq!(stage_title_by_stage(&view), "正在下载更新包...");
    }

    #[test]
    fn end_title_only_up_to_date_has_its_own_fallback() {
        // 结束态里只有「已是最新版本」有自己的文案，其余（含 Available/Error）落「更新完成」
        for stage in [Stage::Done, Stage::Error, Stage::Available] {
            let payload = PhasePayload::new(stage, "", "", None);
            assert_eq!(end_title(&PhaseView::new(&payload, false)), "更新完成");
        }
        let payload = PhasePayload::new(Stage::UpToDate, "", "", None);
        assert_eq!(end_title(&PhaseView::new(&payload, false)), "已是最新版本");

        let payload = PhasePayload::new(Stage::UpToDate, "当前已是最新", "", None);
        assert_eq!(end_title(&PhaseView::new(&payload, false)), "当前已是最新");
    }

    #[test]
    fn actions_row_shows_only_on_error_and_done() {
        for stage in ALL_STAGES {
            let payload = PhasePayload::new(stage, "x", "", None);
            let view = PhaseView::new(&payload, false);
            let want = matches!(stage, Stage::Error | Stage::Done);
            assert_eq!(
                shows_actions(&view),
                want,
                "阶段 {stage:?} 的动作行显示条件变了"
            );
        }
    }

    #[test]
    fn percent_shows_while_working_and_hides_at_end() {
        for stage in [Stage::Downloading, Stage::Installing] {
            let payload = PhasePayload::new(stage, "x", "", Some(0.0));
            assert!(show_percent(&PhaseView::new(&payload, true)));
        }
        // 校验阶段只在真的有进度百分比时才占位
        let zero = PhasePayload::new(Stage::Checking, "x", "", Some(0.0));
        assert!(!show_percent(&PhaseView::new(&zero, false)));
        let some = PhasePayload::new(Stage::Checking, "x", "", Some(40.0));
        assert!(show_percent(&PhaseView::new(&some, false)));
        // 结束态一律不显示 0%
        for stage in [Stage::UpToDate, Stage::Available, Stage::Done, Stage::Error] {
            let payload = PhasePayload::new(stage, "x", "", Some(100.0));
            assert!(!show_percent(&PhaseView::new(&payload, true)));
        }
    }

    #[test]
    fn thin_track_matches_compact_template() {
        // 副模板只用 THIN（3px 纯蓝）；主模板走环形，不再有粗进度条
        assert_eq!(
            (
                widgets::TrackStyle::THIN.height,
                widgets::TrackStyle::THIN.gradient
            ),
            (3.0, false)
        );
    }

    #[test]
    fn sub_line_skips_empty_subtext() {
        // 空副文本不占位是骨架的前提（否则会凭空多出一段间距）
        let payload = PhasePayload::new(Stage::Checking, "正在检查更新...", "", None);
        assert!(PhaseView::new(&payload, false).sub().is_empty());
    }
}
