# qsltdupdater-zsui 使用说明

QsLtd 独立更新程序的 **zsui 原生 UI 版**，与 `src-tauri/`（Tauri 2 + Vue 版）功能对等：检查更新 → 下载（SHA256 校验、分卷支持）→ 安装 → 拉起目标程序。

## 环境要求

- Rust（stable，edition 2021）
- 本机源码依赖（path 依赖，构建前确认目录存在）：

| 依赖 | 路径（相对本项目） | 说明 |
|------|-------------------|------|
| zsui | `../../zsui` | 原生 UI 框架 0.3.0-alpha.1 源码（crates.io 未发布该版本，勿改为 crates 依赖） |
| qingshu | `../../../qingshu.work/rust/qingshu` | 统一能力库（feature `update`）：本地版本记录（app.json）等更新核心 |

即目录布局为：

```
d:\WorkspaceProject\cnb\
├── qingshu.work\rust\qingshu\   ← qingshu 统一能力库源码
└── supbose\
    ├── launcherclient\src-zsui\   ← 本项目
    └── zsui\                      ← zsui 源码（git clone --depth 1 https://github.com/supbose/zsui）
```

## 构建

```powershell
cd src-zsui

# 开发构建
cargo build

# 发布构建（opt-level 3 + LTO，release 模式无控制台窗口）
cargo build --release
```

产物：`target\debug\qsltdupdater-zsui.exe` / `target\release\qsltdupdater-zsui.exe`

## 命令行参数

```
qsltdupdater-zsui.exe --app-name <名称> [--check-only] [--launch <目标程序路径>] [--manifest <manifest地址>] [--current-version <版本>]
```

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--app-name` | 应用名称，决定配置/下载/安装目录（`%APPDATA%\<app_name>`） | `qsm` |
| `--check-only` | 纯检测模式：只判断是否有更新，不建窗、不下载、不拉回目标程序。**退出码**：`0`=无更新、`1`=检测失败、`2`=有更新 | 关闭 |
| `--launch` | 更新完成后拉起的目标程序；相对路径按安装目录解析；缺省为仅检测模式（无更新静默退出，有更新弹窗，完成后等用户关闭） | 无 |
| `--manifest` | 服务器 manifest（version.json）地址，覆盖 update.json 与默认地址 | 内置默认地址 |
| `--current-version` | 当前已安装版本，覆盖本地 app.json 记录 | 自动探测 |

服务处理（`update.json` 的 `services` 数组）：确定安装后、解压前**卸载**列表中的服务（运行中的先停止再卸载，15s 停止超时也继续卸载）；卸载后不恢复、不重启（未安装的服务跳过）。

优先级：命令行参数 > update.json > 内置默认值。

## 配置文件 update.json

与更新器 exe 同目录（其次当前工作目录），首次运行不存在时自动生成模板：

```json
{
  "manifest_url": ["https://qsc.cdn.qcsm.ltd/update/qsm/version.json","https://qsw.us.ci/update/qsm/version.json"],
  "app_name": "qsm",
  "launch": "",
  "current_version": "",
  "max_versions": 2,
  "services": ["QsmServer"]
}
```

- `manifest_url`：单地址字符串或地址数组（数组按序故障转移）
- `launch`：留空为仅检测模式（目标程序拉起场景）
- `max_versions`：保留的版本目录数量（含当前版本，旧目录即回滚资产）
- `services`：更新前需**卸载**的 Windows 服务名数组（如 `["server","qsmserver"]`，可多个）；运行中的先停止后卸载，卸载后不恢复不重启；空或缺省不处理

## 更新流程与 UI

1. **启动决策（窗口创建前）**：清理残留 `_del_*` 目录 → 解析当前版本（损坏自恢复为 0.0.0 触发重装）→ 拉取 manifest（连接超时 3s / 整体 5s，地址故障转移）→ 比较版本
   - **无更新 + launch 模式**：静默拉起目标程序后退出，**全程无窗口**
   - **无更新 + 仅检测模式**（无 --launch，由目标程序启动时拉起）：静默退出，不打扰目标程序
2. **发现新版本**：窗口显示"发现新版本"，后台线程开始下载，UI 切换到下载阶段
3. **下载**：分卷（`parts`）或单包（`url`）模式；连接超时 10s、读空闲超时 60s（弱网防挂死）；300ms 节流推送进度，速度指数平滑；已下载包校验通过则复用（断点续传思想）
4. **校验**：整体 SHA256 比对
5. **服务处理**：卸载 `services` 中的服务（运行中的先停止，15s 停止超时也继续卸载），UI 显示"正在处理服务..."
6. **安装**：解压到临时目录 → 原子重命名为 `app_<版本>` 目录（版本目录隔离，不影响正在运行的旧版本）
7. **收尾**：写本地版本记录 app.json → 清理旧版本目录（保留 `max_versions` 个）→ launch 模式拉起目标程序并退出；仅检测模式显示"更新完成"等待关闭

UI 界面（440×300，置顶）：状态徽标 + 主提示 + 副说明 + 进度条/关闭按钮。

## 客户端集成（demo.exe + update.exe 场景）

推荐**两阶段检测**：客户端启动时先用 `--check-only` 纯检测（约 0.3s），无更新则客户端自己继续启动（**单次启动，无交接开销**）；检测到新版本才交接更新器下载安装。

```
启动 demo.exe
   │ 阶段 1：spawn 更新器 --check-only，轮询等待退出（25ms 间隔，12s 上限防挂死）
   │    ├─ 退出码 0（无更新）/ 1（检测失败）/ 超时 → demo 继续正常启动
   │    └─ 退出码 2（有更新）→ 阶段 2
   └─ 阶段 2：spawn 更新器 --launch <demo自身> → demo 退出自身
        └─ 更新器下载安装到新版本目录 → 自动重启 demo（带 QSLTD_FROM_UPDATER=1 标记）→ 退出
