//! 更新器 UI：zsui State/Msg/View/Update 循环
//!
//! 状态与视图分离：后台更新线程只写共享快照 [`UiSnapshot`] 并通过
//! [`zsui::UiInvalidationHandle`] 请求重建视图，UI 线程每次重建时读取快照渲染。

use std::sync::{Arc, Mutex};

use zsui::{
    badge, button, column, progress_bar, styled_text, Dp, HorizontalAlign, ProgressRange,
    SemanticTextStyle, TextRole, TextWeight, TextWrap, UiInvalidationHandle, ViewNode,
    ZsBadgeContent, ZsBadgeTone, ColorRole,
};

/// 窗口标题
const WINDOW_TITLE: &str = "更新程序";
/// 窗口客户区尺寸（逻辑像素）
const WINDOW_SIZE: (u32, u32) = (440, 300);
/// 内容内边距
const CONTENT_PADDING: f32 = 32.0;
/// 进度条宽度（逻辑像素）
const PROGRESS_WIDTH: f32 = 368.0;

/// 更新阶段（与原 Tauri 版 7 阶段一一对应）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 正在检查更新
    Checking,
    /// 正在下载更新包
    Downloading,
    /// 正在安装更新
    Installing,
    /// 已是最新版本
    UpToDate,
    /// 发现新版本（下载启动前的短暂状态）
    NewVersion,
    /// 更新完成
    Done,
    /// 更新失败
    Error,
}

/// UI 快照：后台线程写入、视图线程读取的唯一共享状态
#[derive(Debug, Clone, Default)]
pub struct UiSnapshot {
    /// 当前阶段
    pub stage: Option<Stage>,
    /// 主提示文本（中文）
    pub message: String,
    /// 附加说明（版本变化、错误详情等）
    pub sub: String,
    /// 下载进度百分比（0-100，仅 downloading 阶段有效）
    pub percent: Option<f64>,
    /// 已下载字节数
    pub downloaded: u64,
    /// 总字节数
    pub total: Option<u64>,
    /// 下载速度（字节/秒）
    pub speed_bps: Option<u64>,
}

impl UiSnapshot {
    /// 按阶段生成初始快照
    pub fn with_phase(stage: Stage, message: &str, sub: &str, percent: Option<f64>) -> Self {
        Self {
            stage: Some(stage),
            message: message.to_string(),
            sub: sub.to_string(),
            ..Self::default()
        }
    }
}

/// 共享快照句柄（后台线程与 UI 状态共用）
pub type SharedUi = Arc<Mutex<UiSnapshot>>;

/// 后台线程 → UI 的状态写入代理
///
/// 更新快照后请求一次视图重建（多次请求由框架自动合并）
#[derive(Clone)]
pub struct UiProxy {
    snapshot: SharedUi,
    handle: UiInvalidationHandle,
}

impl UiProxy {
    /// 从共享快照与失效句柄构造代理
    pub fn new(snapshot: SharedUi, handle: UiInvalidationHandle) -> Self {
        Self { snapshot, handle }
    }

    /// 推送阶段状态（非下载阶段）
    pub fn set_phase(&self, stage: Stage, message: &str, sub: &str, percent: Option<f64>) {
        let mut s = self.lock();
        s.stage = Some(stage);
        s.message = message.to_string();
        s.sub = sub.to_string();
        s.percent = percent;
        self.handle.request_rebuild();
    }

