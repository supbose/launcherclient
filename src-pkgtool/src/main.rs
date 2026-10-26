// 避免 release 模式下弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! QsLtd 更新包打包工具（egui 版）
//!
//! `scripts/versionTools.py` 的纯 Rust GUI 重实现：把配置区的文件/目录压缩成
//! `{app}_{version}.zip`，计算完整包 sha256 与大小；超过单卷上限时切分为
//! `.001/.002/...` 分卷并逐卷校验；按各下载前缀各生成一份 version 文件
//! （首个为 `version.json`，其余 `version_2.json`…）；打包后清理旧包。
//!
//! 与 Python 版的差异：
//! - 配置（FILES / 下载前缀 / 单卷大小 / 输出目录 / app 名）在界面内可编辑，
//!   持久化到 exe 同目录的 `packtool.json`，下次启动自动载入；
//! - 打包引擎完全用 Rust 实现（zip/sha2/walkdir），不依赖 python 环境；
//! - version.json 中 changelog 的中文按 JSON 规范转义（功能等价，客户端解析一致）。

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::Utc;
use egui::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 配置文件名（与 exe 同目录）
const CONFIG_FILE: &str = "packtool.json";

/// 应用配置（界面可编辑 + 持久化）
#[derive(Serialize, Deserialize, Clone, Default)]
struct Config {
    /// 应用名，用于 `{app}_{version}.zip` 与历史包清理匹配前缀
    app_name: String,
    /// 待打包文件/目录列表（目录以 `\*` / `/*` 结尾表示内容平铺到 zip 根）
    files: Vec<String>,
    /// 下载地址前缀（最终地址 = 前缀 + 包名）；每个前缀各生成一份 version 文件
    download_prefixes: Vec<String>,
    /// 单卷大小上限（MB），包体超过则切分为分卷
    volume_size_mb: u64,
    /// 打包输出目录（version.json / 包体落于此）
    output_dir: String,
}

impl Config {
    /// 默认配置（对齐 versionTools.py 的「配置区」初值）
    fn default_config() -> Self {
        Config {
            app_name: "qsm".into(),
            files: vec![
                r"d:\WorkspaceProject\cnb\supbose\qsm\src-tauri\resources".into(),
                r"D:\WorkspaceProject\cnb\supbose\python\musicdl-server\dist\nssm.exe".into(),
                r"D:\WorkspaceProject\cnb\supbose\python\musicdl-server\dist\qsm-server.exe".into(),
                r"D:\WorkspaceProject\cnb\supbose\qsm\src-tauri\target\release\qsm.exe".into(),
            ],
            download_prefixes: vec![
                "https://qsw.us.ci/update/qsm/".into(),
                "https://qsc.cdn.qcsm.ltd/update/qsm/".into(),
            ],
            volume_size_mb: 5,
            output_dir: r"D:\WorkspaceProject\github\supbose\api\update\qsm".into(),
        }
    }

    /// 载入配置：存在则解析，否则写入默认配置
    fn load() -> Self {
        let path = config_path();
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<Config>(&text) {
                return cfg;
            }
        }
        let cfg = Config::default_config();
        let _ = cfg.save();
        cfg
    }

    /// 保存到 exe 同目录的 packtool.json
    fn save(&self) -> Result<(), String> {
        let path = config_path();
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, text + "\n").map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// 配置文件绝对路径（exe 同目录）
fn config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(CONFIG_FILE)))
        .unwrap_or_else(|| PathBuf::from(CONFIG_FILE))
}

/// 版本号末位自动加一，非 x.y.z 返回 None
fn bump_version(v: &str) -> Option<String> {
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() == 3 && parts.iter().all(|p| p.parse::<u32>().is_ok()) {
        let mut n: Vec<u32> = parts.iter().map(|p| p.parse().unwrap()).collect();
        n[2] += 1;
        return Some(format!("{}.{}.{}", n[0], n[1], n[2]));
    }
    None
}

/// 依据输出目录现有 version.json 推算默认新版本号（回车默认）
fn suggest_version(output_dir: &str, app: &str) -> String {
    let vf = Path::new(output_dir).join("version.json");
    if vf.is_file() {
        if let Ok(t) = fs::read_to_string(&vf) {
            if let Ok(j) = serde_json::from_str::<serde_json::Value>(&t) {
                if let Some(vs) = j.get("version").and_then(|x| x.as_str()) {
                    if let Some(b) = bump_version(vs) {
                        return b;
                    }
                    return vs.to_string();
                }
            }
        }
    }
    // 不存在时以 {app} 命名也无妨，默认 1.0.0
    let _ = app;
    "1.0.0".to_string()
}

/// 解析单个配置项为 (源路径, zip 内路径) 列表；不存在返回空
///
/// 目录以 `\*` / `/*` 结尾：内容平铺到 zip 根（relative_to 目录本身）；
/// 目录不带通配：保留目录名层级（relative_to 父目录）；文件：仅文件名。
fn resolve_entry(entry: &str) -> Vec<(PathBuf, String)> {
    let wildcard = entry.ends_with("\\*") || entry.ends_with("/*");
    let p = PathBuf::from(if wildcard {
        &entry[..entry.len() - 2]
    } else {
        entry
    });
    let mut out = Vec::new();
    if p.is_file() {
        if let Some(name) = p.file_name() {
            let arc = name.to_string_lossy().replace('\\', "/");
            out.push((p, arc));
        }
    } else if p.is_dir() {
        let base: PathBuf = if wildcard {
            p.clone()
        } else {
            p.parent().map(|x| x.to_path_buf()).unwrap_or_else(|| p.clone())
        };
        for e in walkdir::WalkDir::new(&p).sort_by_file_name() {
            if let Ok(ent) = e {
                if ent.file_type().is_file() {
                    if let Ok(rel) = ent.path().strip_prefix(&base) {
                        let arc = rel.to_string_lossy().replace('\\', "/");
                        out.push((ent.path().to_path_buf(), arc));
                    }
                }
            }
        }
    }
    out
}

