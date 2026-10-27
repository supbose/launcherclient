# launcherclient（QsLtd 通用启动器 / 更新器）

通用启动器程序：启动前先向服务器版本清单（version.json）询问是否有更新，**无更新直接拉起目标程序并静默退出，有更新才弹出窗口**下载安装。
仓库同时提供配套的**更新包打包工具 pkgtool**（GUI，由 `scripts/versionTools.py` 重实现而来）。

当前实现为 **Rust + egui/eframe 原生 UI**（无 WebView、无 Node 运行时），产物是单个 exe。

| 项目 | 说明 |
| --- | --- |
| `src-egui` | 启动器 / 更新器主程序，产物 `launcherclient.exe` |
| `src-pkgtool` | 更新包打包工具，产物 `pkgtool.exe` |
| `src-core` | 两个程序共享的无 UI 核心：配置、清单、下载、安装、清理、Windows 服务、阶段编排 |

---

## 1. 目录结构

```
launcherclient/
├── src-core/              # 共享核心库（无 UI，可被任何外壳复用）
│   └── src/  cli.rs config.rs defaults.rs manifest.rs
│              download.rs install.rs cleanup.rs services.rs
│              phase.rs flow.rs
├── src-egui/              # 启动器主程序（egui 0.37）
│   ├── src/    main.rs app.rs templates.rs widgets.rs theme.rs preview.rs
│   ├── assets/icon.ico  icon.png
│   └── version.rc         # exe 图标 + 文件版本信息（build.rs 嵌入）
├── src-pkgtool/           # 打包工具（egui 0.37）
│   ├── src/main.rs
│   ├── assets/icon.ico
│   └── icon.rc            # exe 图标 + 文件版本信息
├── scripts/
│   ├── build_egui.py      # 同步 package.json 版本号 → version.rc → cargo release
│   └── versionTools.py    # 旧版 Python 打包脚本（已由 pkgtool 取代，保留参考）
├── docs/                  # 选型与界面设计文档
├── update.json            # 启动器本地配置文件（示例，随 exe 部署）
├── version.json.example   # 服务器版本清单示例
├── package.json           # 构建脚本入口（pnpm）
└── CHANGELOG.md
```

---

## 2. 环境要求

| 项 | 要求 |
| --- | --- |
| 操作系统 | Windows（exe 图标/版本资源由 `rc.exe` 嵌入；Linux/macOS 可编译但跳过资源嵌入） |
| Rust | ≥ 1.95（egui/eframe 0.37 的 `rust-version` 为 1.95），本机验证 1.97 |
| 工具链 | MSVC（`x86_64-pc-windows-msvc`）+ Windows SDK，需提供 `rc.exe` |
| 本地依赖 | 自有能力库 `qingshu`，以相对路径引用：`../../../qingshu.work/rust/qingshu`（即 `D:\WorkspaceProject\cnb\qingshu.work\rust\qingshu`，需开启 `update` feature）。**该目录必须存在**，否则两个 crate 都无法编译 |
| 包管理器 | pnpm（仅用于运行 `package.json` 里的构建脚本，也可直接 `cargo`） |

---

## 3. 构建与运行

```bash
# 启动器：release 构建（= cd src-egui && cargo build --release）
pnpm build
# 产物：src-egui/target/release/launcherclient.exe

# 启动器：debug 运行
pnpm dev

# 启动器：release 构建 + 先把 package.json 的版本号同步进 version.rc
pnpm egui:build        # python scripts/build_egui.py

# 打包工具：release 构建
pnpm pkgtool
# 产物：src-pkgtool/target/release/pkgtool.exe

# 打包工具：debug 运行
pnpm pkgtool:dev
```

> **版本号同步**：`version.rc` / `icon.rc` 里的 `FILEVERSION`/`FileVersion`/`ProductVersion`
> 是硬编码的 Windows 资源字段。只有 `pnpm egui:build`（`scripts/build_egui.py`）会把它同步为
> `package.json` 的版本；直接 `pnpm build` 只跑 cargo，rc 中的版本号保持上次同步的值。
> 当前 `package.json` 为 `0.1.22`，`src-egui/version.rc` 为 `0.1.21.0`——需要一致时请跑一次 `pnpm egui:build`。

---

## 4. 启动器使用说明

### 4.1 部署

把 `launcherclient.exe` 与 `update.json` 放在**同一目录**即可（配置其次会查找当前工作目录）。
首次运行若找不到 `update.json`，程序会按内置默认值自动生成一份带 `_说明` 字段的模板，不覆盖已有文件。

### 4.2 命令行参数

