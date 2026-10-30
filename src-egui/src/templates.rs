//! 八套启动器界面模板（A~H）—— A~E 为 UpdateTemplateA~E.vue 的 egui 复刻，
//! F/G/H 为后续新增的三套设计方向（深色工具风 / 品牌沉浸风 / 极简轻量风）
//!
//! 模板键与后端 `launcher_template` 配置（默认 F）一一对应。每套模板
//! 自持布局与配色（互不覆盖，与 Vue 版结构一致），展示值统一来自
//! `PhaseView`（usePhase.js 的 Rust 对应物）。

use egui::{
    emath::Pos2, Align, Align2, Color32, FontId, Layout, RichText, Sense, Stroke, Ui, UiBuilder,
    Vec2,
};

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
        ui.label(RichText::new(title).size(16.0).color(if err {
            theme::ERROR_TEXT
        } else {
            theme::TEXT
        }));
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
            ui.label(
                RichText::new(speed)
                    .size(12.0)
                    .color(theme::text_alpha(153)),
            );
        }
    });
}

/// 下载明细行（.tpl-x__meta：左 sizeText 右 etaText）
fn meta_row(ui: &mut Ui, left: String, right: String) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&left)
                .size(12.0)
                .color(theme::text_alpha(140)),
        );
        if !right.is_empty() {
            let w = ui.available_width() - 150.0;
            if w > 0.0 {
                ui.add_space(w.min(ui.available_width()));
            }
            ui.label(
                RichText::new(right)
                    .size(12.0)
                    .color(theme::text_alpha(140)),
            );
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
            ui.label(RichText::new(sub).size(12.5).color(theme::text_alpha(166)));
            // 0.65
        }
    });
}

// ---------------------------------------------------------------------------
// 公共骨架：八套模板的差异集中在这里参数化
//
// 八套模板原本的重复度是：`message().is_empty() ? 兜底 : message()` 出现 20 次、
// 结束态「状态点 + 标题 + 副文本 + 错误提示」出现 6 次、「日志卡 + 动作行」尾部出现 6 次、
// 四种进度条各写一遍。以下 helper 把这些复制压成一处，模板函数只声明自己的差异
// （原先的 `dot_title_row` 也并入 [`end_block`]，避免两种写法并存）。
// ---------------------------------------------------------------------------

/// 各阶段的兜底标题（后端没带 message 时用；与 `PhaseView::stage` 一一对应）
///
/// 结束态三个阶段的兜底沿用 E/H 紧凑条原有的文案表：`Available` 是「发现新版本」，
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

/// 进行中阶段的标题（模板里最常见的形态：按 stage 取兜底）
fn stage_title_by_stage<'a>(view: &'a PhaseView<'a>) -> &'a str {
    stage_title(view, default_title(view.stage()))
}

/// 居中大标题（B/D/F 进行中阶段：与结束态同一套「message 优先、阶段兜底」规则）
///
/// `gap` 为与上一块的间距，传 0 表示紧贴（D 的标题就是内容第一行）。
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
/// `centered` 区分 B/D/F 的居中版与 A/C/E/G/H 的左对齐版。
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

/// 左对齐错误提示行（E/G/H 自绘布局用；居中版由 [`end_block`] 内部处理）
fn error_hint_row(ui: &mut Ui, view: &PhaseView, gap: f32) {
    error_hint_line(ui, view, gap, false);
}

/// 结束态（已是最新 / 发现新版本 / 完成 / 错误）的整块布局参数
///
/// 八套模板在这一块只差「点多大、标题多大、间距多少、居中还是左对齐」，
/// 逻辑（状态点颜色取自 [`end_dot_color`]、错误态副文本转红、空副文本不占位）完全一致。
struct EndStyle {
    /// 状态点半径
    dot_r: f32,
    /// 分配给状态点的方形边长（居中版需要 >= 2*dot_r + halo 余量，否则被 painter 裁切）
    box_size: f32,
    /// 状态点与标题之间的间距
    title_gap: f32,
    /// 标题字号
    title_size: f32,
    /// 副文本样式（gap / size / alpha）
    sub_gap: f32,
    sub_size: f32,
    sub_alpha: u8,
    /// 错误提示行与上方的间距
    hint_gap: f32,
    /// true = B/D/F 的居中版（点在标题上方），false = A/C 的左对齐版（点在标题左侧）
    centered: bool,
}

