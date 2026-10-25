//! eframe 应用壳：标题栏 + 模板渲染 + 关闭确认模态 + 阶段快照轮询
//!
//! 对应 Tauri 版的 `src/App.vue`（阶段状态归一维护）与
//! `src/components/MinTitleBar.vue`（标题栏）：
//! - 后台更新线程写 [`PhaseHandle`] 共享快照，UI 每帧轮询应用
//!   （对应 Vue 版监听 `update-phase` 事件 + `applyPhase` 归一化）
//! - 关键阶段（检测/下载/安装中）关闭前弹确认（对应 lib.rs
//!   on_window_event 的 CloseRequested 拦截对话框）

use egui::{Color32, FontId, RichText, Sense, Ui, Vec2};
use qsltd_updater_core::flow;
use qsltd_updater_core::phase::{resolve_template_key, PhaseHandle, PhasePayload, PhaseView, Stage};

use crate::preview;
use crate::templates;
use crate::theme;
use crate::widgets;

/// 窗口标题
pub const WINDOW_TITLE: &str = "更新程序";
/// 窗口客户区尺寸（tauri.conf.json main: 440×420）
pub const WINDOW_SIZE: (f32, f32) = (440.0, 420.0);
/// 预览模式下窗口高度（底部模板/阶段切换条）
pub const PREVIEW_EXTRA_HEIGHT: f32 = 150.0;

/// 更新器应用状态
pub struct UpdaterApp {
    /// 共享阶段快照（后台线程写 / UI 读）
    phase: PhaseHandle,
    /// 当前帧渲染用的阶段载荷
    payload: Option<PhasePayload>,
    /// 是否已进入过下载阶段（步骤条「校验」判定依据）
    has_downloaded: bool,
    /// 错误态保留的最后到达步骤
    last_step: i32,
    /// 生效模板（非预览模式跟随载荷更新）
    template: String,
    /// 预览模式（复刻 preview.html）
    pub preview: bool,
    preview_state: preview::PreviewState,
    /// 重试请求是否已被受理（错误态按钮禁用态）
    retrying: bool,
    /// 关闭确认模态
    show_close_confirm: bool,
    close_confirmed: bool,
    /// 「完成」已触发（防重入，对应 Tauri FINISH_CALLED 守卫）
    finish_triggered: bool,
    /// 首帧居中是否完成（对齐 tauri.conf.json 的 "center": true）
    centered: bool,
    /// 居中后窗口是否已显示（配合隐藏启动，避免默认位置闪现再跳中心）
    shown: bool,
    /// 已渲染帧数（居中兜底计时用）
    frames: u32,
    /// 预览模式最近事件提示
    preview_event: String,
}

impl UpdaterApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        phase: PhaseHandle,
        template: String,
        preview: bool,
        preview_stage: usize,
    ) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::zero_item_spacing(&cc.egui_ctx);
        Self {
            phase,
            payload: None,
            has_downloaded: false,
            last_step: -1,
            template: template.clone(),
            preview,
            preview_state: preview::PreviewState::with_stage(&template.clone(), preview_stage),
            retrying: false,
            show_close_confirm: false,
            close_confirmed: false,
            finish_triggered: false,
            centered: false,
            shown: false,
            frames: 0,
            preview_event: String::new(),
        }
    }

    /// 应用一次阶段状态（对应 App.vue applyPhase 归一化逻辑）
    fn apply_phase(&mut self, p: PhasePayload) {
        let stage = p.stage();
        if stage == Stage::Downloading {
            self.has_downloaded = true;
        }

        // 步骤条下标：错误态（-2）保留最后到达的步骤
        let view = PhaseView::new(&p, self.has_downloaded);
        let step = view.step();
        if step != -2 {
            self.last_step = step;
        }

        // 界面模板：跟随载荷（预览模式由面板控制）
        if !self.preview && !p.template.is_empty() {
            self.template = resolve_template_key(&p.template);
        }

        // 收到任何非 error 阶段即视为重试已被受理，复位按钮禁用态
        if stage != Stage::Error {
            self.retrying = false;
        }

        self.payload = Some(p);
    }

    /// 「完成」：启动更新后的应用，成功后退出（后台线程执行）
    fn finish_update(&mut self) {
        if self.finish_triggered {
            return;
        }
        self.finish_triggered = true;
        let phase = self.phase.clone();
        let _ = std::thread::Builder::new()
            .name("finish-launch".into())
            .spawn(move || {
                let result = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt.block_on(async { flow::finish_and_launch(&phase).await }),
                    Err(e) => Err(format!("创建异步运行时失败: {}", e)),
                };
                // 成功时 finish_and_launch 已置位 PhaseHandle 退出信号，
                // logic() 轮询到后关窗；失败时错误阶段已写回快照，UI 切错误态
                if let Err(e) = result {
                    eprintln!("[更新器] 「完成」启动失败: {}", e);
                }
            });
    }
}