/// 分块计算文件 sha256（返回小写 hex）
fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = fs::File::open(path).map_err(|e| format!("打开 {} 失败: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 计算一段字节的 sha256
fn sha256_bytes(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

/// 将 zip 按固定字节大小切分为 .001/.002/... 分卷，返回 (分卷路径, 分卷sha256) 列表
fn split_zip(zip_path: &Path, volume_size: u64) -> Result<Vec<(PathBuf, String)>, String> {
    let mut parts = Vec::new();
    let mut index = 1u32;
    let mut file =
        fs::File::open(zip_path).map_err(|e| format!("打开 {} 失败: {}", zip_path.display(), e))?;
    let mut buf = vec![0u8; volume_size as usize];
    loop {
        let mut total = 0usize;
        while total < buf.len() {
            let n = file.read(&mut buf[total..]).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            total += n;
        }
        if total == 0 {
            break;
        }
        let part = PathBuf::from(format!("{}.{:03}", zip_path.display(), index));
        let mut pf =
            fs::File::create(&part).map_err(|e| format!("创建 {} 失败: {}", part.display(), e))?;
        pf.write_all(&buf[..total]).map_err(|e| e.to_string())?;
        let h = sha256_bytes(&buf[..total]);
        parts.push((part, h));
        index += 1;
    }
    Ok(parts)
}

/// 清理输出目录中除当前新包（含分卷）外的旧打包文件（匹配 `{app}_*.zip*`）
/// 返回被删除的文件名列表，由调用方统一记日志（避免与 logln 闭包争夺 log 借用）
fn clean_old_packages(output_dir: &Path, app: &str, zip_name: &str) -> Vec<String> {
    let mut deleted = Vec::new();
    if let Ok(entries) = fs::read_dir(output_dir) {
        let prefix = format!("{}_", app);
        for e in entries.flatten() {
            let path = e.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !name.starts_with(&prefix) || !name.contains(".zip") {
                continue;
            }
            if name != zip_name && !name.starts_with(&format!("{}.", zip_name)) {
                if fs::remove_file(&path).is_ok() {
                    deleted.push(name);
                }
            }
        }
    }
    deleted
}

/// 打包阶段名（顺序即执行顺序，UI 直接按此列表渲染阶段条）
const STAGE_NAMES: [&str; 5] = ["展开文件清单", "压缩打包", "计算校验值", "切分分卷", "生成版本清单"];

/// 打包进度快照（后台线程写、UI 线程读）
#[derive(Clone)]
struct Progress {
    /// 当前阶段下标（0-based）
    stage: usize,
    /// 当前阶段内的完成比例 0.0~1.0
    frac: f32,
    /// 当前阶段内的细节文案，如「12 / 300 个文件」
    detail: String,
}

impl Progress {
    fn new(stage: usize, frac: f32, detail: impl Into<String>) -> Self {
        Progress {
            stage,
            frac: frac.clamp(0.0, 1.0),
            detail: detail.into(),
        }
    }

    /// 总体百分比 0~100
    fn percent(&self) -> f32 {
        let total = STAGE_NAMES.len() as f32;
        ((self.stage as f32 + self.frac) / total * 100.0).min(100.0)
    }
}

/// 执行完整打包流程（纯逻辑，日志通过回调逐条输出，便于单测）
/// 返回成功摘要字符串，错误带原因
///
/// 日志采用回调而非持有缓冲：调用方可在回调内部**逐条短暂加锁**并向 UI 请求重绘，
/// 避免「整个打包期间持有互斥锁 → UI 线程读日志阻塞 → 界面假死」。
fn pack_core(
    cfg: &Config,
    version: &str,
    changelog_input: &str,
    mut logln: impl FnMut(&str),
    mut on_progress: impl FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<String, String> {
    /// 取消检查：用户点「取消」后立即中止，返回 Err
    macro_rules! check_cancel {
        () => {
            if cancel.load(Ordering::Relaxed) {
                return Err("已取消".into());
            }
        };
    }

    // 1. 展开配置项为实际文件清单
    on_progress(Progress::new(0, 0.0, "扫描配置项"));
    let mut resolved: Vec<(PathBuf, String)> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for (i, entry) in cfg.files.iter().enumerate() {
        check_cancel!();
        on_progress(Progress::new(
            0,
            (i as f32 + 1.0) / cfg.files.len() as f32,
            format!("{} / {} 项", i + 1, cfg.files.len()),
        ));
        let found = resolve_entry(entry);
        if found.is_empty() {
            missing.push(entry.clone());
        } else {
            resolved.extend(found);
        }
    }
    if !missing.is_empty() {
        logln("以下文件或目录不存在，请检查配置 FILES:");
        for p in &missing {
            logln(&format!("  - {}", p));
        }
        return Err("存在不存在的待打包项".into());
    }
    if resolved.is_empty() {
        return Err("待打包文件列表为空".into());
    }

    let output_dir = Path::new(&cfg.output_dir);
    fs::create_dir_all(output_dir).map_err(|e| format!("创建输出目录失败: {}", e))?;

    let zip_name = format!("{}_{}.zip", cfg.app_name, version);
    let zip_path = output_dir.join(&zip_name);

    // 2. 压缩
    logln(&format!("正在压缩 {} 个文件 -> {}", resolved.len(), zip_path.display()));
    {
        let file = fs::File::create(&zip_path)
            .map_err(|e| format!("创建 {} 失败: {}", zip_path.display(), e))?;
        let mut zip = zip::ZipWriter::new(file);
        // SimpleFileOptions = FileOptions<'static, ()>，默认压缩方式为 Deflated
        let options = zip::write::SimpleFileOptions::default();
        let total = resolved.len();
        for (i, (src, arc)) in resolved.iter().enumerate() {
            check_cancel!();
            on_progress(Progress::new(
                1,
                (i as f32 + 1.0) / total as f32,
                format!("{} / {} 个文件", i + 1, total),
            ));
            zip.start_file(arc, options)
                .map_err(|e| format!("写入 {} 失败: {}", arc, e))?;
            let mut f = fs::File::open(src).map_err(|e| format!("打开 {} 失败: {}", src.display(), e))?;
            std::io::copy(&mut f, &mut zip).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| format!("zip 收尾失败: {}", e))?;
    }

    // 3. 完整包大小与 sha256（分卷合并后客户端整体校验用）
    check_cancel!();
    on_progress(Progress::new(2, 0.2, "读取包体大小"));
    let size_bytes = fs::metadata(&zip_path).map_err(|e| e.to_string())?.len();
    on_progress(Progress::new(2, 0.6, "计算 sha256"));
    let digest = sha256_file(&zip_path)?;
    on_progress(Progress::new(2, 1.0, "校验完成"));

    // 4. 超限切分分卷
    check_cancel!();
    let volume_size = cfg.volume_size_mb * 1024 * 1024;
    let parts = if size_bytes > volume_size {
        logln(&format!(
            "包体 {} 字节超过单卷上限 {} MB，正在切分分卷:",
            size_bytes, cfg.volume_size_mb
        ));
        on_progress(Progress::new(3, 0.1, "切分中"));
        let ps = split_zip(&zip_path, volume_size)?;
        for (p, h) in &ps {
            logln(&format!("  {} ({} 字节, sha256 {})", p.display(), p.metadata().map(|m| m.len()).unwrap_or(0), &h[..16]));
        }
        fs::remove_file(&zip_path)
            .map_err(|e| format!("删除完整包失败: {}", e))?;
        on_progress(Progress::new(3, 1.0, format!("{} 卷", ps.len())));
        ps
    } else {
        on_progress(Progress::new(3, 1.0, "未超限，跳过"));
        Vec::new()
    };

    // 5. changelog：优先用输入（; 或换行分隔）；空则沿用旧 version.json
    let changelog: Vec<String> = if !changelog_input.trim().is_empty() {
        changelog_input
            .split([';', '\n'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        let vf = output_dir.join("version.json");
        if vf.is_file() {
            if let Ok(t) = fs::read_to_string(&vf) {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&t) {
                    if let Some(arr) = j.get("changelog").and_then(|x| x.as_array()) {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    };

    // 6. 每个下载前缀生成一份 version 文件
    check_cancel!();
    on_progress(Progress::new(4, 0.1, "解析更新日志"));
    let release_date = Utc::now().format("%Y-%m-%dT%H:%M:%S.000Z").to_string();
    let version_files: Vec<PathBuf> = std::iter::once(output_dir.join("version.json"))
        .chain((2..=cfg.download_prefixes.len()).map(|i| output_dir.join(format!("version_{}.json", i))))
        .collect();

    let npre = cfg.download_prefixes.len().max(1);
    for (i, (prefix, vfile)) in cfg.download_prefixes.iter().zip(version_files.iter()).enumerate() {
        check_cancel!();
        on_progress(Progress::new(
            4,
            0.1 + 0.7 * ((i as f32 + 1.0) / npre as f32),
            format!("{} / {} 份清单", i + 1, npre),
        ));
        let mut map = serde_json::Map::new();
        map.insert("version".into(), serde_json::json!(version));
        map.insert("update".into(), serde_json::json!(true));
        map.insert("size_bytes".into(), serde_json::json!(size_bytes));
        map.insert("url".into(), serde_json::json!(format!("{}{}", prefix, zip_name)));
        if !parts.is_empty() {
            let parts_urls: Vec<String> = parts
                .iter()
                .map(|(p, _)| {
                    format!(
                        "{}{}",
                        prefix,
                        p.file_name().unwrap().to_string_lossy()
                    )
                })
                .collect();
            let part_hashes: Vec<String> = parts.iter().map(|(_, h)| h.clone()).collect();
            map.insert("parts".into(), serde_json::json!(parts_urls));
            map.insert("part_sha256".into(), serde_json::json!(part_hashes));
        }
        map.insert("sha256".into(), serde_json::json!(digest));
        map.insert("release_date".into(), serde_json::json!(release_date));
        map.insert("changelog".into(), serde_json::json!(changelog));

        let value = serde_json::Value::Object(map);
        let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
        let created = !vfile.is_file();
        fs::write(vfile, text + "\n").map_err(|e| e.to_string())?;
        logln(&format!("{}: {}", if created { "已创建" } else { "已更新" }, vfile.display()));
    }

    // 7. 清理旧包
    on_progress(Progress::new(4, 0.92, "清理旧包"));
    let deleted = clean_old_packages(output_dir, &cfg.app_name, &zip_name);
    for name in &deleted {
        logln(&format!("已删除旧包: {}", name));
    }
    on_progress(Progress::new(4, 1.0, "完成"));

    let summary = format!(
        "打包完成: {}\n大小: {} 字节\nsha256: {}\n下载地址: {}{}",
        zip_path.display(),
        size_bytes,
        digest,
        cfg.download_prefixes.first().unwrap_or(&String::new()),
        zip_name
    );
    logln(&summary);
    Ok(summary)
}

/// 左侧导航页
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Config,
    Files,
    History,
    Version,
    Log,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Page::Config => "配置",
            Page::Files => "打包文件",
            Page::History => "历史构建",
            Page::Version => "下载与版本",
            Page::Log => "日志",
        }
    }
}

/// 导航分组：工作区（本地操作） / 分发（面向更新服务）
const NAV_GROUPS: [(&str, &[Page]); 2] = [
    ("工作区", &[Page::Config, Page::Files, Page::History]),
    ("分发", &[Page::Version, Page::Log]),
];

/// 历史构建条目（只读展示）
struct HistoryItem {
    name: String,
    size: String,
    date: String,
}

/// 扫描输出目录下 `{app}_*.zip` 历史包（只读展示用）
fn scan_history(output_dir: &str, app: &str) -> Vec<HistoryItem> {
    let mut items = Vec::new();
    if let Ok(entries) = std::fs::read_dir(output_dir) {
        let prefix = format!("{}_", app);
        for e in entries.flatten() {
            let path = e.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !name.starts_with(&prefix) || !name.contains(".zip") {
                continue;
            }
            if let Ok(meta) = std::fs::metadata(&path) {
                let size = meta.len();
                let size_str = if size >= 1024 * 1024 {
                    format!("{:.1} MB", size as f64 / 1024.0 / 1024.0)
                } else {
                    format!("{} B", size)
                };
                let date = meta
                    .modified()
                    .ok()
                    .map(|t| {
                        let dt: chrono::DateTime<Utc> = t.into();
                        dt.format("%Y-%m-%d %H:%M").to_string()
                    })
                    .unwrap_or_default();
                items.push(HistoryItem { name, size: size_str, date });
            }
        }
    }
    items.sort_by(|a, b| b.name.cmp(&a.name));
    items
}

/// 主题色板（浅色清爽风：白底卡片 + 极浅侧栏）
/// 设计原则：整屏只有「打包」按钮用实心品牌蓝，其余靠留白与层级区分，避免色块堆砌。
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x2F, 0x6D, 0xF6);
const ACCENT_TEXT: egui::Color32 = egui::Color32::WHITE;
/// 导航选中态：淡蓝底 + 蓝字（替代实心填充，更清爽）
const ACCENT_SOFT: egui::Color32 = egui::Color32::from_rgb(0xEA, 0xF1, 0xFE);
const SIDEBAR_BG: egui::Color32 = egui::Color32::from_rgb(0xFA, 0xFB, 0xFC);
const CARD_BG: egui::Color32 = egui::Color32::WHITE;
const BORDER: egui::Color32 = egui::Color32::from_rgb(0xED, 0xEF, 0xF2);
const INK: egui::Color32 = egui::Color32::from_rgb(0x37, 0x41, 0x51);
const MUTED: egui::Color32 = egui::Color32::from_rgb(0x6B, 0x72, 0x80);

/// 在 content 区渲染一个带标题的分组卡片：左侧品牌色竖条 + 深色标题，下方白色圆角卡片。
fn section<R>(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 15.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 1.5, ACCENT);
        ui.add_space(7.0);
        ui.label(egui::RichText::new(title).size(13.0).strong().color(INK));
    });
    ui.add_space(8.0);
    let r = egui::Frame::default()
        .fill(CARD_BG)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .inner_margin(egui::Margin::symmetric(14, 12))
        .corner_radius(10.0)
        .show(ui, |ui| content(ui));
    ui.add_space(18.0);
    r.inner
}

/// 向共享日志缓冲追加一行：只在写入瞬间持锁，写完立刻释放。
///
/// 用 `try_lock` + 忽略失败而非 `lock().unwrap()`：
/// 一是避免后台线程在锁上长等，二是锁被 poison 时不会 panic 拖垮线程。
fn push_log(log: &Arc<Mutex<Vec<String>>>, line: &str) {
    if let Ok(mut v) = log.try_lock() {
        v.push(line.to_string());
    }
}

/// 表单行：**标签置顶** → 控件占满整行 → 说明文字下挂并降一级灰度。
///
/// 对照设计稿「标签从左侧改为顶部，输入框可以占满整行；说明文字下挂并降一级灰度，
/// 不再和值抢注意力」——原先标签与控件同行会挤压输入框宽度，长路径只能横向滚动。
fn field<R>(ui: &mut egui::Ui, label: &str, hint: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.label(egui::RichText::new(label).size(12.5).strong().color(INK));
    ui.add_space(4.0);
    let r = add(ui);
    if !hint.is_empty() {
        ui.add_space(3.0);
        ui.label(egui::RichText::new(hint).size(11.0).color(MUTED));
    }
    ui.add_space(14.0);
    r
}

/// 等宽单行输入：路径、文件名、体积一律用等宽字体，避免字符宽度不同导致的抖动。
fn mono_edit(ui: &mut egui::Ui, text: &mut String) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(text)
            .font(egui::FontId::monospace(13.0))
            .desired_width(f32::INFINITY),
    )
}

