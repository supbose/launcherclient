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
use crate::defaults::{DEFAULT_APP_NAME, DEFAULT_MANIFEST_URLS, PROGRESS_INTERVAL_MS};
use crate::download;
use crate::install;
use crate::manifest;
use crate::phase::{self, fmt_mb, PhaseHandle, PhasePayload, Stage};
use crate::services;

/// 配置合并后的生效配置（优先级：命令行 > update.json > 默认值）
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub app_name: String,
    /// manifest 地址列表（按序故障转移）
    pub manifest_urls: Vec<String>,
    pub launch: Option<String>,
    /// 保留的版本目录数量（含当前版本）
    pub max_versions: usize,
    /// 更新前需停止、更新后恢复的 Windows 服务名列表
    pub services: Vec<String>,
    /// 生效的启动器界面模板（A/B/C/D/E）
    pub template: String,
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

/// 合并命令行参数与 update.json 配置
pub fn resolve_config(args: &CliArgs) -> ResolvedConfig {
    let settings = config::load();

    let app_name = if args.app_name != "QsLtd" {
        // 命令行显式指定了应用名
        args.app_name.clone()
    } else {
        non_empty(settings.app_name.clone()).unwrap_or_else(|| DEFAULT_APP_NAME.to_string())
    };

    // 界面模板优先级：命令行 --template > update.json launcher_template > 内置默认（C）。
    // 未配置或写了未知值时 resolve 内部回落默认，界面不会因配置笔误白屏。
    let settings_template = settings.launcher_template();
    let template = config::resolve_launcher_template(
        args.template.as_deref().or(settings_template.as_deref()),
    );

    ResolvedConfig {
        app_name,
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
    }
}

/// 按 app_name 派生下载目录
pub fn download_dir(app_name: &str) -> PathBuf {
    qingshu::update::get_myapp_dir(Some(app_name)).join("downloads")
}

/// 按 app_name 派生版本根目录（各版本目录的父目录）
fn install_root(app_name: &str) -> PathBuf {
    qingshu::update::get_myapp_dir(Some(app_name)).join("app")
}

/// 按 app_name 派生版本安装目录（app_{version}）
fn install_dir(app_name: &str, version: &str) -> PathBuf {
    install_root(app_name).join(format!("app_{}", version))
}