impl EndStyle {
    /// A/C：左对齐，点 7px + 16px 标题
    const ROW: EndStyle = EndStyle {
        dot_r: 7.0,
        box_size: 14.0,
        title_gap: 10.0,
        title_size: 16.0,
        sub_gap: 8.0,
        sub_size: 12.0,
        sub_alpha: 140,
        hint_gap: 8.0,
        centered: false,
    };
    /// C：左对齐但副文本更大更亮（12.5 / 166）
    const ROW_C: EndStyle = EndStyle {
        sub_size: 12.5,
        sub_alpha: 166,
        ..EndStyle::ROW
    };
    /// B/F：居中，点 8px（28×28 留 halo 余量）+ 15px 标题
    const CENTERED: EndStyle = EndStyle {
        dot_r: 8.0,
        box_size: 28.0,
        title_gap: 4.0,
        title_size: 15.0,
        sub_gap: 8.0,
        sub_size: 12.0,
        sub_alpha: 140,
        hint_gap: 8.0,
        centered: true,
    };
    /// F：同 B，但点与标题几乎贴着（2px）
    const CENTERED_F: EndStyle = EndStyle {
        title_gap: 2.0,
        ..EndStyle::CENTERED
    };
    /// D：居中，点 7px（26×26）+ 更紧的间距与更淡的副文本
    const CENTERED_D: EndStyle = EndStyle {
        dot_r: 7.0,
        box_size: 26.0,
        title_gap: 6.0,
        title_size: 15.0,
        sub_gap: 6.0,
        sub_size: 12.0,
        sub_alpha: 128,
        hint_gap: 6.0,
        centered: true,
    };
}

/// 结束态整块：状态点 + 标题（+ 副文本 + 错误提示）
///
/// 居中版必须在 `top_down(Align::Center)` 里直接分配方形，不能包一层 horizontal 子容器——
/// 子容器会吃掉剩余高度，把圆点顶到左侧、标题挤到底部（B/D/F 实测踩过）。
fn end_block(ui: &mut Ui, view: &PhaseView, style: &EndStyle) {
    let is_err = view.stage() == Stage::Error;
    let title_color = if is_err {
        theme::ERROR_TEXT
    } else {
        theme::TEXT
    };
    let sub_color = |alpha: u8| {
        if is_err {
            theme::tint(theme::ERROR_TEXT, 191)
        } else {
            theme::text_alpha(alpha)
        }
    };

    if style.centered {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            let (r, _) =
                ui.allocate_exact_size(Vec2::new(style.box_size, style.box_size), Sense::hover());
            let (color, halo) = end_dot_color(view);
            widgets::status_dot(&ui.painter_at(r), r.center(), style.dot_r, color, halo);
            ui.add_space(style.title_gap);
            ui.label(
                RichText::new(end_title(view))
                    .size(style.title_size)
                    .color(title_color),
            );
        });
    } else {
        ui.horizontal(|ui| {
            let (r, _) =
                ui.allocate_exact_size(Vec2::new(style.box_size, style.box_size), Sense::hover());
            let (color, halo) = end_dot_color(view);
            widgets::status_dot(&ui.painter_at(r), r.center(), style.dot_r, color, halo);
            ui.add_space(style.title_gap);
            ui.label(
                RichText::new(end_title(view))
                    .size(style.title_size)
                    .color(title_color),
            );
        });
    }

    if !view.sub().is_empty() {
        ui.add_space(style.sub_gap);
        let sub = view.sub();
        if style.centered {
            widgets::centered_label(ui, sub, style.sub_size, sub_color(style.sub_alpha));
        } else {
            ui.label(
                RichText::new(sub)
                    .size(style.sub_size)
                    .color(sub_color(style.sub_alpha)),
            );
        }
    }
    error_hint_line(ui, view, style.hint_gap, style.centered);
}

