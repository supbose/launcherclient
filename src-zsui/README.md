# qsltdupdater-zsui 使用说明

QsLtd 独立更新程序的 **zsui 原生 UI 版**，与 `src-tauri/`（Tauri 2 + Vue 版）功能对等：检查更新 → 下载（SHA256 校验、分卷支持）→ 安装 → 拉起目标程序。

## 环境要求

- Rust（stable，edition 2021）
- 本机源码依赖（path 依赖，构建前确认目录存在）：

| 依赖 | 路径（相对本项目） | 说明 |
|------|-------------------|------|
| zsui | `../../zsui` | 原生 UI 框架 0.3.0-alpha.1 源码（crates.io 未发布该版本，勿改为 crates 依赖） |
| qsltd-update | `../../cargo-project/qsltd-update` | 本地版本记录（app.json）等更新核心库 |

即目录布局为：

```
d:\WorkspaceProject\cnb\supbose\
├── launcherclient\src-zsui\   ← 本项目
├── zsui\                      ← zsui 源码（git clone --depth 1 https://github.com/supbose/zsui）
└── cargo-project\qsltd-update\
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
qsltdupdater-zsui.exe --app-name <名称> [--launch <目标程序路径>] [--manifest <manifest地址>] [--current-version <版本>]
```

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--app-name` | 应用名称，决定配置/下载/安装目录（`%APPDATA%\<app_name>`） | `qsm` |
| `--launch` | 更新完成后拉起的目标程序；相对路径按安装目录解析；**缺省为仅检测模式**（无更新静默退出，有更新弹窗，完成后等用户关闭） | 无 |
| `--manifest` | 服务器 manifest（version.json）地址，覆盖 update.json 与默认地址 | 内置默认地址 |
| `--current-version` | 当前已安装版本，覆盖本地 app.json 记录 | 自动探测 |

优先级：命令行参数 > update.json > 内置默认值。

## 配置文件 update.json

与更新器 exe 同目录（其次当前工作目录），首次运行不存在时自动生成模板：

```json
{
  "manifest_url": ["https://qsw.us.ci/update/qsm/version.json"],
  "app_name": "qsm",
  "launch": "",
  "current_version": "",
  "max_versions": 2
}
```

- `manifest_url`：单地址字符串或地址数组（数组按序故障转移）
- `launch`：留空为仅检测模式（目标程序拉起场景）
- `max_versions`：保留的版本目录数量（含当前版本，旧目录即回滚资产）

## 更新流程与 UI

1. **启动决策（窗口创建前）**：清理残留 `_del_*` 目录 → 解析当前版本（损坏自恢复为 0.0.0 触发重装）→ 拉取 manifest → 比较版本
   - **无更新 + launch 模式**：静默拉起目标程序后退出，**全程无窗口**
   - **无更新 + 仅检测模式**（无 --launch，由目标程序启动时拉起）：静默退出，不打扰目标程序
2. **发现新版本**：窗口显示"发现新版本"，后台线程开始下载，UI 切换到下载阶段
3. **下载**：分卷（`parts`）或单包（`url`）模式；300ms 节流推送进度，速度指数平滑；已下载包校验通过则复用（断点续传思想）
4. **校验**：整体 SHA256 比对
5. **安装**：解压到临时目录 → 原子重命名为 `app_<版本>` 目录（版本目录隔离，不影响正在运行的旧版本）
6. **收尾**：写本地版本记录 app.json → 清理旧版本目录（保留 `max_versions` 个）→ launch 模式拉起目标程序并退出；仅检测模式显示"更新完成"等待关闭

UI 界面（440×300，置顶）：状态徽标 + 主提示 + 副说明 + 进度条/关闭按钮。

## 客户端集成（demo.exe + update.exe 场景）

入口是**客户端 demo.exe 本身**：demo 启动第一行拉起更新器做后台检测，demo 继续自己的启动流程。

```
启动 demo.exe
   │ spawn update.exe（不等待，异步）
   ├─ demo 继续启动 → 后续窗口正常出现
   │
   └─ update.exe 后台检测
        ├─ 无更新 → 静默退出（demo 全程无感知）
        └─ 有更新 → 弹出更新窗口 → 下载安装到新版本目录
                  → 显示"更新完成"，重启 demo 后生效
```

demo.exe 侧（Rust 示例，放在 `main()` 最前面）：

```rust
// 拉起同目录（或 exe 父目录）的更新器做后台检测，不等待不阻塞
fn spawn_update_check() {
    let updater = std::env::current_exe().ok().and_then(|exe| {
        let dir = exe.parent()?;
        // 更新器与 demo 同目录部署；版本目录部署时更新器在上一级
        [dir.join("update.exe"), dir.parent()?.join("update.exe")]
            .into_iter()
            .find(|p| p.exists())
    });
    if let Some(updater) = updater {
        let _ = std::process::Command::new(updater)
            .arg("--app-name")
            .arg("demo")
            .spawn();
    }
}
```

更新器 exe 需与 demo.exe 同目录（或 demo 版本目录的上一级）部署，并放置同名的 `update.json` 配置（`app_name` 与 demo 一致）。

另一种集成：**更新器先行模式**——快捷方式指向 `update.exe --launch demo.exe`，检测无更新时由更新器拉起 demo。适合不希望改 demo 代码的场景。

## 数据目录

```
%APPDATA%\<app_name>\
├── app.json                 # 本地版本记录
├── app\app_<版本>\          # 各版本安装目录
└── downloads\               # 更新包与分卷缓存
```

## 冒烟测试

```powershell
# 无更新（仅检测模式）：应静默退出、无窗口（$LASTEXITCODE 得 0）
.\target\debug\qsltdupdater-zsui.exe --current-version 9.9.9

# 无更新（launch 模式）：应静默拉起目标程序后退出
.\target\debug\qsltdupdater-zsui.exe --current-version 9.9.9 --launch notepad.exe

# manifest 不可达：应显示错误窗口
.\target\debug\qsltdupdater-zsui.exe --manifest http://127.0.0.1:9/version.json

# 发现新版本并真实下载（测试后删除 %APPDATA%\qsm-smoke）
.\target\debug\qsltdupdater-zsui.exe --current-version 0.0.0 --app-name qsm-smoke
```

## 代码结构

| 文件 | 职责 |
|------|------|
| `src/main.rs` | 入口：CLI 解析 → 启动决策 → 进入 UI 循环 |
| `src/updater.rs` | 流程编排：`run_startup` 决策、后台线程下载流程、进度节流与速度平滑 |
| `src/ui.rs` | zsui State/Msg/View：共享快照 + 居中布局视图（7 阶段） |
| `src/cli.rs` `src/config.rs` `src/manifest.rs` `src/download.rs` `src/install.rs` `src/cleanup.rs` | 纯后端模块，与 `src-tauri` 版逐字相同 |

架构要点：启动决策在窗口创建前完成（无更新 + launch 模式不创建窗口）；下载在 `std::thread` + 独立 tokio runtime 执行，通过 `UiProxy`（`Arc<Mutex<UiSnapshot>>` + `request_rebuild`）驱动视图刷新，UI 线程零阻塞。
