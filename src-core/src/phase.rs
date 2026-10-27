//! 阶段数据与派生视图模型（UI 无关）
//!
//! 由 `src-tauri/src/updater.rs`（Tauri 版，已于 2026-10-26 删除）的 `PhasePayload` 与前端
//! `src/components/templates/usePhase.js`（Vue 版，随 Tauri 壳一并删除）的派生逻辑合并抽离而来：
//! - `PhasePayload`：阶段事件载荷（字段名与 Tauri 版 serde 输出一一对应，
//!   结构保持稳定，便于其它推送式 UI 复用）
//! - [`PhaseHandle`]：后台线程 → UI 的共享快照句柄（egui 轮询式 UI 用）
//! - [`PhaseView`]：载荷 → 展示文本/状态的纯换算（不含布局与配色，
//!   与 usePhase.js 逐项对应）
//!
//! 业务布局与配色由各 UI 实现自己持有（Vue 模板 / egui 模板），互不覆盖。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;

/// 更新阶段（与 `PhasePayload.stage` 字符串一一对应）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 正在检查更新 / 校验中 / 合并分卷中
    Checking,
    /// 正在下载更新包
    Downloading,
    /// 正在安装（解压）更新
    Installing,
    /// 已是最新版本
    UpToDate,
    /// 发现新版本（下载启动前的短暂状态）
    Available,
    /// 更新完成
    Done,
    /// 更新失败
    Error,
}

impl Stage {
    /// 阶段字符串（与 Tauri 版 PhasePayload.stage 一致）
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Checking => "checking",
            Stage::Downloading => "downloading",
            Stage::Installing => "installing",
            Stage::UpToDate => "up-to-date",
            Stage::Available => "available",
            Stage::Done => "done",
            Stage::Error => "error",
        }
    }

    /// 从阶段字符串解析（未知值回落 Checking，与前端 `p.stage || 'checking'` 一致）
    pub fn from_stage_str(s: &str) -> Self {
        match s {
            "downloading" => Stage::Downloading,
            "installing" => Stage::Installing,
            "up-to-date" => Stage::UpToDate,
            "available" => Stage::Available,
            "done" => Stage::Done,
            "error" => Stage::Error,
            _ => Stage::Checking,
        }
    }
}

/// 推送给 UI 的阶段事件载荷（字段名与 Tauri 版 serde 输出一致）
#[derive(Debug, Clone, Serialize)]
pub struct PhasePayload {
    /// 阶段：checking | downloading | installing | up-to-date | available | done | error
    pub stage: String,
    /// 主提示文本（中文）
    pub message: String,
    /// 附加说明（版本变化、错误详情等）
    pub sub: String,
    /// 进度百分比（仅下载/安装/合并阶段有效）
    pub percent: Option<f64>,
    /// 已下载字节数（仅下载阶段携带）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloaded_bytes: Option<u64>,
    /// 总字节数（仅下载阶段携带，服务器未返回时缺失）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    /// 下载速度 字节/秒（仅下载阶段携带）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_bps: Option<u64>,
    /// 更新日志（manifest.changelog，空则不序列化）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changelog: Vec<String>,
    /// 生效的启动器界面模板（A/B/C/D/E），UI 据此渲染对应布局
    #[serde(default)]
    pub template: String,
    /// 右上角小徽标文本（分卷进度等，仅下载阶段携带）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// 本次失败是否出在下载/合并环节（错误态据此提供「前往官网」入口）。
    /// 下载地址全挂时再点「重试」也是徒劳，得让用户有条直接拿到安装包的路。
    #[serde(default)]
    pub download_failed: bool,
}

impl PhasePayload {
    /// 构造基础阶段载荷（无下载详情/日志/徽标）
    pub fn new(stage: Stage, message: &str, sub: &str, percent: Option<f64>) -> Self {
        Self {
            stage: stage.as_str().to_string(),
            message: message.to_string(),
            sub: sub.to_string(),
            percent,
            downloaded_bytes: None,
            total_bytes: None,
            speed_bps: None,
            changelog: changelog_snapshot(),
            template: String::new(),
            badge: None,
            download_failed: false,
        }
    }

