//! 启动器专用控件：更新日志卡 / 动作行 / 错误提示 / 强制更新动作行。
//!
//! 通用原语（背景渐变、标题栏、spinner、进度条、状态点、徽标、按钮、居中文本、
//! 环形进度）已迁到共享 crate `qsltd-ui`，这里原样转出，调用方仍写
//! `crate::widgets::xxx`，不必关心它来自哪个 crate。

pub use qsltd_ui::widgets::*;

use egui::{emath::Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

use qsltd_ui::theme;

/// 更新日志卡片（.tpl-x__log + .tpl-x__log-list）
///
/// - `body_h`: 正文区固定高度（卡片总高 = body_h + 42）。条目超出时在
///   卡片内部出竖向滚动条，卡片自身高度恒定，不会撑动整体布局
pub fn log_card(ui: &mut Ui, items: &[&str], body_h: f32) {
    let full_width = ui.available_width();
    // 固定高度：标题+内边距约 42px，正文区恒为 body_h，多行内部滚动
    let (rect, _) = ui.allocate_exact_size(Vec2::new(full_width, body_h + 42.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::W04);
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, theme::W08),
        egui::StrokeKind::Inside,
    );

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

/// 动作行按钮间距（.tpl-x__actions 的 gap）
const ACTIONS_GAP: f32 = 10.0;

/// 动作行里的一个按钮：文案 / 主按钮 / 可用态 / 点击后派发的动作
struct ActionButton {
    label: &'static str,
    primary: bool,
    enabled: bool,
    action: Action,
}

/// 常规动作行的按钮清单
///
/// - 错误态：重试 + 关闭；若失败出在下载链路（重试用尽）或 manifest
///   所有地址都失效（`show_homepage`），再加一枚「前往官网」——这两类
///   失败重试未必有解，得给条能直接拿到完整包的路
/// - 完成态：完成
fn action_buttons(
    retrying: bool,
    show_retry: bool,
    show_finish: bool,
    show_homepage: bool,
) -> Vec<ActionButton> {
    let mut buttons = Vec::new();
    if show_retry {
        buttons.push(ActionButton {
            label: if retrying {
                "正在重试..."
            } else {
                "重试"
            },
            primary: true,
            enabled: !retrying,
            action: Action::Retry,
        });
        if show_homepage {
            buttons.push(ActionButton {
                label: "前往官网重新下载",
                primary: false,
                enabled: true,
                action: Action::Homepage,
            });
        }
        buttons.push(ActionButton {
            label: "关闭",
            primary: false,
            enabled: true,
            action: Action::Close,
        });
    } else if show_finish {
        buttons.push(ActionButton {
            label: "完成",
            primary: true,
            enabled: true,
            action: Action::Finish,
        });
    }
    buttons
}

/// 强制更新动作行的按钮清单
///
/// - 有商店地址（`has_store`）：展示「前往应用商店」
/// - 始终展示「下载升级包」（primary，走更新流程）
/// - 非阻断（`blocking=false`）：额外展示「稍后升级」，允许应用照常启动
fn force_update_buttons(blocking: bool, has_store: bool) -> Vec<ActionButton> {
    let mut buttons = Vec::new();
    if has_store {
        buttons.push(ActionButton {
            label: "前往应用商店",
            primary: false,
            enabled: true,
            action: Action::Store,
        });
    }
    buttons.push(ActionButton {
        label: "下载升级包",
        primary: true,
        enabled: true,
        action: Action::ForceDownload,
    });
    if !blocking {
        buttons.push(ActionButton {
            label: "稍后升级",
            primary: false,
            enabled: true,
            action: Action::ForceLater,
        });
    }
    buttons
}

/// 量出整行按钮的占位尺寸（宽 = 各按钮宽 + 间距，高 = 最高按钮）
///
/// 居中布局要先知道宽度才能把整行放进居中的子区域；这里与 [`draw_action_row`]
/// 共用同一份按钮清单与 `button_size` 度量，不会出现「量出来 300、画出来 310」。
fn action_row_size(ui: &Ui, buttons: &[ActionButton]) -> Vec2 {
    let mut size = Vec2::ZERO;
    for (i, b) in buttons.iter().enumerate() {
        let s = button_size(ui, b.label);
        if i > 0 {
            size.x += ACTIONS_GAP;
        }
        size.x += s.x;
        size.y = size.y.max(s.y);
    }
    size
}

/// 按清单排布一行按钮（左对齐；居中由调用方包一层子区域完成）
fn draw_action_row(ui: &mut Ui, buttons: &[ActionButton]) -> Option<Action> {
    let mut act = None;
    ui.horizontal(|ui| {
        ui.add_space(0.0);
        for (i, b) in buttons.iter().enumerate() {
            if i > 0 {
                ui.add_space(ACTIONS_GAP);
            }
            if button(ui, b.label, b.primary, b.enabled) {
                act = Some(b.action);
            }
        }
    });
    act
}

/// 底部动作行（.tpl-x__actions：gap 10）
pub fn actions_row(
    ui: &mut Ui,
    retrying: bool,
    show_retry: bool,
    show_finish: bool,
    show_homepage: bool,
) -> Option<Action> {
    let buttons = action_buttons(retrying, show_retry, show_finish, show_homepage);
    draw_action_row(ui, &buttons)
}

/// 动作行占位尺寸（居中排布用）
pub fn actions_row_size(
    ui: &Ui,
    retrying: bool,
    show_retry: bool,
    show_finish: bool,
    show_homepage: bool,
) -> Vec2 {
    let buttons = action_buttons(retrying, show_retry, show_finish, show_homepage);
    action_row_size(ui, &buttons)
}

/// 错误态一句话说明（接在错误详情下方）
///
/// 说清两件事：这次失败在哪个环节试了几次、接下来该点哪个按钮。
/// 只有「下载链路的重试用尽」与「manifest 全部地址失效」才把用户往官网引——
/// 解压失败让他去官网重下，相当于让他重走一遍一模一样的坑。
pub fn error_hint(attempt_text: &str, stage_name: &str, needs_homepage: bool) -> String {
    // 环节名 + 已试次数：「下载已自动重试 2 次（共 3 次尝试）」
    let tried = if attempt_text.is_empty() {
        format!(
            "{}失败",
            if stage_name.is_empty() {
                "更新"
            } else {
                stage_name
            }
        )
    } else if stage_name.is_empty() {
        attempt_text.to_string()
    } else {
        format!("{}{}", stage_name, attempt_text)
    };
    if needs_homepage {
        // 检查失败时标题已经是「更新检查失败」，这里不再复述标题
        if stage_name == "更新检查" {
            return "所有更新地址均不可用。可点击「前往官网重新下载」获取完整安装包".to_string();
        }
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

/// 强制更新界面底部动作行
pub fn force_update_actions_row(ui: &mut Ui, blocking: bool, has_store: bool) -> Option<Action> {
    let buttons = force_update_buttons(blocking, has_store);
    draw_action_row(ui, &buttons)
}

/// 强制更新动作行占位尺寸（居中排布用）
pub fn force_update_actions_row_size(ui: &Ui, blocking: bool, has_store: bool) -> Vec2 {
    let buttons = force_update_buttons(blocking, has_store);
    action_row_size(ui, &buttons)
}

#[cfg(test)]
mod tests {
    use super::{action_buttons, error_hint, force_update_buttons, Action};

    #[test]
    fn check_failure_hint_points_to_homepage_without_repeating_title() {
        // 检查失败：标题已是「更新检查失败」，提示不再复述，直接给出路
        let hint = error_hint("", "更新检查", true);
        assert_eq!(
            hint,
            "所有更新地址均不可用。可点击「前往官网重新下载」获取完整安装包"
        );
        assert!(!hint.contains("更新检查失败"), "不应重复窗口标题: {hint}");
    }

    #[test]
    fn download_failure_hint_keeps_attempt_count() {
        let hint = error_hint("已自动重试 2 次（共 3 次尝试）", "下载", true);
        assert!(hint.starts_with("下载已自动重试 2 次（共 3 次尝试）"));
        assert!(hint.contains("前往官网重新下载"));
    }

    #[test]
    fn local_failure_hint_does_not_send_user_to_homepage() {
        // 解压/落盘失败：换地址重下会踩同一个坑，不该引导去官网
        let hint = error_hint("已尝试 1 次", "解压校验", false);
        assert!(hint.contains("请检查网络或磁盘后点「重试」"));
        assert!(!hint.contains("前往官网"));
    }

    #[test]
    fn error_actions_put_homepage_between_retry_and_close() {
        // 顺序即视觉顺序：重试（primary）→ 前往官网 → 关闭。
        // 居中是按这份清单量宽后整体平移，顺序错了画面就对不上设计稿。
        let buttons = action_buttons(false, true, false, true);
        let labels: Vec<_> = buttons.iter().map(|b| b.label).collect();
        assert_eq!(labels, ["重试", "前往官网重新下载", "关闭"]);
        assert!(buttons[0].primary, "重试应是主按钮");
        assert!(!buttons[1].primary && !buttons[2].primary);
        assert_eq!(buttons[0].action, Action::Retry);
        assert_eq!(buttons[1].action, Action::Homepage);
        assert_eq!(buttons[2].action, Action::Close);
    }

    #[test]
    fn retrying_action_is_disabled_and_says_so() {
        let buttons = action_buttons(true, true, false, false);
        assert_eq!(buttons[0].label, "正在重试...");
        assert!(!buttons[0].enabled, "重试进行中不应可再次点击");
        assert_eq!(buttons[0].action, Action::Retry);
    }

    #[test]
    fn done_actions_only_show_finish() {
        let buttons = action_buttons(false, false, true, false);
        assert_eq!(buttons.len(), 1);
        assert_eq!(buttons[0].label, "完成");
        assert_eq!(buttons[0].action, Action::Finish);
    }

    #[test]
    fn force_actions_follow_store_and_blocking_flags() {
        // 阻断 + 有商店：商店 / 下载升级包（primary），没有「稍后升级」
        let buttons = force_update_buttons(true, true);
        let labels: Vec<_> = buttons.iter().map(|b| b.label).collect();
        assert_eq!(labels, ["前往应用商店", "下载升级包"]);
        assert_eq!(buttons[0].action, Action::Store);
        assert!(buttons[1].primary, "下载升级包应是主按钮");
        assert_eq!(buttons[1].action, Action::ForceDownload);

        // 非阻断且无商店：只剩下载升级包 + 稍后升级
        let buttons = force_update_buttons(false, false);
        let labels: Vec<_> = buttons.iter().map(|b| b.label).collect();
        assert_eq!(labels, ["下载升级包", "稍后升级"]);
        assert_eq!(buttons[1].action, Action::ForceLater);
    }
}