/// 尾部日志区的形态
enum LogMode {
    /// 吸底并占满剩余高度（动作行显示时预留其高度，对应 CSS `flex-shrink: 0`）
    Flex {
        actions_reserve: f32,
        actions_min: f32,
        plain_reserve: f32,
        plain_min: f32,
    },
    /// 固定高度，列表内部滚动
    Fixed(f32),
    /// 无卡片内联列表（D）
    Inline,
    /// 模板自绘日志区（E/G/H），骨架不介入
    Custom,
}

/// 尾部（日志 + 动作行）参数
struct TailStyle {
    /// 日志区与上方内容的间距
    log_gap: f32,
    log: LogMode,
    /// 动作行与日志/内容的间距
    actions_gap: f32,
    /// 动作行居中（B/D/F）还是左对齐（A/C/E/G/H）
    actions_centered: bool,
}

impl TailStyle {
    /// A/C：日志吸底自适应 + 左对齐动作行
    const FLEX_LEFT: TailStyle = TailStyle {
        log_gap: 16.0,
        log: LogMode::Flex {
            actions_reserve: 89.0,
            actions_min: 80.0,
            plain_reserve: 42.0,
            plain_min: 60.0,
        },
        actions_gap: 14.0,
        actions_centered: false,
    };
    /// C：日志前间距 14（其余同 A）
    const FLEX_LEFT_C: TailStyle = TailStyle {
        log_gap: 14.0,
        actions_gap: 14.0,
        ..TailStyle::FLEX_LEFT
    };
    /// B/F：固定 96px 日志卡 + 居中动作行
    const FIXED_CENTERED: TailStyle = TailStyle {
        log_gap: 14.0,
        log: LogMode::Fixed(96.0),
        actions_gap: 14.0,
        actions_centered: true,
    };
    /// F：日志/动作行间距统一 16
    const FIXED_CENTERED_F: TailStyle = TailStyle {
        log_gap: 16.0,
        actions_gap: 16.0,
        ..TailStyle::FIXED_CENTERED
    };
    /// D：无卡片内联日志 + 居中动作行
    const INLINE_CENTERED: TailStyle = TailStyle {
        log_gap: 14.0,
        log: LogMode::Inline,
        actions_gap: 14.0,
        actions_centered: true,
    };
    /// E/G/H：模板自己接管日志与动作行
    const CUSTOM: TailStyle = TailStyle {
        log_gap: 0.0,
        log: LogMode::Custom,
        actions_gap: 0.0,
        actions_centered: false,
    };
}

/// 是否需要在底部显示动作行（重试 / 前往官网 / 立即启动）
fn shows_actions(view: &PhaseView) -> bool {
    let stage = view.stage();
    stage == Stage::Error || stage == Stage::Done
}

/// 动作行：左对齐直接排布，居中版套一层 top_down(Center)
fn actions_line(ui: &mut Ui, view: &PhaseView, retrying: bool, centered: bool) -> Option<Action> {
    let stage = view.stage();
    let call = |ui: &mut Ui| {
        widgets::actions_row(
            ui,
            retrying,
            stage == Stage::Error,
            stage == Stage::Done,
            view.needs_homepage(),
        )
    };
    if centered {
        let mut act = None;
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.horizontal(|ui| {
                act = call(ui);
            })
        });
        act
    } else {
        call(ui)
    }
}