impl eframe::App for UpdaterApp {
    /// 每帧逻辑（渲染前）：轮询快照 + 关闭请求拦截 + 首帧居中
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 轮询后台线程写入的阶段快照（对应 Vue 版事件监听 + current_phase 快照兜底）
        if !self.preview {
            if let Some(p) = self.phase.snapshot() {
                self.apply_phase(p);
            }
        }

        // 流程内自动拉起目标程序成功（或「完成」启动成功）→ 关闭更新器窗口。
        // 对齐 Tauri 版更新完成后 app.exit(0) 的行为：窗口不停在完成页挡在应用前面
        if !self.preview && self.phase.exit_requested() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // 首帧居中（对齐 tauri.conf.json main 的 "center": true）：
        // egui 无 with_center，等 monitor_size / outer_rect 就绪后按屏幕中心
        // 发 OuterPosition；窗口以隐藏方式启动，居中后再显示，避免
        // 「先在默认级联位置闪现、再跳到中心」的观感
        self.frames += 1;
        if !self.centered {
            let (monitor, outer) = ctx.input(|i| {
                let vp = i.viewport();
                (vp.monitor_size, vp.outer_rect)
            });
            if let (Some(mon), Some(outer)) = (monitor, outer) {
                let pos = egui::Pos2::new(
                    (mon.x - outer.width()) / 2.0,
                    (mon.y - outer.height()) / 2.0,
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                self.centered = true;
            }
            // 兜底：个别环境拿不到显示器信息时也要把窗口显示出来，
            // 不能让窗口永远隐藏（按 33ms/帧约 1s 后放弃居中）
            if self.frames > 30 {
                self.centered = true;
            }
            if self.centered && !self.shown {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                self.shown = true;
            }
        }

        // 关键阶段关闭拦截（对应 lib.rs on_window_event 的 CloseRequested 确认）：
        // 用户确认放行前取消关闭请求并弹确认模态
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && self.phase.is_critical() && !self.close_confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show_close_confirm = true;
        }

        // 重绘节奏：动画/下载中高频刷新；静止态低频轮询快照
        let animating = matches!(
            self.payload.as_ref().map(|p| p.stage()),
            Some(Stage::Checking) | Some(Stage::Downloading) | Some(Stage::Installing) | None
        );
        ctx.request_repaint_after(std::time::Duration::from_millis(if animating { 33 } else { 150 }));
    }

    /// 每帧渲染：根 ui 无边距无背景，全窗口自绘
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let full = ui.max_rect();
        widgets::paint_bg(ui.painter(), full);

        // 标题栏
        let tb = widgets::draw_titlebar(ui);
        match tb {
            widgets::TitleBarAction::Minimize => {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            widgets::TitleBarAction::Close => {
                // 与系统关闭同路径：send Close → logic 拦截关键阶段
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            widgets::TitleBarAction::None => {}
        }

        // 模板内容区（真实模式：占满剩余高度；
        // 预览模式：固定 420-32 高度，底部留切换条）
        let content_h = if self.preview {
            WINDOW_SIZE.1 - theme::TITLEBAR_HEIGHT
        } else {
            ui.available_height()
        };
        let (area, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), content_h),
            Sense::hover(),
        );

        // 预览模式：阶段载荷来自面板预设；真实模式：来自共享快照
        let (payload, has_downloaded, template) = if self.preview {
            (
                preview::payload(&self.preview_state),
                self.preview_state.has_downloaded(),
                self.preview_state.template.clone(),
            )
        } else {
            match &self.payload {
                Some(p) => (p.clone(), self.has_downloaded, self.template.clone()),
                None => (
                    PhasePayload::new(Stage::Checking, "正在检查更新...", "", None),
                    false,
                    self.template.clone(),
                ),
            }
        };

        let view = PhaseView::new(&payload, has_downloaded);
        let action = {
            let mut child = ui.new_child(
                egui::UiBuilder::new().max_rect(area),
            );
            templates::draw(&mut child, &view, &template, self.retrying, self.last_step)
        };

        match action {
            Some(widgets::Action::Retry) => {
                self.retrying = true;
                flow::retry(self.phase.clone());
            }
            Some(widgets::Action::Close) => {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(widgets::Action::Finish) => self.finish_update(),
            None => {}
        }

        // 预览模式：底部切换条
        if self.preview {
            preview::draw_strip(ui, &mut self.preview_state, &mut self.preview_event);
        }

        // 关闭确认模态（关键阶段）：遮罩吞掉点击 + 居中对话框
        if self.show_close_confirm {
            draw_close_confirm(ui, &mut self.show_close_confirm, &mut self.close_confirmed);
        }
    }
}

