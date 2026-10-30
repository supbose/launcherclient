//! 更新流程编排（UI 无关）：读取配置 → 检查 → 下载（SHA256 校验）→ 安装 →
//! 拉起目标程序。
//!
//! 从 `src-tauri/src/updater.rs`（Tauri 版，已于 2026-10-26 删除）与 `src-zsui/src/updater.rs`
//! （zsui 版，亦已于 2026-10-26 删除）的公共部分抽离：
//! 两版编排层的「窗口前启动决策 + 下载安装全流程」逻辑一致，仅阶段推送
//! 通道不同（Tauri 事件 emit / 原生 UI 共享快照）。本模块统一走
//! [`PhaseHandle`] 共享快照：
//! - egui 版（src-egui，当前唯一生产壳）：UI 线程每帧轮询快照渲染
//! - 历史上 Tauri 版（src-tauri，已删除）：原走「快照变更 → emit 事件」通道，载荷
//!   结构（[`PhasePayload`]）保持一致
//!
//! 窗口可见性策略（早期 zsui 版亦采用此「先决策后建窗」策略）：
//! - 无更新 + launch 模式：静默拉起目标程序退出，进程从未创建窗口
//! - 发现新版本 / 出错：创建窗口显示对应阶段，下载安装由后台线程执行
//! - `--check-only`：不建窗，输出结果后按退出码结束

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::cleanup;
use crate::cli::CliArgs;
use crate::config;
use crate::defaults::{
    DEFAULT_APP_NAME, DEFAULT_HOMEPAGE, DEFAULT_MANIFEST_URLS, PROGRESS_INTERVAL_MS,
};
use crate::disk;
use crate::download;
use crate::elevation;
use crate::install;
use crate::local_manifest::LocalManifest;
use crate::manifest;
use crate::phase::{self, fmt_mb, PhaseHandle, PhasePayload, Stage};
use crate::services;

/// 配置合并后的生效配置（优先级：命令行 > update.json > 默认值）
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub app_name: String,
    /// 启动器标题（标题栏左侧文字 + 系统窗口标题）：命令行 --name > update.json name > 烘焙默认
    pub launcher_name: String,
    /// manifest 地址列表（按序故障转移）
    pub manifest_urls: Vec<String>,
    pub launch: Option<String>,
    /// 保留的版本目录数量（含当前版本）
    pub max_versions: usize,
    /// 更新前需卸载的 Windows 服务名列表（运行中先停止；卸载后不恢复不重启）
    pub services: Vec<String>,
    /// 生效的启动器界面模板（F 主 / H 副）
    pub template: String,
    /// 官网首页地址：下载地址全挂时错误态「前往官网」按钮的目标
    pub homepage: String,
    /// 同一环节自动重试次数（含首次尝试）：下载、解压校验各自独立计数
    pub retry_times: u32,
    /// 是否要求清单提供包完整性校验值（默认 true）。
    /// 为真且清单既没给整包 `sha256`、分卷也没给齐 `part_sha256` 时，
    /// [`run_update_flow`] 在下载前直接拒绝自动安装；关它的两条路见
    /// [`crate::config::resolve_require_sha256`]。
    pub require_sha256: bool,
    /// 分卷并发下载数（1~8，默认 3）：CDN 单连接限速时并发能明显提速
    pub parts_concurrency: usize,
    /// 检查更新失败（网络/服务端不可达）时是否直接启动已安装版本（默认 false）
    pub fail_open: bool,
}

/// 启动决策：窗口创建前确定是否需要 UI
pub enum StartupDecision {
    /// 无需 UI：静默处理完毕，进程按退出码结束
    SilentExit(i32),
    /// 需要 UI：携带启动上下文（Box 避免枚举尺寸被上下文撑大）
    Show(Box<StartupContext>),
}

/// 需要 UI 时的启动上下文
pub struct StartupContext {
    pub cfg: ResolvedConfig,
    /// 服务器 manifest（Some 表示发现新版本，需要后台执行下载流程）
    pub manifest: Option<manifest::FlatManifest>,
    /// UI 首帧渲染的初始阶段快照
    pub initial: PhasePayload,
}

/// 过滤空字符串配置项（update.json 中 "" 视为未配置）
fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

/// 归一应用名：非法值回退内置默认名并留日志
///
/// 应用名会拼进 `%APPDATA%\<name>`、下载缓存、版本目录等全部落盘路径，
/// 校验规则见 [`crate::appdir::validate_app_name`]。
fn normalize_app_name(raw: String) -> String {
    match crate::appdir::validate_app_name(&raw) {
        Ok(()) => raw,
        Err(e) => {
            eprintln!(
                "[配置] 应用名不可用（{}），回退默认值 {}",
                e, DEFAULT_APP_NAME
            );
            DEFAULT_APP_NAME.to_string()
        }
    }
}

/// 合并命令行参数与 update.json 配置
pub fn resolve_config(args: &CliArgs) -> ResolvedConfig {
    let settings = config::load();

    let app_name = if args.app_name != "QsLtd" {
        // 命令行显式指定了应用名
        args.app_name.clone()
    } else {
        non_empty(settings.app_name.clone()).unwrap_or_else(|| DEFAULT_APP_NAME.to_string())
    };

    // 应用名直接拼进 %APPDATA%\<name> 等路径：非法值（路径分隔符、盘符、
    // `..` 等）不能带着写入任意位置，这里显式回退并留日志，而不是等到
    // 建目录/解压时才出现难以理解的行为。
    let app_name = normalize_app_name(app_name);

    // 界面模板优先级：命令行 --template > update.json launcher_template > 自动判定。
    // 自动判定按启动方式：带参（视作被其他应用调用）用副模板 H，无参直启用主模板 F；
    // 未配置或写了旧版 A~E/G 等值时 resolve 内部回落主模板 F，界面不会白屏。
    let settings_template = settings.launcher_template();
    let template = config::resolve_launcher_template(
        args.template.as_deref().or(settings_template.as_deref()),
        args.has_user_arguments,
    );

    // 标题优先级同上：命令行 --name > update.json name > 编译期烘焙默认
    // （空串/全空白按未指定处理，规则集中在 resolve_launcher_name）
    let settings_name = settings.launcher_name();
    let launcher_name =
        config::resolve_launcher_name(args.launcher_name.as_deref(), settings_name.as_deref());

    ResolvedConfig {
        app_name,
        launcher_name,
        manifest_urls: args
            .manifest_url
            .clone()
            .map(|u| vec![u])
            .or_else(|| settings.manifest_url.as_ref().map(|m| m.to_urls()))
            .unwrap_or_else(|| {
                DEFAULT_MANIFEST_URLS
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            }),
        launch: args.launch.clone().or_else(|| non_empty(settings.launch)),
        template,
        // 配置写了空串当没写：回落到内置默认，按钮点了才有地方去
        homepage: non_empty(settings.homepage).unwrap_or_else(|| DEFAULT_HOMEPAGE.to_string()),
        retry_times: config::resolve_retry_times(settings.retry_times),
        // 命令行 --allow-missing-sha256 > update.json require_sha256 > 默认要求
        require_sha256: config::resolve_require_sha256(
            args.allow_missing_sha256,
            settings.require_sha256,
        ),
        max_versions: settings
            .max_versions
            .filter(|v| *v > 0)
            .unwrap_or(config::DEFAULT_MAX_VERSIONS),
        services: settings
            .services
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect(),
        parts_concurrency: config::resolve_parts_concurrency(settings.parts_concurrency),
        fail_open: config::resolve_fail_open(settings.fail_open),
    }
}

/// 按 app_name 派生应用数据根目录（`%APPDATA%\<app_name>`）
///
/// 所有派生路径（下载缓存、版本目录、app.json、manifest 缓存、单实例锁）
/// 都从这里长出来，集中一处好改；UI 层要取单实例锁也调它，别自己拼路径。
/// 解析规则（config.toml 接管、SYSTEM 账户降级）见 [`crate::appdir`]。
pub fn app_dir(app_name: &str) -> PathBuf {
    crate::appdir::app_dir(Some(app_name))
}

/// 按 app_name 派生下载目录
pub fn download_dir(app_name: &str) -> PathBuf {
    app_dir(app_name).join("downloads")
}

/// 按 app_name 派生版本根目录（各版本目录的父目录）
fn install_root(app_name: &str) -> PathBuf {
    app_dir(app_name).join("app")
}

/// 按 app_name 派生版本安装目录（app_{version}）
fn install_dir(app_name: &str, version: &str) -> PathBuf {
    install_root(app_name).join(format!("app_{}", version))
}

/// 本地版本记录 app.json 路径
fn local_manifest_path(app_name: &str) -> PathBuf {
    app_dir(app_name).join("app.json")
}

/// 清理本次更新的下载缓存：完整 zip + 分卷目录（安装成功后调用）
///
/// 失败仅告警不阻塞：缓存而已，重下即可，不影响已完成的安装。
/// 安装失败**不**清理——保留的包供重跑时复用（[`run_download`] 的
/// 本地校验复用路径）。
async fn cleanup_download_cache(app_name: &str, version: &str) {
    let dir = download_dir(app_name);
    let zip = dir.join(format!("update_{}.zip", version));
    let parts = dir.join(format!("update_{}_parts", version));

    if zip.exists() {
        match tokio::fs::remove_file(&zip).await {
            Ok(()) => eprintln!("[更新器] 已清理下载缓存: {}", zip.display()),
            Err(e) => eprintln!(
                "[更新器] 清理下载缓存失败（忽略）: {}（{}）",
                zip.display(),
                e
            ),
        }
    }
    if parts.exists() {
        match tokio::fs::remove_dir_all(&parts).await {
            Ok(()) => eprintln!("[更新器] 已清理分卷缓存: {}", parts.display()),
            Err(e) => eprintln!(
                "[更新器] 清理分卷缓存失败（忽略）: {}（{}）",
                parts.display(),
                e
            ),
        }
    }
}

/// --check-only 入口：合并配置 → 纯检测 → 返回退出码
///
/// 退出码：0 = 无更新，1 = 检测失败，2 = 有更新（可跳过），
/// 3 = 必须更新（服务端 manifest 设了 `min_version` 门槛 / `force_update` 标记置真）。
pub fn check_only_exit(args: &CliArgs) -> i32 {
    let cfg = resolve_config(args);
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt.block_on(run_check_only(args, &cfg)),
        Err(e) => {
            eprintln!("[更新器] 创建异步运行时失败: {}", e);
            1
        }
    }
}