/// 尾部：按 [`TailStyle`] 渲染日志区与动作行，返回动作
fn tail_block(ui: &mut Ui, view: &PhaseView, retrying: bool, style: &TailStyle) -> Option<Action> {
    let show_actions = shows_actions(view);
    let log = view.changelog();
    match style.log {
        LogMode::Custom => {}
        LogMode::Inline => {
            if !log.is_empty() {
                ui.add_space(style.log_gap);
                widgets::inline_log(ui, &log);
            }
        }
        LogMode::Fixed(h) => {
            if !log.is_empty() {
                ui.add_space(style.log_gap);
                widgets::log_card(ui, &log, h);
            }
        }
        LogMode::Flex {
            actions_reserve,
            actions_min,
            plain_reserve,
            plain_min,
        } => {
            if !log.is_empty() {
                ui.add_space(style.log_gap);
                let (reserve, min_h) = if show_actions {
                    (actions_reserve, actions_min)
                } else {
                    (plain_reserve, plain_min)
                };
                widgets::log_card(ui, &log, (ui.available_height() - reserve).max(min_h));
            }
        }
    }

    if show_actions {
        ui.add_space(style.actions_gap);
        actions_line(ui, view, retrying, style.actions_centered)
    } else {
        None
    }
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
                    RichText::new(stage_title(view, default_title(stage)))
                        .size(16.0)
                        .color(theme::TEXT),
                );
                sub_line(ui, view, 8.0, 12.0, 140, false);
                if !view.sub().is_empty() && view.percent() > 0.0 {
                    ui.add_space(12.0);
                    widgets::progress_track(ui, width, view.percent() as f32);
                }
            }
            Stage::Downloading => {
                head_with_pill(
                    ui,
                    stage_title(view, default_title(stage)),
                    view.badge(),
                    false,
                );
                ui.add_space(14.0);
                metrics_row(ui, view);
                ui.add_space(14.0);
                widgets::progress_track(ui, width, view.percent() as f32);
                ui.add_space(10.0);
                meta_row(ui, view.size_text(), view.eta_text());
            }
            Stage::Installing => {
                // 安装态：spinner 与标题同行（与 Checking 的「spinner 独占一行 + 下方标题」不同）
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
                    widgets::spinner(&ui.painter_at(r), r.center(), 7.75, 2.5, theme::SUCCESS);
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(stage_title(view, default_title(stage)))
                            .size(16.0)
                            .color(theme::TEXT),
                    );
                });
                sub_line(ui, view, 8.0, 12.0, 140, false);
                ui.add_space(12.0);
                widgets::progress_track(ui, width, view.percent() as f32);
                ui.add_space(10.0);
                meta_row(
                    ui,
                    format!("{:.1}%", view.percent()),
                    "请勿关闭窗口".to_string(),
                );
            }
            Stage::ForceUpdate => {
                return draw_force_update(ui, view, store_error);
            }
            _ => end_block(ui, view, &EndStyle::ROW),
        }

        tail_block(ui, view, retrying, &TailStyle::FLEX_LEFT)
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
        match view.stage() {
            Stage::Checking => {
                // 空环（灰盘 + 内盘 spinner）
                let (r, _) = ui.allocate_exact_size(Vec2::new(96.0, 96.0), Sense::hover());
                let p = ui.painter_at(r);
                let c = r.center();
                p.circle_filled(c, 48.0, theme::W10);
                p.circle_filled(c, 39.0, theme::RING_INNER);
                widgets::spinner(&p, c, 13.0, 3.0, theme::ACCENT);
                centered_title(ui, view, 14.0, 15.0);
                sub_line(ui, view, 8.0, 12.0, 140, true);
            }
            Stage::Downloading => {
                widgets::progress_ring(
                    ui,
                    48.0,
                    view.percent() as f32,
                    theme::TEXT,
                    &view.speed_text(),
                );
                centered_title(ui, view, 14.0, 15.0);
                ui.add_space(10.0);
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(view.size_text())
                                .size(12.0)
                                .color(theme::text_alpha(140)),
                        );
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
                centered_title(ui, view, 14.0, 15.0);
                sub_line(ui, view, 8.0, 12.0, 140, true);
            }
            Stage::ForceUpdate => {
                return draw_force_update(ui, view, store_error);
            }
            _ => end_block(ui, view, &EndStyle::CENTERED),
        }

        tail_block(ui, view, retrying, &TailStyle::FIXED_CENTERED)
    })
}

