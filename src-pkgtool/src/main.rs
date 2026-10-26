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
use std::sync::{Arc, Mutex};

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

/// 执行完整打包流程（纯逻辑，日志写入传入的 `log` 缓冲，便于单测）
/// 返回成功摘要字符串，错误带原因
fn pack_core(
    cfg: &Config,
    version: &str,
    changelog_input: &str,
    log: &mut Vec<String>,
) -> Result<String, String> {
    let mut logln = |s: &str| log.push(s.to_string());

    // 1. 展开配置项为实际文件清单
    let mut resolved: Vec<(PathBuf, String)> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for entry in &cfg.files {
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
        for (src, arc) in &resolved {
            zip.start_file(arc, options)
                .map_err(|e| format!("写入 {} 失败: {}", arc, e))?;
            let mut f = fs::File::open(src).map_err(|e| format!("打开 {} 失败: {}", src.display(), e))?;
            std::io::copy(&mut f, &mut zip).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| format!("zip 收尾失败: {}", e))?;
    }

    // 3. 完整包大小与 sha256（分卷合并后客户端整体校验用）
    let size_bytes = fs::metadata(&zip_path).map_err(|e| e.to_string())?.len();
    let digest = sha256_file(&zip_path)?;

    // 4. 超限切分分卷
    let volume_size = cfg.volume_size_mb * 1024 * 1024;
    let parts = if size_bytes > volume_size {
        logln(&format!(
            "包体 {} 字节超过单卷上限 {} MB，正在切分分卷:",
            size_bytes, cfg.volume_size_mb
        ));
        let ps = split_zip(&zip_path, volume_size)?;
        for (p, h) in &ps {
            logln(&format!("  {} ({} 字节, sha256 {})", p.display(), p.metadata().map(|m| m.len()).unwrap_or(0), &h[..16]));
        }
        fs::remove_file(&zip_path)
            .map_err(|e| format!("删除完整包失败: {}", e))?;
        ps
    } else {
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
    let release_date = Utc::now().format("%Y-%m-%dT%H:%M:%S.000Z").to_string();
    let version_files: Vec<PathBuf> = std::iter::once(output_dir.join("version.json"))
        .chain((2..=cfg.download_prefixes.len()).map(|i| output_dir.join(format!("version_{}.json", i))))
        .collect();

    for (prefix, vfile) in cfg.download_prefixes.iter().zip(version_files.iter()) {
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
    let deleted = clean_old_packages(output_dir, &cfg.app_name, &zip_name);
    for name in &deleted {
        logln(&format!("已删除旧包: {}", name));
    }

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

/// 打包工具应用状态
struct PackToolApp {
    config: Config,
    version: String,
    changelog: String,
    log: Arc<Mutex<Vec<String>>>,
    running: Arc<Mutex<bool>>,
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
        }
    }

    /// 触发打包（后台线程执行，避免 UI 卡顿）
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
            self.log
                .lock()
                .unwrap()
                .push(format!("版本号格式无效: {}（示例 2.1.3）", version));
            return;
        }
        let cfg = self.config.clone();
        let changelog = self.changelog.clone();
        let log = self.log.clone();
        let running = self.running.clone();
        *running.lock().unwrap() = true;
        std::thread::spawn(move || {
            let mut buf = log.lock().unwrap();
            let res = pack_core(&cfg, &version, &changelog, &mut buf);
            match res {
                Ok(_) => {}
                Err(e) => buf.push(format!("❌ 打包失败: {}", e)),
            }
            drop(buf);
            *running.lock().unwrap() = false;
            ctx.request_repaint();
        });
    }
}

impl eframe::App for PackToolApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("QsLtd 更新包打包工具");
            ui.label("versionTools.py 的纯 Rust GUI 版 · 配置存于 packtool.json");

            egui::ScrollArea::vertical().show(ui, |ui| {
                // ---- 配置区 ----
                ui.collapsing("配置（FILES / 前缀 / 输出目录）", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("应用名:");
                        ui.text_edit_singleline(&mut self.config.app_name);
                        ui.label("用于 {app}_{version}.zip 与旧包清理匹配");
                    });
                    ui.horizontal(|ui| {
                        ui.label("输出目录:");
                        ui.text_edit_singleline(&mut self.config.output_dir);
                        if ui.button("浏览").clicked() {
                            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                self.config.output_dir =
                                    p.to_string_lossy().replace('\\', "/");
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("单卷大小(MB):");
                        ui.add(egui::DragValue::new(&mut self.config.volume_size_mb).range(1..=2048));
                        ui.label("包体超过则切分 .001/.002 分卷");
                    });

                    ui.separator();
                    ui.label("待打包文件 / 目录（目录以 /* 结尾表示内容平铺到 zip 根）:");
                    let mut remove_file: Option<usize> = None;
                    for (i, f) in self.config.files.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(f);
                            if ui.button("删除").clicked() {
                                remove_file = Some(i);
                            }
                        });
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

                    ui.separator();
                    ui.label("下载前缀（每个前缀各生成一份 version 文件）:");
                    let mut remove_pre: Option<usize> = None;
                    for (i, p) in self.config.download_prefixes.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(p);
                            if ui.button("删除").clicked() {
                                remove_pre = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove_pre {
                        self.config.download_prefixes.remove(i);
                    }
                    if ui.button("+ 添加前缀").clicked() {
                        self.config.download_prefixes.push(String::new());
                    }

                    ui.separator();
                    if ui.button("保存配置").clicked() {
                        match self.config.save() {
                            Ok(_) => self.log.lock().unwrap().push("配置已保存到 packtool.json".into()),
                            Err(e) => self.log.lock().unwrap().push(format!("保存失败: {}", e)),
                        }
                    }
                });

                // ---- 版本号 ----
                ui.horizontal(|ui| {
                    ui.label("版本号:");
                    ui.text_edit_singleline(&mut self.version);
                    if ui.button("↻ 取默认（末位+1）").clicked() {
                        self.version = suggest_version(&self.config.output_dir, &self.config.app_name);
                    }
                });

                // ---- 更新日志 ----
                ui.label("更新日志（多条用 ; 或换行分隔；留空则沿用上一版）:");
                ui.text_edit_multiline(&mut self.changelog);

                // ---- 操作 ----
                ui.horizontal(|ui| {
                    let running = *self.running.lock().unwrap();
                    if ui.button("打包").clicked() && !running {
                        self.start_pack(ui.ctx().clone());
                    }
                    if running {
                        ui.label("打包中…");
                    }
                    if ui.button("清空日志").clicked() {
                        self.log.lock().unwrap().clear();
                    }
                });

                // ---- 日志 ----
                ui.separator();
                ui.label("日志");
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        let log = self.log.lock().unwrap();
                        for line in log.iter() {
                            ui.monospace(line);
                        }
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
        let res = pack_core(&cfg, "1.2.3", "修复了某些bugs;新增下载前缀配置", &mut log);
        assert!(res.is_ok(), "pack_core 失败: {:?}", log);

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

    #[test]
    fn window_icon_from_ico() {
        // 验证 icon.ico 经 include_bytes! + ico 解码得到有效的窗口/任务栏图标
        let icon = load_window_icon().expect("icon.ico 应能被解码为 IconData");
        assert!(icon.rgba.len() == icon.width as usize * icon.height as usize * 4,
            "rgba 字节数应等于 width*height*4");
        assert!(icon.width > 0 && icon.height > 0);
    }
}