/// --check-only 纯检测模式：只判断是否有更新，不建窗、不下载、不拉回目标程序。
pub async fn run_check_only(args: &CliArgs, cfg: &ResolvedConfig) -> i32 {
    // 先清理上次删除失败的残留目录
    let app_root = install_root(&cfg.app_name);
    cleanup::cleanup_pending(&app_root).await;

    let current_version = resolve_current_version(args, &cfg.app_name).await;
    eprintln!("[更新器] 当前版本: {}", current_version);

    eprintln!("[更新器] 开始拉取 manifest...");
    let m = match manifest::fetch_with_failover(&cfg.manifest_urls).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[更新器] 检查更新失败: {}", e);
            return 1;
        }
    };

    // 首次安装（0.0.0）：没有可运行的旧版本，「可跳过」没有意义，按必须更新处理
    if is_first_install(&current_version) && manifest::need_update(&current_version, &m) {
        eprintln!(
            "[更新器] 未检测到已安装版本，按强制更新处理：最新 {}",
            m.version
        );
        return 3;
    }

    if manifest::requires_force_update(&current_version, &m) {
        eprintln!(
            "[更新器] 检测到服务端强制更新要求：当前 {}，要求 {}",
            current_version,
            manifest::required_version(&m)
        );
        return 3;
    }

    if manifest::need_update(&current_version, &m) {
        eprintln!(
            "[更新器] 发现新版本（可跳过）: {} → {}",
            current_version, m.version
        );
        2
    } else {
        eprintln!("[更新器] 已是最新版本: {}", current_version);
        0
    }
}

/// 启动阶段（窗口创建前）：清理残留 → 解析当前版本 → 拉取 manifest → 更新判断
///
/// 返回 [`StartupDecision::Show`] 时，`initial` 已携带首帧阶段快照
/// （发现新版本 / 检测失败）；发现新版本时调用方随后调用
/// [`spawn_update_flow`] 启动后台下载流程。
pub async fn run_startup(args: &CliArgs, cfg: &ResolvedConfig) -> StartupDecision {
    // 启动 GC：恢复/清理上次中断安装的备份、解压半成品、原子写残留与过期下载包。
    // 放在进程最前面：既让「上次崩溃」在下一次启动就自愈，也避免这些
    // 中间产物长期占着磁盘（旧实现只清 `_del_`，`_bak_`/`_tmp` 无人管）。
    let app_root = install_root(&cfg.app_name);
    cleanup::cleanup_startup(&app_root, &download_dir(&cfg.app_name)).await;

    // 当前版本：命令行 > update.json > 本地 app.json > 0.0.0（视为未安装/损坏自恢复）
    let current_version = resolve_current_version(args, &cfg.app_name).await;
    eprintln!("[更新器] 当前版本: {}", current_version);

    // 旧版本目录按 max_versions 保留。清理从「安装收尾」挪到「启动」：
    // 删一个 GB 级旧目录要好几秒，不该压在「更新完成 → 拉起应用」之间；
    // 启动清理还能兜住历史上收尾失败留下的目录。
    if !is_first_install(&current_version) {
        cleanup::cleanup_old_versions(&app_root, &current_version, cfg.max_versions).await;
    }

    eprintln!("[更新器] 开始拉取 manifest...");
    let m = match manifest::fetch_with_failover(&cfg.manifest_urls).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[更新器] 检查更新失败: {}", e);
            // fail-open（opt-in）：弱网/服务端故障时，已安装版本仍可运行时
            // 不把用户挡在错误窗口后面。「仅本地安装有效」是硬条件——没有
            // 可启动的程序时放行只会得到一次启动失败。
            if cfg.fail_open {
                let installed = read_installed_path(&cfg.app_name).await;
                if should_fail_open(cfg, &current_version, installed.as_deref()) {
                    match launch_outcome(cfg, installed.as_deref().map(Path::new)) {
                        Ok(()) => {
                            eprintln!(
                                "[更新器] fail_open 已开启：无法检查更新，直接启动本地已安装版本"
                            );
                            return StartupDecision::SilentExit(0);
                        }
                        Err(detail) => eprintln!(
                            "[更新器] fail_open 未能启动本地版本（{}），保留错误界面",
                            detail
                        ),
                    }
                }
            }
            return StartupDecision::Show(Box::new(StartupContext {
                cfg: cfg.clone(),
                manifest: None,
                initial: PhasePayload::check_failure(&e),
            }));
        }
    };

    // 登记更新日志：此后所有阶段载荷都携带它，界面在发现新版本起到
    // 更新完成全程可见（下载/安装期间用户正好有空看更新了什么）
    phase::set_changelog(m.changelog.clone());

    // ---- 更新决策 ----
    //
    // 三种形态：
    // - 首次安装（本地 0.0.0 / app.json 缺失 / 安装目录丢失）：没有可运行的
    //   旧版本，「稍后升级」无处可去 → 直接进入下载安装，不展示选择界面。
    // - 服务端强制（`force_update` 或 `min_version`）：阻断式提示，必须升级。
    // - 其余有更新：非阻断提示，可稍后升级直接进应用。
    if !manifest::need_update(&current_version, &m) {
        eprintln!("[更新器] 已是最新版本: {}", current_version);
        // 拉起目标程序后静默退出；launch 失败时显示错误窗口让用户知道
        // 发生了什么（交接模式下旧客户端已退出，静默等于应用凭空消失）
        let installed = read_installed_path(&cfg.app_name).await;
        let outcome = launch_outcome(cfg, installed.as_deref().map(Path::new));
        match outcome {
            Ok(()) => StartupDecision::SilentExit(0),
            Err(e) => StartupDecision::Show(Box::new(StartupContext {
                cfg: cfg.clone(),
                manifest: None,
                initial: PhasePayload::new(Stage::Error, "启动失败", &e, None),
            })),
        }
    } else {
        update_available_decision(cfg, &current_version, m)
    }
}

/// 当前版本是否代表「尚未安装」
///
/// `resolve_current_version` 在 app.json 缺失、读取失败、或记录的安装目录
/// 已不存在时都返回 `0.0.0`（见其文档）。这类机器上没有可启动的程序，
/// 「稍后升级」只会得到一次启动失败，因此必须按强制安装处理。
fn is_first_install(current_version: &str) -> bool {
    current_version.trim() == "0.0.0"
}

/// 检查失败时是否放行启动（纯判定，便于单测）
///
/// 三个条件缺一不可：配置显式开启、本地有版本（0.0.0 视为未安装）、
/// 记录的安装目录真实存在。缺少任何一条都退回错误界面——fail-open 只服务
/// 「网络挂了但程序本来能跑」这一种场景，不是跳过更新检查的通用开关。
fn should_fail_open(cfg: &ResolvedConfig, current_version: &str, installed: Option<&str>) -> bool {
    cfg.fail_open
        && !is_first_install(current_version)
        && installed.is_some_and(|p| !p.trim().is_empty() && Path::new(p).exists())
}

/// 有可用更新时的启动决策（从 [`run_startup`] 抽出的纯决策，便于单测）
///
/// - 首次安装：首帧即 `Downloading`，由 UI 自动拉起更新流程（无选择界面）
/// - 服务端强制：阻断式 `force-update`，按钮驱动更新
/// - 其余：非阻断式 `force-update`，可稍后升级
fn update_available_decision(
    cfg: &ResolvedConfig,
    current_version: &str,
    m: manifest::FlatManifest,
) -> StartupDecision {
    let has_store = m.store_url.as_ref().is_some_and(|s| !s.trim().is_empty());

    if is_first_install(current_version) {
        eprintln!(
            "[更新器] 未检测到已安装版本（当前 {}），按首次安装直接下载安装：最新 {}",
            current_version, m.version
        );
        return StartupDecision::Show(Box::new(StartupContext {
            cfg: cfg.clone(),
            // 携带 manifest：UI 消费决策时据此自动启动下载流程
            manifest: Some(m),
            // 非 force-update 阶段：app.rs 会自动 spawn 更新流程，
            // 窗口直接呈现下载进度，不出现「稍后升级」
            initial: PhasePayload::new(
                Stage::Downloading,
                "正在下载更新包...",
                "首次安装，完成后将自动启动",
                None,
            ),
        }));
    }

    if manifest::requires_force_update(current_version, &m) {
        let required = manifest::required_version(&m);
        eprintln!(
            "[更新器] 触发服务端强制更新（阻断）：当前 {}，要求 {}",
            current_version, required
        );
        StartupDecision::Show(Box::new(StartupContext {
            cfg: cfg.clone(),
            // 保存 manifest：强制更新界面「下载升级包」按钮据此走更新流程
            manifest: Some(m),
            initial: PhasePayload::force_update(current_version, &required, true, has_store),
        }))
    } else {
        eprintln!(
            "[更新器] 发现新版本（可跳过）：当前 {}，最新 {}",
            current_version, m.version
        );
        let latest = m.version.clone();
        StartupDecision::Show(Box::new(StartupContext {
            cfg: cfg.clone(),
            // 保存 manifest：界面「下载升级包」据此走更新流程；
            // 「稍后升级」则启动当前已安装应用并退出（见 app.rs ForceLater）
            manifest: Some(m),
            initial: PhasePayload::force_update(current_version, &latest, false, has_store),
        }))
    }
}