// ---------------------------------------------------------------------------
// 模板 C：顶部步骤条 · 顶对齐 · 日志卡片吸底
// ---------------------------------------------------------------------------

fn template_c(
    ui: &mut Ui,
    view: &PhaseView,
    retrying: bool,
    last_step: i32,
    store_error: Option<&str>,
) -> Option<Action> {
    content(ui, (16.0, 28.0, 22.0, 28.0), |ui| {
        let width = ui.available_width();

        // 步骤条：下载 → 校验 → 解压 → 完成
        draw_steps(ui, view, width, last_step);

        match view.stage() {
            Stage::Checking => {
                ui.add_space(14.0);
                head_with_pill(ui, stage_title_by_stage(view), view.badge(), false);
                ui.add_space(14.0);
                spinner_row(ui, view.sub(), theme::ACCENT);
                if !view.sub().is_empty() && view.percent() > 0.0 {
                    ui.add_space(14.0);
                    widgets::progress_track(ui, width, view.percent() as f32);
                }
            }
            Stage::Downloading => {
                ui.add_space(14.0);
                head_with_pill(ui, stage_title_by_stage(view), view.badge(), false);
                ui.add_space(14.0);
                metrics_row(ui, view);
                ui.add_space(14.0);
                widgets::progress_track(ui, width, view.percent() as f32);
                ui.add_space(10.0);
                meta_row(ui, view.size_text(), view.eta_text());
            }
            Stage::Installing => {
                ui.add_space(14.0);
                head_with_pill(ui, stage_title_by_stage(view), view.badge(), false);
                ui.add_space(14.0);
                spinner_row(ui, view.sub(), theme::SUCCESS);
                ui.add_space(14.0);
                widgets::progress_track(ui, width, view.percent() as f32);
                ui.add_space(10.0);
                meta_row(
                    ui,
                    format!("{:.1}%", view.percent()),
                    "请勿关闭窗口".to_string(),
                );
            }
            Stage::ForceUpdate => {
                return draw_force_update(ui, view, store_error);
            }
            _ => {
                ui.add_space(14.0);
                end_block(ui, view, &EndStyle::ROW_C);
            }
        }

        tail_block(ui, view, retrying, &TailStyle::FLEX_LEFT_C)
    })
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
// 模板 D：极简留白 · 居中 · 细进度条 · 日志内联无卡片
// ---------------------------------------------------------------------------

fn template_d(
    ui: &mut Ui,
    view: &PhaseView,
    retrying: bool,
    store_error: Option<&str>,
) -> Option<Action> {
    content(ui, (34.0, 32.0, 30.0, 32.0), |ui| {
        let width = ui.available_width();

        match view.stage() {
            Stage::Checking => {
                centered_title(ui, view, 0.0, 15.0);
                ui.add_space(10.0);
                d_spinner_row(ui, view.sub(), theme::ACCENT);
                if !view.sub().is_empty() && view.percent() > 0.0 {
                    ui.add_space(10.0);
                    widgets::track(ui, width, view.percent() as f32, &widgets::TrackStyle::D);
                }
            }
            Stage::Downloading => {
                centered_title(ui, view, 0.0, 15.0);
                if let Some(b) = view.badge() {
                    ui.add_space(6.0);
                    widgets::centered_label(ui, b, 11.0, theme::text_alpha(115));
                }
                ui.add_space(16.0);
                widgets::centered_label(ui, &format!("{:.1}%", view.percent()), 20.0, theme::TEXT);
                ui.add_space(10.0);
                widgets::track(ui, width, view.percent() as f32, &widgets::TrackStyle::D);
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
                centered_title(ui, view, 0.0, 15.0);
                ui.add_space(10.0);
                d_spinner_row(ui, view.sub(), theme::SUCCESS);
                ui.add_space(16.0);
                widgets::centered_label(ui, &format!("{:.1}%", view.percent()), 20.0, theme::TEXT);
                ui.add_space(10.0);
                widgets::track(ui, width, view.percent() as f32, &widgets::TrackStyle::D);
            }
            Stage::ForceUpdate => {
                return draw_force_update(ui, view, store_error);
            }
            _ => end_block(ui, view, &EndStyle::CENTERED_D),
        }

        tail_block(ui, view, retrying, &TailStyle::INLINE_CENTERED)
    })
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
            ui.label(RichText::new(stage_title_by_stage(view)).size(14.0).color(
                if stage == Stage::Error {
                    theme::ERROR_TEXT
                } else {
                    theme::TEXT
                },
            ));
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
            widgets::track(ui, width, view.percent() as f32, &widgets::TrackStyle::E);
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
            bullet_log(ui, &log, &BulletLogStyle::E);
        }

        // 下载态的副文本已由顶部状态条表达，这里跳过（其余阶段排在日志之后）
        if stage != Stage::Downloading {
            sub_line(ui, view, 14.0, 12.0, 153, false);
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

        // E 的日志与明细区自绘，骨架只接管动作行（TailStyle::CUSTOM）
        if shows_actions(view) {
            ui.add_space(14.0);
            actions_line(ui, view, retrying, false)
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

/// 圆点日志列表的形态（E 的满高日志区 / H 的限高折叠区共用一套排布）
struct BulletLogStyle {
    /// 点所在列的宽度
    row_w: f32,
    /// 字号
    font: f32,
    /// 行高 = font * line_mul
    line_mul: f32,
    /// 圆点相对行顶的下移量
    dot_dy: f32,
    /// 圆点颜色
    dot: Color32,
    /// 正文 alpha
    text_alpha: u8,
    /// 列表最大高度（None = 占满主体）
    max_height: Option<f32>,
}

impl BulletLogStyle {
    /// E：蓝点、12.5px、占满主体
    const E: BulletLogStyle = BulletLogStyle {
        row_w: 14.0,
        font: 12.5,
        line_mul: 1.8,
        dot_dy: 10.0,
        dot: theme::ACCENT,
        text_alpha: 217,
        max_height: None,
    };
    /// H：灰点、12px、限高 64px 内滚动
    const H: BulletLogStyle = BulletLogStyle {
        row_w: 12.0,
        font: 12.0,
        line_mul: 1.7,
        dot_dy: 9.0,
        dot: theme::text_alpha(128),
        text_alpha: 204,
        max_height: Some(64.0),
    };
}

/// 圆点日志列表（滚动）
fn bullet_log(ui: &mut Ui, items: &[&str], style: &BulletLogStyle) {
    let mut area = egui::ScrollArea::vertical().auto_shrink(false);
    if let Some(max_h) = style.max_height {
        area = area.max_height(max_h);
    }
    area.show(ui, |ui| {
        for line in items {
            ui.horizontal(|ui| {
                let line_h = style.font * style.line_mul;
                let (dot_rect, _) =
                    ui.allocate_exact_size(Vec2::new(style.row_w, line_h), Sense::hover());
                ui.painter_at(dot_rect).circle_filled(
                    Pos2::new(dot_rect.left() + 3.0, dot_rect.top() + style.dot_dy),
                    2.0,
                    style.dot,
                );
                ui.label(
                    RichText::new(*line)
                        .size(style.font)
                        .color(theme::text_alpha(style.text_alpha)),
                );
            });
        }
    });
}

// ---------------------------------------------------------------------------
// 模板 F：深色工具风 · 环形进度 + 步骤条 · 间距统一 8 的倍数（默认模板）
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
            _ => end_block(ui, view, &EndStyle::CENTERED_F),
        }

        // 步骤条：环形给「当下进度」，步骤条给「流程位置」（错误态保留最后步骤）
        ui.add_space(16.0);
        draw_steps(ui, view, width, last_step);

        tail_block(ui, view, retrying, &TailStyle::FIXED_CENTERED_F)
    })
}

