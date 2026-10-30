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
use qsltd_ui::theme::{
    self, apply_theme, install_fonts, tint, ACCENT, ACCENT_SOFT, MUTED, SUCCESS, TEXT,
};
use qsltd_ui::widgets::{draw_titlebar, paint_bg, progress_track, TitleBarAction};
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
    /// 最低允许版本（写入 manifest 的 `min_version`，由服务端下发）：
    /// 低于它的客户端触发阻断式强制更新；留空表示不约束
    #[serde(default)]
    min_version: String,
    /// 是否强制更新（写入 manifest 的 `force_update`）：true 时所有客户端
    /// 必须升级到本版本，不给「稍后升级」出口
    #[serde(default)]
    force_update: bool,
    /// 应用商店地址（写入 manifest 的 `store_url`）：强制更新界面
    /// 「前往应用商店」入口；留空则不显示该入口
    #[serde(default)]
    store_url: String,
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
            min_version: String::new(),
            force_update: false,
            store_url: String::new(),
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
            p.parent()
                .map(|x| x.to_path_buf())
                .unwrap_or_else(|| p.clone())
        };
        // sort_by_file_name() 返回 &mut WalkDir，要先 into_iter() 才是迭代器；
        // flatten() 跳过遍历出错的路径，等价于原来的 `if let Ok(ent)`
        for ent in walkdir::WalkDir::new(&p)
            .sort_by_file_name()
            .into_iter()
            .flatten()
        {
            if ent.file_type().is_file() {
                if let Ok(rel) = ent.path().strip_prefix(&base) {
                    let arc = rel.to_string_lossy().replace('\\', "/");
                    out.push((ent.path().to_path_buf(), arc));
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
            if name != zip_name
                && !name.starts_with(&format!("{}.", zip_name))
                && fs::remove_file(&path).is_ok()
            {
                deleted.push(name);
            }
        }
    }
    deleted
}

/// 打包阶段名（顺序即执行顺序，UI 直接按此列表渲染阶段条）
const STAGE_NAMES: [&str; 5] = [
    "展开文件清单",
    "压缩打包",
    "计算校验值",
    "切分分卷",
    "生成版本清单",
];

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
    logln(&format!(
        "正在压缩 {} 个文件 -> {}",
        resolved.len(),
        zip_path.display()
    ));
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
            let mut f =
                fs::File::open(src).map_err(|e| format!("打开 {} 失败: {}", src.display(), e))?;
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
            logln(&format!(
                "  {} ({} 字节, sha256 {})",
                p.display(),
                p.metadata().map(|m| m.len()).unwrap_or(0),
                &h[..16]
            ));
        }
        fs::remove_file(&zip_path).map_err(|e| format!("删除完整包失败: {}", e))?;
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
        .chain(
            (2..=cfg.download_prefixes.len())
                .map(|i| output_dir.join(format!("version_{}.json", i))),
        )
        .collect();

    let npre = cfg.download_prefixes.len().max(1);
    for (i, (prefix, vfile)) in cfg
        .download_prefixes
        .iter()
        .zip(version_files.iter())
        .enumerate()
    {
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
        map.insert(
            "url".into(),
            serde_json::json!(format!("{}{}", prefix, zip_name)),
        );
        if !parts.is_empty() {
            let parts_urls: Vec<String> = parts
                .iter()
                .map(|(p, _)| format!("{}{}", prefix, p.file_name().unwrap().to_string_lossy()))
                .collect();
            let part_hashes: Vec<String> = parts.iter().map(|(_, h)| h.clone()).collect();
            map.insert("parts".into(), serde_json::json!(parts_urls));
            map.insert("part_sha256".into(), serde_json::json!(part_hashes));
        }
        map.insert("sha256".into(), serde_json::json!(digest));
        // 强制更新策略（服务端下发，客户端不可本地覆盖）：
        // - force_update 恒写入（bool，明确表达本次是否为强制发布）
        // - min_version / store_url 仅在非空时写入，留空即表示「不约束 / 不显示入口」
        map.insert("force_update".into(), serde_json::json!(cfg.force_update));
        if !cfg.min_version.trim().is_empty() {
            map.insert(
                "min_version".into(),
                serde_json::json!(cfg.min_version.trim()),
            );
        }
        if !cfg.store_url.trim().is_empty() {
            map.insert("store_url".into(), serde_json::json!(cfg.store_url.trim()));
        }
        map.insert("release_date".into(), serde_json::json!(release_date));
        map.insert("changelog".into(), serde_json::json!(changelog));

        let value = serde_json::Value::Object(map);
        let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
        let created = !vfile.is_file();
        fs::write(vfile, text + "\n").map_err(|e| e.to_string())?;
        logln(&format!(
            "{}: {}",
            if created { "已创建" } else { "已更新" },
            vfile.display()
        ));
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

/// 页面（顶部标签行切换）
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Config,
    Files,
    History,
    Version,
    Log,
    About,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Page::Config => "配置",
            Page::Files => "打包文件",
            Page::History => "历史构建",
            Page::Version => "下载与版本",
            Page::Log => "日志",
            Page::About => "关于",
        }
    }
}

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
                items.push(HistoryItem {
                    name,
                    size: size_str,
                    date,
                });
            }
        }
    }
    items.sort_by(|a, b| b.name.cmp(&a.name));
    items
}