```
launcherclient.exe [--app-name <名称>] [--launch <目标程序路径>]
                   [--manifest <manifest地址>] [--current-version <版本>]
                   [--template <A|B|C|D|E>] [--check-only]
                   [--preview [--stage <0-7|名称>]]
```

| 参数 | 说明 | 默认 |
| --- | --- | --- |
| `--app-name` | 应用名，决定配置/下载/安装目录与拉取的清单 | 未给时取 update.json 的 `app_name`，再回落内置默认 `qsm` |
| `--launch` | 更新完成后拉起的目标程序；相对路径按安装目录解析；留空则自动定位安装目录中的主程序 | 空 |
| `--manifest` | 服务器 version.json 地址，覆盖 update.json | 见 `defaults::DEFAULT_MANIFEST_URLS` |
| `--current-version` | 强制指定当前版本，覆盖本地 app.json | 自动探测 |
| `--template` | 启动器界面模板 A/B/C/D/E（大小写不敏感） | `C` |
| `--check-only` | 纯检测：不建窗、不下载、不拉起目标程序，按退出码结束 | 关闭 |
| `--preview` / `--stage` | 仅 src-egui：本地预览五套布局与各阶段画面，不联网 | 关闭 |

未知参数只打印告警并忽略，不会中断启动。

`--check-only` 退出码：

| 退出码 | 含义 |
| --- | --- |
| 0 | 无更新 |
| 1 | 检测失败 |
| 2 | 有更新 |

### 4.3 update.json（本地配置）

全部字段可省略，省略或空串回退默认值。**优先级：命令行参数 > update.json > 内置默认值**。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `manifest_url` | 字符串 或 字符串数组 | 服务器 version.json 地址；数组时**按序故障转移**，第一个失败自动换下一个 |
| `app_name` | 字符串 | 应用名，决定本地目录 |
| `launch` | 字符串 | 更新完成后拉起的目标程序 |
| `current_version` | 字符串 | 强制指定当前版本，留空自动从本地 `app.json` 探测 |
| `max_versions` | 数字 | 保留的版本目录数量（含当前版本，旧目录即回滚资产），默认 2 |
| `services` | 字符串数组 | 更新前需卸载的 Windows 服务名；运行中的先停止后卸载，**卸载后不恢复、不重启** |
| `launcher_template` | 字符串 | 界面模板 A/B/C/D/E，大小写不敏感；留空或未知值回落默认 `C`（未知值会在 stderr 告警，不会白屏） |
| `homepage` | 字符串 | 官网首页地址；下载/合并环节的自动重试用尽后，错误态出现「前往官网重新下载」按钮，点击用系统浏览器打开这里。留空则用内置默认 `https://www.qcsm.ltd` |
| `retry_times` | 数字 | 同一环节的自动重试次数（**含首次尝试**），默认 `3` = 首次 + 自动重试 2 次。下载与解压校验各自独立计数；写 `0` 视为 `1`（只试一次），上限 `10` |

示例：

```json
{
  "manifest_url": [
    "https://qsw.us.ci/update/qsm/version.json",
    "https://down.cdn.qcsm.ltd/manifest.json"
  ],
  "app_name": "qsm",
  "launch": "",
  "current_version": "",
  "max_versions": 2,
  "services": ["QsmServer"],
  "launcher_template": "C",
  "homepage": "https://www.qcsm.ltd",
  "retry_times": 3
}
```

### 4.4 version.json（服务器版本清单）

两种模式可随时切换，客户端自动识别：`parts` 非空走分卷，`url` 走单包。

| 字段 | 说明 |
| --- | --- |
| `version` | 最新版本号 |
| `update` | 是否开放更新，`false` 时强制视为无更新（缺省 true） |
| `size_bytes` | 更新包大小（字节） |
| `url` | 单包下载地址（分卷模式下被忽略） |
| `parts` | 分卷地址数组（`.zip.001/.002/...`，按序排列） |
| `part_sha256` | 各分卷 SHA256，与 `parts` 等长时逐卷校验，已下载合格的卷自动复用 |
| `sha256` | 完整 zip 的 SHA256，合并后整体校验（强烈建议填写） |
| `release_date` | 发布时间 |
| `changelog` | 更新日志数组，界面直接展示 |

完整示例见 [`version.json.example`](version.json.example)。

### 4.5 运行流程与阶段

启动后先在后台线程完成首检（窗口**隐藏创建**，仅在真的需要或首检超过约 300ms 时才显示，避免无更新时窗口闪现）：

1. 拉取 manifest（多地址故障转移）
2. 与本地版本比对 → 无更新则拉起目标程序并静默退出；有更新才显示窗口
3. 下载（单包 / 分卷；已下载且校验通过的卷自动复用，不重复下载）
4. 校验 SHA256（分卷逐卷 + 合并后整体）
5. 解压安装到新版本目录（带 zip-slip 与超大条目防护）
6. 卸载 `services` 配置的服务、处理占用的文件
7. 写本地 `app.json`、清理下载缓存与旧版本目录
8. 更新完成 → 拉起目标程序