```

对比旧的「启动即交接」方案（demo 启动即退出、更新器再拉回）：无更新时消除了**双重冷启动**开销，启动速度显著提升；只有真的有更新时才付出交接成本。

demo.exe 侧（Rust 示例，放在 `main()`/`setup` 中）：

```rust
fn spawn_update_check() {
    // 由更新器拉回的启动不再重复检测，防止「拉回 → 再交接 → 再拉回」循环
    if std::env::var("QSLTD_FROM_UPDATER").is_ok() {
        return;
    }
    // 阶段 1：spawn 更新器 --check-only --app-name <名称>，
    // try_wait 轮询退出码；退出码 0/1/超时 → 继续启动；
    // 退出码 2 → 阶段 2：spawn 更新器 --launch <自身> 后退出自身
}
```

防重入机制：更新器拉起目标程序时注入环境变量 `QSLTD_FROM_UPDATER=1`；客户端启动检测前检查该变量，存在则跳过。

已在正式客户端 qsm 与 [launcherclient-demo](../../launcherclient-demo)（Tauri 2 客户端）中实装并端到端验证：`spawn_update_check()` 在服务端连接**之前**执行（qsm 位于 `src-tauri/src/main.rs`，demo 位于 `src-tauri/src/lib.rs`）。

部署约定：更新器 exe（src-tauri 版为 `launcherclient.exe`，本 zsui 版为 `qsltdupdater-zsui.exe`）与客户端 exe 同目录，旁边放 `update.json`（`app_name` 与客户端一致；`--launch` 由客户端命令行传入优先级最高）。

另一种集成：**仅检测模式**——客户端 spawn 更新器时不带 `--launch` 且不退出自身：无更新静默退出，有更新弹窗下载安装（不占用正在运行的客户端），重启后生效。适合不希望客户端中途退出的场景。

## 数据目录

```
%APPDATA%\<app_name>\
├── app.json                 # 本地版本记录
├── app\app_<版本>\          # 各版本安装目录
└── downloads\               # 更新包与分卷缓存
```

## 冒烟测试

```powershell
# --check-only 纯检测：无更新静默退出，$LASTEXITCODE 得 0
.\target\debug\qsltdupdater-zsui.exe --check-only --app-name qsm; $LASTEXITCODE

# --check-only 有更新（app.json 版本低于 manifest）：退出码 2
.\target\debug\qsltdupdater-zsui.exe --check-only --app-name qsm; $LASTEXITCODE

# 无更新（launch 模式）：应静默拉起目标程序后退出
.\target\debug\qsltdupdater-zsui.exe --current-version 9.9.9 --launch notepad.exe

# manifest 不可达：--check-only 退出码 1；普通模式应显示错误窗口
.\target\debug\qsltdupdater-zsui.exe --check-only --manifest http://127.0.0.1:9/version.json; $LASTEXITCODE

# 发现新版本并真实下载（测试后删除 %APPDATA%\qsm-smoke）
.\target\debug\qsltdupdater-zsui.exe --current-version 0.0.0 --app-name qsm-smoke
```

## 代码结构

| 文件 | 职责 |
|------|------|
| `src/main.rs` | 入口：CLI 解析 → `--check-only` 纯检测分支 → 启动决策 → 进入 UI 循环 |
| `src/updater.rs` | 流程编排：`run_startup` 决策、`run_check_only` 纯检测（退出码 0/1/2）、后台线程下载流程、进度节流与速度平滑 |
| `src/ui.rs` | zsui State/Msg/View：共享快照 + 居中布局视图（7 阶段） |
| `src/cli.rs` `src/config.rs` `src/manifest.rs` `src/download.rs` `src/install.rs` `src/cleanup.rs` `src/services.rs` | 纯后端模块，与 `src-tauri` 版逐字相同 |

架构要点：启动决策在窗口创建前完成（无更新 + launch 模式不创建窗口；check-only 全程无窗口）；下载在 `std::thread` + 独立 tokio runtime 执行，通过 `UiProxy`（`Arc<Mutex<UiSnapshot>>` + `request_rebuild`）驱动视图刷新，UI 线程零阻塞。