    /// 构造错误阶段载荷，并标记本次失败是否出在下载环节
    pub fn failure(
        message: &str,
        sub: &str,
        download_failed: bool,
    ) -> Self {
        let mut p = Self::new(Stage::Error, message, sub, None);
        p.download_failed = download_failed;
        p
    }

    /// 构造下载阶段载荷（携带字节数与速度）
    #[allow(clippy::too_many_arguments)]
    pub fn download(
        message: &str,
        badge: Option<String>,
        percent: Option<f64>,
        downloaded: u64,
        total: Option<u64>,
        speed_bps: Option<u64>,
    ) -> Self {
        Self {
            stage: Stage::Downloading.as_str().to_string(),
            message: message.to_string(),
            sub: String::new(),
            percent,
            downloaded_bytes: Some(downloaded),
            total_bytes: total,
            speed_bps,
            changelog: changelog_snapshot(),
            template: String::new(),
            badge,
            download_failed: false,
        }
    }

    /// 阶段枚举（未知值回落 Checking）
    pub fn stage(&self) -> Stage {
        Stage::from_stage_str(&self.stage)
    }
}

/// 进程级更新日志副本（对齐 Tauri 版 `set_changelog` / `changelog_snapshot`）
///
/// manifest 拉取成功后登记一次，此后 [`PhasePayload::new`] / [`PhasePayload::download`]
/// 构造的每个阶段载荷都自动携带它 —— 界面在「发现新版本 → 更新完成」全程可见
/// 更新日志（下载/安装期间用户正好有空看更新了什么）。
///
/// manifest 拉取之前产生的阶段（检测中）日志为空，界面据此不展示该区块。
static CHANGELOG: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn changelog_cell() -> &'static Mutex<Vec<String>> {
    CHANGELOG.get_or_init(|| Mutex::new(Vec::new()))
}

/// 登记本次更新的更新日志（manifest.changelog）
pub fn set_changelog(items: Vec<String>) {
    let mut guard = changelog_cell()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = items;
}

