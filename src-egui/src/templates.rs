//! 八套启动器界面模板（A~H）—— A~E 为 UpdateTemplateA~E.vue 的 egui 复刻，
//! F/G/H 为后续新增的三套设计方向（深色工具风 / 品牌沉浸风 / 极简轻量风）
//!
//! 模板键与后端 `launcher_template` 配置（默认 F）一一对应。每套模板
//! 自持布局与配色（互不覆盖，与 Vue 版结构一致），展示值统一来自
//! `PhaseView`（usePhase.js 的 Rust 对应物）。

use egui::{emath::Pos2, Align, Align2, Color32, FontId, Layout, RichText, Sense, Stroke, Ui, UiBuilder, Vec2};

use qsltd_updater_core::phase::{PhaseView, Stage, STEP_LABELS};

use crate::theme;
use crate::widgets::{self, Action};

/// 绘制当前模板（按 template 键分发；未知键回落默认模板）
pub fn draw(
    ui: &mut Ui,
    view: &PhaseView,
    template: &str,
    retrying: bool,
    last_step: i32,
    store_error: Option<&str>,
) -> Option<Action> {
    match template {
        "A" => template_a(ui, view, retrying, store_error),
        "B" => template_b(ui, view, retrying, store_error),
        "C" => template_c(ui, view, retrying, last_step, store_error),
        "D" => template_d(ui, view, retrying, store_error),
        "E" => template_e(ui, view, retrying, store_error),
        "F" => template_f(ui, view, retrying, last_step, store_error),
        "G" => template_g(ui, view, retrying, store_error),
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
    let accent = if blocking { theme::WARNING } else { theme::ACCENT };
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
            ui.label(
                RichText::new(err)
                    .size(11.5)
                    .color(theme::text_alpha(140)),
            );
        });
    }

    // 版本对比卡：当前版本 → 要求版本
    ui.add_space(18.0);
    let width = ui.available_width();
    let (card, _) = ui.allocate_exact_size(Vec2::new(width, 60.0), Sense::hover());
    let painter = ui.painter_at(card);
    painter.rect_filled(card, 8.0, theme::W04);
    painter.rect_stroke(card, 8.0, Stroke::new(1.0, theme::W08), egui::StrokeKind::Inside);
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
        act = widgets::force_update_actions_row(ui, blocking, view.has_store());
    });

    act
}

/// 结束态（已是最新 / 发现新版本 / 完成 / 错误）的主文本兜底
/// （各模板 `message || (up-to-date ? '已是最新版本' : '更新完成')`）
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

/// 顶对齐标题行（.tpl-x__head）：标题 + 可选 pill（A 模板 pill 在右侧）
fn head_with_pill(ui: &mut Ui, title: &str, pill: Option<&str>, err: bool) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .size(16.0)
                .color(if err { theme::ERROR_TEXT } else { theme::TEXT }),
        );
        if let Some(b) = pill {
            // pill 推到行尾
            let w = ui.available_width() - 80.0;
            if w > 80.0 {
                ui.add_space(w.min(ui.available_width()));
            }
            widgets::pill(ui, b);
        }
    });
}

/// 下载数值区（.tpl-x__metrics：大百分比 26px 蓝 + 右侧速度）
fn metrics_row(ui: &mut Ui, view: &PhaseView) {
    ui.horizontal(|ui| {
        let pct = format!("{:.1}", view.percent());
        ui.label(RichText::new(pct).size(26.0).color(theme::ACCENT));
        ui.label(RichText::new("%").size(14.0).color(theme::text_alpha(153)));
        // 速度推到行尾
        let speed = view.speed_text();
        let w = ui.available_width() - if speed.is_empty() { 0.0 } else { 90.0 };
        if w > 0.0 {
            ui.add_space(w.min(ui.available_width()));
        }
        if !speed.is_empty() {
            ui.label(RichText::new(speed).size(12.0).color(theme::text_alpha(153)));
        }
    });
}

/// 下载明细行（.tpl-x__meta：左 sizeText 右 etaText）
fn meta_row(ui: &mut Ui, left: String, right: String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(&left).size(12.0).color(theme::text_alpha(140)));
        if !right.is_empty() {
            let w = ui.available_width() - 150.0;
            if w > 0.0 {
                ui.add_space(w.min(ui.available_width()));
            }
            ui.label(RichText::new(right).size(12.0).color(theme::text_alpha(140)));
        }
    });
}

/// spinner + 副文本行（.tpl-x__row）
fn spinner_row(ui: &mut Ui, sub: &str, color: Color32) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
        widgets::spinner(&ui.painter_at(r), r.center(), 7.75, 2.5, color);
        if !sub.is_empty() {
            ui.add_space(10.0);
            ui.label(RichText::new(sub).size(12.5).color(theme::text_alpha(166))); // 0.65
        }
    });
}