/// 启动一次更新流程（首次运行与「重试」共用入口）
///
/// 运行中标志防并发：流程进行中再启动直接忽略（前端按钮会同时
/// disabled，双保险；脚本调用场景靠这里兜底）。
pub fn spawn_flow(args: CliArgs, handle: PhaseHandle) {
    if CHECK_RUNNING.swap(true, Ordering::SeqCst) {
        eprintln!("[更新器] 已有更新流程在运行，忽略本次启动");
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("update-flow".into())
        .spawn(move || {
            let cfg = resolve_config(&args);
            eprintln!(
                "[更新器] 启动，应用: {}，标题: {}，界面模板: {}，manifest 地址 {} 个: {}",
                cfg.app_name,
                cfg.launcher_name,
                cfg.template,
                cfg.manifest_urls.len(),
                cfg.manifest_urls.join(" → ")
            );

            let result = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt.block_on(run_check_flow(&args, &cfg, &handle)),
                Err(e) => Err(format!("创建异步运行时失败: {}", e)),
            };
            if let Err(e) = result {
                // 各失败点已把错误写回共享快照，这里只留运行日志
                eprintln!("[更新器] 更新流程失败: {}", e);
            }
            CHECK_RUNNING.store(false, Ordering::SeqCst);
        });
    if let Err(e) = spawned {
        eprintln!("[更新器] 后台更新线程启动失败: {}", e);
        CHECK_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// 启动后台更新线程（发现新版本后由 UI 侧调用，manifest 已由启动决策取得）
///
/// 与 [`spawn_flow`] 的差异：跳过重复的 manifest 拉取（[`run_startup`]
/// 已完成检查），直接执行「下载 → 校验 → 安装 → 收尾」流程。
pub fn spawn_flow_with(args: CliArgs, handle: PhaseHandle, m: manifest::FlatManifest) {
    if CHECK_RUNNING.swap(true, Ordering::SeqCst) {
        eprintln!("[更新器] 已有更新流程在运行，忽略本次启动");
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("update-flow".into())
        .spawn(move || {
            let cfg = resolve_config(&args);
            let result = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt.block_on(run_update_flow(&cfg, &m, &handle)),
                Err(e) => Err(format!("创建异步运行时失败: {}", e)),
            };
            if let Err(e) = result {
                // 各失败点已把错误写回共享快照，这里只留运行日志
                eprintln!("[更新器] 更新流程失败: {}", e);
            }
            CHECK_RUNNING.store(false, Ordering::SeqCst);
        });
    if let Err(e) = spawned {
        eprintln!("[更新器] 后台更新线程启动失败: {}", e);
        CHECK_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// 完整检查 + 更新流程（后台线程内执行）：
/// 检查 → 发现新版本则下载 → 校验 → 安装 → 记录 → 清理 → 拉起目标程序
///
/// 与 [`run_startup`] 的差异：本函数走完整流程（含 manifest TTL 缓存），
/// 供「重试」与错误态重跑使用；每个阶段都推送快照到 `handle`。
/// 拉起目标程序成功或失败后流程结束（成功时由调用方决定是否退出进程）。
async fn run_check_flow(
    args: &CliArgs,
    cfg: &ResolvedConfig,
    handle: &PhaseHandle,
) -> Result<(), String> {
    let current_version = resolve_current_version(args, &cfg.app_name).await;
    eprintln!("[更新器] 当前版本: {}", current_version);

    handle.set(PhasePayload::new(
        Stage::Checking,
        "正在检查更新...",
        "",
        None,
    ));

    // manifest 拉取走 TTL 缓存（5 分钟内重复启动免联网）；重试场景准确优先，
    // 与 Tauri 版一致：首跑用缓存，重试同样命中（5 分钟窗口内服务端无变化）
    let m = match manifest::fetch_with_cache(&cfg.manifest_urls, &cfg.app_name).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[更新器] 检查更新失败: {}", e);
            handle.set(PhasePayload::check_failure(&e));
            return Err(e);
        }
    };

    // 登记更新日志：此后所有阶段载荷都携带它（重试路径同样要带）
    phase::set_changelog(m.changelog.clone());

    if !manifest::need_update(&current_version, &m) {
        eprintln!("[更新器] 已是最新版本: {}", current_version);
        handle.set(PhasePayload::new(
            Stage::UpToDate,
            "已是最新版本",
            &current_version,
            None,
        ));
        // launch 模式拉起目标程序后由调用方退出；失败切错误态
        let installed = read_installed_path(&cfg.app_name).await;
        if let Err(e) = launch_outcome(cfg, installed.as_deref().map(Path::new)) {
            eprintln!("[更新器] 无更新但启动失败: {}", e);
            handle.set(PhasePayload::new(Stage::Error, "启动失败", &e, None));
            return Err(e);
        }
        // 启动成功：目标程序已在前台，更新器窗口（重试后走到这里的场景）
        // 不应停留在「已是最新版本」，通知 UI 关闭退出
        handle.request_exit();
        return Ok(());
    }

    // ---- 发现新版本：下载 → 校验 → 安装 ----
    let sub = format!("{} → {}", current_version, m.version);
    handle.set(PhasePayload::new(
        Stage::Available,
        "发现新版本",
        &sub,
        None,
    ));
    run_update_flow(cfg, &m, handle).await
}

/// 完整更新流程：下载 → 校验 → 安装 → 记录 → 清理 → 收尾
///
/// 前提：已确认有新版本（`m` 为服务器 manifest）。各失败点把错误阶段
/// 写回 `handle` 后返回 Err；成功且 launch 模式下拉起目标程序。
pub async fn run_update_flow(
    cfg: &ResolvedConfig,
    m: &manifest::FlatManifest,
    handle: &PhaseHandle,
) -> Result<(), String> {
    // 版本号会被拼进下载缓存名与安装目录名（`app_<version>`）：正常路径上
    // manifest::fetch 已校验，这里再挡一次（manifest 也可能由 UI 侧直接传入），
    // 确保任何入口都不会用未校验的版本串拼路径。
    if let Err(e) = crate::appdir::validate_version(&m.version) {
        let e = format!("服务器返回的版本号不可用: {}", e);
        eprintln!("[更新器] 拒绝更新: {}", e);
        handle.set(PhasePayload::failure("更新包信息无效", &e, true));
        return Err(e);
    }

    // ---- 清单体检（在下载、提权之前）----
    //
    // 「没有下载地址」和「没有任何完整性校验值」都属于服务端配置问题：
    // 重下、重试、管理员权限一个都解决不了。把它们放在最前面 fail fast，
    // 免得先弹一次 UAC、用户点了「是」，才告之「这包装不了」——那是最差的顺序。
    let is_parts = m.parts.as_ref().is_some_and(|p| !p.is_empty());

    if !is_parts && m.url.is_empty() {
        // 单包模式却没有地址：等于没有下载这条路，同属「拿不到安装包」
        let e = "manifest 缺少 url 字段（单包模式必填）".to_string();
        eprintln!("[更新器] 下载失败: {}", e);
        handle.set(PhasePayload::failure("更新包下载失败", &e, true));
        return Err(e);
    }

    // 包完整性闸门：默认要求清单给出校验值（整包 sha256，或分卷模式给齐 part_sha256）。
    // 二者皆无时，下载回来的字节没有任何完整性证据，而清单与包通常同源同域——
    // 服务器被改写或 CDN 被劫持时，校验值缺失正是攻击者最希望的状态，故宁可拒装。
    // 放行出口（自建/内网等可信环境）：update.json `"require_sha256": false`
    // 或命令行 `--allow-missing-sha256`。
    if cfg.require_sha256 && !manifest::can_verify_package(m) {
        let e = if is_parts {
            "manifest 缺少包完整性校验值：整包 sha256 未提供，part_sha256 也未与 parts 一一对齐"
        } else {
            "manifest 缺少包完整性校验值：未提供整包 sha256"
        }
        .to_string();
        eprintln!("[更新器] 拒绝安装: {}", e);
        handle.set(PhasePayload::failure(
            "更新包缺少校验值，已拒绝安装",
            &format!(
                "{}。请由服务端补上校验值后重试；确需在无校验值时继续安装，可在 update.json 设 \"require_sha256\": false 或以 --allow-missing-sha256 启动",
                e
            ),
            true,
        ));
        return Err(e);
    }

    // ---- 磁盘空间预检（下载前）----
    //
    // 2.3 GB 的包在「下载 + 合并/解压」期间峰值约为两倍包大小：先查一次，
    // 免得用户等了半小时才在解压中途失败（还会自动重试两轮）。清单没给
    // size_bytes、或平台查不到可用空间时跳过——预检只是体验优化，不能误拦。
    if let Some(size) = m.size_bytes {
        let probe = download_dir(&cfg.app_name);
        if let Err(e) = disk::ensure_space(&probe, disk::required_for_download(size)) {
            eprintln!("[更新器] 磁盘预检未通过: {}", e);
            handle.set(PhasePayload::failure("磁盘空间不足", &e, false));
            return Err(e);
        }
    }

    // ---- 提权前置检查（删除服务 / 安装前）----
    //
    // 删除 Windows 服务（`sc delete`）需要管理员权限；当前进程未提权时会拿到
    // 退出码 5。这里把检查前移：真正执行下载/安装/删服务之前，若配置了需删除的
    // 服务且当前进程未提权，就以 `runas` 重新拉起一个管理员副本（触发 UAC），
    // 随后退出当前（未提权）进程，由提权副本接管——从源头避免那次失败的 sc 调用。
    // 提权失败（用户取消 UAC）则给出明确错误并终止；已有 services.rs 的退出码 5
    // 文案作为防御性兜底。
    match elevation::ensure_admin_for_update(cfg) {
        Ok(false) => {}
        Ok(true) => {
            // 已成功拉起提权副本：退出当前进程，避免两个更新器窗口并存
            eprintln!("[更新器] 提权副本已拉起，退出当前（未提权）进程");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("[更新器] 提权前置检查失败，终止更新: {}", e);
            handle.set(PhasePayload::new(Stage::Error, "需要管理员权限", &e, None));
            return Err(e);
        }
    }

    let zip_path = download_dir(&cfg.app_name).join(format!("update_{}.zip", m.version));

    // ---- 下载 + 包完整性校验：同一环节自动重试（默认 3 次，含首次） ----
    //
    // 其余失败（网络抖动、CDN 5xx、下载后 SHA256 对不上、分卷合并失败）
    // 都视为「可能是这次运气不好」，重下一遍比拼人品靠谱 —— 每轮重试
    // 前会删掉残留包，避免把半包当完整包复用。
    // 重试次数耗尽才把「可跳官网」标记写进快照：地址确实全挂了，
    // 界面此时给「前往官网」才是负责任的出口。
    let residual_free = residual_is_free(&zip_path);
    if !residual_free {
        eprintln!(
            "[更新器] 残留更新包被其他进程占用，本次不自动重试: {}",
            zip_path.display()
        );
    }
    let download_max = retry_limit(cfg.retry_times, residual_free);
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        let outcome = if is_parts {
            run_download_parts(
                handle,
                m,
                &cfg.app_name,
                cfg.parts_concurrency,
                &zip_path,
                attempt,
                download_max,
            )
            .await
        } else {
            if attempt > 1 {
                // 重试前丢弃上一轮的残包：复用逻辑只认校验通过的包，
                // 但 sha256 缺失时它会直接信任本地文件，这里先删干净
                let _ = tokio::fs::remove_file(&zip_path).await;
            }
            run_download(handle, m, &zip_path, attempt, download_max).await
        };

        // 下载成功后再核一遍完整包 SHA256：早期中断/磁盘写坏都在这关露头。
        // 下载/合并阶段已流式算出的哈希直接复用（Some），没有才回落到整读校验。
        let result = match outcome {
            Ok(precomputed) => verify_package(handle, &zip_path, &m.sha256, precomputed).await,
            Err(e) => Err(e),
        };

        match result {
            Ok(()) => break,
            // 只有「再试一次可能就好了」的错误才重试：404/403 这类 4xx 与
            // 本地写盘失败（磁盘满、目录只读）重下多少遍都是同一个结果，
            // 白等三轮外加一次 1.4 GB 的无效流量。
            Err(e) if attempt < download_max && download::is_retryable(&e) => {
                eprintln!(
                    "[更新器] 下载/校验失败（第 {}/{} 次），自动重试: {}",
                    attempt, download_max, e
                );
                notify_retrying(handle, "download", attempt, download_max, &e);
                // 失败后清掉残缺产物，下一轮从零开始
                let _ = tokio::fs::remove_file(&zip_path).await;
                tokio::time::sleep(retry_backoff(attempt)).await;
            }
            Err(e) => {
                if attempt < download_max {
                    eprintln!("[更新器] 该类失败重试无意义，停止自动重试: {}", e);
                }
                // 不可重试的错误把上限记成 0：界面文案只写「已尝试 N 次」，
                // 不再暗示还有「共 M 次」轮次要来
                let effective_max = if download::is_retryable(&e) {
                    download_max
                } else {
                    0
                };
                record_download_failure(handle, &e, attempt, effective_max);
                return Err(e);
            }
        }
    }
    eprintln!("[更新器] 更新包校验通过: {}", zip_path.display());

    // 安装前卸载用户配置的服务（运行中先停止；卸载后不恢复、不重启）。
    // 卸载失败必须在此终止：否则服务仍注册指向旧路径而新版本已装完，
    // 形成「半更新」状态。
    if !cfg.services.is_empty() {
        let svc_names = cfg.services.clone();
        let svc_handle = handle.clone();
        let svc_result = tokio::task::spawn_blocking(move || {
            services::uninstall_configured(&services::SystemScRunner, &svc_names, &|msg| {
                svc_handle.set(PhasePayload::new(
                    Stage::Installing,
                    "正在处理服务...",
                    msg,
                    None,
                ));
            })
        })
        .await;
        let svc_detail = match svc_result {
            // 任务内全部成功（含未安装跳过）
            Ok(Ok(())) => None,
            // 有服务卸载失败（详情已聚合）
            Ok(Err(failures)) => Some(failures.join("；")),
            // 任务本身异常（阻塞线程 panic 被 JoinError 捕获）
            Err(e) => Some(format!("服务卸载任务异常: {e}")),
        };
        if let Some(detail) = svc_detail {
            eprintln!("[更新器] 服务卸载失败，终止更新: {}", detail);
            handle.set(PhasePayload::new(
                Stage::Error,
                "服务卸载失败，更新已终止",
                &detail,
                None,
            ));
            return Err(detail);
        }
    }

    // ---- 安装（解压 + 校验文件）：同样允许自动重试 ----
    //
    // 解压失败多为「包不完整 / 磁盘瞬时占用」，重试一次常常就好了；
    // 但这不是「下载地址」的问题，故耗尽后也不引导用户去官网。
    let target_dir = install_dir(&cfg.app_name, &m.version);
    let install_max = retry_limit(cfg.retry_times, true);
    let mut install_attempt: u32 = 0;
    loop {
        install_attempt += 1;
        let install_handle = handle.clone();
        let install_cb: install::StageCallback = Arc::new(move |msg, pct| {
            install_handle.set(PhasePayload::new(
                Stage::Installing,
                "正在安装更新...",
                msg,
                pct,
            ))
        });
        match install::install(&zip_path, &target_dir, install_cb).await {
            Ok(_) => break,
            // 与下载环节同一条原则：包非法 / 磁盘写不进去这类确定性失败
            // 不再陪着等两轮退避；只有改名交换阶段的瞬时占用值得重试。
            Err(e) if install_attempt < install_max && install::is_retryable_error(&e) => {
                eprintln!(
                    "[更新器] 安装失败（第 {}/{} 次），自动重试: {}",
                    install_attempt, install_max, e
                );
                notify_retrying(handle, "install", install_attempt, install_max, &e);
                tokio::time::sleep(retry_backoff(install_attempt)).await;
            }
            Err(e) => {
                eprintln!("[更新器] 安装失败: {}", e);
                let effective_max = if install::is_retryable_error(&e) {
                    install_max
                } else {
                    0
                };
                handle.set(
                    PhasePayload::failure("更新安装失败", &e, false).with_attempt(
                        "install",
                        install_attempt,
                        effective_max,
                    ),
                );
                return Err(e);
            }
        }
    }
    eprintln!("[更新器] 安装完成: {}", target_dir.display());

    // 写入本地版本记录，供下次检查与目标程序定位
    if let Err(e) = install::save_local_manifest(
        &local_manifest_path(&cfg.app_name),
        &cfg.app_name,
        &m.version,
        &target_dir,
        cfg.launch.clone(),
    )
    .await
    {
        eprintln!("[更新器] 本地版本记录写入失败: {}", e);
        handle.set(PhasePayload::new(
            Stage::Error,
            "本地版本记录写入失败",
            &e,
            None,
        ));
        return Err(e);
    }

    // 清理本次更新的下载缓存（zip + 分卷），downloads 回归暂存区语义。
    // 旧版本目录不在这里删——那是回滚资产，交由下次启动的 cleanup_startup
    // 按 max_versions 处理，别让用户在「更新完成」前多等一次大目录删除。
    cleanup_download_cache(&cfg.app_name, &m.version).await;

    let done_msg = format!("已更新到版本 {}", m.version);
    handle.set(PhasePayload::new(Stage::Done, "更新完成", &done_msg, None));

    // 更新完成后拉起目标程序（launch 留空时自动定位主程序启动）
    let outcome = match &cfg.launch {
        Some(_) => launch_target(cfg, Some(&target_dir)),
        None => match install::find_main_executable(&target_dir, &cfg.app_name) {
            Some(exe) => spawn_detached(&exe),
            None => Err(format!(
                "安装目录中未找到可执行文件: {}",
                target_dir.display()
            )),
        },
    };
    if let Err(e) = outcome {
        // 安装已成功（app.json 已写入新版本），仅启动失败：保留错误态，
        // 用户可点「重试」重走检测（无更新分支会再次尝试启动）或手动启动
        eprintln!("[更新器] 更新完成但启动失败: {}", e);
        handle.set(PhasePayload::new(
            Stage::Error,
            "更新已完成，但启动应用失败",
            &e,
            None,
        ));
        return Err(e);
    }
    // 启动成功：对齐 Tauri 版 app.exit(0) —— 目标程序已在前台，
    // 通知 UI 关闭更新器窗口，不要停在「更新完成」界面挡在应用前面
    handle.request_exit();
    Ok(())
}

/// 执行分卷下载：逐卷下载（复用合格卷）→ 合并为完整 zip → 整体校验
///
/// 合并产物与单包模式共用 zip_path，后续校验/安装/清理逻辑完全一致
async fn run_download_parts(
    handle: &PhaseHandle,
    m: &manifest::FlatManifest,
    app_name: &str,
    concurrency: usize,
    zip_path: &Path,
    attempt: u32,
    max_attempts: u32,
) -> Result<Option<String>, String> {
    let urls = m.parts.as_ref().ok_or("分卷地址列表缺失")?;
    let parts_dir = download_dir(app_name).join(format!("update_{}_parts", m.version));
    let sha256s = m.part_sha256.as_deref().filter(|s| s.len() == urls.len());
    // 整体总大小（manifest.size_bytes），分卷之和等于完整 zip 大小
    let total_size = m.size_bytes;

    // 上次已合并好的完整包优先复用：合并成功后分卷会立即删除，
    // 不复用这里就得为一次安装失败/手动重试重下整包。
    if let Some(actual) = try_reuse_zip(handle, m, zip_path).await {
        return Ok(Some(actual));
    }

    handle.set(PhasePayload::new(
        Stage::Downloading,
        &retry_title("download", attempt, max_attempts),
        &format!("共 {} 卷", urls.len()),
        Some(0.0),
    ));

    let tracker = std::sync::Mutex::new(SpeedTracker::new());
    let handle_cb = handle.clone();
    let result = download::download_parts(
        urls,
        sha256s,
        &parts_dir,
        total_size,
        concurrency,
        move |done, total, p| {
            let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
            let (should, speed) = t.tick(p.downloaded, p.percent() == Some(100.0));
            if should {
                // 标题只描述动作，分卷进度交由右上角徽标（"分卷 x/y"）展示。
                // 并发下载时 x 是「已完成卷数」，不是正在下载的卷号。
                let msg = retry_title("download", attempt, max_attempts);
                // 总大小未知时按卷数估算百分比
                let percent = p
                    .percent()
                    .unwrap_or((done as f64 / total.max(1) as f64) * 100.0);
                handle_cb.set(PhasePayload::download(
                    &msg,
                    Some(format!("分卷 {}/{}", done, total)),
                    Some(percent),
                    p.downloaded,
                    p.total,
                    Some(speed),
                ));
            }
        },
    )
    .await;
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            // download_parts 已把卷号补进错误里，这里原样保留。
            // 本地落盘失败（磁盘满/目录只读）不带下载前缀：既不该重试，
            // 也不该把用户引到官网——换个地址一样写不进同一块磁盘。
            let msg = if e.starts_with(download::DOWNLOAD_FAIL_PREFIX)
                || download::is_local_io_error(&e)
            {
                e
            } else {
                format!("{}{}", download::DOWNLOAD_FAIL_PREFIX, e)
            };
            return Err(msg);
        }
    };

    handle.set(PhasePayload::new(
        Stage::Checking,
        "正在合并分卷...",
        "",
        Some(0.0),
    ));
    eprintln!("[更新器] 合并 {} 个分卷...", result.part_paths.len());
    let merge_handle = handle.clone();
    // 合并是 GB 级顺序读写，按 256 KB 推一次进度会把 UI 锁砸出上万次；
    // 与下载一样 300ms 节流，末次强制推送 100%。
    let merge_throttle =
        std::sync::Mutex::new(Instant::now() - Duration::from_millis(PROGRESS_INTERVAL_MS));
    let merged_sha = download::merge_parts(
        &result.part_paths,
        zip_path,
        &m.sha256,
        move |done, total| {
            let force = total > 0 && done >= total;
            {
                let mut last = merge_throttle.lock().unwrap_or_else(|e| e.into_inner());
                if !force && last.elapsed() < Duration::from_millis(PROGRESS_INTERVAL_MS) {
                    return;
                }
                *last = Instant::now();
            }
            let pct = if total > 0 {
                (done as f64 / total as f64 * 100.0).min(100.0)
            } else {
                0.0
            };
            merge_handle.set(PhasePayload::new(
                Stage::Checking,
                "正在合并分卷...",
                &format!("{} / {}", fmt_mb(done), fmt_mb(total)),
                Some(pct),
            ));
        },
    )
    .await
    .map_err(|e| {
        // 合并失败多为分卷本身残缺（下载环节的锅），统一按下载环节处理；
        // 错误本身可能已带前缀（整体哈希不匹配），避免重复拼接。
        // 写合并文件失败属于本地磁盘问题，不挂下载前缀（不给官网入口）。
        let detail = e.strip_prefix(download::DOWNLOAD_FAIL_PREFIX).unwrap_or(&e);
        if download::is_local_io_error(&e) {
            format!("分卷合并失败: {}", detail)
        } else {
            format!("{}分卷合并失败: {}", download::DOWNLOAD_FAIL_PREFIX, detail)
        }
    })?;
    eprintln!("[更新器] 分卷合并完成: {}", zip_path.display());

    // 合并产物已经过整体 SHA256 校验（或逐卷校验），分卷不再被任何后续步骤需要。
    // 立即删除，别让它一直占着与完整包等量的空间穿过解压/安装阶段。
    if let Err(e) = tokio::fs::remove_dir_all(&parts_dir).await {
        eprintln!(
            "[更新器] 清理分卷缓存失败（忽略）: {}（{}）",
            parts_dir.display(),
            e
        );
    }
    Ok(Some(merged_sha))
}