/// 读取当前生效的更新日志副本
pub fn changelog_snapshot() -> Vec<String> {
    changelog_cell()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// 共享阶段快照句柄（后台更新线程写、UI 线程读）
///
/// egui 等轮询式 UI 每帧调用 [`PhaseHandle::snapshot`]；未来若有其它推送式 UI 接入时
/// 可在 `set` 处替换为事件 emit（载荷结构不变）。
#[derive(Clone, Default)]
pub struct PhaseHandle {
    cell: Arc<Mutex<Option<PhasePayload>>>,
    /// 「启动目标程序成功，UI 可退出」信号（后台线程置位、UI 帧循环消费）。
    /// 对齐 Tauri 版更新完成后 `app.exit(0)` 的行为：更新器窗口不该停在
    /// 「更新完成」界面挡在应用前面。
    exit: Arc<AtomicBool>,
}

impl PhaseHandle {
    /// 新建空句柄
    pub fn new() -> Self {
        Self::default()
    }

    /// 置位退出信号：目标程序启动成功后由流程调用，UI 轮询到后自行关窗
    pub fn request_exit(&self) {
        self.exit.store(true, Ordering::SeqCst);
    }

    /// 退出信号是否已置位（UI 每帧轮询）
    pub fn exit_requested(&self) -> bool {
        self.exit.load(Ordering::SeqCst)
    }

    /// 写入阶段快照（覆盖式，最后一条即当前状态）
    pub fn set(&self, payload: PhasePayload) {
        let mut guard = self
            .cell
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(payload);
    }

    /// 读取阶段快照副本（无任何阶段时返回 None）
    pub fn snapshot(&self) -> Option<PhasePayload> {
        self.cell
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 当前是否处于「不可中断」的关键阶段（检测中 / 下载中 / 安装中）
    ///
    /// 与 Tauri 版 `is_critical_phase` 语义一致：这些阶段关闭窗口前
    /// 必须弹确认框，避免留下不一致态或浪费已发起的检测。
    pub fn is_critical(&self) -> bool {
        self.snapshot()
            .map(|p| {
                matches!(
                    p.stage(),
                    Stage::Checking | Stage::Downloading | Stage::Installing
                )
            })
            .unwrap_or(false)
    }
}

/// 步骤条节点文案（C 模板使用）：下载 → 校验 → 解压 → 完成
pub const STEP_LABELS: [&str; 4] = ["下载", "校验", "解压", "完成"];

/// 阶段载荷的派生展示值（与前端 usePhase.js 逐项对应）
///
/// 一次构造、多次取用；只做「数据 → 展示文本/状态」换算，不含布局配色。
pub struct PhaseView<'a> {
    /// 阶段载荷
    pub payload: &'a PhasePayload,
    /// 是否已进入过下载阶段（「检测中」与「校验中」同为 checking，
    /// 只有下载之后的 checking 才是步骤条上的「校验」）
    pub has_downloaded: bool,
}

impl<'a> PhaseView<'a> {
    /// 从载荷构造视图
    pub fn new(payload: &'a PhasePayload, has_downloaded: bool) -> Self {
        Self {
            payload,
            has_downloaded,
        }
    }

    /// 阶段枚举
    pub fn stage(&self) -> Stage {
        self.payload.stage()
    }

    /// 主提示文本
    pub fn message(&self) -> &str {
        &self.payload.message
    }

    /// 附加说明
    pub fn sub(&self) -> &str {
        &self.payload.sub
    }

    /// 右上角徽标
    pub fn badge(&self) -> Option<&str> {
        self.payload.badge.as_deref()
    }

    /// 进度百分比（clamp 到 0-100；非法值回落 0）
    pub fn percent(&self) -> f64 {
        self.payload.percent.unwrap_or(0.0).clamp(0.0, 100.0)
    }

    /// "12.30 MB / 45.60 MB"（总大小未知时只显示已下载）
    pub fn size_text(&self) -> String {
        let done = self.payload.downloaded_bytes.unwrap_or(0);
        let total = self.payload.total_bytes.unwrap_or(0);
        if done == 0 {
            return String::new();
        }
        if total > 0 {
            format!("{} / {}", format_bytes(done), format_bytes(total))
        } else {
            format_bytes(done)
        }
    }

    /// "2.50 MB/s"
    pub fn speed_text(&self) -> String {
        match self.payload.speed_bps {
            Some(bps) if bps > 0 => format!("{}/s", format_bytes(bps)),
            _ => String::new(),
        }
    }

    /// 剩余时间（按当前速度估算；速度未知或已下完时不显示）
    pub fn eta_text(&self) -> String {
        let done = self.payload.downloaded_bytes.unwrap_or(0);
        let total = self.payload.total_bytes.unwrap_or(0);
        let bps = self.payload.speed_bps.unwrap_or(0);
        let remain = total.saturating_sub(done);
        if bps == 0 || remain == 0 {
            return String::new();
        }
        let secs = (remain / bps).max(1);
        if secs < 60 {
            return format!("剩余约 {} 秒", secs);
        }
        let mins = (secs / 60).max(1);
        if mins < 60 {
            return format!("剩余约 {} 分钟", mins);
        }
        format!("剩余约 {} 小时 {} 分", mins / 60, mins % 60)
    }

    /// 更新日志（过滤空行，避免渲染空条目）
    pub fn changelog(&self) -> Vec<&str> {
        self.payload
            .changelog
            .iter()
            .map(|s| s.as_str())
            .filter(|s| !s.trim().is_empty())
            .collect()
    }

    /// 错误是否出在下载环节（错误态显示「前往官网」按钮的依据）
    ///
    /// 只认下载环节的失败：本地磁盘写不进、解压失败这类问题换个地址
    /// 也解决不了，不该把用户往官网引。
    pub fn download_failed(&self) -> bool {
        self.stage() == Stage::Error && self.payload.download_failed
    }

    /// 是否显示进度条：下载/安装/合并（checking 带副文本即合并分卷）
    pub fn show_progress(&self) -> bool {
        match self.stage() {
            Stage::Downloading | Stage::Installing => true,
            Stage::Checking => !self.sub().is_empty(),
            _ => false,
        }
    }

    /// 步骤条下标（-1 表示流程尚未开始；错误态保留最后到达的步骤）
    ///
    /// available/downloading → 0；下载后的 checking → 1；installing → 2；
    /// done/up-to-date → 3
    pub fn step(&self) -> i32 {
        match self.stage() {
            Stage::Available | Stage::Downloading => 0,
            Stage::Checking => {
                if self.has_downloaded {
                    1
                } else {
                    -1
                }
            }
            Stage::Installing => 2,
            Stage::Done | Stage::UpToDate => 3,
            Stage::Error => -2, // -2 = 保持不变（由 UI 端用上次步骤覆盖）
        }
    }
}

/// 字节数格式化为易读文本（与登录器 QsLtdLauncherClient 同款格式）
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let i = (bytes as f64).log(1024.0).floor() as usize;
    let i = i.min(UNITS.len() - 1);
    let value = bytes as f64 / 1024f64.powi(i as i32);
    format!("{:.2} {}", value, UNITS[i])
}