/// 关闭确认模态（对应 lib.rs CloseRequested 确认对话框）
///
/// 全窗口遮罩吞掉点击，居中卡片：标题 + 说明 + 取消/确定。
fn draw_close_confirm(ui: &mut Ui, show: &mut bool, confirmed: &mut bool) {
    let full = ui.max_rect();
    // 遮罩：吞掉下层点击 + 半透明变暗
    let (rect, resp) = ui.allocate_exact_size(full.size(), Sense::click());
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, Color32::from_black_alpha(128));
    resp.request_focus();

    // 居中卡片
    let card = egui::Rect::from_center_size(full.center(), Vec2::new(330.0, 148.0));
    let painter = ui.painter();
    painter.rect_filled(card, 10.0, theme::BG_TOP);
    painter.rect_stroke(card, 10.0, egui::Stroke::new(1.0, theme::W18), egui::StrokeKind::Inside);

    painter.text(
        card_label_pos(card, 24.0),
        egui::Align2::LEFT_TOP,
        "中断更新？",
        FontId::proportional(15.0),
        theme::TEXT,
    );
    painter.text(
        card_label_pos(card, 48.0),
        egui::Align2::LEFT_TOP,
        "更新正在进行中。中断可能导致安装不",
        FontId::proportional(12.0),
        theme::text_alpha(166),
    );
    painter.text(
        card_label_pos(card, 66.0),
        egui::Align2::LEFT_TOP,
        "完整或版本记录不一致，确定要退出吗？",
        FontId::proportional(12.0),
        theme::text_alpha(166),
    );

    // 按钮行（右下角）
    let cancel = egui::Rect::from_min_size(
        egui::pos2(card.right() - 180.0, card.bottom() - 44.0),
        Vec2::new(76.0, 30.0),
    );
    let ok = egui::Rect::from_min_size(
        egui::pos2(card.right() - 94.0, card.bottom() - 44.0),
        Vec2::new(76.0, 30.0),
    );
    let click_cancel = modal_button(ui, cancel, "取消", false);
    let click_ok = modal_button(ui, ok, "确定", true);
    if click_ok {
        *confirmed = true;
        *show = false;
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }
    if click_cancel {
        *show = false;
    }
}

fn card_label_pos(card: egui::Rect, dy: f32) -> egui::Pos2 {
    egui::pos2(card.left() + 20.0, card.top() + dy)
}

/// 模态内按钮（简版自绘，交互逻辑同 widgets::button）
fn modal_button(ui: &mut Ui, rect: egui::Rect, label: &str, primary: bool) -> bool {
    let resp = ui.interact(rect, ui.id().with(label), Sense::click());
    let painter = ui.painter_at(rect);
    if primary {
        painter.rect_filled(rect, 6.0, theme::ACCENT);
    } else {
        if resp.hovered() {
            painter.rect_filled(rect, 6.0, theme::W10_HOVER);
        }
        painter.rect_stroke(rect, 6.0, egui::Stroke::new(1.0, theme::W20), egui::StrokeKind::Inside);
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(13.0),
        if primary { Color32::WHITE } else { theme::TEXT },
    );
    let clicked = resp.clicked();
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    clicked
}