// ---- 深色主题（共享 qsltd-ui：与 launcherclient 同一份色板）----
//
// 色值、透明度层级、标题栏、背景渐变、进度条、字体安装都来自 `qsltd-ui`，
// 这样两个 exe 的观感不会再各自漂移。pkgtool 专属的少量别名保留在下面。

/// 主按钮文字色（白）
const ACCENT_TEXT: egui::Color32 = egui::Color32::WHITE;
/// 卡片/侧栏底（白色 4% 叠层，= qsltd-ui 的 W04）
const CARD_BG: egui::Color32 = theme::W04;
/// 卡片描边（白色 8% 叠层，= qsltd-ui 的 W08）
const BORDER: egui::Color32 = theme::W08;

/// 两色线性插值（进度环 蓝→绿 渐变用）
fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from_rgba_premultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t) as u8,
    )
}

/// 环形进度（方案 E 仪表盘右栏）：全圆轨道 + 蓝→绿渐变填充弧，中心两行文字
fn draw_progress_ring(ui: &mut egui::Ui, diameter: f32, pct: f32, big: &str, sub: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let c = rect.center();
    let r_out = diameter / 2.0 - 3.0;
    let r_in = r_out - 9.0;
    let n = 64usize;
    let filled = (pct.clamp(0.0, 100.0) / 100.0 * n as f32) as usize;
    let start = -std::f32::consts::FRAC_PI_2;
    for i in 0..n {
        let a0 = start + (i as f32) / n as f32 * std::f32::consts::TAU;
        let a1 = start + ((i + 1) as f32) / n as f32 * std::f32::consts::TAU;
        let color = if i < filled {
            lerp_color(
                ACCENT,
                SUCCESS,
                i as f32 / (n.saturating_sub(1) as f32).max(1.0),
            )
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 26)
        };
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(c + egui::vec2(a0.cos(), a0.sin()) * r_in, color);
        mesh.colored_vertex(c + egui::vec2(a0.cos(), a0.sin()) * r_out, color);
        mesh.colored_vertex(c + egui::vec2(a1.cos(), a1.sin()) * r_out, color);
        mesh.colored_vertex(c + egui::vec2(a1.cos(), a1.sin()) * r_in, color);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
    }
    painter.text(
        c + egui::vec2(0.0, -6.0),
        egui::Align2::CENTER_CENTER,
        big,
        egui::FontId::proportional(22.0),
        TEXT,
    );
    painter.text(
        c + egui::vec2(0.0, 16.0),
        egui::Align2::CENTER_CENTER,
        sub,
        egui::FontId::proportional(11.0),
        MUTED,
    );
}

/// 关于页图标：ico 解码结果缓存一次（OnceLock），避免每帧重复解析
fn about_icon() -> Option<std::sync::Arc<egui::ColorImage>> {
    static ICON: std::sync::OnceLock<Option<std::sync::Arc<egui::ColorImage>>> =
        std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        load_window_icon().map(|ic| {
            std::sync::Arc::new(egui::ColorImage::from_rgba_unmultiplied(
                [ic.width as usize, ic.height as usize],
                &ic.rgba,
            ))
        })
    })
    .clone()
}

/// 在 content 区渲染一个带标题的分组卡片：左侧品牌色竖条 + 主文字色标题，下方半透明圆角卡片。
fn section<R>(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 15.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 1.5, ACCENT);
        ui.add_space(7.0);
        ui.label(egui::RichText::new(title).size(13.0).strong().color(TEXT));
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
    ui.label(egui::RichText::new(label).size(12.5).strong().color(TEXT));
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
    /// 上一次打包耗时（秒），打包结束时写入，仪表盘「最近耗时」指标用
    last_secs: Option<f64>,
    /// 上一帧打包中状态（true→false 下降沿时结算 last_secs）
    prev_running: bool,
}