/// 状态点 + 标题行（结束态 .tpl-x__head）
fn dot_title_row(ui: &mut Ui, view: &PhaseView, dot_r: f32) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::new(dot_r * 2.0, dot_r * 2.0), Sense::hover());
        let (color, halo) = end_dot_color(view);
        widgets::status_dot(&ui.painter_at(r), r.center(), dot_r, color, halo);
        ui.add_space(10.0);
        ui.label(
            RichText::new(end_title(view))
                .size(16.0)
                .color(if view.stage() == Stage::Error {
                    theme::ERROR_TEXT
                } else {
                    theme::TEXT
                }),
        );
    });
}

// ---------------------------------------------------------------------------
// 模板 A：信息密度 · 顶对齐 · 大百分比 · 日志卡片吸底
// ---------------------------------------------------------------------------

fn template_a(
    ui: &mut Ui,
    view: &PhaseView,
    retrying: bool,
    store_error: Option<&str>,
) -> Option<Action> {
    content(ui, (16.0, 28.0, 22.0, 28.0), |ui| {
        let stage = view.stage();
        let width = ui.available_width();

        match stage {
            Stage::Checking => {
                spinner_row(ui, "", theme::ACCENT);
                ui.add_space(10.0);
                ui.label(
                    RichText::new(if view.message().is_empty() { "正在检查更新..." } else { view.message() })
                        .size(16.0)
                        .color(theme::TEXT),
                );
                if !view.sub().is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(view.sub()).size(12.0).color(theme::text_alpha(140)));
                }
                if !view.sub().is_empty() && view.percent() > 0.0 {
                    ui.add_space(12.0);
                    widgets::progress_track(ui, width, view.percent() as f32);
                }
            }
            Stage::Downloading => {
                head_with_pill(ui, if view.message().is_empty() { "正在下载更新包..." } else { view.message() }, view.badge(), false);
                ui.add_space(14.0);
                metrics_row(ui, view);
                ui.add_space(14.0);
                widgets::progress_track(ui, width, view.percent() as f32);
                ui.add_space(10.0);
                meta_row(ui, view.size_text(), view.eta_text());
            }
            Stage::Installing => {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
                    widgets::spinner(&ui.painter_at(r), r.center(), 7.75, 2.5, theme::SUCCESS);
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(if view.message().is_empty() { "正在安装更新..." } else { view.message() })
                            .size(16.0)
                            .color(theme::TEXT),
                    );
                });
                if !view.sub().is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(view.sub()).size(12.0).color(theme::text_alpha(140)));
                }
                ui.add_space(12.0);
                widgets::progress_track(ui, width, view.percent() as f32);
                ui.add_space(10.0);
                meta_row(ui, format!("{:.1}%", view.percent()), "请勿关闭窗口".to_string());
            }
            Stage::ForceUpdate => {
                return draw_force_update(ui, view, store_error);
            }
            _ => {
                dot_title_row(ui, view, 7.0);
                if !view.sub().is_empty() {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(view.sub())
                            .size(12.0)
                            .color(if stage == Stage::Error { theme::tint(theme::ERROR_TEXT, 191) } else { theme::text_alpha(140) }),
                    );
                }
                error_hint_row(ui, view, 8.0);
            }
        }

        // 日志卡片吸底占满剩余高度（动作行显示时为其预留空间，对应 CSS flex-shrink:0）
        let show_actions = stage == Stage::Error || stage == Stage::Done;
        let log = view.changelog();
        if !log.is_empty() {
            ui.add_space(16.0);
            let log_h = if show_actions {
                (ui.available_height() - 89.0).max(80.0)
            } else {
                (ui.available_height() - 42.0).max(60.0)
            };
            widgets::log_card(ui, &log, log_h);
        }

        if show_actions {
            ui.add_space(14.0);
            widgets::actions_row(
                ui,
                retrying,
                stage == Stage::Error,
                stage == Stage::Done,
                view.needs_homepage(),
            )
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// 模板 B：环形居中 · 视觉重心居中 · 日志贴底
// ---------------------------------------------------------------------------

fn template_b(
    ui: &mut Ui,
    view: &PhaseView,
    retrying: bool,
    store_error: Option<&str>,
) -> Option<Action> {
    content(ui, (14.0, 28.0, 20.0, 28.0), |ui| {
        let stage = view.stage();

        match stage {
            Stage::Checking => {
                // 空环（灰盘 + 内盘 spinner）
                let (r, _) = ui.allocate_exact_size(Vec2::new(96.0, 96.0), Sense::hover());
                let p = ui.painter_at(r);
                let c = r.center();
                p.circle_filled(c, 48.0, theme::W10);
                p.circle_filled(c, 39.0, theme::RING_INNER);
                widgets::spinner(&p, c, 13.0, 3.0, theme::ACCENT);
                ui.add_space(14.0);
                widgets::centered_label(
                    ui,
                    if view.message().is_empty() { "正在检查更新..." } else { view.message() },
                    15.0,
                    theme::TEXT,
                );
                if !view.sub().is_empty() {
                    ui.add_space(8.0);
                    widgets::centered_label(ui, view.sub(), 12.0, theme::text_alpha(140));
                }
            }
            Stage::Downloading => {
                widgets::progress_ring(ui, 48.0, view.percent() as f32, theme::TEXT, &view.speed_text());
                ui.add_space(14.0);
                widgets::centered_label(
                    ui,
                    if view.message().is_empty() { "正在下载更新包..." } else { view.message() },
                    15.0,
                    theme::TEXT,
                );
                ui.add_space(10.0);
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(view.size_text()).size(12.0).color(theme::text_alpha(140)));
                        if let Some(b) = view.badge() {
                            ui.add_space(10.0);
                            widgets::pill(ui, b);
                        }
                    });
                });
                if !view.eta_text().is_empty() {
                    ui.add_space(8.0);
                    widgets::centered_label(ui, &view.eta_text(), 11.0, theme::text_alpha(115));
                }
            }
            Stage::Installing => {
                widgets::progress_ring(ui, 48.0, view.percent() as f32, theme::SUCCESS, "解压中");
                ui.add_space(14.0);
                widgets::centered_label(
                    ui,
                    if view.message().is_empty() { "正在安装更新..." } else { view.message() },
                    15.0,
                    theme::TEXT,
                );
                if !view.sub().is_empty() {
                    ui.add_space(8.0);
                    widgets::centered_label(ui, view.sub(), 12.0, theme::text_alpha(140));
                }
            }
            Stage::ForceUpdate => {
                return draw_force_update(ui, view, store_error);
            }
            _ => {
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    // 直接在 top_down(Center) 里分配 28×28（halo 外沿 13 + 余量），
                    // 居中布局自动水平居中；不可包 horizontal 子容器——子容器
                    // 会吃掉剩余高度，把圆点顶到左侧、标题挤到底部
                    let (r, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
                    let (color, halo) = end_dot_color(view);
                    widgets::status_dot(&ui.painter_at(r), r.center(), 8.0, color, halo);
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(end_title(view))
                            .size(15.0)
                            .color(if stage == Stage::Error { theme::ERROR_TEXT } else { theme::TEXT }),
                    );
                });
                if !view.sub().is_empty() {
                    ui.add_space(8.0);
                    widgets::centered_label(
                        ui,
                        view.sub(),
                        12.0,
                        if stage == Stage::Error { theme::tint(theme::ERROR_TEXT, 191) } else { theme::text_alpha(140) },
                    );
                }
                error_hint_centered(ui, view, 8.0);
            }
        }

        // 日志卡片贴底（固定高度，列表滚动）
        let log = view.changelog();
        if !log.is_empty() {
            ui.add_space(14.0);
            widgets::log_card(ui, &log, 96.0);
        }

        let mut act = None;
        if stage == Stage::Error || stage == Stage::Done {
            ui.add_space(14.0);
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.horizontal(|ui| {
                    act = widgets::actions_row(
                        ui,
                        retrying,
                        stage == Stage::Error,
                        stage == Stage::Done,
                        view.needs_homepage(),
                    );
                })
            });
        }
        act
    })
}