界面阶段（`PhasePayload.stage`）：`checking` → `available` / `up-to-date` → `downloading` → `installing` → `done` / `error`。

**自动重试（默认 3 次）**：下载环节（拉包、逐卷下载、分卷合并、下载后 SHA256 校验）与解压校验环节
失败会自动重试，节奏为 1s → 2s → 3s → …（封顶 5s），每次重试前清掉失败产物，避免把半包当完整包复用。
重试期间界面停在「正在下载更新包...（第 2/3 次）」，副标题写明上一次失败原因——流程没死，别让用户以为卡住。
次数配置项 `retry_times`（含首次尝试），下载与解压各自独立计数。

**重试用尽仍失败时**：错误界面除「重试」「关闭」外，下载/合并环节失败的场景多一个「前往官网重新下载」按钮，
用系统默认浏览器打开 `homepage`（配置 > 内置默认），用户不必再被卡在没有出口的错误页上；同时错误详情下方
写明「下载失败，已自动重试 2 次（共 3 次尝试）。可点击「前往官网重新下载」获取完整安装包」。
解压校验、磁盘写满这类本地问题换地址也解决不了，不给官网入口，只提示处理后重试，免得把用户往官网引一轮再回来。

### 4.6 本地目录布局

根目录 = `qingshu::update::get_myapp_dir(app_name)`：

- 默认：`%APPDATA%\<app_name>\`（如 `C:\Users\<user>\AppData\Roaming\qsm`）
- 若 exe 同目录存在 `config/config.toml` 或 `config.toml` 且 `[appdata] dir=` 配置了合法路径，则用该路径
- 若运行在 SYSTEM 用户目录，降级到 `%LOCALAPPDATA%\<app_name>` 或 `%PROGRAMDATA%\QsLtdService\<app_name>`

```
<root>/
├── app.json              # 本地版本记录（当前版本等）
├── downloads/            # 下载暂存：完整 zip 与分卷（安装成功后清理，失败保留以便复用）
└── app/
    ├── app_2.1.2/        # 当前版本
    └── app_2.1.1/        # 旧版本 = 回滚资产（保留 max_versions 个）