/// 本地版本记录 app.json 路径
fn local_manifest_path(app_name: &str) -> PathBuf {
    qingshu::update::get_myapp_dir(Some(app_name)).join("app.json")
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
/// 退出码：0 = 无更新，1 = 检测失败，2 = 有更新。
pub fn check_only_exit(args: &CliArgs) -> i32 {
    let cfg = resolve_config(args);
    match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
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

    if manifest::need_update(&current_version, &m) {
        eprintln!("[更新器] 发现新版本: {} → {}", current_version, m.version);
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
    // 先清理上次删除失败的残留目录
    let app_root = install_root(&cfg.app_name);
    cleanup::cleanup_pending(&app_root).await;

    // 当前版本：命令行 > update.json > 本地 app.json > 0.0.0（视为未安装/损坏自恢复）
    let current_version = resolve_current_version(args, &cfg.app_name).await;
    eprintln!("[更新器] 当前版本: {}", current_version);

    eprintln!("[更新器] 开始拉取 manifest...");
    let m = match manifest::fetch_with_failover(&cfg.manifest_urls).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[更新器] 检查更新失败: {}", e);
            return StartupDecision::Show(Box::new(StartupContext {
                cfg: cfg.clone(),
                manifest: None,
                initial: PhasePayload::new(Stage::Error, "更新检查失败", &e, None),
            }));
        }
    };

    // 登记更新日志：此后所有阶段载荷都携带它，界面在发现新版本起到
    // 更新完成全程可见（下载/安装期间用户正好有空看更新了什么）
    phase::set_changelog(m.changelog.clone());

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
        // ---- 发现新版本：UI 显示"发现新版本"，后台执行下载 → 校验 → 安装 ----
        let is_parts = m.parts.as_ref().is_some_and(|p| !p.is_empty());
        eprintln!(
            "[更新器] 发现新版本: {} -> {}（{}）",
            current_version,
            m.version,
            if is_parts {
                format!("分卷 {} 个", m.parts.as_ref().map_or(0, Vec::len))
            } else {
                format!("下载地址: {}", m.url)
            }
        );
        let sub = format!("{} → {}", current_version, m.version);
        StartupDecision::Show(Box::new(StartupContext {
            cfg: cfg.clone(),
            manifest: Some(m),
            initial: PhasePayload::new(Stage::Available, "发现新版本", &sub, None),
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
                "[更新器] 启动，应用: {}，界面模板: {}，manifest 地址 {} 个: {}",
                cfg.app_name,
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
async fn run_check_flow(args: &CliArgs, cfg: &ResolvedConfig, handle: &PhaseHandle) -> Result<(), String> {
    let current_version = resolve_current_version(args, &cfg.app_name).await;
    eprintln!("[更新器] 当前版本: {}", current_version);

    handle.set(PhasePayload::new(Stage::Checking, "正在检查更新...", "", None));

    // manifest 拉取走 TTL 缓存（5 分钟内重复启动免联网）；重试场景准确优先，
    // 与 Tauri 版一致：首跑用缓存，重试同样命中（5 分钟窗口内服务端无变化）
    let m = match manifest::fetch_with_cache(&cfg.manifest_urls, &cfg.app_name).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[更新器] 检查更新失败: {}", e);
            handle.set(PhasePayload::new(Stage::Error, "更新检查失败", &e, None));
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
    handle.set(PhasePayload::new(Stage::Available, "发现新版本", &sub, None));
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
    let is_parts = m.parts.as_ref().is_some_and(|p| !p.is_empty());
    let zip_path = download_dir(&cfg.app_name).join(format!("update_{}.zip", m.version));

    // 下载：分卷模式（parts 存在）或单包模式
    if is_parts {
        run_download_parts(handle, m, &cfg.app_name, &zip_path).await?;
    } else {
        if m.url.is_empty() {
            let e = "manifest 缺少 url 字段（单包模式必填）".to_string();
            eprintln!("[更新器] 下载失败: {}", e);
            handle.set(PhasePayload::new(Stage::Error, "更新包下载失败", &e, None));
            return Err(e);
        }
        run_download(handle, m, &zip_path).await?;
    }

    // SHA256 校验
    handle.set(PhasePayload::new(Stage::Checking, "正在校验更新包...", "", None));
    if let Err(e) = tokio::task::spawn_blocking({
        let zip_path = zip_path.clone();
        let sha256 = m.sha256.clone();
        move || download::verify_sha256(&zip_path, &sha256)
    })
    .await
    .unwrap_or_else(|e| Err(format!("校验任务异常: {}", e)))
    {
        eprintln!("[更新器] 更新包校验失败: {}", e);
        handle.set(PhasePayload::new(Stage::Error, "更新包校验失败", &e, None));
        return Err(e);
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
                svc_handle.set(PhasePayload::new(Stage::Installing, "正在处理服务...", msg, None));
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

    // 安装
    let target_dir = install_dir(&cfg.app_name, &m.version);
    let install_handle = handle.clone();
    let install_cb: install::StageCallback =
        Arc::new(move |msg, pct| install_handle.set(PhasePayload::new(Stage::Installing, "正在安装更新...", msg, pct)));
    if let Err(e) = install::install(&zip_path, &target_dir, install_cb).await {
        eprintln!("[更新器] 安装失败: {}", e);
        handle.set(PhasePayload::new(Stage::Error, "更新安装失败", &e, None));
        return Err(e);
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
        handle.set(PhasePayload::new(Stage::Error, "本地版本记录写入失败", &e, None));
        return Err(e);
    }

    // 清理本次更新的下载缓存（zip + 分卷），downloads 回归暂存区语义
    cleanup_download_cache(&cfg.app_name, &m.version).await;

    // 清理旧版本目录（旧目录即回滚资产，保留 max_versions 个）
    cleanup::cleanup_old_versions(&install_root(&cfg.app_name), &m.version, cfg.max_versions).await;

    let done_msg = format!("已更新到版本 {}", m.version);
    handle.set(PhasePayload::new(Stage::Done, "更新完成", &done_msg, None));

    // 更新完成后拉起目标程序（launch 留空时自动定位主程序启动）
    let outcome = match &cfg.launch {
        Some(_) => launch_target(cfg, Some(&target_dir)),
        None => {
            match install::find_main_executable(&target_dir, &cfg.app_name) {
                Some(exe) => spawn_detached(&exe),
                None => Err(format!(
                    "安装目录中未找到可执行文件: {}",
                    target_dir.display()
                )),
            }
        }
    };
    if let Err(e) = outcome {
        // 安装已成功（app.json 已写入新版本），仅启动失败：保留错误态，
        // 用户可点「重试」重走检测（无更新分支会再次尝试启动）或手动启动
        eprintln!("[更新器] 更新完成但启动失败: {}", e);
        handle.set(PhasePayload::new(Stage::Error, "更新已完成，但启动应用失败", &e, None));
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
    zip_path: &Path,
) -> Result<(), String> {
    let urls = m.parts.as_ref().ok_or("分卷地址列表缺失")?;
    let parts_dir = download_dir(app_name).join(format!("update_{}_parts", m.version));
    let sha256s = m.part_sha256.as_deref().filter(|s| s.len() == urls.len());
    // 整体总大小（manifest.size_bytes），分卷之和等于完整 zip 大小
    let total_size = m.size_bytes;

    handle.set(PhasePayload::new(
        Stage::Downloading,
        "正在下载更新包（分卷）...",
        &format!("共 {} 卷", urls.len()),
        Some(0.0),
    ));

    let tracker = std::sync::Mutex::new(SpeedTracker::new());
    let handle_cb = handle.clone();
    let result = download::download_parts(urls, sha256s, &parts_dir, total_size, move |i, total, p| {
        let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
        let (should, speed) = t.tick(p.downloaded, p.percent() == Some(100.0));
        if should {
            let msg = format!("正在下载更新包（分卷 {}/{}）...", i + 1, total);
            // 总大小未知时按卷数估算百分比
            let percent = p
                .percent()
                .unwrap_or(((i + 1) as f64 / total as f64) * 100.0);
            handle_cb.set(PhasePayload::download(
                &msg,
                Some(format!("分卷 {}/{}", i + 1, total)),
                Some(percent),
                p.downloaded,
                p.total,
                Some(speed),
            ));
        }
    })
    .await?;

    handle.set(PhasePayload::new(Stage::Checking, "正在合并分卷...", "", Some(0.0)));
    eprintln!("[更新器] 合并 {} 个分卷...", result.part_paths.len());
    let merge_handle = handle.clone();
    download::merge_parts(&result.part_paths, zip_path, &m.sha256, move |done, total| {
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
    })
    .await?;
    eprintln!("[更新器] 分卷合并完成: {}", zip_path.display());
    Ok(())
}

/// 执行下载：本地已有同名包且校验通过则复用，否则流式下载并推送进度
async fn run_download(
    handle: &PhaseHandle,
    m: &manifest::FlatManifest,
    zip_path: &Path,
) -> Result<(), String> {
    // 复用前提：manifest 提供了非空 SHA256。无校验值时 verify_sha256 直接
    // 放行，等于把上次下载中断的半包当完整包复用（后面安装的就是损坏包），
    // 故强制重新下载。
    let has_hash = m
        .sha256
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if zip_path.exists() && has_hash {
        let cached = zip_path.to_path_buf();
        let sha256 = m.sha256.clone();
        let ok = tokio::task::spawn_blocking(move || download::verify_sha256(&cached, &sha256))
            .await
            .unwrap_or_else(|e| Err(format!("校验任务异常: {}", e)))
            .is_ok();
        if ok {
            let size = tokio::fs::metadata(zip_path).await.map(|m| m.len()).unwrap_or(0);
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
            return Ok(());
        }
        eprintln!("[更新器] 本地更新包校验不通过，重新下载");
        let _ = tokio::fs::remove_file(zip_path).await;
    }

    if let Some(parent) = zip_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建下载目录失败 {}: {}", parent.display(), e))?;
    }

    handle.set(PhasePayload::new(Stage::Downloading, "正在下载更新包...", "", Some(0.0)));
    let tracker = std::sync::Mutex::new(SpeedTracker::new());
    let handle_cb = handle.clone();
    let result = download::download_file(&m.url, zip_path, move |p| {
        let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
        let (should, speed) = t.tick(p.downloaded, p.percent() == Some(100.0));
        if should {
            handle_cb.set(PhasePayload::download(
                "正在下载更新包...",
                None,
                p.percent(),
                p.downloaded,
                p.total,
                Some(speed),
            ));
        }
    })
    .await;

    match result {
        Ok((_, sha)) => {
            let size = tokio::fs::metadata(zip_path).await.map(|m| m.len()).unwrap_or(0);
            eprintln!(
                "[更新器] 下载完成，大小 {}，SHA256: {}",
                crate::phase::format_bytes(size),
                sha
            );
            Ok(())
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
        match qingshu::update::LocalManifest::load(&path).await {
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
    match qingshu::update::LocalManifest::load(&path).await {
        Ok(Some(m)) if !m.installed_path.is_empty() => Some(m.installed_path),
        _ => None,
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
        let d = std::env::temp_dir().join(format!(
            "qsltd_flow_test_{}_{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
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

        let resolved = resolve_launch_exe(&other.join("tool.exe").to_string_lossy(), Some(&install));
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
        assert_eq!(resolved, bin.join("qsm.exe"), "应递归找到嵌套目录内的新 exe");
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