// ---------------------------------------------------------------------------
// 模板 C：顶部步骤条 · 顶对齐 · 日志卡片吸底（默认模板）
// ---------------------------------------------------------------------------

fn template_c(
    ui: &mut Ui,
    view: &PhaseView,
    retrying: bool,
    last_step: i32,
    store_error: Option<&str>,
) -> Option<Action> {
    content(ui, (16.0, 28.0, 22.0, 28.0), |ui| {
        let stage = view.stage();
        let width = ui.available_width();

        // 步骤条：下载 → 校验 → 解压 → 完成
        draw_steps(ui, view, width, last_step);

        match stage {
            Stage::Checking => {
                ui.add_space(14.0);
                head_with_pill(
                    ui,
                    if view.message().is_empty() { "正在检查更新..." } else { view.message() },
                    view.badge(),
                    false,
                );
                ui.add_space(14.0);
                spinner_row(ui, view.sub(), theme::ACCENT);
                if !view.sub().is_empty() && view.percent() > 0.0 {
                    ui.add_space(14.0);
                    widgets::progress_track(ui, width, view.percent() as f32);
                }
            }
            Stage::Downloading => {
                ui.add_space(14.0);
                head_with_pill(ui, if view.message().is_empty() { "正在下载更新包..." } else { view.message() }, view.badge(), false);
                ui.add_space(14.0);
                metrics_row(ui, view);
                ui.add_space(14.0);
                widgets::progress_track(ui, width, view.percent() as f32);
                ui.add_space(10.0);
                meta_row(ui, view.size_text(), view.eta_text());
            }
            Stage::Installing => {
                ui.add_space(14.0);
                head_with_pill(
                    ui,
                    if view.message().is_empty() { "正在安装更新..." } else { view.message() },
                    view.badge(),
                    false,
                );
                ui.add_space(14.0);
                spinner_row(ui, view.sub(), theme::SUCCESS);
                ui.add_space(14.0);
                widgets::progress_track(ui, width, view.percent() as f32);
                ui.add_space(10.0);
                meta_row(ui, format!("{:.1}%", view.percent()), "请勿关闭窗口".to_string());
            }
            Stage::ForceUpdate => {
                return draw_force_update(ui, view, store_error);
            }
            _ => {
                ui.add_space(14.0);
                dot_title_row(ui, view, 7.0);
                if !view.sub().is_empty() {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(view.sub())
                            .size(12.5)
                            .color(if stage == Stage::Error { theme::tint(theme::ERROR_TEXT, 191) } else { theme::text_alpha(166) }),
                    );
                }
                error_hint_row(ui, view, 8.0);
            }
        }

        // 日志卡片吸底（动作行显示时预留空间，对应 CSS flex-shrink:0）
        let show_actions = stage == Stage::Error || stage == Stage::Done;
        let log = view.changelog();
        if !log.is_empty() {
            ui.add_space(14.0);
            let log_h = if show_actions {
                (ui.available_height() - 89.0).max(80.0)
            } else {
                (ui.available_height() - 42.0).max(60.0)
            };
            widgets::log_card(ui, &log, log_h);
        }

        if show_actions {
            ui.add_space(14.0);
            widgets::actions_row(
                ui,
                retrying,
                stage == Stage::Error,
                stage == Stage::Done,
                view.needs_homepage(),
            )
        } else {
            None
        }
    })
}