/// 复用已存在的完整更新包：校验通过返回实际 SHA256，不可复用返回 None
///
/// 复用前提：manifest 提供了非空整包 SHA256。无校验值时无法确认文件完整，
/// 等于把上次中断的半包当完整包复用（后面安装的就是损坏包），故宁可重下。
/// （缺校验值的包能否走到这一步由 `require_sha256` 闸门决定；这里保留同一
/// 判断，是为了闸门被显式关掉时仍然不拿半包去装。）
///
/// 单包与分卷模式共用：分卷合并成功后分卷已被删除，安装失败后点「重试」
/// 时只有这个完整包能省下整包重下的流量。
async fn try_reuse_zip(
    handle: &PhaseHandle,
    m: &manifest::FlatManifest,
    zip_path: &Path,
) -> Option<String> {
    if !zip_path.exists() || !manifest::has_whole_hash(m) {
        return None;
    }
    let cached = zip_path.to_path_buf();
    let sha256 = m.sha256.clone();
    // 复用校验只读一次：算出实际哈希的同时完成比对，并把结果上抛，
    // 外层 verify_package 直接复用，不再整读第二遍（GB 级包差别明显）
    let verified = tokio::task::spawn_blocking(move || {
        let actual = download::sha256_of_file(&cached)?;
        let expected = sha256.as_deref().unwrap_or_default();
        download::compare_sha256(expected, &actual).map(|()| actual)
    })
    .await
    .unwrap_or_else(|e| Err(format!("校验任务异常: {}", e)));
    match verified {
        Ok(actual) => {
            let size = tokio::fs::metadata(zip_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            eprintln!(
                "[更新器] 复用已下载的更新包: {}（{}）",
                zip_path.display(),
                crate::phase::format_bytes(size)
            );
            handle.set(PhasePayload::download(
                "使用已下载的更新包",
                None,
                Some(100.0),
                size,
                Some(size),
                None,
            ));
            Some(actual)
        }
        Err(e) => {
            eprintln!("[更新器] 本地更新包校验不通过，重新下载: {}", e);
            let _ = tokio::fs::remove_file(zip_path).await;
            None
        }
    }
}

/// 执行下载：本地已有同名包且校验通过则复用，否则流式下载并推送进度
async fn run_download(
    handle: &PhaseHandle,
    m: &manifest::FlatManifest,
    zip_path: &Path,
    attempt: u32,
    max_attempts: u32,
) -> Result<Option<String>, String> {
    if let Some(actual) = try_reuse_zip(handle, m, zip_path).await {
        return Ok(Some(actual));
    }

    if let Some(parent) = zip_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建下载目录失败 {}: {}", parent.display(), e))?;
    }

    handle.set(PhasePayload::new(
        Stage::Downloading,
        &retry_title("download", attempt, max_attempts),
        "",
        Some(0.0),
    ));
    let tracker = std::sync::Mutex::new(SpeedTracker::new());
    let handle_cb = handle.clone();
    // 服务器不给 Content-Length 时用清单的 size_bytes 兜底百分比：
    // 否则界面只剩「已下载 X MB」，用户无法判断还要等多久
    let known_size = m.size_bytes;
    let result = download::download_file(&m.url, zip_path, move |p| {
        let total = p.total.or(known_size);
        let percent = download::percent_of(p.downloaded, total);
        let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
        let (should, speed) = t.tick(p.downloaded, percent == Some(100.0));
        if should {
            handle_cb.set(PhasePayload::download(
                &retry_title("download", attempt, max_attempts),
                None,
                percent,
                p.downloaded,
                total,
                Some(speed),
            ));
        }
    })
    .await;

    match result {
        Ok((_, sha)) => {
            let size = tokio::fs::metadata(zip_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            eprintln!(
                "[更新器] 下载完成，大小 {}，SHA256: {}",
                crate::phase::format_bytes(size),
                sha
            );
            // 哈希是下载时流式算出的，交给外层直接比对，避免再读一遍
            Ok(Some(sha))
        }
        Err(e) => Err(e),
    }
}

/// 下载进度节流 + 速度平滑器（单包/分卷共用）
///
/// 按 PROGRESS_INTERVAL_MS 节流推送，速度用指数平滑（0.6 旧值 + 0.4 瞬时）避免数字跳动
pub struct SpeedTracker {
    /// 上次推送时间
    last_emit: Instant,
    /// 上次推送时的已下载字节
    last_bytes: u64,
    /// 平滑后的速度（字节/秒）
    smooth: f64,
}

impl Default for SpeedTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeedTracker {
    pub fn new() -> Self {
        Self {
            last_emit: Instant::now(),
            last_bytes: 0,
            smooth: 0.0,
        }
    }

    /// 记录一次进度，判断是否应推送
    ///
    /// - `downloaded`: 当前已下载字节
    /// - `force`: 强制推送（如进度到 100%）
    ///
    /// 返回 (是否应推送, 平滑速度 字节/秒)
    pub fn tick(&mut self, downloaded: u64, force: bool) -> (bool, u64) {
        let elapsed = self.last_emit.elapsed().as_secs_f64();
        if !force && elapsed < PROGRESS_INTERVAL_MS as f64 / 1000.0 {
            return (false, 0);
        }
        let instant = if elapsed > 0.0 {
            (downloaded - self.last_bytes) as f64 / elapsed
        } else {
            0.0
        };
        self.smooth = if self.smooth > 0.0 {
            self.smooth * 0.6 + instant * 0.4
        } else {
            instant
        };
        self.last_bytes = downloaded;
        self.last_emit = Instant::now();
        (true, self.smooth as u64)
    }
}

/// 解析当前版本：命令行 > update.json > 本地 app.json > 默认 0.0.0
///
/// 损坏自恢复：本地 app.json 存在但其记录的安装目录缺失（被误删/损坏）时，
/// 视为未安装（0.0.0），触发自动重装
pub async fn resolve_current_version(args: &CliArgs, app_name: &str) -> String {
    if let Some(v) = &args.current_version {
        return v.clone();
    }
    if let Some(v) = non_empty(config::load().current_version) {
        return v;
    }
    let path = local_manifest_path(app_name);
    if path.exists() {
        match LocalManifest::load(&path).await {
            Ok(Some(m)) if !m.version.is_empty() => {
                // 校验安装目录是否存在，缺失则自恢复重装
                if !m.installed_path.is_empty() && !Path::new(&m.installed_path).exists() {
                    eprintln!(
                        "[更新器] 本地版本 {} 的安装目录缺失（{}），视为未安装并自动重装",
                        m.version, m.installed_path
                    );
                    return "0.0.0".to_string();
                }
                return m.version;
            }
            Ok(_) => {}
            Err(e) => eprintln!("[更新器] 本地版本记录读取失败: {}", e),
        }
    }
    "0.0.0".to_string()
}

/// 解析启动目标 exe 的路径（纯函数，供单测）
///
/// 优先级（更新后要启动的是**新版本**，安装目录是权威来源）：
/// 1. 安装目录内的目标程序：绝对路径取同名文件、相对路径按原相对
///    结构拼接；直接拼接未命中时在安装目录内按文件名递归查找
///    （限深 3 层，兼容 zip 根带一层目录的结构）
/// 2. 绝对路径原样使用 —— 仅当安装目录内找不到时回退。
///    **不能无条件优先**：交接模式下 `--launch` 是旧版客户端传入的
///    自身绝对路径，原样使用会启动旧二进制（旧目录还在时）或直接
///    启动失败（旧目录已被 max_versions 清理时）
/// 3. 相对路径回退到更新器 exe 所在目录（客户端与更新器同目录部署
///    的兜底）
pub fn resolve_launch_exe(launch: &str, installed_path: Option<&Path>) -> PathBuf {
    let raw = Path::new(launch);

    if let Some(dir) = installed_path {
        // 绝对路径只取文件名（Path::join 遇绝对路径会整体替换，丢失 dir）；
        // 相对路径保留原相对结构
        let direct = if raw.is_absolute() {
            raw.file_name().map(|n| dir.join(n))
        } else {
            Some(dir.join(raw))
        };
        if let Some(p) = direct.filter(|p| p.exists()) {
            return p;
        }
        // 嵌套结构兜底：安装目录内递归找同名文件
        if let Some(name) = raw.file_name() {
            if let Some(found) = find_file_by_name(dir, name, 3) {
                return found;
            }
        }
    }

    if raw.is_absolute() {
        return raw.to_path_buf();
    }
    std::env::current_exe()
        .ok()
        .and_then(|cur| cur.parent().map(|d| d.join(raw)))
        .unwrap_or_else(|| raw.to_path_buf())
}

/// 在目录内按文件名递归查找（限深，防止异常目录树拖慢启动）
fn find_file_by_name(dir: &Path, name: &std::ffi::OsStr, max_depth: usize) -> Option<PathBuf> {
    if max_depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name() == Some(name) {
            return Some(path);
        }
    }
    // 第二轮再递归（同名文件优先出现在浅层）
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_by_name(&path, name, max_depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

/// 拉起目标程序（launch 已配置时）
///
/// 启动失败（目标不存在 / spawn 失败）返回 Err 详情，由调用方决定
/// 是提示用户还是静默退出 —— 更新完成后静默失败会让用户「更新完
/// 了应用却没回来」。
///
/// - `installed_path`: 目标程序安装目录（更新完成分支传入本次安装目录；无更新分支从本地 app.json 读取）
pub fn launch_target(cfg: &ResolvedConfig, installed_path: Option<&Path>) -> Result<(), String> {
    let Some(exe) = &cfg.launch else {
        return Ok(());
    };
    let exe_path = resolve_launch_exe(exe, installed_path);

    if !exe_path.exists() {
        return Err(format!(
            "目标程序不存在: {}（解析路径: {}）",
            exe,
            exe_path.display()
        ));
    }
    spawn_detached(&exe_path)
}

/// launch 配置下的完整启动结果（launch 留空时自动定位安装目录主程序）
///
/// 与 [`launch_target`] 的差异：launch 留空 = 启动器语义「自动定位」，
/// 从安装目录找主 exe 启动（与「完成」按钮同一套定位逻辑）。
pub fn launch_outcome(cfg: &ResolvedConfig, installed_path: Option<&Path>) -> Result<(), String> {
    match &cfg.launch {
        Some(_) => launch_target(cfg, installed_path),
        None => {
            let Some(dir) = installed_path else {
                return Err("无本地版本记录，无法定位已安装的应用".to_string());
            };
            match install::find_main_executable(Path::new(dir), &cfg.app_name) {
                Some(exe) => spawn_detached(&exe),
                None => Err(format!("安装目录中未找到可执行文件: {}", dir.display())),
            }
        }
    }
}

/// 以分离进程方式启动指定 exe（工作目录取 exe 所在目录）
///
/// 注入 `QSLTD_FROM_UPDATER=1`：客户端据此跳过启动检测，防止「拉回→再交接」循环
pub fn spawn_detached(exe_path: &Path) -> Result<(), String> {
    let workdir = exe_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    match std::process::Command::new(exe_path)
        .current_dir(workdir)
        .env("QSLTD_FROM_UPDATER", "1")
        .spawn()
    {
        Ok(_) => {
            eprintln!("[更新器] 已启动: {}", exe_path.display());
            Ok(())
        }
        Err(e) => Err(format!("启动失败 {}: {}", exe_path.display(), e)),
    }
}

/// 读取本地 app.json 记录的安装目录（不存在或未记录时返回 None）
pub async fn read_installed_path(app_name: &str) -> Option<String> {
    let path = local_manifest_path(app_name);
    if !path.exists() {
        return None;
    }
    match LocalManifest::load(&path).await {
        Ok(Some(m)) if !m.installed_path.is_empty() => Some(m.installed_path),
        _ => None,
    }
}

/// 下载环节失败：把「可跳官网」标记写进快照，再按原样上抛错误
///
/// 只在错误信息出自下载链路（带 [`download::DOWNLOAD_FAIL_PREFIX`]）时打标；
/// 磁盘满、创建目录失败这类本地问题不引导用户去官网——重试或清理磁盘才有用。
fn record_download_failure(handle: &PhaseHandle, err: &str, attempt: u32, max_attempts: u32) {
    let is_download = err.starts_with(download::DOWNLOAD_FAIL_PREFIX);
    eprintln!("[更新器] 下载失败: {}", err);
    handle.set(
        PhasePayload::failure("更新包下载失败", err, is_download).with_attempt(
            "download",
            attempt,
            max_attempts,
        ),
    );
}

/// 生效的重试上限（含首次尝试）
///
/// - 配置值经 [`config::resolve_retry_times`] 归一（默认 3，上限 10）
/// - `retryable` 为 false 时只试一次：比如下载目录建不出来，重试同样建不出来
fn retry_limit(configured: u32, retryable: bool) -> u32 {
    if retryable {
        // 上限放在这里兜底，防止直接构造 ResolvedConfig 时传个离谱值
        configured.clamp(1, 10)
    } else {
        1
    }
}

/// 重试前先确认上一次的残留包**没有被别的进程占着**
///
/// 占着（杀软正在扫、资源管理器打开了它、目录只读）就只试一次：重下只会
/// 再写一遍半包，白等一轮。
///
/// **只探测，不删除**。曾经的实现是「真删一次看删不删得掉」，副作用直接
/// 把上次下好的完整包抹掉，使 [`run_download`] 的「复用已下载的更新包」
/// 分支永远进不去（1.4 GB 的包每次重来），与 README 承诺的行为相反。
/// 现在改用 [`crate::instance::is_unlocked`] 的独占打开探测；清理动作仍由
/// 下载循环按轮次显式做。
fn residual_is_free(path: &Path) -> bool {
    crate::instance::is_unlocked(path)
}

/// 校验完整包 SHA256（期望值为空时跳过）
///
/// `precomputed` 为下载/合并阶段已经算出的实际哈希：给出时直接比对，
/// 不再整读文件；没有才回落到 [`download::verify_sha256`] 的流式读盘。
async fn verify_package(
    handle: &PhaseHandle,
    zip_path: &Path,
    sha256: &Option<String>,
    precomputed: Option<String>,
) -> Result<(), String> {
    let Some(expected) = sha256.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    handle.set(PhasePayload::new(
        Stage::Checking,
        "正在校验更新包...",
        "",
        None,
    ));
    if let Some(actual) = precomputed {
        return download::compare_sha256(expected, &actual);
    }
    let zip_path = zip_path.to_path_buf();
    let expected = expected.to_string();
    tokio::task::spawn_blocking(move || download::verify_sha256(&zip_path, &Some(expected)))
        .await
        .unwrap_or_else(|e| Err(format!("校验任务异常: {}", e)))
}

/// 失败后自动重试前的等待（1s、2s、3s…，最多 5s）
///
/// 递增而非固定：短暂抖动立刻重试就够；持续故障时没必要每 500ms 砸一次服务器。
fn retry_backoff(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs(u64::from(attempt.clamp(1, 5)))
}

/// 推送「正在自动重试」快照
///
/// 走 download/install 阶段而非 error：error 是终点，这里流程还在跑，
/// 界面标题会说清楚「第几次」，用户知道程序没死。
fn notify_retrying(
    handle: &PhaseHandle,
    stage_name: &str,
    attempt: u32,
    max_attempts: u32,
    err: &str,
) {
    let stage = if stage_name == "install" {
        Stage::Installing
    } else {
        Stage::Downloading
    };
    let title = retry_title(stage_name, attempt + 1, max_attempts);
    // 不带 percent：percent 为 None 时界面按「刚重新开始」渲染（0 进度），
    // 比硬塞一个 0.0 更诚实——上一轮已下载的字节数已经作废
    handle.set(PhasePayload::new(
        stage,
        &title,
        &format!(
            "第 {}/{} 次尝试失败，正在自动重试（{}）",
            attempt,
            max_attempts,
            first_line(err)
        ),
        None,
    ));
}

/// 阶段标题：首次尝试就是常规文案，重试时标出「第 N/M 次」
fn retry_title(stage_name: &str, attempt: u32, max_attempts: u32) -> String {
    let base = if stage_name == "install" {
        "正在安装更新..."
    } else {
        "正在下载更新包..."
    };
    if attempt <= 1 {
        base.to_string()
    } else {
        format!("{}（第 {}/{} 次）", base, attempt, max_attempts)
    }
}

/// 取错误信息首行：多行错误塞进进度条副标题只会被截断，不如只留关键一句
fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s).trim()
}