```

- **回滚**：旧版本目录按 `max_versions` 保留（含当前版本），需要回滚时把启动目标指回旧目录即可。
- **清理**：安装成功后清理本次下载缓存（失败不清理，便于重跑复用），再按版本从旧到新删除超出 `max_versions` 的目录，当前版本目录始终保留。
- **服务**：`services` 中的服务在**安装前**逐个卸载（先停止后卸载，单个失败不中断），卸载后不恢复不重启。

### 4.7 界面模板

| 键 | 布局 |
| --- | --- |
| A | 信息密度 |
| B | 环形居中 |
| C | 顶部步骤条（**默认**） |
| D | 极简留白 |
| E | 日志为主 |

本地预览不需要联网：

```bash
cd src-egui && cargo run -- --preview
cd src-egui && cargo run -- --preview --stage 4   # 0检测中 1下载中 2校验中 3安装中 4发现新版本 5已是最新 6更新完成 7错误
```

---

## 5. 打包工具 pkgtool 使用说明

`pkgtool.exe` 是 `scripts/versionTools.py` 的纯 Rust GUI 重实现：把文件列表压成 `{app}_{version}.zip`，
算完整包 SHA256 与大小，超过单卷上限时切分分卷并逐卷校验，按每个下载前缀各生成一份 version 文件，最后清理旧包。

**与 Python 版的差异**：配置在界面内可编辑并持久化；打包引擎纯 Rust（zip/sha2/walkdir），不依赖 Python 环境。

### 5.1 启动与界面

```bash
pnpm pkgtool:dev          # 开发运行
pnpm pkgtool              # 构建 release：src-pkgtool/target/release/pkgtool.exe
```

窗口居中启动，左侧导航分两组：

| 分组 | 页面 | 内容 |
| --- | --- | --- |
| 工作区 | **配置** | 应用名、输出目录、下载前缀列表、单卷大小上限；「保存配置」写回 `packtool.json` |
| 工作区 | **打包文件** | 待打包文件/目录列表，可新增、删除、用系统对话框「浏览…」选取 |
| 工作区 | **历史构建** | 只读扫描输出目录下的 `{app}_*.zip`（名称 / 体积 / 日期） |
| 分发 | **下载与版本** | 版本号（可「取默认（末位+1）」）、更新日志、开始打包 |
| 分发 | **日志** | 打包全过程日志；打包中顶部显示进度卡片 |

### 5.2 packtool.json（工具配置，与 exe 同目录）

首次启动自动生成，之后自动载入。**注意：界面上改完要点「保存配置」才会落盘。**

| 字段 | 说明 |
| --- | --- |
| `app_name` | 应用名，决定包名 `{app}_{version}.zip` 与旧包清理匹配前缀 |
| `files` | 待打包项列表，见下方路径规则 |
| `download_prefixes` | 下载地址前缀数组，最终地址 = 前缀 + 包名；**每个前缀各生成一份 version 文件** |
| `volume_size_mb` | 单卷大小上限（MB），包体超过则切分为分卷 |
| `output_dir` | 打包输出目录，包体与 version 文件都落在这里 |

`files` 项规则：

| 写法 | zip 内布局 |
| --- | --- |
| `D:\dir\*` 或 `D:\dir/*` | 目录内容**平铺**到 zip 根 |
| `D:\dir`（不带通配） | 保留目录名层级 |
| `D:\path\file.exe` | 仅文件名 |

### 5.3 打包流程

点击「开始打包」后，日志页顶部出现进度卡片（进度条 + 百分比 + 阶段 + 已用时 + 取消按钮），五个阶段：

1. 展开文件清单（配置项 → 实际文件；有不存在的项直接报错并列出）
2. 压缩打包（Deflate）
3. 计算校验值（包体大小 + 完整包 SHA256）
4. 切分分卷（未超限则跳过，切分后删除完整 zip）
5. 生成版本清单（按前缀写 version 文件）+ 清理旧包

- **版本号**：默认取输出目录 `version.json` 的 `version` 末位 +1，不存在时为 `1.0.0`；必须大于线上已发布版本。
- **更新日志**：多条用 `;` 或换行分隔；**留空则沿用上一版**的 changelog。
- **多前缀产出**：第 1 个前缀写 `version.json`，第 2 个起写 `version_2.json`、`version_3.json`…（多 CDN 容灾，各自指向自己域名下的包）。
- **旧包清理**：打包完成后删除输出目录中同 `{app}_` 前缀、且不属于本次包名（含分卷）的旧 zip。
- **取消**：打包中点「取消」立即中止，返回「已取消」且不留下产物。
- 打包在后台线程执行，日志逐条推送并请求重绘，界面不会假死。

---

## 6. 测试

```bash
# 共享核心（纯单元测试）
cd src-core && cargo test --lib

# 服务模块真实 sc.exe 冒烟（需管理员权限，默认关闭）
cd src-core && cargo test --features integration

# 打包工具（含打包/切分/取消/版本号推算等用例）
cd src-pkgtool && cargo test
```

两个 crate 都开启了 `unwrap_used = "deny"`（clippy），新增代码请避免使用 `unwrap()`。

---

## 7. 注意事项

- `version.rc` / `icon.rc` 中的公司名、产品名、版本号是 Windows 资源硬编码字段，改动后需重新构建；改版本号推荐走 `pnpm egui:build`。
- release 构建会 `strip` 符号并关闭 panic 位置信息，用于减小体积、降低杀软启发式误报。
- `src-pkgtool` 的 release profile 刻意放宽了 LTO 与优化级别（`opt-level = 2` / `lto = false`），以规避特定环境下编译 `windows-sys` 的偶发崩溃；`src-egui` 保持 `opt-level = 3` + LTO。
- 更新器只做「下载 → 校验 → 解压 → 拉起」，不做包签名校验；分发链路请自行保证 HTTPS 与 CDN 可信。
- 打包工具目前不含：文件 diff 勾选/筛选、发布通道概念、压缩级别与 SHA-256 开关、构建历史持久化与回滚、日志级别筛选。

---

## 8. 相关文档

| 文档 | 内容 |
| --- | --- |
| [`CHANGELOG.md`](CHANGELOG.md) | 版本变更记录 |
| [`docs/选型决策记录.md`](docs/选型决策记录.md) | 为什么选 egui 而非 Tauri |
| [`docs/Tauri-vs-egui性能对比.md`](docs/Tauri-vs-egui性能对比.md) | 两版体积/启动/内存对比 |
| [`docs/egui界面复刻说明.md`](docs/egui界面复刻说明.md) | 五套模板与阶段画面的复刻说明 |
| [`docs/更新包打包工具-界面重设计.html`](docs/更新包打包工具-界面重设计.html) | pkgtool 界面设计稿（浏览器打开） |
| [`docs/ui-drafts-下载校验解压.html`](docs/ui-drafts-下载校验解压.html) | 下载/校验/解压阶段界面草稿 |