/// 步骤条（.tpl-c__steps：4 节点 + 3 连接线）
///
/// 整条步骤条占一个区域，用绝对 painter 绘制：先画连接线（贯穿相邻圆点
/// 边缘，不再留空隙），再画节点与标签。已完成段连接线为绿色半透明。
fn draw_steps(ui: &mut Ui, view: &PhaseView, width: f32, last_step: i32) {
    // 错误态保留最后到达的步骤（view.step() 返回 -2 表示保持不变）
    let step = if view.step() == -2 { last_step } else { view.step() };
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
            if done_seg { theme::tint(theme::SUCCESS, 128) } else { theme::W14 },
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
            painter.circle_stroke(dot_c, dot_r + 3.0, Stroke::new(1.0, theme::tint(theme::ACCENT, 128)));
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
            if on { theme::ACCENT } else { theme::text_alpha(115) },
        );
    }
}

// ---------------------------------------------------------------------------
// 模板 D：极简留白 · 居中 · 细进度条 · 日志内联无卡片
// ---------------------------------------------------------------------------

fn template_d(
    ui: &mut Ui,
    view: &PhaseView,
    retrying: bool,
    store_error: Option<&str>,
) -> Option<Action> {
    content(ui, (34.0, 32.0, 30.0, 32.0), |ui| {
        let stage = view.stage();
        let width = ui.available_width();

        match stage {
            Stage::Checking => {
                widgets::centered_label(
                    ui,
                    if view.message().is_empty() { "正在检查更新..." } else { view.message() },
                    15.0,
                    theme::TEXT,
                );
                ui.add_space(10.0);
                d_spinner_row(ui, view.sub(), theme::ACCENT);
                if !view.sub().is_empty() && view.percent() > 0.0 {
                    ui.add_space(10.0);
                    d_track(ui, width, view.percent() as f32);
                }
            }
            Stage::Downloading => {
                widgets::centered_label(
                    ui,
                    if view.message().is_empty() { "正在下载更新包..." } else { view.message() },
                    15.0,
                    theme::TEXT,
                );
                if let Some(b) = view.badge() {
                    ui.add_space(6.0);
                    widgets::centered_label(ui, b, 11.0, theme::text_alpha(115));
                }
                ui.add_space(16.0);
                widgets::centered_label(ui, &format!("{:.1}%", view.percent()), 20.0, theme::TEXT);
                ui.add_space(10.0);
                d_track(ui, width, view.percent() as f32);
                ui.add_space(10.0);
                let info = [view.size_text(), view.speed_text()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                let info = if view.eta_text().is_empty() {
                    info
                } else if info.is_empty() {
                    view.eta_text()
                } else {
                    format!("{} · {}", info, view.eta_text())
                };
                widgets::centered_label(ui, &info, 12.0, theme::text_alpha(128));
            }
            Stage::Installing => {
                widgets::centered_label(
                    ui,
                    if view.message().is_empty() { "正在安装更新..." } else { view.message() },
                    15.0,
                    theme::TEXT,
                );
                ui.add_space(10.0);
                d_spinner_row(ui, view.sub(), theme::SUCCESS);
                ui.add_space(16.0);
                widgets::centered_label(ui, &format!("{:.1}%", view.percent()), 20.0, theme::TEXT);
                ui.add_space(10.0);
                d_track(ui, width, view.percent() as f32);
            }
            Stage::ForceUpdate => {
                return draw_force_update(ui, view, store_error);
            }
            _ => {
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    // 同 B：居中布局直接分配 26×26（dot 7 + halo 5 + 余量）
                    let (r, _) = ui.allocate_exact_size(Vec2::new(26.0, 26.0), Sense::hover());
                    let (color, halo) = end_dot_color(view);
                    widgets::status_dot(&ui.painter_at(r), r.center(), 7.0, color, halo);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(end_title(view))
                            .size(15.0)
                            .color(if stage == Stage::Error { theme::ERROR_TEXT } else { theme::TEXT }),
                    );
                });
                if !view.sub().is_empty() {
                    ui.add_space(6.0);
                    widgets::centered_label(
                        ui,
                        view.sub(),
                        12.0,
                        if stage == Stage::Error { theme::tint(theme::ERROR_TEXT, 191) } else { theme::text_alpha(128) },
                    );
                }
                error_hint_centered(ui, view, 6.0);
            }
        }

        let log = view.changelog();
        if !log.is_empty() {
            ui.add_space(14.0);
            widgets::inline_log(ui, &log);
        }

        let mut act = None;
        if stage == Stage::Error || stage == Stage::Done {
            ui.add_space(14.0);
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.horizontal(|ui| {
                    act = widgets::actions_row(
                        ui,
                        retrying,
                        stage == Stage::Error,
                        stage == Stage::Done,
                        view.needs_homepage(),
                    );
                })
            });
        }
        act
    })
}