/// 在系统浏览器打开官网首页（错误态「前往官网」）
///
/// 两个场景共用：更新检查失败（manifest 全部地址失效）与下载/合并环节
/// 重试用尽。地址取生效配置 `homepage`（update.json > 内置默认），与流程
/// 同源，避免界面和配置各说各话。
pub fn open_homepage() -> Result<(), String> {
    let args = crate::cli::parse();
    let cfg = resolve_config(&args);
    crate::shell::open_url(&cfg.homepage)
}

/// 打开应用商店（强制更新界面「前往应用商店」按钮）
///
/// 地址取服务端 manifest 的 `store_url`；缺省或空串回落到本地配置 `homepage`
/// （前往官网）。注意两者**可信度不同**：`homepage` 来自本地 update.json / 编译期
/// 烘焙，`store_url` 是服务端下发的远程字符串，所以 [`crate::shell::open_url`]
/// 会按不可信输入校验它。校验不过时不硬弹错误：告警后改开本地配置的官网首页，
/// 用户仍有一个能用的出口，而危险的字符串一次也不会进 shell。
pub fn open_store_url(store_url: Option<&str>) -> Result<(), String> {
    let args = crate::cli::parse();
    let cfg = resolve_config(&args);
    let Some(requested) = store_url.map(str::trim).filter(|s| !s.is_empty()) else {
        return crate::shell::open_url(&cfg.homepage);
    };
    match crate::shell::sanitize_url(requested) {
        Ok(validated) => crate::shell::open_url(&validated),
        Err(e) => {
            eprintln!("[更新器] 服务端 store_url 未通过校验（{e}），改开本地配置的官网首页");
            crate::shell::open_url(&cfg.homepage)
        }
    }
}