impl PackToolApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 安装中文字体，修复中文豆腐块（乱码）显示
        install_fonts(&cc.egui_ctx);
        // 深色主题对齐 launcherclient（src-egui/src/theme.rs 色板）
        apply_theme(&cc.egui_ctx);
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
            last_secs: None,
            prev_running: false,
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
            push_log(
                &self.log,
                &format!("版本号格式无效: {}（示例 2.1.3）", version),
            );
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
        // 仪表盘右栏进度环 + 日志常驻，无需切页

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

    /// 仪表盘主屏（方案 E）：指标条 + 左栏表单 + 右栏进度环与最近日志
    fn page_config(&mut self, ui: &mut egui::Ui) {
        let running = self.running.try_lock().map(|v| *v).unwrap_or(false);
        let prog = match self.progress.try_lock() {
            Ok(g) => g.clone(),
            Err(_) => None,
        };

        // ---- 指标条：版本号 / 配置项 / 下载前缀 / 最近耗时 ----
        let metrics: [(&str, String, bool); 4] = [
            ("版本号", self.version.clone(), true),
            ("配置项", format!("{} 项", self.config.files.len()), false),
            (
                "下载前缀",
                format!("{} 条", self.config.download_prefixes.len()),
                false,
            ),
            (
                "最近耗时",
                match self.last_secs {
                    Some(s) => format!("{:.1} s", s),
                    None => "—".into(),
                },
                false,
            ),
        ];
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 3.0 * 10.0) / 4.0;
            for (k, v, accent) in metrics {
                ui.allocate_ui(egui::vec2(w, 58.0), |ui| {
                    egui::Frame::default()
                        .fill(CARD_BG)
                        .stroke(egui::Stroke::new(1.0, BORDER))
                        .inner_margin(egui::Margin::symmetric(12, 9))
                        .corner_radius(8.0)
                        .show(ui, |ui| {
                            ui.set_min_width(w - 24.0);
                            ui.label(egui::RichText::new(k).size(11.0).color(MUTED));
                            ui.label(
                                egui::RichText::new(v)
                                    .size(17.0)
                                    .strong()
                                    .color(if accent { ACCENT } else { TEXT })
                                    .font(egui::FontId::monospace(17.0)),
                            );
                        });
                });
            }
        });
        ui.add_space(12.0);

        // ---- 两栏：左 = 表单（打包输入集中一屏），右 = 进度环 + 最近日志 ----
        // 注意：allocate_ui 会继承父 ui 的横向布局（horizontal 内子项全部横排居中），
        // 两栏必须显式指定 top_down，否则内容挤成一行。
        let h = ui.available_height();
        let right_w = 236.0;
        let top_down = egui::Layout::top_down(egui::Align::Min);
        ui.horizontal(|ui| {
            let left_w = (ui.available_width() - right_w - 14.0).max(280.0);
            ui.allocate_ui_with_layout(
                egui::vec2(left_w, h),
                top_down,
                |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
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
                                            ui.add_sized(
                                                egui::vec2(ui.available_width() - 58.0, 26.0),
                                                egui::TextEdit::singleline(
                                                    &mut self.config.output_dir,
                                                )
                                                .font(egui::FontId::monospace(13.0)),
                                            );
                                            if ui.button("浏览…").clicked() {
                                                if let Some(p) =
                                                    rfd::FileDialog::new().pick_folder()
                                                {
                                                    self.config.output_dir =
                                                        p.to_string_lossy().replace('\\', "/");
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
                                        ui.add(
                                            egui::DragValue::new(&mut self.config.volume_size_mb)
                                                .range(1..=2048),
                                        );
                                    },
                                );
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("保存配置")
                                                    .strong()
                                                    .color(ACCENT_TEXT),
                                            )
                                            .fill(ACCENT)
                                            .corner_radius(6.0)
                                            .min_size(egui::vec2(88.0, 28.0)),
                                        )
                                        .clicked()
                                    {
                                        match self.config.save() {
                                            Ok(_) => push_log(
                                                &self.log,
                                                "配置已保存到 packtool.json",
                                            ),
                                            Err(e) => push_log(
                                                &self.log,
                                                &format!("保存失败: {}", e),
                                            ),
                                        }
                                    }
                                    ui.label(
                                        egui::RichText::new("未保存的改动不会写进 packtool.json")
                                            .size(11.0)
                                            .color(MUTED),
                                    );
                                });
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
                                                self.version = suggest_version(
                                                    &self.config.output_dir,
                                                    &self.config.app_name,
                                                );
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
                                                .desired_rows(3)
                                                .desired_width(f32::INFINITY),
                                        );
                                    },
                                );
                            });
                            section(ui, "强制更新策略（写入 version.json）", |ui| {
                                field(
                                    ui,
                                    "最低允许版本",
                                    "min_version：低于此版本的客户端触发阻断式强制更新；留空=不约束",
                                    |ui| {
                                        ui.add_sized(
                                            egui::vec2(200.0, 26.0),
                                            egui::TextEdit::singleline(&mut self.config.min_version)
                                                .font(egui::FontId::monospace(13.0)),
                                        );
                                    },
                                );
                                field(
                                    ui,
                                    "强制更新",
                                    "force_update：勾选后所有客户端必须升级到本版本，不可跳过",
                                    |ui| {
                                        ui.checkbox(
                                            &mut self.config.force_update,
                                            "本次发布为强制更新",
                                        );
                                    },
                                );
                                field(
                                    ui,
                                    "应用商店地址",
                                    "store_url：强制更新界面「前往应用商店」入口；留空则仅显示「下载升级包」",
                                    |ui| {
                                        ui.add_sized(
                                            egui::vec2(ui.available_width(), 26.0),
                                            egui::TextEdit::singleline(&mut self.config.store_url)
                                                .font(egui::FontId::monospace(13.0)),
                                        );
                                    },
                                );
                            });
                        });
                },
            );

            // ---- 右栏：进度环 + 状态 + 最近日志（常驻，打包免切页） ----
            ui.allocate_ui_with_layout(egui::vec2(right_w, h), top_down, |ui| {
                egui::Frame::default()
                    .fill(CARD_BG)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .inner_margin(egui::Margin::symmetric(14, 14))
                    .corner_radius(8.0)
                    .show(ui, |ui| {
                        let pct = prog.as_ref().map(|p| p.percent()).unwrap_or(0.0);
                        let sub = match &prog {
                            Some(p) => STAGE_NAMES.get(p.stage).copied().unwrap_or(""),
                            None => {
                                if running {
                                    "准备中"
                                } else {
                                    "待打包"
                                }
                            }
                        };
                        let big = if running || prog.is_some() {
                            format!("{:.0}%", pct)
                        } else {
                            "—".into()
                        };
                        ui.vertical_centered(|ui| {
                            draw_progress_ring(ui, 118.0, pct, &big, sub);
                        });
                        ui.add_space(6.0);
                        if running {
                            ui.horizontal(|ui| {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let cancel_btn = egui::Button::new(
                                            egui::RichText::new("取消").color(MUTED),
                                        )
                                        .corner_radius(6.0)
                                        .stroke(egui::Stroke::new(1.0, BORDER))
                                        .min_size(egui::vec2(56.0, 26.0));
                                        if ui.add(cancel_btn).clicked() {
                                            self.cancel_pack();
                                        }
                                        ui.label(
                                            egui::RichText::new("打包中…")
                                                .size(12.0)
                                                .color(ACCENT)
                                                .strong(),
                                        );
                                    },
                                );
                            });
                        } else {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("点右上角「打包」开始")
                                        .size(11.5)
                                        .color(MUTED),
                                );
                            });
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("最近日志")
                                    .size(11.0)
                                    .color(MUTED)
                                    .strong(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("查看全部")
                                                    .size(10.5)
                                                    .color(ACCENT),
                                            )
                                            .fill(egui::Color32::TRANSPARENT)
                                            .corner_radius(4.0),
                                        )
                                        .clicked()
                                    {
                                        self.current_page = Page::Log;
                                    }
                                },
                            );
                        });
                        ui.add_space(4.0);
                        // 最新 3 条：try_lock 短暂取样，绝不阻塞 UI
                        let tail: Vec<String> = match self.log.try_lock() {
                            Ok(log) => {
                                let mut t: Vec<String> = log
                                    .iter()
                                    .rev()
                                    .take(3)
                                    .cloned()
                                    .collect();
                                t.reverse();
                                t
                            }
                            Err(_) => Vec::new(),
                        };
                        if tail.is_empty() {
                            ui.label(
                                egui::RichText::new("（暂无日志）").size(11.5).color(MUTED),
                            );
                        } else {
                            for (i, line) in tail.iter().enumerate() {
                                let last = i + 1 == tail.len();
                                let text = if line.chars().count() > 30 {
                                    let cut: String = line.chars().take(30).collect();
                                    format!("{}…", cut)
                                } else {
                                    line.clone()
                                };
                                ui.label(
                                    egui::RichText::new(text)
                                        .size(11.5)
                                        .font(egui::FontId::monospace(11.5))
                                        .color(if last && running { ACCENT } else { MUTED }),
                                );
                            }
                        }
                    });
            });
        });
    }

    /// 历史构建页：输出目录内已有产物（按名称倒序 = 版本由新到旧）
    fn page_history(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            egui::RichText::new("历史构建")
                .size(18.0)
                .strong()
                .color(TEXT),
        );
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
                ui.label(
                    egui::RichText::new("还没有构建记录")
                        .size(13.0)
                        .color(MUTED),
                );
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
                            .color(TEXT)
                            .font(egui::FontId::monospace(13.0)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.allocate_ui(egui::vec2(76.0, 18.0), |ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(&h.size)
                                            .size(12.0)
                                            .color(MUTED)
                                            .font(egui::FontId::monospace(12.0)),
                                    );
                                },
                            );
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
        ui.heading(
            egui::RichText::new("打包文件")
                .size(18.0)
                .strong()
                .color(TEXT),
        );
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
                        self.config
                            .files
                            .push(p.to_string_lossy().replace('\\', "/"));
                    }
                }
                if ui.button("+ 目录").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.config
                            .files
                            .push(p.to_string_lossy().replace('\\', "/"));
                    }
                }
            });
        });
    }

    /// 下载与版本页：前缀 + 版本号 + 更新日志
    fn page_version(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            egui::RichText::new("下载与版本")
                .size(18.0)
                .strong()
                .color(TEXT),
        );
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
        // 版本与更新日志已并入仪表盘主屏（方案 E：打包输入集中一屏）
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
                ui.label(
                    egui::RichText::new("正在打包")
                        .size(14.0)
                        .strong()
                        .color(TEXT),
                );
                ui.label(egui::RichText::new(&title_desc).size(11.0).color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cancel_btn = egui::Button::new(egui::RichText::new("取消").color(MUTED))
                        .corner_radius(6.0)
                        .stroke(egui::Stroke::new(1.0, BORDER))
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
            ui.horizontal(|ui| {
                let w = ui.available_width() - 46.0;
                progress_track(ui, w, pct);
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!("{:.0}%", pct))
                        .size(12.0)
                        .color(ACCENT)
                        .strong(),
                );
            });
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
                        .color(TEXT),
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
                        Some(p) if i < p.stage => (SUCCESS, false),
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
        ui.heading(egui::RichText::new("日志").size(18.0).strong().color(TEXT));
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
                                    ui.label(
                                        egui::RichText::new("（暂无日志）").size(12.0).color(MUTED),
                                    );
                                } else {
                                    for line in log.iter() {
                                        ui.monospace(line);
                                    }
                                }
                            }
                            Err(_) => {
                                ui.label(
                                    egui::RichText::new("（日志正被写入，稍后刷新）")
                                        .size(12.0)
                                        .color(MUTED),
                                );
                            }
                        }
                    });
            });
    }

    /// 关于页：头部英雄卡（图标 + 名称 + 版本徽标 + 元信息）+ 双栏（应用信息 | 第三方依赖）。
    /// 版本取自 package.json（PKG_VERSION，build.rs 注入），其余项目信息取自 Cargo.toml [package]。
    fn page_about(&self, ui: &mut egui::Ui) {
        ui.heading(egui::RichText::new("关于").size(18.0).strong().color(TEXT));
        ui.add_space(4.0);

        // ---- 头部英雄卡：图标 | 名称+版本徽标 / 元信息 / 主页链接 ----
        egui::Frame::default()
            .fill(CARD_BG)
            .stroke(egui::Stroke::new(1.0, BORDER))
            .inner_margin(egui::Margin::symmetric(16, 14))
            .corner_radius(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(img) = about_icon() {
                        let tex = ui.ctx().load_texture_cached(
                            "about_icon",
                            ui.scope_id().with(egui::IdSalt::new("about_icon")),
                            img,
                            egui::TextureOptions::LINEAR,
                        );
                        ui.add(egui::Image::new(&tex).max_size(egui::vec2(56.0, 56.0)));
                        ui.add_space(10.0);
                    }
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("QsLtd 更新包打包工具")
                                    .size(17.0)
                                    .strong()
                                    .color(TEXT),
                            );
                            ui.add_space(8.0);
                            // 版本徽标：蓝底圆角，宽度按文本实测
                            let txt = format!("v{}", env!("PKG_VERSION"));
                            let w = ui
                                .painter()
                                .layout_no_wrap(
                                    txt.clone(),
                                    egui::FontId::proportional(12.0),
                                    ACCENT,
                                )
                                .size()
                                .x
                                + 16.0;
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(w, 20.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect.shrink(1.0), 5.0, ACCENT_SOFT);
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                &txt,
                                egui::FontId::proportional(12.0),
                                ACCENT,
                            );
                        });
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "{} · {} · {}",
                                env!("CARGO_PKG_NAME"),
                                env!("CARGO_PKG_LICENSE"),
                                env!("CARGO_PKG_AUTHORS")
                            ))
                            .size(12.0)
                            .color(MUTED),
                        );
                        ui.add_space(2.0);
                        ui.hyperlink(env!("CARGO_PKG_HOMEPAGE"));
                    });
                });
            });
        ui.add_space(18.0);

        // ---- 双栏：左 = 应用信息，右 = 第三方依赖 ----
        // 注意：allocate_ui 会继承父布局，horizontal 内必须显式 top_down（配置页同款坑）
        let deps: [(&str, &str, &str); 13] = [
            ("eframe", "0.37", "MIT OR Apache-2.0"),
            ("egui", "0.37", "MIT OR Apache-2.0"),
            ("zip", "2", "MIT"),
            ("sha2", "0.10", "MIT OR Apache-2.0"),
            ("hex", "0.4", "MIT OR Apache-2.0"),
            ("serde", "1", "MIT OR Apache-2.0"),
            ("serde_json", "1", "MIT OR Apache-2.0"),
            ("chrono", "0.4", "MIT OR Apache-2.0"),
            ("walkdir", "2", "Unlicense OR MIT"),
            ("regex", "1", "MIT OR Apache-2.0"),
            ("rfd", "0.14", "MIT"),
            ("ico", "0.4", "MIT"),
            ("embed-resource", "3", "MIT"),
        ];
        let top_down = egui::Layout::top_down(egui::Align::Min);
        let h = ui.available_height();
        ui.horizontal(|ui| {
            let left_w = 320.0;
            let right_w = ui.available_width() - left_w - 14.0;
            ui.allocate_ui_with_layout(egui::vec2(left_w, h), top_down, |ui| {
                section(ui, "应用信息", |ui| {
                    let rows: [(&str, &str); 4] = [
                        ("名称", env!("CARGO_PKG_NAME")),
                        ("版本", env!("PKG_VERSION")),
                        ("许可证", env!("CARGO_PKG_LICENSE")),
                        ("描述", env!("CARGO_PKG_DESCRIPTION")),
                    ];
                    for (k, v) in rows {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [64.0, 20.0],
                                egui::Label::new(egui::RichText::new(k).size(12.5).color(MUTED)),
                            );
                            if k == "描述" {
                                // 长文本自动换行，与标签顶对齐
                                ui.vertical(|ui| {
                                    ui.add_space(2.0);
                                    ui.label(egui::RichText::new(v).size(12.5).color(TEXT));
                                });
                            } else {
                                ui.label(egui::RichText::new(v).size(12.5).color(TEXT));
                            }
                        });
                        ui.add_space(6.0);
                    }
                });
            });
            ui.add_space(14.0);
            ui.allocate_ui_with_layout(egui::vec2(right_w, h), top_down, |ui| {
                // 卡片内固定高度滚动：13 行依赖不再溢出窗口底部
                // （section 头部 ≈23 + 间距 8 + 卡片上下内边距 24 + 尾部 18 ≈ 73）
                let list_h = (h - 74.0).max(140.0);
                section(ui, "第三方依赖", |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(list_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (name, ver, lic) in deps {
                                ui.horizontal(|ui| {
                                    // 名称列固定宽：许可证列纵向对齐
                                    ui.add_sized(
                                        [120.0, 20.0],
                                        egui::Hyperlink::from_label_and_url(
                                            egui::RichText::new(format!("{name} {ver}")).size(12.5),
                                            format!("https://crates.io/crates/{name}"),
                                        ),
                                    );
                                    ui.label(egui::RichText::new(lic).size(12.5).color(MUTED));
                                });
                                ui.add_space(5.0);
                            }
                        });
                });
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
        // 打包结束（true→false 下降沿）：结算仪表盘「最近耗时」指标
        if self.prev_running && !running {
            self.last_secs = self.pack_started.map(|t| t.elapsed().as_secs_f64());
            // 版本号跟随输出目录实时同步：pack_core 已先写入新 version.json（成功时），
            // 重算建议版本 = 刚打的版本末位 +1；失败/取消时 version.json 未变，
            // 同步回本次尝试的版本号，便于直接重试。
            self.version = suggest_version(&self.config.output_dir, &self.config.app_name);
        }
        self.prev_running = running;

        // 全窗口自绘渐变背景 + 自定义标题栏（对齐 launcherclient 无边框风格）
        let full = ui.max_rect();
        paint_bg(ui.painter(), full);
        let tb = draw_titlebar(ui, "QsLtd 更新包打包工具");
        match tb {
            TitleBarAction::Minimize => {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            TitleBarAction::Close => {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            TitleBarAction::None => {}
        }

        egui::CentralPanel::default().show(ui, |ui| {
            // 顶部行：页面标签（替代左导航，方案 E 顶栏）+ 常驻打包按钮（右）
            ui.horizontal(|ui| {
                for p in [
                    Page::Config,
                    Page::Files,
                    Page::History,
                    Page::Version,
                    Page::Log,
                    Page::About,
                ] {
                    let active = self.current_page == p;
                    let resp = ui.add(
                        egui::Button::new(
                            egui::RichText::new(p.title())
                                .size(12.5)
                                .color(if active { ACCENT } else { MUTED })
                                .strong(),
                        )
                        .fill(if active {
                            ACCENT_SOFT
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .stroke(if active {
                            egui::Stroke::new(1.0, tint(ACCENT, 80))
                        } else {
                            egui::Stroke::NONE
                        })
                        .corner_radius(6.0)
                        .min_size(egui::vec2(0.0, 28.0)),
                    );
                    if resp.clicked() {
                        self.current_page = p;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if running {
                        ui.spinner();
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("打包中…").color(ACCENT).strong());
                        ui.add_space(8.0);
                    }
                    let btn =
                        egui::Button::new(egui::RichText::new("打包").strong().color(ACCENT_TEXT))
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
            ui.add_space(8.0);

            // 内容区：仪表盘主屏自带两栏布局，其余页面整页滚动
            if self.current_page == Page::Config {
                self.page_config(ui);
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.current_page {
                        Page::Files => self.page_files(ui),
                        Page::History => self.page_history(ui),
                        Page::Version => self.page_version(ui),
                        Page::Log => self.page_log(ui),
                        Page::About => self.page_about(ui),
                        Page::Config => unreachable!(),
                    });
            }
        });
    }
}

/// 把 `assets/icon.ico` 编进二进制并解码为 egui::IconData，用于窗口/任务栏图标。
/// 用 `include_bytes!` 直接内嵌，避免运行时依赖外部文件；取尺寸最大的那一帧。
fn load_window_icon() -> Option<egui::IconData> {
    const ICON_BYTES: &[u8] = include_bytes!("../assets/icon.ico");
    let cursor = std::io::Cursor::new(ICON_BYTES);
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
    // 无边框窗口：标题栏由 draw_titlebar 自绘（对齐 launcherclient），
    // 故禁用系统缩放，尺寸固定
    let viewport = egui::ViewportBuilder::default()
        .with_title("QsLtd 更新包打包工具")
        .with_inner_size([760.0, 680.0])
        .with_resizable(false)
        .with_decorations(false);
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
            min_version: String::new(),
            force_update: false,
            store_url: String::new(),
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
        assert!(
            stages.contains(&0) && stages.contains(&4),
            "进度应覆盖首尾阶段: {:?}",
            stages
        );
        assert!(
            stages.windows(2).all(|w| w[0] <= w[1]),
            "阶段下标不应回退: {:?}",
            stages
        );

        // version.json 字段校验
        let vf = out.join("version.json");
        assert!(vf.is_file(), "version.json 未生成");
        let j: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&vf).unwrap()).unwrap();
        assert_eq!(j["version"], "1.2.3");
        assert_eq!(j["update"], true);
        assert!(j["size_bytes"].as_u64().unwrap() > 0);
        assert_eq!(
            j["url"],
            "https://example.com/update/testapp/testapp_1.2.3.zip"
        );

        let zip_name = "testapp_1.2.3.zip";
        let zip_path = out.join(zip_name);
        // 分卷模式：完整包被删除，分卷存在
        assert!(!zip_path.is_file(), "超限后完整包应被删除");
        let part1 = out.join(format!("{}.001", zip_name));
        let part2 = out.join(format!("{}.002", zip_name));
        assert!(
            part1.is_file() && part2.is_file(),
            "应切分为 .001/.002 分卷"
        );

        let parts = j["parts"].as_array().unwrap();
        let phashes = j["part_sha256"].as_array().unwrap();
        assert_eq!(parts.len(), phashes.len());
        assert_eq!(parts.len(), 2);
        assert_eq!(
            parts[0],
            "https://example.com/update/testapp/testapp_1.2.3.zip.001"
        );

        // 合并分卷还原后 sha256 应与 version.json 的 sha256 一致
        let mut merged = Vec::new();
        let mut f1 = std::fs::File::open(&part1).unwrap();
        std::io::Read::read_to_end(&mut f1, &mut merged).unwrap();
        let mut f2 = std::fs::File::open(&part2).unwrap();
        std::io::Read::read_to_end(&mut f2, &mut merged).unwrap();
        let merged_sha = hex::encode(Sha256::digest(&merged));
        assert_eq!(
            merged_sha,
            j["sha256"].as_str().unwrap(),
            "合并后 sha256 应与 version.json 一致"
        );

        // changelog 解析
        let cl = j["changelog"].as_array().unwrap();
        assert_eq!(cl.len(), 2);
        assert_eq!(cl[0], "修复了某些bugs");
        assert_eq!(cl[1], "新增下载前缀配置");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 构造最小可打包工程并跑一次 pack_core，返回解析后的 version.json
    fn pack_once(
        base_tag: &str,
        min_version: &str,
        force: bool,
        store_url: &str,
    ) -> serde_json::Value {
        let base = tmpdir(base_tag);
        let src = base.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        let out = base.join("out");
        let cfg = Config {
            app_name: "forceapp".into(),
            files: vec![src.join("a.txt").to_string_lossy().into()],
            download_prefixes: vec!["https://example.com/u/".into()],
            volume_size_mb: 64,
            output_dir: out.to_string_lossy().into(),
            min_version: min_version.into(),
            force_update: force,
            store_url: store_url.into(),
        };
        let res = pack_core(
            &cfg,
            "2.0.0",
            "t",
            |_s: &str| {},
            |_p: Progress| {},
            &AtomicBool::new(false),
        );
        assert!(res.is_ok(), "pack_core 失败: {:?}", res.err());
        let text = std::fs::read_to_string(out.join("version.json")).unwrap();
        let j: serde_json::Value = serde_json::from_str(&text).unwrap();
        let _ = std::fs::remove_dir_all(&base);
        j
    }

    #[test]
    fn pack_core_writes_force_update_fields() {
        // 三个服务端字段都应写入生成的 manifest
        let j = pack_once("force", "1.0.0", true, "https://example.com/store");
        assert_eq!(j["force_update"].as_bool(), Some(true));
        assert_eq!(j["min_version"].as_str(), Some("1.0.0"));
        assert_eq!(j["store_url"].as_str(), Some("https://example.com/store"));
    }

    #[test]
    fn pack_core_omits_blank_force_fields() {
        // force_update 恒写入（未勾选为 false）；min_version / store_url 留空则省略
        let j = pack_once("blank", "", false, "");
        assert_eq!(j["force_update"].as_bool(), Some(false));
        assert!(j.get("min_version").is_none(), "min_version 留空不应写入");
        assert!(j.get("store_url").is_none(), "store_url 留空不应写入");
    }

    #[test]
    fn bump_and_suggest() {
        assert_eq!(bump_version("1.2.3"), Some("1.2.4".into()));
        assert_eq!(bump_version("2.0"), None);
        let base = tmpdir("ver");
        let out = base.join("o");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("version.json"), r#"{"version":"3.1.9"}"#).unwrap();
        assert_eq!(
            suggest_version(out.to_string_lossy().as_ref(), "qsm"),
            "3.1.10"
        );
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
            min_version: String::new(),
            force_update: false,
            store_url: String::new(),
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
            min_version: String::new(),
            force_update: false,
            store_url: String::new(),
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
        assert!(
            icon.rgba.len() == icon.width as usize * icon.height as usize * 4,
            "rgba 字节数应等于 width*height*4"
        );
        assert!(icon.width > 0 && icon.height > 0);
    }
}