/// D 模板细进度条（4px 圆角，纯蓝填充）
fn d_track(ui: &mut Ui, width: f32, percent: f32) {
    let height = 4.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::W10);
    let fill_w = (rect.width() * percent.clamp(0.0, 100.0) / 100.0).max(3.0);
    let fill = egui::Rect::from_min_size(rect.left_top(), Vec2::new(fill_w, height));
    painter.rect_filled(fill, 2.0, theme::ACCENT);
}

/// D 模板 spinner 行（16px spinner）
fn d_spinner_row(ui: &mut Ui, sub: &str, color: Color32) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
            widgets::spinner(&ui.painter_at(r), r.center(), 7.0, 2.0, color);
            if !sub.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new(sub).size(12.0).color(theme::text_alpha(128)));
            }
        });
    });
}

// ---------------------------------------------------------------------------
// 模板 E：日志为主 · 进度压成顶部状态条 · 日志占主体
// ---------------------------------------------------------------------------

fn template_e(
    ui: &mut Ui,
    view: &PhaseView,
    retrying: bool,
    store_error: Option<&str>,
) -> Option<Action> {
    content(ui, (20.0, 26.0, 18.0, 26.0), |ui| {
        let stage = view.stage();
        let width = ui.available_width();

        // 强制更新：提前返回专属界面，避免走下面通用状态条/日志布局
        if stage == Stage::ForceUpdate {
            return draw_force_update(ui, view, store_error);
        }

        // 顶部紧凑状态条：指示灯 + 文案 + 百分比
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), Sense::hover());
            let led_color = match stage {
                Stage::UpToDate | Stage::Done => theme::SUCCESS,
                Stage::Error => theme::DANGER,
                _ => theme::ACCENT,
            };
            widgets::status_dot(&ui.painter_at(r), r.center(), 4.0, led_color, 3.0);
            ui.add_space(10.0);
            ui.label(
                RichText::new(if view.message().is_empty() { status_fallback(view) } else { view.message() })
                    .size(14.0)
                    .color(if stage == Stage::Error { theme::ERROR_TEXT } else { theme::TEXT }),
            );
            if show_percent(view) {
                let pct = format!("{:.1}%", view.percent());
                let w = ui.available_width() - 70.0;
                if w > 0.0 {
                    ui.add_space(w.min(ui.available_width()));
                }
                ui.label(RichText::new(pct).size(14.0).color(theme::ACCENT));
            }
        });

        if view.show_progress() {
            ui.add_space(14.0);
            e_track(ui, width, view.percent() as f32);
        }

        // 主体：更新日志
        ui.add_space(14.0);
        let log = view.changelog();
        ui.horizontal(|ui| {
            ui.label(RichText::new("更新日志").size(14.0).color(theme::TEXT));
            if !log.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("共 {} 项", log.len()))
                        .size(11.0)
                        .color(theme::text_alpha(102)),
                );
            }
        });
        if log.is_empty() {
            ui.with_layout(
                Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.label(
                        RichText::new("本次更新未提供更新日志")
                            .size(12.5)
                            .color(theme::text_alpha(89)),
                    );
                },
            );
        } else {
            e_log_list(ui, &log);
        }

        if !view.sub().is_empty() && stage != Stage::Downloading {
            ui.add_space(14.0);
            ui.label(RichText::new(view.sub()).size(12.0).color(theme::text_alpha(153)));
        }
        error_hint_row(ui, view, 10.0);

        // 底部明细：左 sizeText（错误时兜底文案）右 speed/eta，上分割线
        ui.add_space(10.0);
        let (fr, _) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::hover());
        let fp = ui.painter_at(fr);
        fp.rect_filled(
            egui::Rect::from_min_max(fr.left_top(), Pos2::new(fr.right(), fr.top() + 1.0)),
            0.0,
            theme::W08,
        );
        let left = if !view.size_text().is_empty() {
            view.size_text()
        } else if stage == Stage::Error {
            "更新未完成".to_string()
        } else {
            String::new()
        };
        let right = if !view.speed_text().is_empty() {
            view.speed_text()
        } else {
            view.eta_text()
        };
        fp.text(
            Pos2::new(fr.left(), fr.top() + 14.0),
            egui::Align2::LEFT_CENTER,
            &left,
            egui::FontId::proportional(11.5),
            theme::text_alpha(128),
        );
        if !right.is_empty() {
            fp.text(
                Pos2::new(fr.right(), fr.top() + 14.0),
                egui::Align2::RIGHT_CENTER,
                &right,
                egui::FontId::proportional(11.5),
                theme::text_alpha(128),
            );
        }

        if stage == Stage::Error || stage == Stage::Done {
            ui.add_space(14.0);
            widgets::actions_row(
                ui,
                retrying,
                stage == Stage::Error,
                stage == Stage::Done,
                view.needs_homepage(),
            )
        } else {
            None
        }
    })
}