/// 更新流程运行中标志：首次运行与「重试」共用，防并发双跑
static CHECK_RUNNING: AtomicBool = AtomicBool::new(false);

/// 原始命令行参数（不含程序名）：错误态「重试」时用它重新解析
/// （重试与首次完全同参）
static CLI_ARGV: OnceLock<Vec<String>> = OnceLock::new();

/// 登记原始命令行参数（UI 入口启动时调用一次，重试时取用）
pub fn register_cli_argv(argv: Vec<String>) {
    let _ = CLI_ARGV.set(argv);
}

/// 错误态「重试」：用相同命令行参数重走一遍流程
///
/// 重试与首次运行完全同参（含 --manifest 等覆盖）——「同一配置再试
/// 一次」正是弱网/服务端临时故障时用户的诉求；想换地址应重启更新器。
/// run_check_flow 首行即推送 checking 阶段，UI 自动回到检测态。
pub fn retry(handle: PhaseHandle) {
    let Some(argv) = CLI_ARGV.get().cloned() else {
        eprintln!("[更新器] 未登记命令行参数，无法重试");
        return;
    };
    let args = crate::cli::parse_from(&argv);
    eprintln!("[更新器] 用户请求重试，重新开始检查更新");
    spawn_flow(args, handle);
}

/// 「完成」按钮（done 阶段调用）：启动更新后的应用并返回
///
/// 正常路径下更新完成后流程内已自动拉起应用，本函数是窗口尚未退出
/// 期间的兜底入口（快速点击场景）。launch 未配置时从本地安装目录
/// 自动定位主 exe 启动。失败返回错误详情（UI 切错误态）。
pub async fn finish_and_launch(handle: &PhaseHandle) -> Result<(), String> {
    let args = crate::cli::parse();
    let cfg = resolve_config(&args);
    let installed = read_installed_path(&cfg.app_name).await;
    match launch_outcome(&cfg, installed.as_deref().map(Path::new)) {
        Ok(()) => {
            // 启动成功：通知 UI 关闭更新器窗口（对齐 Tauri 版 command 的 app.exit(0)）
            handle.request_exit();
            Ok(())
        }
        Err(e) => {
            eprintln!("[更新器] 「完成」启动失败: {}", e);
            handle.set(PhasePayload::new(Stage::Error, "启动失败", &e, None));
            Err(e)
        }
    }
}