/// B/F 的「检测中」空环：灰盘 + 内盘 + 居中 spinner，96×96
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
        let show_actions = shows_actions(view);

        // 底部区：进度线 + 明细行（+ 动作行）；hero 吃掉其余高度
        let bottom_h = 40.0 + if show_actions { 44.0 } else { 0.0 };
        let hero_h = (ui.available_height() - bottom_h - 16.0).max(180.0);

        // 品牌面板：圆角卡片 + 居中状态主视觉（painter 直绘，避免嵌套布局）
        let (hero, _) = ui.allocate_exact_size(Vec2::new(width, hero_h), Sense::hover());
        let hp = ui.painter_at(hero);
        hp.rect_filled(hero, 10.0, theme::W04);
        hp.rect_stroke(
            hero,
            10.0,
            Stroke::new(1.0, theme::W08),
            egui::StrokeKind::Inside,
        );
        let c = hero.center();
        match stage {
            Stage::Checking => {
                widgets::spinner(&hp, Pos2::new(c.x, c.y - 26.0), 14.0, 3.0, theme::ACCENT);
                hp.text(
                    Pos2::new(c.x, c.y + 14.0),
                    egui::Align2::CENTER_CENTER,
                    stage_title_by_stage(view),
                    egui::FontId::proportional(15.0),
                    theme::TEXT,
                );
            }
            Stage::Downloading | Stage::Installing => {
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
                    stage_title_by_stage(view),
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
                    if stage == Stage::Error {
                        theme::ERROR_TEXT
                    } else {
                        theme::TEXT
                    },
                );
                if !view.sub().is_empty() {
                    hp.text(
                        Pos2::new(c.x, c.y + 22.0),
                        egui::Align2::CENTER_CENTER,
                        view.sub(),
                        egui::FontId::proportional(12.0),
                        if stage == Stage::Error {
                            theme::tint(theme::ERROR_TEXT, 191)
                        } else {
                            theme::text_alpha(140)
                        },
                    );
                }
            }
        }

        ui.add_space(16.0);
        widgets::track(ui, width, view.percent() as f32, &widgets::TrackStyle::THIN);
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
                ui.label(
                    RichText::new(right)
                        .size(11.5)
                        .color(theme::text_alpha(128)),
                );
            }
        });

        if show_actions {
            error_hint_row(ui, view, 8.0);
            ui.add_space(12.0);
            actions_line(ui, view, retrying, false)
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
            bullet_log(ui, &log, &BulletLogStyle::H);
        }

        if shows_actions(view) {
            ui.add_space(12.0);
            actions_line(ui, view, retrying, false)
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// 纯逻辑 helper 单测
//
// 模板的重构只动「差异怎么排布」，不动「文案与显示条件怎么算」。这一组测试
// 锁住抽出来的公共规则（阶段兜底文案、message 优先级、动作行/百分比的显示
// 条件、四种进度条的规格），使八套画面的文案与状态判定不会随布局改动漂移。
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
    fn four_track_styles_keep_their_spec() {
        // A/C 8px 渐变、E 6px 渐变、D 4px 纯蓝、G/H 3px 纯蓝——模板间只差这组数字
        assert_eq!(
            (
                widgets::TrackStyle::STD.height,
                widgets::TrackStyle::STD.gradient
            ),
            (8.0, true)
        );
        assert_eq!(
            (
                widgets::TrackStyle::E.height,
                widgets::TrackStyle::E.gradient
            ),
            (6.0, true)
        );
        assert_eq!(
            (
                widgets::TrackStyle::D.height,
                widgets::TrackStyle::D.gradient
            ),
            (4.0, false)
        );
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