/// E 模板百分比显示条件（结束态不显示 0%）
fn show_percent(view: &PhaseView) -> bool {
    match view.stage() {
        Stage::Downloading | Stage::Installing => true,
        Stage::Checking => view.percent() > 0.0,
        _ => false,
    }
}

/// 错误态左对齐提示行（「下载失败，已自动重试 2 次（共 3 次尝试）。可点击…」）
///
/// 只在错误态出现：正常阶段用户不需要知道自己被重试了几次。
fn error_hint_row(ui: &mut Ui, view: &PhaseView, gap: f32) {
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
    ui.label(RichText::new(hint).size(11.5).color(theme::text_alpha(153)));
}

/// 错误态居中提示行（B/D 模板居中版）
fn error_hint_centered(ui: &mut Ui, view: &PhaseView, gap: f32) {
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
    widgets::centered_label(ui, &hint, 11.5, theme::text_alpha(153));
}

/// E 模板结束态文案兜底（后端未带 message 时）
fn status_fallback(view: &PhaseView) -> &'static str {
    match view.stage() {
        Stage::UpToDate => "已是最新版本",
        Stage::Available => "发现新版本",
        Stage::Checking => "正在检查更新...",
        Stage::Downloading => "正在下载更新包...",
        Stage::Installing => "正在安装更新...",
        _ => "更新完成",
    }
}

/// E 模板 6px 渐变进度条
fn e_track(ui: &mut Ui, width: f32, percent: f32) {
    let height = 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, theme::W10);
    let fill_w = (rect.width() * percent.clamp(0.0, 100.0) / 100.0).max(4.0);
    let fill = egui::Rect::from_min_size(rect.left_top(), Vec2::new(fill_w, height));
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(fill.left_top(), theme::ACCENT);
    mesh.colored_vertex(fill.right_top(), theme::SUCCESS);
    mesh.colored_vertex(fill.right_bottom(), theme::SUCCESS);
    mesh.colored_vertex(fill.left_bottom(), theme::ACCENT);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// E 模板日志列表（滚动，蓝点条目，占满主体）
fn e_log_list(ui: &mut Ui, items: &[&str]) {
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for line in items {
                ui.horizontal(|ui| {
                    let line_h = 12.5 * 1.8;
                    let (dot_rect, _) =
                        ui.allocate_exact_size(Vec2::new(14.0, line_h), Sense::hover());
                    ui.painter_at(dot_rect).circle_filled(
                        Pos2::new(dot_rect.left() + 3.0, dot_rect.top() + 10.0),
                        2.0,
                        theme::ACCENT,
                    );
                    ui.label(RichText::new(*line).size(12.5).color(theme::text_alpha(217)));
                });
            }
        });
}