// 测试里构造失败即用例本身失败，按 clippy 惯例局部放行 expect_used
#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("qsltd_flow_test_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
    }

    /// 构造一份最小可用配置（测试用：不依赖 update.json 与命令行）
    fn cfg_for_test() -> ResolvedConfig {
        ResolvedConfig {
            app_name: "qsltd_flow_test".to_string(),
            launcher_name: "启动器".to_string(),
            manifest_urls: vec![],
            launch: None,
            max_versions: 2,
            services: vec![],
            template: "F".to_string(),
            homepage: "https://example.test".to_string(),
            retry_times: 3,
            require_sha256: true,
            parts_concurrency: 3,
            fail_open: false,
        }
    }

    /// 构造一份最小清单（单包模式，校验值按参数给）
    fn manifest_with_hash(sha256: Option<&str>) -> manifest::FlatManifest {
        manifest::FlatManifest {
            version: "9.9.9".to_string(),
            update: true,
            size_bytes: None,
            sha256: sha256.map(str::to_string),
            url: "https://example.test/pkg.zip".to_string(),
            parts: None,
            part_sha256: None,
            release_date: None,
            changelog: Vec::new(),
            min_version: None,
            force_update: None,
            store_url: None,
        }
    }

    // ---------- 应用名 / 版本号的路径安全 ----------

    #[test]
    fn normalize_app_name_keeps_safe_names() {
        assert_eq!(normalize_app_name("qsm".to_string()), "qsm");
        assert_eq!(normalize_app_name("我的应用".to_string()), "我的应用");
        assert_eq!(normalize_app_name("My App".to_string()), "My App");
    }

    #[test]
    fn normalize_app_name_falls_back_for_unsafe_names() {
        for bad in ["../evil", r"C:\evil", "a/b", "", "."] {
            assert_eq!(
                normalize_app_name(bad.to_string()),
                DEFAULT_APP_NAME,
                "非法应用名应回落默认值: {:?}",
                bad
            );
        }
    }

    // ---------- 首次安装 / 强制更新决策 ----------

    #[test]
    fn is_first_install_only_matches_zero_placeholder() {
        assert!(is_first_install("0.0.0"));
        assert!(is_first_install(" 0.0.0 "));
        assert!(!is_first_install("0.0.1"));
        assert!(!is_first_install("0.4.38"));
        assert!(!is_first_install(""));
    }

    #[test]
    fn first_install_starts_download_without_prompt() {
        // 首装没有可运行的旧版本：必须直接下载安装，不能出现「稍后升级」
        let cfg = cfg_for_test();
        let m = manifest_with_hash(Some(&"a".repeat(64)));
        let decision = update_available_decision(&cfg, "0.0.0", m);
        match decision {
            StartupDecision::Show(ctx) => {
                assert_eq!(ctx.initial.stage(), Stage::Downloading);
                assert!(
                    !ctx.initial.force_update_blocking,
                    "首装不应进入需要点击的强制更新提示界面"
                );
                assert!(
                    ctx.manifest.is_some(),
                    "必须携带 manifest，UI 才能自动拉起下载流程"
                );
            }
            _ => panic!("首次安装应直接显示下载进度"),
        }
    }

    #[test]
    fn first_install_overrides_non_forced_prompt() {
        // 服务端没写 force_update/min_version 时，0.0.0 仍按首装直接安装
        let cfg = cfg_for_test();
        let m = manifest_with_hash(Some(&"a".repeat(64)));
        assert!(!manifest::requires_force_update("0.0.0", &m));
        let decision = update_available_decision(&cfg, "0.0.0", m);
        match decision {
            StartupDecision::Show(ctx) => {
                assert_eq!(ctx.initial.stage(), Stage::Downloading);
            }
            _ => panic!("首次安装应直接显示下载进度"),
        }
    }

    #[test]
    fn existing_install_keeps_later_option_when_not_forced() {
        let cfg = cfg_for_test();
        let m = manifest_with_hash(Some(&"a".repeat(64)));
        let decision = update_available_decision(&cfg, "9.9.8", m);
        match decision {
            StartupDecision::Show(ctx) => {
                assert_eq!(ctx.initial.stage(), Stage::ForceUpdate);
                assert!(
                    !ctx.initial.force_update_blocking,
                    "已有安装的非强制更新应保留「稍后升级」"
                );
            }
            _ => panic!("已有安装应展示可稍后升级的提示"),
        }
    }

    #[test]
    fn server_forced_update_stays_blocking() {
        let cfg = cfg_for_test();
        let mut m = manifest_with_hash(Some(&"a".repeat(64)));
        m.force_update = Some(true);
        let decision = update_available_decision(&cfg, "9.9.8", m);
        match decision {
            StartupDecision::Show(ctx) => {
                assert_eq!(ctx.initial.stage(), Stage::ForceUpdate);
                assert!(ctx.initial.force_update_blocking, "服务端强制更新应阻断");
            }
            _ => panic!("服务端强制更新应展示阻断式提示"),
        }
    }

    #[tokio::test]
    async fn run_update_flow_rejects_unsafe_version_before_network() {
        // 版本号会被拼进 app_<version> / update_<version>.zip：非法版本必须
        // 在下载、提权之前被拒，而不是先弹 UAC 再失败
        let cfg = cfg_for_test();
        let mut m = manifest_with_hash(Some(&"a".repeat(64)));
        m.version = "../../Startup".to_string();
        m.url = "http://127.0.0.1:9/unreachable.zip".to_string();
        let handle = PhaseHandle::new();

        let err = run_update_flow(&cfg, &m, &handle)
            .await
            .err()
            .expect("非法版本号应拒绝更新");
        assert!(err.contains("版本号"), "错误应指明版本号问题: {err}");
        assert_eq!(
            handle.snapshot().expect("应写入错误阶段快照").stage(),
            Stage::Error
        );
    }

    #[tokio::test]
    async fn run_update_flow_refuses_package_without_any_hash() {
        // 包完整性闸门的端到端回归：清单没给校验值 → 拒绝自动安装，
        // 且必须在任何下载/提权动作之前返回（此用例连不上网也不该报错成网络问题）
        let cfg = cfg_for_test();
        let m = manifest_with_hash(None);
        let handle = PhaseHandle::new();

        let err = run_update_flow(&cfg, &m, &handle)
            .await
            .err()
            .expect("缺校验值应拒绝安装");
        assert!(err.contains("校验值"), "错误应说明缺校验值: {err}");

        let snap = handle.snapshot().expect("应写入错误阶段快照");
        assert_eq!(snap.stage(), Stage::Error);
        // 界面文案也是契约：主提示要短、副提示要给出逃生口，否则用户只能干等
        assert_eq!(snap.message, "更新包缺少校验值，已拒绝安装");
        assert!(
            snap.sub.contains("require_sha256") && snap.sub.contains("--allow-missing-sha256"),
            "副提示应写明两条放行途径: {}",
            snap.sub
        );
        assert!(
            snap.download_failed,
            "拒装应给出「前往官网」出口（服务端配置问题，本地重试无解）"
        );
    }

    #[tokio::test]
    async fn run_update_flow_allows_package_when_gate_disabled() {
        // 关掉闸门后不得再因缺校验值报错：这里的失败必须来自网络（不可达域名），
        // 而不是配置检查 —— 用错误前缀区分两者
        let mut cfg = cfg_for_test();
        cfg.require_sha256 = false;
        // 只试一次：retry_times=1 时不会进入退避等待，用例保持秒级
        cfg.retry_times = 1;
        let mut m = manifest_with_hash(None);
        m.url = "http://127.0.0.1:9/unreachable.zip".to_string();
        let handle = PhaseHandle::new();

        let err = run_update_flow(&cfg, &m, &handle)
            .await
            .err()
            .expect("地址不可达应下载失败");
        assert!(
            err.starts_with(download::DOWNLOAD_FAIL_PREFIX),
            "应落到下载环节而非校验值闸门: {err}"
        );

        // 下载目录是按 app_name 派生的真实目录，用例自己收干净
        let root = app_dir(&cfg.app_name);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn retry_limit_never_below_one_and_capped() {
        // 配置 0 也必须试一次（「不试」不是合法语义）；上限 10 兜底
        assert_eq!(retry_limit(0, true), 1);
        assert_eq!(retry_limit(3, true), 3);
        assert_eq!(retry_limit(99, true), 10);
        // 不可重试的失败（如下载目录建不出来）只试一次
        assert_eq!(retry_limit(5, false), 1);
    }

    #[test]
    fn fail_open_requires_switch_and_existing_install() {
        let dir = temp_dir("fail_open_install");
        let installed = dir.to_string_lossy().to_string();

        let mut cfg = cfg_for_test();
        assert!(
            !should_fail_open(&cfg, "1.0.0", Some(&installed)),
            "默认关闭：不显式开启绝不跳过检查"
        );
        cfg.fail_open = true;
        assert!(should_fail_open(&cfg, "1.0.0", Some(&installed)));
        assert!(
            !should_fail_open(&cfg, "0.0.0", Some(&installed)),
            "未安装（0.0.0）没有可启动的程序"
        );
        assert!(!should_fail_open(&cfg, "1.0.0", None), "无安装记录不放行");
        assert!(!should_fail_open(&cfg, "1.0.0", Some("  ")));
        let missing = dir.join("gone").to_string_lossy().to_string();
        assert!(
            !should_fail_open(&cfg, "1.0.0", Some(&missing)),
            "记录存在但目录已丢：放行只会得到启动失败"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retry_backoff_grows_then_caps() {
        // 1s → 2s → 3s → …，封顶 5s：持续故障时别拿高频重试捶服务器
        assert_eq!(retry_backoff(1).as_secs(), 1);
        assert_eq!(retry_backoff(3).as_secs(), 3);
        assert_eq!(retry_backoff(9).as_secs(), 5);
    }

    #[test]
    fn retry_title_marks_attempt_only_after_first() {
        // 首次尝试保持常规文案，重试才标「第 N/M 次」
        assert_eq!(retry_title("download", 1, 3), "正在下载更新包...");
        assert_eq!(
            retry_title("download", 2, 3),
            "正在下载更新包...（第 2/3 次）"
        );
        assert_eq!(retry_title("install", 2, 3), "正在安装更新...（第 2/3 次）");
    }

    #[test]
    fn residual_probe_reports_state_without_deleting() {
        // 不存在 → 可重试；存在且没被占用 → 可重试，**且文件必须原地留着**
        // （留着才有 run_download 的「复用已下载包」；旧实现顺手删掉，那条分支成了死路）
        let dir = temp_dir("retry_probe");
        let f = dir.join("update_1.0.0.zip");
        assert!(residual_is_free(&f));

        std::fs::write(&f, b"partial").expect("写残包");
        assert!(residual_is_free(&f));
        assert!(f.exists(), "探测不得删除残留包");
        assert_eq!(
            std::fs::read(&f).expect("读回"),
            b"partial",
            "探测不得改动残留包内容"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 占用探测的判定能力只在 Windows 有强制文件锁语义可验
    #[test]
    #[cfg(windows)]
    fn residual_probe_detects_file_held_by_another_handle() {
        let dir = temp_dir("retry_locked");
        let f = dir.join("update_1.0.0.zip");
        std::fs::write(&f, b"partial").expect("写残包");

        let held = std::fs::File::open(&f).expect("持有句柄");
        assert!(
            !residual_is_free(&f),
            "文件被其他句柄打开时应判定为不可重试（重下也是白下）"
        );
        assert!(f.exists(), "判定为被占用同样不得删除文件");
        drop(held);

        assert!(residual_is_free(&f), "句柄释放后应恢复可重试");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_download_reuses_verified_local_package() {
        // 复用分支的端到端回归：本地已有校验合格的包 → 直接复用，不发起任何网络请求。
        // 用不可达地址做断言依据：走到下载就一定失败，返回 Ok 只可能是复用。
        use sha2::{Digest, Sha256};

        let dir = temp_dir("reuse");
        let zip = dir.join("update_9.9.9.zip");
        std::fs::write(&zip, b"already-downloaded").expect("写本地包");
        let expected = hex::encode(Sha256::digest(b"already-downloaded"));

        let mut m = manifest_with_hash(Some(&expected));
        m.url = "http://127.0.0.1:9/unreachable.zip".to_string();
        let handle = PhaseHandle::new();

        let reused_hash = run_download(&handle, &m, &zip, 1, 3)
            .await
            .expect("校验合格的本地包应被复用");
        assert_eq!(
            reused_hash.as_deref(),
            Some(expected.as_str()),
            "复用分支应返回已算出的实际哈希，供外层免二次读盘"
        );
        assert!(zip.exists(), "复用不得删掉本地包");
        assert_eq!(
            std::fs::read(&zip).expect("读回"),
            b"already-downloaded",
            "复用的是同一个文件，不该被重写"
        );
        let snap = handle.snapshot().expect("应推送阶段快照");
        assert_eq!(snap.message, "使用已下载的更新包");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_download_redownloads_when_local_package_is_corrupted() {
        // 反例：本地包与清单校验值不符 → 丢弃并重下（这里重下必然失败，
        // 失败信息必须来自下载环节而非复用分支）
        use sha2::{Digest, Sha256};

        let dir = temp_dir("reuse_bad");
        let zip = dir.join("update_9.9.9.zip");
        std::fs::write(&zip, b"half-written").expect("写本地包");
        let expected = hex::encode(Sha256::digest(b"the-real-package"));

        let mut m = manifest_with_hash(Some(&expected));
        m.url = "http://127.0.0.1:9/unreachable.zip".to_string();
        let handle = PhaseHandle::new();

        let err = run_download(&handle, &m, &zip, 1, 3)
            .await
            .err()
            .expect("校验不合格的本地包不得复用");
        assert!(
            err.starts_with(download::DOWNLOAD_FAIL_PREFIX),
            "应落到下载环节: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn verify_package_uses_precomputed_hash_without_reading_file() {
        // 下载/合并阶段已算出的哈希应直接用于比对：文件路径故意不存在，
        // 只要没有回落到读盘就会成功
        let handle = PhaseHandle::new();
        let missing = temp_dir("precomputed").join("absent.zip");
        let actual = "ab".repeat(32);

        verify_package(&handle, &missing, &Some(actual.clone()), Some(actual))
            .await
            .expect("给出预计算哈希时不应再读文件");
        let _ = std::fs::remove_dir_all(missing.parent().expect("父目录"));
    }

    #[tokio::test]
    async fn sha_mismatch_is_classified_as_download_failure() {
        // 下载完成但哈希对不上属于下载链路问题：错误必须带统一前缀，
        // 重试用尽后界面才会出现「前往官网重新下载」出口
        let dir = temp_dir("sha_mismatch");
        let zip = dir.join("update_9.9.9.zip");
        std::fs::write(&zip, b"corrupted-payload").expect("写本地包");
        let handle = PhaseHandle::new();
        let expected = "00".repeat(32);
        let actual = "11".repeat(32);

        let err = verify_package(&handle, &zip, &Some(expected), Some(actual))
            .await
            .expect_err("哈希不匹配应失败");
        assert!(
            err.starts_with(download::DOWNLOAD_FAIL_PREFIX),
            "校验失败应归入下载环节: {err}"
        );

        record_download_failure(&handle, &err, 3, 3);
        let snap = handle.snapshot().expect("应写入错误阶段快照");
        assert_eq!(snap.stage(), Stage::Error);
        assert!(
            snap.download_failed,
            "哈希不匹配应提供「前往官网重新下载」出口"
        );
        assert_eq!(snap.failed_stage, "download");
        assert_eq!(snap.attempt, 3);
        assert_eq!(snap.max_attempts, 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn absolute_launch_prefers_same_named_exe_in_install_dir() {
        // 核心回归：交接模式下 --launch 是旧版客户端传入的自身绝对路径，
        // 更新完成后必须启动新安装目录内的同名 exe —— 否则启动的是旧
        // 二进制（旧目录还在时）或直接启动失败（旧目录被清理时）。
        let install = temp_dir("prefer_new");
        std::fs::write(install.join("qsm.exe"), b"new").expect("写新版本 exe");
        let old = temp_dir("old_bin");
        std::fs::write(old.join("qsm.exe"), b"old").expect("写旧版本 exe");

        let resolved = resolve_launch_exe(&old.join("qsm.exe").to_string_lossy(), Some(&install));
        assert_eq!(resolved, install.join("qsm.exe"), "必须解析到新安装目录");
        let _ = std::fs::remove_dir_all(&install);
        let _ = std::fs::remove_dir_all(&old);
    }

    #[test]
    fn absolute_launch_falls_back_when_install_dir_lacks_exe() {
        let install = temp_dir("fallback_abs");
        let other = temp_dir("other_bin");
        std::fs::write(other.join("tool.exe"), b"x").expect("写文件");

        let resolved =
            resolve_launch_exe(&other.join("tool.exe").to_string_lossy(), Some(&install));
        assert_eq!(resolved, other.join("tool.exe"));
        let _ = std::fs::remove_dir_all(&install);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn relative_launch_resolves_in_install_dir() {
        let install = temp_dir("relative");
        std::fs::write(install.join("qsm.exe"), b"new").expect("写 exe");
        let resolved = resolve_launch_exe("qsm.exe", Some(&install));
        assert_eq!(resolved, install.join("qsm.exe"));
        let _ = std::fs::remove_dir_all(&install);
    }

    #[test]
    fn nested_install_dir_found_by_name() {
        // zip 根带一层目录：app_2.0.0/bin/qsm.exe，--launch 指旧版 bin 路径，
        // 直接拼接未命中，应递归（限深）找到嵌套目录内的新 exe
        let install = temp_dir("nested");
        let bin = install.join("bin");
        std::fs::create_dir_all(&bin).expect("建 bin");
        std::fs::write(bin.join("qsm.exe"), b"new").expect("写 exe");
        let old = temp_dir("old_nested");
        std::fs::write(old.join("qsm.exe"), b"old").expect("写旧 exe");

        let resolved = resolve_launch_exe(&old.join("qsm.exe").to_string_lossy(), Some(&install));
        assert_eq!(
            resolved,
            bin.join("qsm.exe"),
            "应递归找到嵌套目录内的新 exe"
        );
        let _ = std::fs::remove_dir_all(&install);
        let _ = std::fs::remove_dir_all(&old);
    }

    #[test]
    fn absolute_launch_without_install_dir_used_verbatim() {
        let p = PathBuf::from("D:/apps/qsm.exe");
        assert_eq!(resolve_launch_exe(&p.to_string_lossy(), None), p);
    }

    #[test]
    fn argv_registered_then_retry_uses_same_args() {
        // 重试入口依赖登记的原始参数；未登记时 retry 静默跳过
        register_cli_argv(vec!["--app-name".to_string(), "egui-test".to_string()]);
        // 只验证登记路径可用（真正重跑流程需要网络，不在单测覆盖范围）
        assert!(CLI_ARGV.get().is_some());
    }
}