/// 打包工具应用状态
struct PackToolApp {
    config: Config,
    version: String,
    changelog: String,
    log: Arc<Mutex<Vec<String>>>,
    running: Arc<Mutex<bool>>,
    /// 当前打包进度（后台线程写、UI 线程读；None 表示未在打包）
    progress: Arc<Mutex<Option<Progress>>>,
    /// 取消标志：用户点「取消」置 true，后台线程各阶段检查并中止
    cancel: Arc<AtomicBool>,
    /// 打包开始时刻，用于显示已用时
    pack_started: Option<Instant>,
    current_page: Page,
    /// 是否已执行过窗口居中（仅首帧做一次）
    centered_once: bool,
}

impl PackToolApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 安装中文字体，修复中文豆腐块（乱码）显示
        install_fonts(&cc.egui_ctx);
        let config = Config::load();
        let version = suggest_version(&config.output_dir, &config.app_name);
        PackToolApp {
            config,
            version,
            changelog: String::new(),
            log: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(Mutex::new(false)),
            progress: Arc::new(Mutex::new(None)),
            cancel: Arc::new(AtomicBool::new(false)),
            pack_started: None,
            current_page: Page::Config,
            centered_once: false,
        }
    }

    /// 触发打包（后台线程执行）
    ///
    /// 关键：日志回调内部**每条短暂加锁后立即释放**，绝不在整个打包期间持有锁；
    /// 否则 UI 线程读取日志时会被阻塞，Windows 会把窗口标记为「无响应」（假死）。
    fn start_pack(&mut self, ctx: Context) {
        let version = self.version.trim().to_string();
        let version = if version.is_empty() {
            suggest_version(&self.config.output_dir, &self.config.app_name)
        } else {
            version
        };
        // 版本号格式校验：x.y / x.y.z / x.y.z.w
        let re = regex::Regex::new(r"^\d+(\.\d+){1,3}$").unwrap();
        if !re.is_match(&version) {
            push_log(&self.log, &format!("版本号格式无效: {}（示例 2.1.3）", version));
            return;
        }
        let cfg = self.config.clone();
        let changelog = self.changelog.clone();
        let log = self.log.clone();
        let running = self.running.clone();
        let progress = self.progress.clone();
        let cancel = self.cancel.clone();
        cancel.store(false, Ordering::Relaxed);
        *running.lock().unwrap() = true;
        self.pack_started = Some(Instant::now());
        // 打包开始即切到日志页，让用户直接看到进度
        self.current_page = Page::Log;

        std::thread::spawn(move || {
            // 每条日志：加锁 → push → 立即释放 → 请求重绘
            let emit = {
                let log = log.clone();
                let ctx = ctx.clone();
                move |s: &str| {
                    push_log(&log, s);
                    ctx.request_repaint();
                }
            };
            // 进度：写快照 → 立即释放锁 → 请求重绘（同样不长期持锁）
            let report = {
                let progress = progress.clone();
                let ctx = ctx.clone();
                move |p: Progress| {
                    if let Ok(mut g) = progress.try_lock() {
                        *g = Some(p);
                    }
                    ctx.request_repaint();
                }
            };
            let res = pack_core(&cfg, &version, &changelog, emit, report, &cancel);
            match res {
                Ok(summary) => {
                    for line in summary.lines() {
                        push_log(&log, line);
                    }
                }
                Err(ref e) if e == "已取消" => {
                    push_log(&log, "已取消：打包在中途被中止");
                }
                Err(e) => push_log(&log, &format!("[失败] 打包失败: {}", e)),
            }
            *running.lock().unwrap() = false;
            if let Ok(mut g) = progress.try_lock() {
                *g = None;
            }
            ctx.request_repaint();
        });
    }

    /// 请求取消当前打包（协作式：后台线程在下一个检查点退出）
    fn cancel_pack(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        push_log(&self.log, "正在取消，等待当前阶段收尾…");
    }

    /// 配置页：基本信息 + 保存
    fn page_config(&mut self, ui: &mut egui::Ui) {
        ui.heading(egui::RichText::new("配置").size(18.0).strong().color(INK));
        ui.add_space(4.0);
        section(ui, "基本信息", |ui| {
            field(
                ui,
                "应用名",
                "决定产物文件名 {app}_{version}.zip，同时也是旧包清理的匹配前缀",
                |ui| mono_edit(ui, &mut self.config.app_name),
            );
            field(
                ui,
                "输出目录",
                "分卷产物与 version.json 清单都会写到这里",
                |ui| {
                    ui.horizontal(|ui| {
                        // 输入框吃掉除「浏览」按钮外的全部宽度
                        ui.add_sized(
                            egui::vec2(ui.available_width() - 58.0, 26.0),
                            egui::TextEdit::singleline(&mut self.config.output_dir)
                                .font(egui::FontId::monospace(13.0)),
                        );
                        if ui.button("浏览…").clicked() {
                            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                self.config.output_dir = p.to_string_lossy().replace('\\', "/");
                            }
                        }
                    });
                },
            );
            field(
                ui,
                "单卷大小 (MB)",
                "包体超过该体积自动切分，编号 .001 / .002",
                |ui| {
                    ui.add(egui::DragValue::new(&mut self.config.volume_size_mb).range(1..=2048));
                },
            );
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("保存配置").strong().color(ACCENT_TEXT),
                        )
                        .fill(ACCENT)
                        .corner_radius(6.0)
                        .min_size(egui::vec2(88.0, 28.0)),
                    )
                    .clicked()
                {
                    match self.config.save() {
                        Ok(_) => push_log(&self.log, "配置已保存到 packtool.json"),
                        Err(e) => push_log(&self.log, &format!("保存失败: {}", e)),
                    }
                }
                ui.label(
                    egui::RichText::new("未保存的改动不会写进 packtool.json")
                        .size(11.0)
                        .color(MUTED),
                );
            });
        });
    }

    /// 历史构建页：输出目录内已有产物（按名称倒序 = 版本由新到旧）
    fn page_history(&mut self, ui: &mut egui::Ui) {
        ui.heading(egui::RichText::new("历史构建").size(18.0).strong().color(INK));
        ui.add_space(4.0);
        section(ui, "输出目录内的历史产物", |ui| {
            ui.label(
                egui::RichText::new("按构建时间倒序，文件名与体积用等宽字体对齐")
                    .size(11.0)
                    .color(MUTED),
            );
            ui.add_space(8.0);
            let history = scan_history(&self.config.output_dir, &self.config.app_name);
            if history.is_empty() {
                ui.label(egui::RichText::new("还没有构建记录").size(13.0).color(MUTED));
                ui.label(
                    egui::RichText::new("点右上角「打包」，产物会自动登记到这里")
                        .size(11.0)
                        .color(MUTED),
                );
                return;
            }
            let n = history.len();
            for (i, h) in history.iter().enumerate() {
                ui.horizontal(|ui| {
                    // 文件名用等宽，避免不同字符宽度导致各行错位
                    ui.label(
                        egui::RichText::new(&h.name)
                            .size(13.0)
                            .color(INK)
                            .font(egui::FontId::monospace(13.0)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.allocate_ui(egui::vec2(76.0, 18.0), |ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(&h.size)
                                        .size(12.0)
                                        .color(MUTED)
                                        .font(egui::FontId::monospace(12.0)),
                                );
                            });
                        });
                        ui.allocate_ui(egui::vec2(118.0, 18.0), |ui| {
                            ui.label(
                                egui::RichText::new(&h.date)
                                    .size(12.0)
                                    .color(MUTED)
                                    .font(egui::FontId::monospace(12.0)),
                            );
                        });
                    });
                });
                if i + 1 < n {
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                }
            }
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!("共 {} 个构建包", n))
                    .size(11.0)
                    .color(MUTED),
            );
        });
    }

    /// 打包文件页：文件/目录列表
    fn page_files(&mut self, ui: &mut egui::Ui) {
        ui.heading(egui::RichText::new("打包文件").size(18.0).strong().color(INK));
        ui.add_space(4.0);
        section(ui, "待打包文件 / 目录", |ui| {
            ui.label(
                egui::RichText::new("目录以 /* 结尾表示内容平铺到 zip 根")
                    .size(11.0)
                    .color(MUTED),
            );
            ui.add_space(8.0);
            let mut remove_file: Option<usize> = None;
            for (i, f) in self.config.files.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    // 路径等宽且吃掉除「删除」外的全部宽度
                    ui.add_sized(
                        egui::vec2(ui.available_width() - 52.0, 26.0),
                        egui::TextEdit::singleline(f).font(egui::FontId::monospace(12.5)),
                    );
                    if ui.button("删除").clicked() {
                        remove_file = Some(i);
                    }
                });
                ui.add_space(4.0);
            }
            if let Some(i) = remove_file {
                self.config.files.remove(i);
            }
            ui.horizontal(|ui| {
                if ui.button("+ 文件").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                        self.config.files.push(p.to_string_lossy().replace('\\', "/"));
                    }
                }
                if ui.button("+ 目录").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.config.files.push(p.to_string_lossy().replace('\\', "/"));
                    }
                }
            });
        });
    }

    /// 下载与版本页：前缀 + 版本号 + 更新日志
    fn page_version(&mut self, ui: &mut egui::Ui) {
        ui.heading(egui::RichText::new("下载与版本").size(18.0).strong().color(INK));
        ui.add_space(4.0);
        section(ui, "下载前缀", |ui| {
            ui.label(
                egui::RichText::new("每个前缀各生成一份 version 文件（多地址容灾）")
                    .size(11.0)
                    .color(MUTED),
            );
            ui.add_space(8.0);
            let mut remove_pre: Option<usize> = None;
            for (i, p) in self.config.download_prefixes.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        egui::vec2(ui.available_width() - 52.0, 26.0),
                        egui::TextEdit::singleline(p).font(egui::FontId::monospace(12.5)),
                    );
                    if ui.button("删除").clicked() {
                        remove_pre = Some(i);
                    }
                });
                ui.add_space(4.0);
            }
            if let Some(i) = remove_pre {
                self.config.download_prefixes.remove(i);
            }
            if ui.button("+ 添加前缀").clicked() {
                self.config.download_prefixes.push(String::new());
            }
        });
        section(ui, "版本与更新日志", |ui| {
            field(
                ui,
                "版本号",
                "语义化版本 主.次.修订，需大于已发布的线上版本",
                |ui| {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            egui::vec2(ui.available_width() - 140.0, 26.0),
                            egui::TextEdit::singleline(&mut self.version)
                                .font(egui::FontId::monospace(13.0)),
                        );
                        if ui.button("取默认（末位+1）").clicked() {
                            self.version =
                                suggest_version(&self.config.output_dir, &self.config.app_name);
                        }
                    });
                },
            );
            field(
                ui,
                "更新日志",
                "多条用 ; 或换行分隔；留空则沿用上一版",
                |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.changelog)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    );
                },
            );
        });
    }

    /// 打包进度卡片：阶段条 + 百分比 + 已用时 + 取消（仅打包期间显示）
    fn draw_progress_card(&mut self, ui: &mut egui::Ui) {
        // 先取出进度快照（clone），避免闭包内再次借用 self.progress
        let prog = match self.progress.try_lock() {
            Ok(g) => g.clone(),
            Err(_) => None,
        };
        let elapsed = self
            .pack_started
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(0.0);
        let title_desc = format!(
            "{}_{}.zip · 输出到 {}",
            self.config.app_name, self.version, self.config.output_dir
        );

        section(ui, "打包进度", |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("正在打包").size(14.0).strong().color(INK));
                ui.label(egui::RichText::new(&title_desc).size(11.0).color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cancel_btn = egui::Button::new(egui::RichText::new("取消").color(MUTED))
                        .corner_radius(6.0)
                        .min_size(egui::vec2(56.0, 26.0));
                    if ui.add(cancel_btn).clicked() {
                        self.cancel_pack();
                    }
                    ui.label(egui::RichText::new("进行中").size(11.0).color(ACCENT));
                    ui.add_space(8.0);
                });
            });
            ui.add_space(8.0);

            let pct = prog.as_ref().map(|p| p.percent()).unwrap_or(0.0);
            ui.add(egui::ProgressBar::new(pct / 100.0).desired_height(8.0).show_percentage());
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let (idx, name, detail) = match &prog {
                    Some(p) => (
                        p.stage + 1,
                        STAGE_NAMES.get(p.stage).copied().unwrap_or(""),
                        p.detail.clone(),
                    ),
                    None => (0, "准备中", String::new()),
                };
                ui.label(
                    egui::RichText::new(format!("阶段 {} / {} · {}", idx, STAGE_NAMES.len(), name))
                        .size(12.0)
                        .color(INK),
                );
                if !detail.is_empty() {
                    ui.label(
                        egui::RichText::new(detail)
                            .size(11.0)
                            .color(MUTED)
                            .font(egui::FontId::monospace(11.0)),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("已用时 {:.1}s", elapsed))
                            .size(11.0)
                            .color(MUTED)
                            .font(egui::FontId::monospace(11.0)),
                    );
                });
            });
            ui.add_space(6.0);

            // 阶段条：已完成 / 进行中 / 未开始
            ui.horizontal_wrapped(|ui| {
                for (i, name) in STAGE_NAMES.iter().enumerate() {
                    let (color, strong) = match &prog {
                        Some(p) if i < p.stage => (ACCENT, false),
                        Some(p) if i == p.stage => (ACCENT, true),
                        _ => (MUTED, false),
                    };
                    let mut t = egui::RichText::new(*name).size(11.5).color(color);
                    if strong {
                        t = t.strong();
                    }
                    ui.label(t);
                    if i + 1 < STAGE_NAMES.len() {
                        ui.label(egui::RichText::new("·").size(11.5).color(MUTED));
                    }
                }
            });
        });
    }

    /// 日志页
    fn page_log(&mut self, ui: &mut egui::Ui) {
        ui.heading(egui::RichText::new("日志").size(18.0).strong().color(INK));
        ui.add_space(4.0);
        if self.running.try_lock().map(|v| *v).unwrap_or(false) {
            self.draw_progress_card(ui);
        }
        ui.horizontal(|ui| {
            if ui.button("清空日志").clicked() {
                if let Ok(mut v) = self.log.try_lock() {
                    v.clear();
                }
            }
            ui.label(egui::RichText::new("实时打包输出").small().weak());
        });
        ui.add_space(6.0);
        egui::Frame::default()
            .fill(CARD_BG)
            .stroke(egui::Stroke::new(1.0, BORDER))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .corner_radius(8.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        // try_lock：绝不因日志锁而阻塞 UI 线程（后台打包时锁可能正被占用）
                        match self.log.try_lock() {
                            Ok(log) => {
                                if log.is_empty() {
                                    ui.label(egui::RichText::new("（暂无日志）").weak());
                                } else {
                                    for line in log.iter() {
                                        ui.monospace(line);
                                    }
                                }
                            }
                            Err(_) => {
                                ui.label(egui::RichText::new("（日志正被写入，稍后刷新）").weak());
                            }
                        }
                    });
            });
    }
}