// ---------------------------------------------------------------------------
// 模板 F：深色工具风 · 环形进度 + 步骤条 · 间距统一 8 的倍数
// （JetBrains Toolbox / Discord 参考：B 的视觉重心 + C 的流程感合一）
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
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(96.0, 96.0), Sense::hover());
                    let p = ui.painter_at(r);
                    let c = r.center();
                    p.circle_filled(c, 48.0, theme::W10);
                    p.circle_filled(c, 39.0, theme::RING_INNER);
                    widgets::spinner(&p, c, 13.0, 3.0, theme::ACCENT);
                });
                ui.add_space(16.0);
                widgets::centered_label(
                    ui,
                    if view.message().is_empty() { "正在检查更新..." } else { view.message() },
                    15.0,
                    theme::TEXT,
                );
                if !view.sub().is_empty() {
                    ui.add_space(8.0);
                    widgets::centered_label(ui, view.sub(), 12.0, theme::text_alpha(140));
                }
            }
            Stage::Downloading | Stage::Installing => {
                let installing = stage == Stage::Installing;
                let inner_sub = if installing { "解压中".to_string() } else { view.speed_text() };
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    widgets::progress_ring(ui, 48.0, view.percent() as f32, theme::TEXT, &inner_sub);
                });
                ui.add_space(16.0);
                widgets::centered_label(
                    ui,
                    if view.message().is_empty() {
                        if installing { "正在安装更新..." } else { "正在下载更新包..." }
                    } else {
                        view.message()
                    },
                    15.0,
                    theme::TEXT,
                );
                if !view.sub().is_empty() {
                    ui.add_space(8.0);
                    widgets::centered_label(ui, view.sub(), 12.0, theme::text_alpha(140));
                }
            }
            Stage::ForceUpdate => {
                return draw_force_update(ui, view, store_error);
            }
            _ => {
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    // 同 B：居中布局直接分配 28×28，避免 horizontal 子容器撑爆高度
                    let (r, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
                    let (color, halo) = end_dot_color(view);
                    widgets::status_dot(&ui.painter_at(r), r.center(), 8.0, color, halo);
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(end_title(view))
                            .size(15.0)
                            .color(if stage == Stage::Error { theme::ERROR_TEXT } else { theme::TEXT }),
                    );
                });
                if !view.sub().is_empty() {
                    ui.add_space(8.0);
                    widgets::centered_label(
                        ui,
                        view.sub(),
                        12.0,
                        if stage == Stage::Error { theme::tint(theme::ERROR_TEXT, 191) } else { theme::text_alpha(140) },
                    );
                }
                error_hint_centered(ui, view, 8.0);
            }
        }

        // 步骤条：环形给「当下进度」，步骤条给「流程位置」（错误态保留最后步骤）
        ui.add_space(16.0);
        draw_steps(ui, view, width, last_step);

        let log = view.changelog();
        if !log.is_empty() {
            ui.add_space(16.0);
            widgets::log_card(ui, &log, 96.0);
        }

        let mut act = None;
        if stage == Stage::Error || stage == Stage::Done {
            ui.add_space(16.0);
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.horizontal(|ui| {
                    act = widgets::actions_row(
                        ui,
                        retrying,
                        stage == Stage::Error,
                        stage == Stage::Done,
                        view.needs_homepage(),
                    );
                })
            });
        }
        act
    })
}

// ---------------------------------------------------------------------------
// 模板 G：品牌沉浸风 · 上部品牌面板 + 底部细进度条
// （Steam / Epic 参考：更新器兼作品牌展示位，进度压缩为一条线）
// ---------------------------------------------------------------------------