    /// 推送下载进度（携带字节数与速度）
    pub fn set_download(
        &self,
        message: &str,
        percent: Option<f64>,
        downloaded: u64,
        total: Option<u64>,
        speed_bps: Option<u64>,
    ) {
        let mut s = self.lock();
        s.stage = Some(Stage::Downloading);
        s.message = message.to_string();
        s.percent = percent;
        s.downloaded = downloaded;
        s.total = total;
        s.speed_bps = speed_bps;
        self.handle.request_rebuild();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, UiSnapshot> {
        self.snapshot.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// UI 消息
#[derive(Debug, Clone, Copy)]
enum Msg {
    /// 关闭窗口
    Close,
}

/// UI 状态：仅持有共享快照
struct UiState {
    snapshot: SharedUi,
}

/// 运行更新器 UI 循环（阻塞直到窗口关闭）
///
/// - `snapshot`: 已含初始阶段的共享快照
/// - `handle`: 绑定窗口的失效句柄（后台线程持有克隆用于请求重建）
/// - `icon_path`: 窗口图标路径（不存在时传 None）
pub fn run(
    snapshot: SharedUi,
    handle: UiInvalidationHandle,
    icon_path: Option<String>,
) -> Result<(), String> {
    let mut builder = zsui::NativeWindowBuilder::new(WINDOW_TITLE)
        .size(WINDOW_SIZE.0, WINDOW_SIZE.1)
        .min_size(WINDOW_SIZE.0, WINDOW_SIZE.1)
        .resizable(false)
        .always_on_top(true)
        .invalidation_handle(handle);

    if let Some(icon) = icon_path {
        builder = builder.icon_path(icon);
    }

    builder
        .stateful_view(UiState { snapshot }, view, update)
        .run()
        .map(|_| ())
        .map_err(|e| format!("UI 循环异常退出: {}", e))
}

/// 消息处理：关闭按钮 → 退出事件循环
fn update(_state: &mut UiState, message: Msg, cx: &mut zsui::AppCx) {
    match message {
        Msg::Close => cx.quit(),
    }
}

/// 视图：居中布局 = 状态徽标 + 主文本 + 副文本 +（进度条 / 关闭按钮）
fn view(state: &UiState) -> ViewNode<Msg> {
    let snapshot = state.snapshot.lock().unwrap_or_else(|e| e.into_inner());
    let stage = snapshot.stage.unwrap_or(Stage::Checking);

    let mut children: Vec<ViewNode<Msg>> = Vec::with_capacity(5);

    // 状态徽标（原版彩色圆点的语义对应物）
    let tone = match stage {
        Stage::Error => ZsBadgeTone::Danger,
        Stage::UpToDate | Stage::Done => ZsBadgeTone::Success,
        Stage::NewVersion => ZsBadgeTone::Warning,
        _ => ZsBadgeTone::Accent,
    };
    children.push(badge(ZsBadgeContent::Dot).badge_tone(tone));

    // 主提示文本
    children.push(styled_text(
        snapshot.message.clone(),
        SemanticTextStyle {
            role: TextRole::Subtitle,
            weight: TextWeight::Semibold,
            horizontal_align: HorizontalAlign::Center,
            ..SemanticTextStyle::body()
        },
    ));

    // 副说明文本（错误详情、版本变化；下载阶段展示数值明细行）
    let sub = if stage == Stage::Downloading {
        download_detail(&snapshot)
    } else {
        snapshot.sub.clone()
    };
    if !sub.is_empty() {
        children.push(styled_text(
            sub,
            SemanticTextStyle {
                role: TextRole::Caption,
                color: ColorRole::SecondaryText,
                horizontal_align: HorizontalAlign::Center,
                wrap: TextWrap::Word,
                ellipsis: false,
                ..SemanticTextStyle::body()
            },
        ));
    }

    // 下载进度条
    if stage == Stage::Downloading {
        let value = snapshot.percent.unwrap_or(0.0).clamp(0.0, 100.0) as f32;
        children.push(
            progress_bar(value, ProgressRange::new(0.0, 100.0)).width(Dp::new(PROGRESS_WIDTH)),
        );
    }

    // 结束态提供关闭按钮
    if matches!(stage, Stage::Done | Stage::Error | Stage::UpToDate) {
        children.push(button("关闭").on_click(Msg::Close));
    }

    column(children)
        .padding(Dp::new(CONTENT_PADDING))
        .gap(Dp::new(14.0))
        .width(Dp::new(WINDOW_SIZE.0 as f32))
        .height(Dp::new(WINDOW_SIZE.1 as f32))
}

/// 下载数值明细行：`45.23% · 12.30 MB / 45.60 MB · 2.50 MB/s`
fn download_detail(s: &UiSnapshot) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(3);
    if let Some(p) = s.percent {
        parts.push(format!("{:.2}%", p));
    }
    parts.push(match s.total {
        Some(total) => format!("{} / {}", format_bytes(s.downloaded), format_bytes(total)),
        None => format_bytes(s.downloaded),
    });
    if let Some(speed) = s.speed_bps {
        parts.push(format!("{}/s", format_bytes(speed)));
    }
    parts.join(" · ")
}

/// 字节数格式化为易读文本（与登录器同款格式）
pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let i = (bytes as f64).log(1024.0).floor() as usize;
    let i = i.min(UNITS.len() - 1);
    let value = bytes as f64 / 1024f64.powi(i as i32);
    format!("{:.2} {}", value, UNITS[i])
}