impl eframe::App for PackToolApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 首帧把窗口居中到主显示器（ViewportCommand::center_on_screen 在视口信息就绪后返回 Some）
        if !self.centered_once {
            if let Some(cmd) = egui::ViewportCommand::center_on_screen(ui.ctx()) {
                ui.ctx().send_viewport_cmd(cmd);
                self.centered_once = true;
            }
        }

        // 打包期间持续请求重绘：保证 spinner 转动、日志实时刷新
        let running = self.running.try_lock().map(|v| *v).unwrap_or(false);
        if running {
            ui.ctx().request_repaint();
        }

        egui::CentralPanel::default().show(ui, |ui| {
            // 顶部：标题 + 常驻打包按钮
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("QsLtd 更新包打包工具")
                        .size(18.0)
                        .strong()
                        .color(INK),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if running {
                        ui.spinner();
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("打包中…").color(ACCENT).strong());
                        ui.add_space(8.0);
                    }
                    let btn = egui::Button::new(egui::RichText::new("打包").strong().color(ACCENT_TEXT))
                        .fill(ACCENT)
                        .corner_radius(6.0)
                        .min_size(egui::vec2(80.0, 30.0));
                    if ui.add_enabled(!running, btn).clicked() {
                        self.start_pack(ui.ctx().clone());
                    }
                });
            });
            ui.add_space(6.0);
            ui.separator();

            // 左导航 + 右内容
            egui::Panel::left("nav")
                .resizable(false)
                .default_size(168.0)
                .show(ui, |ui| {
                    egui::Frame::default().fill(SIDEBAR_BG).show(ui, |ui| {
                        ui.add_space(14.0);
                        // 分组导航（工作区 / 分发）+ 当前项左侧指示条
                        for (gname, pages) in NAV_GROUPS {
                            ui.label(egui::RichText::new(gname).size(10.5).color(MUTED).strong());
                            ui.add_space(6.0);
                            for p in pages {
                                let active = self.current_page == *p;
                                ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(3.0, 30.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        1.5,
                                        if active {
                                            ACCENT
                                        } else {
                                            egui::Color32::TRANSPARENT
                                        },
                                    );
                                    ui.add_space(5.0);
                                    let resp = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new(p.title())
                                                .size(13.0)
                                                .color(if active { ACCENT } else { MUTED })
                                                .strong(),
                                        )
                                        .fill(
                                            if active {
                                                ACCENT_SOFT
                                            } else {
                                                egui::Color32::TRANSPARENT
                                            },
                                        )
                                        .stroke(egui::Stroke::NONE)
                                        .corner_radius(8.0)
                                        .min_size(egui::vec2(ui.available_width(), 30.0)),
                                    );
                                    if resp.clicked() {
                                        self.current_page = *p;
                                    }
                                });
                                ui.add_space(5.0);
                            }
                            ui.add_space(12.0);
                        }
                    });
                });
            egui::CentralPanel::default().show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| match self.current_page {
                    Page::Config => self.page_config(ui),
                    Page::Files => self.page_files(ui),
                    Page::History => self.page_history(ui),
                    Page::Version => self.page_version(ui),
                    Page::Log => self.page_log(ui),
                });
            });
        });
    }
}