fn template_g(
    ui: &mut Ui,
    view: &PhaseView,
    retrying: bool,
    store_error: Option<&str>,
) -> Option<Action> {
    content(ui, (16.0, 20.0, 16.0, 20.0), |ui| {
        let stage = view.stage();
        let width = ui.available_width();
        let show_actions = stage == Stage::Error || stage == Stage::Done;

        // 底部区：进度线 + 明细行（+ 动作行）；hero 吃掉其余高度
        let bottom_h = 40.0 + if show_actions { 44.0 } else { 0.0 };
        let hero_h = (ui.available_height() - bottom_h - 16.0).max(180.0);

        // 品牌面板：圆角卡片 + 居中状态主视觉（painter 直绘，避免嵌套布局）
        let (hero, _) = ui.allocate_exact_size(Vec2::new(width, hero_h), Sense::hover());
        let hp = ui.painter_at(hero);
        hp.rect_filled(hero, 10.0, theme::W04);
        hp.rect_stroke(hero, 10.0, Stroke::new(1.0, theme::W08), egui::StrokeKind::Inside);
        let c = hero.center();
        match stage {
            Stage::Checking => {
                widgets::spinner(&hp, Pos2::new(c.x, c.y - 26.0), 14.0, 3.0, theme::ACCENT);
                hp.text(
                    Pos2::new(c.x, c.y + 14.0),
                    egui::Align2::CENTER_CENTER,
                    if view.message().is_empty() { "正在检查更新..." } else { view.message() },
                    egui::FontId::proportional(15.0),
                    theme::TEXT,
                );
            }
            Stage::Downloading | Stage::Installing => {
                let installing = stage == Stage::Installing;
                hp.text(
                    Pos2::new(c.x, c.y - 12.0),
                    egui::Align2::CENTER_CENTER,
                    format!("{:.0}%", view.percent()),
                    egui::FontId::proportional(34.0),
                    theme::ACCENT,
                );
                hp.text(
                    Pos2::new(c.x, c.y + 22.0),
                    egui::Align2::CENTER_CENTER,
                    if view.message().is_empty() {
                        if installing { "正在安装更新..." } else { "正在下载更新包..." }
                    } else {
                        view.message()
                    },
                    egui::FontId::proportional(13.0),
                    theme::text_alpha(166),
                );
            }
            Stage::ForceUpdate => {
                return draw_force_update(ui, view, store_error);
            }
            _ => {
                let (color, halo) = end_dot_color(view);
                widgets::status_dot(&hp, Pos2::new(c.x, c.y - 34.0), 6.0, color, halo.max(3.0));
                hp.text(
                    Pos2::new(c.x, c.y - 4.0),
                    egui::Align2::CENTER_CENTER,
                    end_title(view),
                    egui::FontId::proportional(18.0),
                    if stage == Stage::Error { theme::ERROR_TEXT } else { theme::TEXT },
                );
                if !view.sub().is_empty() {
                    hp.text(
                        Pos2::new(c.x, c.y + 22.0),
                        egui::Align2::CENTER_CENTER,
                        view.sub(),
                        egui::FontId::proportional(12.0),
                        if stage == Stage::Error { theme::tint(theme::ERROR_TEXT, 191) } else { theme::text_alpha(140) },
                    );
                }
            }
        }

        ui.add_space(16.0);
        thin_track(ui, width, view.percent() as f32);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let left = if !view.size_text().is_empty() {
                view.size_text()
            } else if let Some(b) = view.badge() {
                b.to_string()
            } else {
                String::new()
            };
            if !left.is_empty() {
                ui.label(RichText::new(left).size(11.5).color(theme::text_alpha(128)));
            }
            let right = if !view.speed_text().is_empty() {
                view.speed_text()
            } else {
                view.eta_text()
            };
            if !right.is_empty() {
                let w = ui.available_width() - 150.0;
                if w > 0.0 {
                    ui.add_space(w.min(ui.available_width()));
                }
                ui.label(RichText::new(right).size(11.5).color(theme::text_alpha(128)));
            }
        });

        if show_actions {
            error_hint_row(ui, view, 8.0);
            ui.add_space(12.0);
            widgets::actions_row(
                ui,
                retrying,
                stage == Stage::Error,
                stage == Stage::Done,
                view.needs_homepage(),
            )
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// 模板 H：极简轻量风 · 单行状态 + 3px 进度 + 折叠日志
// （VS Code / macOS 软件更新参考：进度即界面，信息压到最少）
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
                        if stage == Stage::Installing { theme::SUCCESS } else { theme::ACCENT },
                    );
                }
                _ => {
                    let (color, halo) = end_dot_color(view);
                    widgets::status_dot(&ui.painter_at(r), r.center(), 5.0, color, halo);
                }
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new(if view.message().is_empty() { status_fallback(view) } else { view.message() })
                    .size(13.5)
                    .color(if stage == Stage::Error { theme::ERROR_TEXT } else { theme::TEXT }),
            );
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
            thin_track(ui, width, view.percent() as f32);
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
            ui.label(RichText::new("请勿关闭窗口").size(11.5).color(theme::text_alpha(128)));
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
            egui::ScrollArea::vertical()
                .max_height(64.0)
                .auto_shrink(false)
                .show(ui, |ui| {
                    for line in &log {
                        ui.horizontal(|ui| {
                            let line_h = 12.0 * 1.7;
                            let (dot_rect, _) =
                                ui.allocate_exact_size(Vec2::new(12.0, line_h), Sense::hover());
                            ui.painter_at(dot_rect).circle_filled(
                                Pos2::new(dot_rect.left() + 3.0, dot_rect.top() + 9.0),
                                2.0,
                                theme::text_alpha(128),
                            );
                            ui.label(RichText::new(*line).size(12.0).color(theme::text_alpha(204)));
                        });
                    }
                });
        }

        if stage == Stage::Error || stage == Stage::Done {
            ui.add_space(12.0);
            widgets::actions_row(
                ui,
                retrying,
                stage == Stage::Error,
                stage == Stage::Done,
                view.needs_homepage(),
            )
        } else {
            None
        }
    })
}

/// 3px 细进度条（G/H 共用：纯蓝填充，无渐变，比 D 的 4px 更收敛）
fn thin_track(ui: &mut Ui, width: f32, percent: f32) {
    let height = 3.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 1.5, theme::W10);
    let fill_w = (rect.width() * percent.clamp(0.0, 100.0) / 100.0).max(2.0);
    let fill = egui::Rect::from_min_size(rect.left_top(), Vec2::new(fill_w, height));
    painter.rect_filled(fill, 1.5, theme::ACCENT);
}