/// 字节数格式化为 MB 显示（保留 1 位小数，分卷合并进度用）
pub fn fmt_mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
}

/// 归一化模板键：去空白 + 大写，非法值回落默认模板（与前端 resolveTemplateKey 一致）
pub fn resolve_template_key(value: &str) -> String {
    const KEYS: [&str; 5] = ["A", "B", "C", "D", "E"];
    let normalized = value.trim().to_uppercase();
    if KEYS.contains(&normalized.as_str()) {
        normalized
    } else {
        crate::defaults::DEFAULT_LAUNCHER_TEMPLATE.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_marks_download_only_when_asked() {
        // 下载环节失败：错误态下界面给「前往官网」
        let p = PhasePayload::failure("更新包下载失败", "下载失败: 连接超时", true);
        assert_eq!(p.stage(), Stage::Error);
        assert!(PhaseView::new(&p, true).download_failed());

        // 非下载环节（解压、磁盘写失败）：不该把用户往官网引
        let q = PhasePayload::failure("更新安装失败", "磁盘空间不足", false);
        assert!(!PhaseView::new(&q, true).download_failed());
    }

    #[test]
    fn download_failed_false_outside_error_stage() {
        // 阶段不是 error 时即便标记为 true 也不显示（避免正常态冒出按钮）
        let mut p = PhasePayload::new(Stage::Done, "更新完成", "", None);
        p.download_failed = true;
        assert!(!PhaseView::new(&p, true).download_failed());
    }

    #[test]
    fn stage_roundtrip() {
        for s in [
            Stage::Checking,
            Stage::Downloading,
            Stage::Installing,
            Stage::UpToDate,
            Stage::Available,
            Stage::Done,
            Stage::Error,
        ] {
            assert_eq!(Stage::from_stage_str(s.as_str()), s);
        }
        // 未知值回落 Checking（与前端 `p.stage || 'checking'` 一致）
        assert_eq!(Stage::from_stage_str("bogus"), Stage::Checking);
    }

    #[test]
    fn size_text_known_total() {
        let mut p = PhasePayload::new(Stage::Downloading, "下载", "", Some(50.0));
        p.downloaded_bytes = Some(4_896_378); // 4.67 MB
        p.total_bytes = Some(145_122_643); // 138.39 MB
        let v = PhaseView::new(&p, true);
        assert_eq!(v.size_text(), "4.67 MB / 138.40 MB");
        assert_eq!(v.speed_text(), "");
    }

    #[test]
    fn size_text_unknown_total_shows_done_only() {
        let mut p = PhasePayload::new(Stage::Downloading, "下载", "", None);
        p.downloaded_bytes = Some(1024 * 1024);
        let v = PhaseView::new(&p, true);
        assert_eq!(v.size_text(), "1.00 MB");
    }

    #[test]
    fn eta_text_units() {
        let mut p = PhasePayload::new(Stage::Downloading, "下载", "", None);
        p.downloaded_bytes = Some(0);
        p.total_bytes = Some(600 * 1024);
        p.speed_bps = Some(1024); // 600s = 10 分钟
        let v = PhaseView::new(&p, true);
        assert_eq!(v.eta_text(), "剩余约 10 分钟");

        p.total_bytes = Some(30 * 1024);
        let v = PhaseView::new(&p, true);
        assert_eq!(v.eta_text(), "剩余约 30 秒");

        p.total_bytes = Some(0);
        let v = PhaseView::new(&p, true);
        assert_eq!(v.eta_text(), "");
    }

    #[test]
    fn percent_clamped() {
        let p = PhasePayload::new(Stage::Downloading, "下载", "", Some(120.0));
        let v = PhaseView::new(&p, true);
        assert!((v.percent() - 100.0).abs() < f64::EPSILON);
        let p = PhasePayload::new(Stage::Downloading, "下载", "", Some(-5.0));
        let v = PhaseView::new(&p, true);
        assert!(v.percent().abs() < f64::EPSILON);
    }

    #[test]
    fn step_of_stage_tracks_download() {
        // 下载前的 checking：流程尚未开始
        let p = PhasePayload::new(Stage::Checking, "", "", None);
        assert_eq!(PhaseView::new(&p, false).step(), -1);
        // 下载后的 checking：步骤条「校验」
        assert_eq!(PhaseView::new(&p, true).step(), 1);

        let p = PhasePayload::new(Stage::Downloading, "", "", None);
        assert_eq!(PhaseView::new(&p, true).step(), 0);
        let p = PhasePayload::new(Stage::Installing, "", "", None);
        assert_eq!(PhaseView::new(&p, true).step(), 2);
        let p = PhasePayload::new(Stage::Done, "", "", None);
        assert_eq!(PhaseView::new(&p, true).step(), 3);
        // 错误态用 -2 标记「保持不变」，由 UI 端保留最后到达的步骤
        let p = PhasePayload::new(Stage::Error, "", "", None);
        assert_eq!(PhaseView::new(&p, true).step(), -2);
    }

    #[test]
    fn format_bytes_matches_launcher_format() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(112 * 1024), "112.00 KB");
        assert_eq!(format_bytes(4_896_378), "4.67 MB");
    }

    #[test]
    fn template_key_normalized_with_fallback() {
        assert_eq!(resolve_template_key(" b "), "B");
        assert_eq!(resolve_template_key("e"), "E");
        // 非法值回落默认模板（与后端 DEFAULT_LAUNCHER_TEMPLATE 一致）
        assert_eq!(resolve_template_key("x"), "C");
        assert_eq!(resolve_template_key(""), "C");
    }

    #[test]
    fn handle_critical_phase() {
        let h = PhaseHandle::new();
        assert!(!h.is_critical(), "无阶段快照时不算关键阶段");
        h.set(PhasePayload::new(Stage::Downloading, "", "", None));
        assert!(h.is_critical());
        h.set(PhasePayload::new(Stage::Done, "", "", None));
        assert!(!h.is_critical());
    }

    #[test]
    fn changelog_registered_then_attached_to_payload() {
        // 回归：更新日志必须随阶段载荷一起进 UI（生产环境曾整段消失）
        set_changelog(vec!["修复了某些bugs".to_string()]);
        let p = PhasePayload::new(Stage::Available, "发现新版本", "0.0.0 → 1.0.0", None);
        assert_eq!(p.changelog, vec!["修复了某些bugs".to_string()]);
        // 下载阶段同样携带（Tauri 版 push_phase 统一注入）
        let d = PhasePayload::download("下载", None, Some(10.0), 1024, Some(2048), Some(2048));
        assert_eq!(d.changelog, vec!["修复了某些bugs".to_string()]);
        // 副本可被覆盖（重试路径重新登记）
        set_changelog(vec!["第二条".to_string()]);
        assert_eq!(changelog_snapshot(), vec!["第二条".to_string()]);
        // 复位，避免影响并行用例
        set_changelog(Vec::new());
        assert!(changelog_snapshot().is_empty());
    }
}
