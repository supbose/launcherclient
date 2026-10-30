//! 启动器专用控件：更新日志卡 / 内联日志 / 动作行 / 错误提示 / 强制更新动作行。
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
/// - D 模板走 [`inline_log`]（无卡片容器，同样固定高度内部滚动）
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
                    ui.painter_at(r).rect_filled(r, 0.0, theme::W08);
                }
                ui.label(
                    RichText::new(*line)
                        .size(13.0)
                        .color(theme::text_alpha(204)), // 0.8
                );
            }
        });
}

/// 底部动作行（.tpl-x__actions：gap 10）
///
/// - 错误态：重试 + 关闭；若失败出在下载链路（重试用尽）或 manifest
///   所有地址都失效（`show_homepage`），再加一枚「前往官网」——这两类
///   失败重试未必有解，得给条能直接拿到完整包的路
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
            if button(
                ui,
                if retrying {
                    "正在重试..."
                } else {
                    "重试"
                },
                true,
                !retrying,
            ) {
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
///
/// - 有商店地址（`has_store`）：展示「前往应用商店」
/// - 始终展示「下载升级包」（primary，走更新流程）
/// - 非阻断（`blocking=false`）：额外展示「稍后升级」，允许应用照常启动
pub fn force_update_actions_row(ui: &mut Ui, blocking: bool, has_store: bool) -> Option<Action> {
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

#[cfg(test)]
mod tests {
    use super::error_hint;

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
}