/// Windows 系统中文字体候选（按序取第一个存在的；微软雅黑优先）。
/// egui 默认字体无 CJK 字形（会显示豆腐块），从系统字体目录加载中文字体修复。
/// 与 `src-egui/src/theme.rs::install_fonts` 同方案。
const FONT_CANDIDATES: [&str; 5] = [
    "C:/Windows/Fonts/msyh.ttc",
    "C:/Windows/Fonts/msyhl.ttc",
    "C:/Windows/Fonts/simhei.ttf",
    "C:/Windows/Fonts/Deng.ttf",
    "C:/Windows/Fonts/simsun.ttc",
];

/// 安装中文字体：把系统中文字体插到 egui 字体族最前，修复中文豆腐块显示。
fn install_fonts(ctx: &Context) {
    let Some(path) = FONT_CANDIDATES.iter().find(|p| std::path::Path::new(p).exists()) else {
        eprintln!("[pkgtool] 未找到系统中文字体，中文将无法显示");
        return;
    };
    let Ok(data) = std::fs::read(path) else {
        eprintln!("[pkgtool] 读取系统字体失败: {}", path);
        return;
    };
    eprintln!("[pkgtool] 使用中文字体: {}", path);

    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".to_owned(), egui::FontData::from_owned(data).into());
    if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        prop.insert(0, "cjk".to_owned());
    }
    if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        mono.insert(0, "cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// 把 `assets/icon.ico` 编进二进制并解码为 egui::IconData，用于窗口/任务栏图标。
/// 用 `include_bytes!` 直接内嵌，避免运行时依赖外部文件；取尺寸最大的那一帧。
fn load_window_icon() -> Option<egui::IconData> {
    const ICON_BYTES: &[u8] = include_bytes!("../assets/icon.ico");
    let cursor = std::io::Cursor::new(&ICON_BYTES[..]);
    let icon_dir = ico::IconDir::read(cursor).ok()?;
    // 取尺寸最大的那一帧（任务栏通常用 32×32，但取最大最通用）
    let mut best: Option<&ico::IconDirEntry> = None;
    for entry in icon_dir.entries() {
        let take = match best {
            Some(b) => entry.width() > b.width(),
            None => true,
        };
        if take {
            best = Some(entry);
        }
    }
    let entry = best?;
    let image = entry.decode().ok()?;
    let rgba = image.rgba_data().to_vec();
    Some(egui::IconData {
        rgba,
        width: image.width(),
        height: image.height(),
    })
}

fn main() -> eframe::Result<()> {
    let icon = load_window_icon();
    let viewport = egui::ViewportBuilder::default()
        .with_title("QsLtd 更新包打包工具")
        .with_inner_size([760.0, 680.0])
        .with_resizable(true)
        .with_min_inner_size([640.0, 480.0]);
    let viewport = match icon {
        Some(icon) => viewport.with_icon(icon),
        None => viewport,
    };
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "QsLtd 更新包打包工具",
        native_options,
        Box::new(|cc| Ok(Box::new(PackToolApp::new(cc)) as Box<dyn eframe::App>)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pkgtool_test_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn pack_core_single_and_split() {
        let base = tmpdir("pack");
        let src = base.join("src");
        std::fs::create_dir_all(&src).unwrap();
        // 普通文件
        let f1 = src.join("app.exe");
        std::fs::write(&f1, b"hello world from qsm").unwrap();
        // 目录（通配平铺到 zip 根）
        let sub = src.join("resources");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("a.txt"), b"aaa").unwrap();
        std::fs::write(sub.join("b.txt"), b"bbb").unwrap();
        // 大文件（>1MB 且不可压缩）用以触发分卷
        let big = src.join("big.bin");
        let mut seed = 0x1234u32;
        let payload: Vec<u8> = (0..1500 * 1024)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            })
            .collect();
        std::fs::write(&big, &payload).unwrap();

        let out = base.join("out");
        let cfg = Config {
            app_name: "testapp".into(),
            files: vec![
                f1.to_string_lossy().into(),
                format!("{}\\*", sub.display()),
                big.to_string_lossy().into(),
            ],
            download_prefixes: vec!["https://example.com/update/testapp/".into()],
            volume_size_mb: 1,
            output_dir: out.to_string_lossy().into(),
        };

        let mut log = Vec::new();
        let mut stages = Vec::new();
        let res = pack_core(
            &cfg,
            "1.2.3",
            "修复了某些bugs;新增下载前缀配置",
            |s: &str| log.push(s.to_string()),
            |p: Progress| stages.push(p.stage),
            &AtomicBool::new(false),
        );
        assert!(res.is_ok(), "pack_core 失败: {:?}", log);
        // 进度应覆盖全部 5 个阶段，且单调不回退
        assert!(stages.contains(&0) && stages.contains(&4), "进度应覆盖首尾阶段: {:?}", stages);
        assert!(
            stages.windows(2).all(|w| w[0] <= w[1]),
            "阶段下标不应回退: {:?}",
            stages
        );

        // version.json 字段校验
        let vf = out.join("version.json");
        assert!(vf.is_file(), "version.json 未生成");
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&vf).unwrap()).unwrap();
        assert_eq!(j["version"], "1.2.3");
        assert_eq!(j["update"], true);
        assert!(j["size_bytes"].as_u64().unwrap() > 0);
        assert_eq!(j["url"], "https://example.com/update/testapp/testapp_1.2.3.zip");

        let zip_name = "testapp_1.2.3.zip";
        let zip_path = out.join(zip_name);
        // 分卷模式：完整包被删除，分卷存在
        assert!(!zip_path.is_file(), "超限后完整包应被删除");
        let part1 = out.join(format!("{}.001", zip_name));
        let part2 = out.join(format!("{}.002", zip_name));
        assert!(part1.is_file() && part2.is_file(), "应切分为 .001/.002 分卷");

        let parts = j["parts"].as_array().unwrap();
        let phashes = j["part_sha256"].as_array().unwrap();
        assert_eq!(parts.len(), phashes.len());
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "https://example.com/update/testapp/testapp_1.2.3.zip.001");

        // 合并分卷还原后 sha256 应与 version.json 的 sha256 一致
        let mut merged = Vec::new();
        let mut f1 = std::fs::File::open(&part1).unwrap();
        std::io::Read::read_to_end(&mut f1, &mut merged).unwrap();
        let mut f2 = std::fs::File::open(&part2).unwrap();
        std::io::Read::read_to_end(&mut f2, &mut merged).unwrap();
        let merged_sha = hex::encode(Sha256::digest(&merged));
        assert_eq!(merged_sha, j["sha256"].as_str().unwrap(), "合并后 sha256 应与 version.json 一致");

        // changelog 解析
        let cl = j["changelog"].as_array().unwrap();
        assert_eq!(cl.len(), 2);
        assert_eq!(cl[0], "修复了某些bugs");
        assert_eq!(cl[1], "新增下载前缀配置");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn bump_and_suggest() {
        assert_eq!(bump_version("1.2.3"), Some("1.2.4".into()));
        assert_eq!(bump_version("2.0"), None);
        let base = tmpdir("ver");
        let out = base.join("o");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("version.json"), r#"{"version":"3.1.9"}"#).unwrap();
        assert_eq!(suggest_version(out.to_string_lossy().as_ref(), "qsm"), "3.1.10");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 防回归：打包期间不得长期持有日志锁。
    /// 若 pack_core 全程持锁，UI 线程读取日志会被阻塞 → 窗口被系统判定「无响应」（假死）。
    #[test]
    fn log_lock_not_held_during_pack() {
        let base = tmpdir("lock");
        let src = base.join("src");
        std::fs::create_dir_all(&src).unwrap();
        // 4MB 不可压缩数据，保证打包有可观测耗时（deflate 无法压缩随机字节）
        let big = src.join("big.bin");
        let mut seed = 0x99u32;
        let payload: Vec<u8> = (0..4 * 1024 * 1024)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            })
            .collect();
        std::fs::write(&big, &payload).unwrap();

        let out = base.join("out");
        let cfg = Config {
            app_name: "lockapp".into(),
            files: vec![big.to_string_lossy().into()],
            download_prefixes: vec!["https://example.com/u/".into()],
            volume_size_mb: 64,
            output_dir: out.to_string_lossy().into(),
        };

        let log = Arc::new(Mutex::new(Vec::new()));
        let writer = log.clone();
        let handle = std::thread::spawn(move || {
            pack_core(
                &cfg,
                "1.0.0",
                "t",
                move |s: &str| {
                    writer.lock().unwrap().push(s.to_string());
                },
                |_p: Progress| {},
                &AtomicBool::new(false),
            )
        });

        // 模拟 UI 线程：打包期间反复尝试取锁，只要成功一次就说明锁未被长期占用
        let mut acquired = 0usize;
        while !handle.is_finished() {
            if log.try_lock().is_ok() {
                acquired += 1;
            }
        }
        let res = handle.join().expect("打包线程不应 panic");
        assert!(res.is_ok(), "打包应成功");
        assert!(
            acquired > 0,
            "打包期间 UI 线程应能取到日志锁（说明锁被长期占用 → 界面假死）"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 取消：协作式中止，返回 Err("已取消") 且不留下半成品
    #[test]
    fn pack_core_cancel_aborts() {
        let base = tmpdir("cancel");
        let src = base.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        let out = base.join("out");
        let cfg = Config {
            app_name: "cancelapp".into(),
            files: vec![src.join("a.txt").to_string_lossy().into()],
            download_prefixes: vec!["https://example.com/u/".into()],
            volume_size_mb: 64,
            output_dir: out.to_string_lossy().into(),
        };
        // 取消标志一开始就为 true：第一个检查点即中止
        let res = pack_core(
            &cfg,
            "1.0.0",
            "t",
            |_s: &str| {},
            |_p: Progress| {},
            &AtomicBool::new(true),
        );
        assert_eq!(res.unwrap_err(), "已取消");
        assert!(
            !out.join("cancelapp_1.0.0.zip").is_file(),
            "取消后不应留下产物"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn window_icon_from_ico() {
        // 验证 icon.ico 经 include_bytes! + ico 解码得到有效的窗口/任务栏图标
        let icon = load_window_icon().expect("icon.ico 应能被解码为 IconData");
        assert!(icon.rgba.len() == icon.width as usize * icon.height as usize * 4,
            "rgba 字节数应等于 width*height*4");
        assert!(icon.width > 0 && icon.height > 0);
    }
}
