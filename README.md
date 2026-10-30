# launcherclient（QsLtd 通用启动器 / 更新器）

通用启动器程序：启动前先向服务器版本清单（version.json）询问是否有更新，**无更新直接拉起目标程序并静默退出，有更新才弹出窗口**下载安装。
仓库同时提供配套的**更新包打包工具 pkgtool**（GUI，由 `scripts/versionTools.py` 重实现而来）。

当前实现为 **Rust + egui/eframe 原生 UI**（无 WebView、无 Node 运行时），产物是单个 exe；界面字体**华为 HarmonyOS Sans SC** 已内嵌进 exe，分发时无需附带字体文件（字体缺失时仍会回退系统字体）。

| 项目 | 说明 |
| --- | --- |
| `src-egui` | 启动器 / 更新器主程序，产物 `launcherclient.exe` |
| `src-pkgtool` | 更新包打包工具，产物 `pkgtool.exe` |
| `src-core` | 两个程序共享的无 UI 核心：配置、清单、下载、安装、清理、Windows 服务、阶段编排 |
| `src-ui` | 启动器与打包工具共享的 UI 原语：色板 / 字体 / 主题、标题栏、进度条、圆环、胶囊、按钮（只依赖 egui） |

---

## 1. 目录结构

```
launcherclient/
├── Cargo.toml             # 根 workspace：成员 = src-core + src-egui + src-ui；src-pkgtool 刻意排除（见 §7）
│                          # 也是版本号的 Rust 侧唯一写入点（[workspace.package]），由 pybuild 从 package.json 同步
├── Cargo.lock             # workspace 共享的一份锁文件（三个成员 crate 不再各编一遍依赖）
├── .cnb.yml               # CNB 流水线：版本号一致性 + fmt + clippy + 单测门禁（见 §3.3）
├── target/                # workspace 成员的构建产物（launcherclient.exe 在这里）
├── src-core/              # 共享核心库（无 UI，可被任何外壳复用）
│   └── src/  cli.rs config.rs defaults.rs manifest.rs
│              download.rs install.rs cleanup.rs services.rs
│              appdir.rs local_manifest.rs instance.rs
│              elevation.rs shell.rs client.rs
│              phase.rs flow.rs
│   └── examples/gen_cli_docs.rs  # 从 cli.rs 的 CLI_SPECS 生成 README/手册的参数表（--check / --write）
├── src-ui/                # 共享 UI 原语（只依赖 egui，无产品语义）
│   └── src/    fonts.rs theme.rs widgets.rs
│                # fonts：华为字体加载（内嵌 / 磁盘 / 系统回退）
│                # theme：色板、text_alpha 等色值工具、apply_theme
│                # widgets：标题栏、进度条、圆环、状态点、胶囊、按钮、背景绘制
├── src-egui/              # 启动器主程序（egui 0.37）
│   ├── src/    main.rs app.rs templates.rs widgets.rs theme.rs preview.rs
│   │            # templates：A~H 八套布局（共用 EndStyle/TailStyle/BulletLogStyle 骨架）
│   │            # widgets/theme：启动器专属控件 + 对 qsltd-ui 的转出层
│   ├── assets/icon.ico  icon.png
│   ├── assets/font/                # 华为 HarmonyOS Sans SC（Regular/Medium/Bold）+ LICENSE.txt，编译时内嵌
│   └── version.rc         # exe 图标 + 文件版本信息（build.rs 嵌入）
├── src-pkgtool/           # 打包工具（egui 0.37，独立 crate + 自己的 target/）
│   ├── src/main.rs
│   ├── assets/icon.ico
│   └── icon.rc            # exe 图标 + 文件版本信息
├── scripts/
│   ├── pybuild.py         # 版本铺开（package.json → Cargo/rc）→ cargo release → 产物校验 →（可选）Authenticode 签名
│   ├── build.py           # 全量打包：两个目标依次构建并复制到 dist/<版本号>/
│   └── versionTools.py    # 旧版 Python 打包脚本（已由 pkgtool 取代，保留参考）
├── docs/                  # 选型与界面设计文档
├── update.json            # 启动器本地配置文件（示例，随 exe 部署）
├── version.json.example   # 服务器版本清单示例
├── package.json           # 版本号的**权威来源** + 构建脚本入口（pnpm）
└── CHANGELOG.md
```

---

## 2. 环境要求

| 项 | 要求 |
| --- | --- |
| 操作系统 | Windows（exe 图标/版本资源由 `rc.exe` 嵌入；Linux/macOS 可编译但跳过资源嵌入） |
| Rust | ≥ 1.95（egui/eframe 0.37 的 `rust-version` 为 1.95），本机验证 1.97 |
| 工具链 | MSVC（`x86_64-pc-windows-msvc`）+ Windows SDK，需提供 `rc.exe` |
| 外部依赖 | **无仓库外依赖**。原以相对路径引用的自研能力库 `qingshu`（`../../../qingshu.work/rust/qingshu`）已于 2026-10-30 移除，其用到的两项能力复刻进本仓库：应用数据目录解析 `src-core/src/appdir.rs`、本地版本记录 `app.json` `src-core/src/local_manifest.rs`。克隆后即可直接 `cargo build`，无需再准备同级目录 |
| 包管理器 | pnpm（仅用于运行 `package.json` 里的构建脚本，也可直接 `cargo`） |

---

## 3. 构建与运行

```bash
# 启动器：release 构建（workspace 根执行 = cargo build --release -p qsltdupdater-egui）
pnpm build
# 产物：target/release/launcherclient.exe（字体已内嵌，单文件即可运行）

# 启动器：debug 运行
pnpm dev

# 启动器：release 构建 + 先把版本号从 package.json 铺开到 Cargo 与两个 rc
pnpm egui:build        # python scripts/pybuild.py egui

# 打包工具：release 构建 + 同样的版本铺开（pkgtool 是独立 crate，产物在它自己的目录）
pnpm pkgtool           # python scripts/pybuild.py pkgtool
# 产物：src-pkgtool/target/release/pkgtool.exe

# 打包工具：debug 运行
pnpm pkgtool:dev

# 全量打包：依次打包启动器 + 打包工具，产物复制到 dist/<版本号>/
pnpm dist              # python scripts/build.py

# 只看版本被写到哪些文件、产物会落在哪里（不构建）
python scripts/pybuild.py --show-paths

# 版本号：校验 / 只同步不构建
pnpm version:check     # python scripts/pybuild.py --check-version（CI 用的同一件事）
pnpm version:sync      # python scripts/pybuild.py --sync-version

# CLI 文档：校验 / 重新生成（README §4.2 与使用手册 4.1 的参数表都来自 src-core/src/cli.rs）
pnpm docs:cli:check    # cargo run -q -p qsltd-updater-core --example gen_cli_docs -- --check
pnpm docs:cli:sync     # cargo run -q -p qsltd-updater-core --example gen_cli_docs -- --write
```

### 3.1 构建期产品线参数（qsm:release）

启动器内置默认值（app_name、标题、manifest 地址、官网、模板、重试、版本目录数、服务）默认对应 qsm 产品线；
可通过 `pnpm qsm:release` 追加参数，**编译期烘焙进 exe**（未传项保持 qsm 默认，不影响原有流程）：

```bash
# 默认（不烘焙任何产品线参数，用 src-core 内置 qsm 默认）
pnpm egui:build

# qsm 产品线正式包：烘焙 标题「聚合音乐」+ qsm 双 manifest 容灾 + 模板 F + 卸载 QsmServer
pnpm qsm:release

# 自定义产品线示例：标题 MyApp + myapp + 双 manifest 容灾 + 模板 B + 卸载 MySvc/MyHelper
pnpm qsm:release --name MyApp --app-name myapp --manifest-url https://a.cdn.myapp.ltd/version.json --manifest-url https://b.cdn.myapp.ltd/version.json --homepage https://www.myapp.ltd --template B --retry-times 5 --max-versions 3 --service MySvc --service MyHelper
```

`--name <标题>` 指定启动器标题栏左侧显示的文字（同时作为系统窗口标题），内置默认「启动器」；
不传该参数时标题栏显示默认标题。参数经 `QSLTD_NAME` 烘焙进 exe，作为**该 exe 的默认标题**。
运行时还能再覆盖一层，优先级为 **命令行 `--name` > `update.json` 的 `name` > 编译期烘焙值 > 「启动器」**
（详见 4.2 / 4.3）：同一份 exe 可以给不同部署目录配不同标题，无需重新打包。

参数链路：pybuild.py 校验后转 `QSLTD_*` 环境变量 → `src-core/build.rs` 生成覆盖常量 → `defaults.rs` / `config.rs` 优先采用。
验证方式：把产物放入空目录运行一次，自动生成的 `update.json` 模板即为烘焙后的默认值。

### 3.2 版本号：package.json 是唯一来源

一份版本号在仓库里落在五个文件（两个 rc 各含「数值 + 字符串」两种写法），全部由 `package.json` 的 `version` 派生，写入方只有 `scripts/pybuild.py`：

| 位置 | 形式 | 说明 |
| --- | --- | --- |
| `package.json` → `version` | `0.2.1` 或 `0.2.1.4` | **权威来源**，人工只改这里 |
| `Cargo.toml` → `[workspace.package] version` | `0.2.1` | src-core / src-egui / src-ui 都写 `version.workspace = true` 继承；Cargo 只接受三段 semver，四段版本的第四段进不了这里 |
| `src-pkgtool/Cargo.toml` → `[package] version` | `0.2.1` | 该 crate 不在 workspace 内，单独写一份 |
| `src-egui/version.rc` | `FILEVERSION 0,2,1,0` + `FileVersion`/`ProductVersion` 字符串 | exe 属性面板「文件版本」 |
| `src-pkgtool/icon.rc` | 同上 | 同上 |

> Cargo 的字段继承是**逐字段声明**的：成员只写 `version.workspace = true` 而漏掉 `edition.workspace = true`，
> edition 会静默退回 2015，`async` 代码直接编不过（本项目踩过，故三个成员的 `edition`/`license`/`authors` 都显式继承）。

同步时机与兜底：

- `pnpm egui:build` / `pnpm pkgtool` / `pnpm dist`：**每次构建前自动铺开全部五处**，构建后再回读 exe 内嵌 `FileVersion` 做一致性核对，不一致直接失败——不会再出现「icon.rc 停在 0.1.32」这类漂移。
- `pnpm version:check`（= `python scripts/pybuild.py --check-version`）：只读校验，打印每处「期望/实际」对照表，有漂移时退出码 1。
- `pnpm version:sync`：只铺开、不构建。
- 直接 `pnpm build` / `cargo build` 只跑 cargo，不动 rc —— 想用这条路径出正式包，先跑一次 `version:sync`。

### 3.3 CI 门禁（`.cnb.yml`）

push 与 pull_request 触发同一条流水线（YAML 锚点复用），分两档，理由是「能不能在 Linux 上稳定复现」：

| 档 | 检查 | 说明 |
| --- | --- | --- |
| 硬门禁 | `--check-version` | 版本漂移 = 产物「关于」页与 exe 属性自相矛盾，直接拦 |
| 硬门禁 | CLI 文档一致性（`gen_cli_docs -- --check`） | README / 使用手册里的参数表与 `cli.rs` 的 `CLI_SPECS` 不同步即失败，杜绝「代码加了参数、文档没跟」 |
| 硬门禁 | `cargo fmt --all -- --check` + pkgtool 的 `cargo fmt --check` | 只看源码不触发编译，UI crate 也能覆盖 |
| 硬门禁 | `cargo clippy -p qsltd-updater-core --all-targets -- -D warnings` | 核心库 0 warning（本地已验证） |
| 硬门禁 | `cargo clippy -p qsltd-ui --all-targets -- -D warnings` | 共享 UI 原语只依赖 egui，Linux 不需要图形系统包，可稳定复现 |
| 硬门禁 | `cargo test -p qsltd-updater-core` | 217 个纯单元测试；`integration` feature 不开，不碰 `sc.exe` 与真实网络 |
| 软门禁 | `cargo clippy -p qsltdupdater-egui ...` / pkgtool 同上 | eframe/wgpu 在 Linux 需要 X11/Wayland + OpenSSL + Vulkan 系统包，镜像缺任一项就会变成「和代码无关的红」，故失败只打印不拦合并 |

> 软门禁那两条在 Windows 开发机上是**能过且应当过**的：本地 `cargo clippy --workspace --all-targets -- -D warnings`
> 与 pkgtool 同款命令当前均为 0 warning。等 CI 镜像补齐系统依赖，把 `|| echo` 去掉即可升级为硬门禁。

### 3.4 代码签名（预留，默认不启用）

`scripts/pybuild.py` 支持构建后 Authenticode 签名，**不传参数时构建流程与输出步数完全不变**（三步）：

```bash
# 只签名 + 带时间戳；口令从环境变量取，不落进仓库与 shell 历史
set QSLTD_SIGN_PFX_PASSWORD=<证书口令>      # 发布机上设置，勿写进脚本与 CI 配置文件
python scripts/pybuild.py egui --codesign-pfx D:\certs\qsltd.pfx --codesign-tsa http://timestamp.digicert.com
```

| 参数 | 作用 | 默认 |
| --- | --- | --- |
| `--codesign-pfx <路径>` | 给出才会签名 | 不签名 |
| `--codesign-tsa <地址>` | RFC3161 时间戳；不填会打印「证书过期后历史包全部报红」的警告 | 无时间戳 |
| `--codesign-pass-env <名>` | 存放 pfx 口令的环境变量名 | `QSLTD_SIGN_PFX_PASSWORD` |
| `--signtool <路径>` | signtool.exe 路径 | 取 `PATH`，再回落 Windows SDK 下最新版本 |

签名后会执行 `signtool verify /pa` 复核，失败即构建失败。两点已知限制：
`/p` 传口令仍会让口令出现在那次 signtool 的进程命令行里（发布机更稳妥的做法是证书入库后用 `/sha1` 选择，本脚本暂未提供）；
签名依赖 `signtool`，只能在 Windows 上执行，因此**没有**放进 Linux 流水线。

---

## 4. 启动器使用说明

### 4.1 部署

把 `launcherclient.exe` 与 `update.json` 放在**同一目录**即可（配置其次会查找当前工作目录）。
字体已内嵌进 exe，不需要再附带 `fonts/` 目录；`HarmonyOS-Sans-LICENSE.txt` 是字体许可协议，`scripts/build.py` 会把它复制进 `dist/<版本号>/`，分发时请一并保留。
首次运行若找不到 `update.json`，程序会按内置默认值自动生成一份带 `_说明` 字段的模板，不覆盖已有文件。

### 4.2 命令行参数

<!-- 本节内容由 src-core/src/cli.rs 的 CLI_SPECS 生成。
     改参数请先改代码与解析逻辑，再运行：
     cargo run -p qsltd-updater-core --example gen_cli_docs -- --write -->
<!-- cli-doc:begin -->
```
launcherclient.exe [--app-name <名称>] [--name <标题>]
                   [--launch <目标程序路径>] [--manifest <manifest地址>]
                   [--current-version <版本>] [--template <A~H>]
                   [--check-only] [--allow-missing-sha256] [--preview]
                   [--stage <0-10|名称>]
```

| 参数 | 说明 | 默认 |
| --- | --- | --- |
| `--app-name <名称>` | 应用名，决定配置 / 下载 / 安装目录与拉取的清单 | 未给时取 update.json 的 `app_name`，再回落内置默认 `qsm` |
| `--name <标题>` | 启动器标题：标题栏左侧文字与系统窗口标题。单行、最长 64 字符（换行剔除、超长截断，界面上再按可用宽度以省略号收尾）。只影响显示，不参与目录派生 | 未给时取 update.json 的 `name`，再回落编译期烘焙标题（无烘焙即「启动器」） |
| `--launch <目标程序路径>` | 更新完成后拉起的目标程序；相对路径按安装目录解析 | 空 = 自动定位安装目录中的主程序 |
| `--manifest <manifest地址>` | 服务器 version.json 地址，覆盖 update.json 中的配置 | 见 `defaults::DEFAULT_MANIFEST_URLS` |
| `--current-version <版本>` | 强制指定当前版本，覆盖本地 app.json 的探测结果 | 自动探测 |
| `--template <A~H>` | 界面模板 A~H（大小写不敏感） | 未给时取 update.json 的 `launcher_template`，再回落编译期烘焙值（无烘焙即 `F`） |
| `--check-only` | 纯检测模式：不建窗、不下载、不拉起目标程序，按退出码结束（0 无更新 / 1 检测失败 / 2 有更新可跳过 / 3 必须更新） | 关闭 |
| `--allow-missing-sha256` | 放行「清单没给 SHA256」的更新包，关闭 `require_sha256` 校验闸。仅在服务端确实无法提供校验值时临时使用；只能关不能开 | 关闭（即校验闸开启） |
| `--preview` | 仅 src-egui：本地预览八套布局与各阶段画面，不联网 | 关闭 |
| `--stage <0-10\|名称>` | 仅 src-egui：`--preview` 的阶段。0 检测中、1 下载中（分卷）、2 校验中、3 解压/安装中、4 发现新版本、5 已是最新、6 更新完成、7 错误、8 重试中、9 强制更新（阻断）、10 强制更新（可稍后升级）；也可传中文名称，未知值回落 1 | 1 |
<!-- cli-doc:end -->

未知参数只打印告警并忽略，不会中断启动。

`--check-only` 退出码：

| 退出码 | 含义 |
| --- | --- |
| 0 | 无更新（已是最新，或服务端 `update: false`） |
| 1 | 检测失败（manifest 拉取/解析失败等） |
| 2 | 有更新，可跳过 |
| 3 | 必须更新：首次安装（本地 `0.0.0`，见 4.4）且服务器有可用版本，或服务端 `force_update: true` / 当前版本低于 `min_version` |

> 判定顺序与界面版不同：`--check-only` 先查强制更新条件与首次安装（命中即 `3`），界面版先看有没有可安装的升级包
> （`need_update`）再判阻断。因此「`force_update: true` 但 `version` 已等于本地版本」这种无包可升的清单，
> `--check-only` 返回 `3`，界面版按「已是最新」静默拉起目标程序。正常发布流程（强制更新的清单一定指向更高版本）两者一致。

### 4.3 update.json（本地配置）

全部字段可省略，省略或空串回退默认值。**优先级：命令行参数 > update.json > 内置默认值**。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `manifest_url` | 字符串 或 字符串数组 | 服务器 version.json 地址；数组时**按序故障转移**，第一个失败自动换下一个 |
| `app_name` | 字符串 | 应用名，决定本地目录 |
| `name` | 字符串 | 启动器标题（标题栏左侧文字与系统窗口标题）；单行、最长 64 字符，换行会被剔除、超长会截断。**留空则跟随 exe 内置标题**（构建期 `--name` 烘焙值，未烘焙即「启动器」），这样换一版烘焙了新标题的 exe 覆盖安装即可生效，不会被老配置钉死 |
| `launch` | 字符串 | 更新完成后拉起的目标程序 |
| `current_version` | 字符串 | 强制指定当前版本，留空自动从本地 `app.json` 探测 |
| `max_versions` | 数字 | 保留的版本目录数量（含当前版本，旧目录即回滚资产），默认 2 |
| `services` | 字符串数组 | 更新前需卸载的 Windows 服务名；运行中的先停止后卸载，**卸载后不恢复、不重启** |
| `launcher_template` | 字符串 | 界面模板 A~H，大小写不敏感；留空或未知值回落默认 `F`（未知值会在 stderr 告警，不会白屏） |
| `homepage` | 字符串 | 官网首页地址。两处兜底出口都跳这里：**更新检查失败**（`manifest_url` 全部失效）与**下载/合并环节自动重试用尽**时，错误态出现「前往官网重新下载」按钮，点击用系统浏览器打开。留空则用内置默认 `https://launcherclient.qsw.us.ci/`（构建期 `--homepage` 烘焙值优先） |
| `retry_times` | 数字 | 同一环节的自动重试次数（**含首次尝试**），默认 `3` = 首次 + 自动重试 2 次。下载与解压校验各自独立计数；写 `0` 视为 `1`（只试一次），上限 `10` |
| `require_sha256` | 布尔 | **包完整性校验闸，默认开启**。开启时若清单既没有整包 `sha256`、分卷模式的 `part_sha256` 也没和 `parts` 一一对齐，则在弹 UAC 之前直接拒绝安装，错误态提示改走官网下载。写 `false`（或加命令行 `--allow-missing-sha256`）可放行无校验值的包——只建议在内网/自建源这类链路可信的环境使用。缺省即 `true`，无需显式写 |

示例：

```json
{
  "manifest_url": [
    "https://qsw.us.ci/update/qsm/version.json",
    "https://down.cdn.qcsm.ltd/manifest.json"
  ],
  "app_name": "qsm",
  "name": "",
  "launch": "",
  "current_version": "",
  "max_versions": 2,
  "services": ["QsmServer"],
  "launcher_template": "C",
  "homepage": "https://www.qcsm.ltd",
  "retry_times": 3,
  "require_sha256": true
}
```

### 4.4 version.json（服务器版本清单）

由 `pkgtool` 打包时自动生成（见第五章），也可手写后部署到 `manifest_url` 指向的地址。
除 `version` 外全部字段可选，**未知字段一律忽略**（服务端加字段不会打断老客户端）；
`version` 缺失或类型不符则整份清单解析失败，按「更新检查失败」进入错误态。

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `version` | **必填** | 最新版本号。双方都能按 semver 解析时按 semver 比较（`1.0.0-alpha < 1.0.0`，预发布不会被拽回去），否则退化为「字符串不等即视为有更新」；前后空白会先 trim |
| `update` | 否，默认 `true` | 更新总闸。`false` 时一律视为无更新——**`force_update` / `min_version` 同时失效**，启动器静默拉起目标程序 |
| `url` | 单包模式必填 | 完整 zip 下载地址。单包模式下留空直接报错「manifest 缺少 url 字段（单包模式必填）」，不进入自动重试（重试解决不了配置问题） |
| `parts` | 分卷模式必填 | 分卷地址数组（`.zip.001/.002/...`，按序拼接为完整 zip）。**非空即走分卷，`url` 被忽略**；两种模式随时可切，客户端自动识别 |
| `part_sha256` | 分卷模式建议必填 | 各卷 SHA256。与 `parts` **等长且每项非空**时逐卷校验，长度不符或留有空项则不做逐卷校验；已下载且校验合格的卷在重跑时自动复用，不重复下载。分卷模式下这份「齐全」的数组即可满足 `require_sha256` 闸（见 4.3），无需再给整包 `sha256` |
| `sha256` | **默认必填** | 合并后完整 zip 的 SHA256（十六进制，大小写不敏感），下载结束后整体复核一次；校验按 256 KB 分块流式计算，GB 级包也不会把哈希读进内存。留空 = 跳过整体校验，**并禁用「复用已下载包」**（否则上次中断的半包会被当成完整包直接安装）；在 `require_sha256` 开启时（默认），`sha256` 与齐备的 `part_sha256` **两者都没有 → 直接拒绝安装**，需要放行得显式写 `"require_sha256": false` 或加 `--allow-missing-sha256` |
| `size_bytes` | 否 | 完整包字节数（等于各分卷之和）。**分卷模式**下作为整体进度的分母（单包模式用 HTTP `Content-Length`，不读此字段）；留空时分卷界面只有已下载字节、没有百分比与剩余时间 |
| `release_date` | 否 | 发布时间（ISO 8601，`pkgtool` 写入打包时刻的 UTC 时间）。客户端只解析留存，界面不展示 |
| `changelog` | 否 | 更新日志字符串数组，逐条展示。拉到清单后即登记，「发现新版本 → 下载 → 校验 → 安装 → 完成」全程可见；空数组则界面不出现日志区 |
| `force_update` | 否，默认 `false` | 版本级强制更新标记。`true` 时进入**阻断式**强制更新界面，没有「稍后升级」出口。前提仍是清单 `version` 比本地新——本地已是该版本或更高时无包可升，按「已是最新」处理 |
| `min_version` | 否 | 最低允许版本。当前版本低于它即触发阻断式强制更新，与 `force_update` **任一成立即生效**；留空/缺失 = 不设门槛。强制更新界面显示的「要求版本号」优先取它，否则回落 `version` |
| `store_url` | 否 | 应用商店地址。**非空时**强制更新界面才出现「前往应用商店」按钮，点击用系统默认浏览器打开它；留空则不显示该入口，只留「下载升级包」。打开前会先过一遍校验（见下方「服务端下发链接的校验」），不合格的地址不会送去 shell，而是改开本地配置的 `homepage` |

`min_version` / `force_update` / `store_url` 三个强制更新字段**只由服务端下发**，本地 `update.json` 无法覆盖（写进去也不生效）。

**服务端下发链接的校验**（`shell::sanitize_url`）：这些字符串是远端可控、且可能落到**提权后**的进程里的，
所以不直接拼进 `cmd /C start`。打开前依次要求：能被 URL 解析、协议是 `http`/`https`、有主机名、
重新序列化后不残留裸 `%` 转义、不含 `"` `|` `<` `>` `^` 与控制字符。引号、空格、反斜杠这类会被
URL 归一化（`"` → `%22`），环境变量写法与换行会被解析器吃掉，因此它们到不了 shell。任一条不过即拒绝打开，
`store_url` 的降级动作是改开本地配置的官网首页并在 stderr 记录原因。

**校验闸的时机**：`require_sha256` 的检查放在**弹 UAC 之前**——先让用户点了「是」再告之「这包装不了」是最差的顺序。

> 对照线上清单（`qsm 0.4.37`）：它同时写了 `url` 与 `parts` / `part_sha256`，因为 `parts` 非空，
> 客户端实际走的是**分卷下载**，`url` 只作为手动下载/兜底入口存在——这种「双写」是允许的，不是配置冲突。
> 它也同时给了整包 `sha256` 与 28 条齐备的 `part_sha256`，所以默认开启的 `require_sha256` 闸不影响现网发布。

**判定顺序**（`flow::run_startup`）：`update` 开关 → 版本比较 → 首次安装（本地版本为 `0.0.0`，直接下载安装，不展示选择界面）→ 是否触发强制更新（`min_version` 或 `force_update`）→ 否则展示「发现新版本，可稍后升级」。

**首次安装**：`app.json` 缺失、读取失败、或其中记录的安装目录已不存在时，本地版本按 `0.0.0` 处理。此时机器上没有可启动的程序，「稍后升级」只会换来一次启动失败，因此只要服务器有可用版本就**直接进入下载安装**（界面从下载进度开始，没有「稍后升级」出口）；`--check-only` 同样返回 `3`。

**联网与缓存**：首检（`run_startup`）与 `--check-only` 每次都直连服务器；只有界面内「重试 / 错误态重跑」走 5 分钟 TTL 缓存（`<root>/manifest_cache.json`，按 `app_name` 隔离，拉取失败时不覆盖旧缓存）。刚发布新版本又恰好命中重试路径，最长 5 分钟内可能仍读到旧判定。

完整示例见 [`version.json.example`](version.json.example)。

### 4.5 运行流程与阶段

启动后先在后台线程完成首检（窗口**隐藏创建**，仅在真的需要或首检超过约 300ms 时才显示，避免无更新时窗口闪现）：

1. 拉取 manifest（多地址故障转移）
2. 与本地版本比对 → 无更新则拉起目标程序并静默退出；首次安装（本地 `0.0.0`：`app.json` 缺失 / 记录目录丢失）直接进入下载安装，不展示「稍后升级」；其余有更新才显示窗口
3. 下载（单包 / 分卷；已下载且校验通过的卷自动复用，不重复下载）
4. 校验 SHA256（分卷逐卷 + 合并后整体）
5. 解压安装到新版本目录（带 zip-slip 与超大条目防护）
6. 卸载 `services` 配置的服务、处理占用的文件
7. 写本地 `app.json`、清理下载缓存与旧版本目录
8. 更新完成 → 拉起目标程序

界面阶段（`PhasePayload.stage`）：`checking` → `force-update`（服务端要求强制更新时）/ `available` → `downloading` → `installing` → `done`，或 `up-to-date`（无更新，静默直通）/ `error`。

**自动重试（默认 3 次）**：下载环节（拉包、逐卷下载、分卷合并、下载后 SHA256 校验）与解压校验环节
失败会自动重试，节奏为 1s → 2s → 3s → …（封顶 5s），每次重试前清掉失败产物，避免把半包当完整包复用。
重试期间界面停在「正在下载更新包...（第 2/3 次）」，副标题写明上一次失败原因——流程没死，别让用户以为卡住。
次数配置项 `retry_times`（含首次尝试），下载与解压各自独立计数。

**两类失败会给出「前往官网重新下载」出口**（错误界面除「重试」「关闭」外多一枚按钮，用系统默认浏览器打开
`homepage`，配置 > 内置默认）：

- **更新检查失败**：`manifest_url` 里的地址全部拉取/解析失败，本地拿不到任何版本信息。提示写「所有更新地址均不可用。
  可点击「前往官网重新下载」获取完整安装包」。
- **下载/合并环节重试用尽**：提示写「下载失败，已自动重试 2 次（共 3 次尝试）。可点击「前往官网重新下载」获取完整安装包」。

解压校验、磁盘写满这类本地问题换地址也解决不了，不给官网入口，只提示处理后重试，免得把用户往官网引一轮再回来。

### 4.6 本地目录布局

根目录 = `qsltd_updater_core::appdir::app_dir(app_name)`（原 `qingshu::update::get_myapp_dir` 的等价复刻，行为逐条对齐，历史安装位置不变）：

- 默认：`%APPDATA%\<app_name>\`（如 `C:\Users\<user>\AppData\Roaming\qsm`）；`app_name` 为空时用固定名 `qsltd-update`
- 若 exe 同目录存在 `config/config.toml` 或 `config.toml`（或当前工作目录下的 `config.toml`）且 `[appdata] dir=` 配置了合法路径，则**整盘使用该路径**（不再拼接 `app_name`）；指向 SYSTEM 账户目录的配置视为无效，继续走默认解析
- 若运行在 SYSTEM 用户目录，降级到 `%LOCALAPPDATA%\<app_name>` 或 `%PROGRAMDATA%\QsLtdService\<app_name>`，都取不到时退回当前工作目录

```
<root>/
├── app.json              # 本地版本记录（当前版本等）
├── updater.lock          # 单实例互斥锁：进程存活期间独占打开，退出（含被杀/崩溃）由系统释放
├── manifest_cache.json   # manifest 的 5 分钟 TTL 缓存（仅「重试」路径读取，按 app_name 隔离）
├── downloads/            # 下载暂存：完整 zip（update_<版本>.zip）与分卷（安装成功后清理，失败保留以便复用）
└── app/
    ├── app_2.1.2/        # 当前版本
    ├── app_2.1.1/        # 旧版本 = 回滚资产（保留 max_versions 个）
    └── _del_app_2.1.0/   # 删除失败（目录被占用）的残留，下次启动再清
```

- **单实例**：启动时抢占 `<root>/updater.lock`（Windows 下以 `share_mode(0)` 独占打开），最多等 5 秒；
  拿不到就打日志直接退出，避免两个更新器同时写 `app.json`、同时清理版本目录。提权交接（父进程 spawn 完提权副本
  立刻退出）因此不会互锁，锁文件由 OS 回收，崩溃残留也不会留下「永远打不开」的死锁。`--check-only` 与 `--preview` 不取锁。
- **回滚**：旧版本目录按 `max_versions` 保留（含当前版本），需要回滚时把启动目标指回旧目录即可。
- **清理**：安装成功后清理本次下载缓存（失败不清理，便于重跑复用），再按版本从旧到新删除超出 `max_versions` 的目录，当前版本目录始终保留。
- **服务**：`services` 中的服务在**安装前**逐个卸载（先停止后卸载，单个失败不中断），卸载后不恢复不重启。

### 4.7 界面模板

| 键 | 布局 |
| --- | --- |
| A | 信息密度 |
| B | 环形居中 |
| C | 顶部步骤条 |
| D | 极简留白 |
| E | 日志为主 |
| F | 环形步骤（**默认**，深色工具风） |
| G | 品牌沉浸 |
| H | 极简轻量 |

本地预览不需要联网：

```bash
cargo run -p qsltdupdater-egui -- --preview
cargo run -p qsltdupdater-egui -- --preview --template F --stage 9   # 强制更新（阻断）

# 或直接跑已构建的 release 产物（workspace 化后在根 target/ 下）
pnpm preview
```

`--stage` 取值（也可传中文名称，未知值回落 1 下载中）：
`0` 检测中 / `1` 下载中（分卷） / `2` 校验中 / `3` 解压·安装中 / `4` 发现新版本 /
`5` 已是最新 / `6` 更新完成 / `7` 错误 / `8` 重试中 / `9` 强制更新（阻断） / `10` 强制更新（可稍后升级）。

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

窗口居中启动，顶部一排页面标签（无分组），右侧常驻「打包」按钮（打包中显示 spinner +「打包中…」并禁用按钮）：

| 页面 | 内容 |
| --- | --- |
| **配置** | 仪表盘主屏（两栏）：应用名、输出目录、下载前缀列表、单卷大小上限；「保存配置」写回 `packtool.json` |
| **打包文件** | 待打包文件/目录列表，可新增、删除、用系统对话框「浏览…」选取 |
| **历史构建** | 只读扫描输出目录下的 `{app}_*.zip`（名称 / 体积 / 日期） |
| **下载与版本** | 版本号（可「取默认（末位+1）」）、更新日志、强制更新策略（`min_version` / `force_update` / `store_url`，写进 version.json）、开始打包 |
| **日志** | 打包全过程日志；打包中顶部显示进度卡片 |
| **关于** | 图标 + 版本号 + 元信息与主页链接 |

### 5.2 packtool.json（工具配置，与 exe 同目录）

首次启动自动生成，之后自动载入。**注意：界面上改完要点「保存配置」才会落盘。**

| 字段 | 说明 |
| --- | --- |
| `app_name` | 应用名，决定包名 `{app}_{version}.zip` 与旧包清理匹配前缀 |
| `files` | 待打包项列表，见下方路径规则 |
| `download_prefixes` | 下载地址前缀数组，最终地址 = 前缀 + 包名；**每个前缀各生成一份 version 文件** |
| `volume_size_mb` | 单卷大小上限（MB），包体超过则切分为分卷 |
| `output_dir` | 打包输出目录，包体与 version 文件都落在这里 |
| `min_version` | 写入 manifest 的最低允许版本；留空 = 不约束（低于它的客户端触发阻断式强制更新） |
| `force_update` | 写入 manifest 的版本级强制更新标记；勾选后本版本对所有客户端不给「稍后升级」出口，默认 `false` |
| `store_url` | 写入 manifest 的应用商店地址；留空则强制更新界面不显示「前往应用商店」入口 |

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
# 共享核心（纯单元测试，217 个）——workspace 根执行，无需 cd 进目录
cargo test -p qsltd-updater-core

# 服务模块真实 sc.exe 冒烟（需管理员权限，默认关闭）
cargo test -p qsltd-updater-core --features integration

# 启动器 UI 逻辑测试（标题/副标题兜底、动作行显示条件、进度条规格等 7 个）
cargo test -p qsltdupdater-egui

# 打包工具（含打包/切分/取消/版本号推算等用例）：不在 workspace 内，仍进目录跑
cd src-pkgtool && cargo test

# 门禁命令（CI 里跑的同一组；src-egui/src-pkgtool 见 3.3 的软门禁说明）
cargo fmt --all -- --check
cargo clippy -p qsltd-updater-core --all-targets -- -D warnings
cargo clippy -p qsltd-ui --all-targets -- -D warnings
```

lint 口径：`unwrap_used = "deny"` / `expect_used = "warn"` 是生产代码的约束，两处刻意开口子——
单元测试里 `unwrap()/expect()` 就是想要的写法（失败即 panic 并带原因），构建脚本没有调用方可接错误、
只能 panic 让 cargo 停下。所以 `src-core/src/lib.rs` 用 `#![cfg_attr(test, allow(...))]` 只对 test
配置放行，`build.rs` 各自 `#![allow(clippy::expect_used)]` 并注明原因；业务代码不受影响。

---

## 7. 注意事项

- `version.rc` / `icon.rc` 中的公司名、产品名、版本号是 Windows 资源硬编码字段，改动后需重新构建；版本号本身不要手改，改 `package.json` 后由 `scripts/pybuild.py` 铺开（见 3.2），CI 用 `--check-version` 兜底。
- release 构建会 `strip` 符号并关闭 panic 位置信息，用于减小体积、降低杀软启发式误报。profile 现在写在**根 `Cargo.toml`**（workspace 成员不能各自定义）：`opt-level = 3` + `lto = true` + `strip = true` + `codegen-units = 1` + `panic = "abort"`。
- 界面字体使用**华为 HarmonyOS Sans SC**（`src-egui/assets/font/`，Regular/Medium/Bold），由 `src-egui/src/app.rs` 用 `include_bytes!` 直接编进 exe（约 +5.1 MB），单文件分发不依赖外部字体目录。运行期仍保留两级回退：exe 同级 `fonts/` 或源码目录（仅在内嵌字节不可用或未内嵌时生效，pkgtool 走这条路径），再不行按平台回退系统字体——Windows 雅黑/黑体/宋体/等线/Segoe UI/Arial，macOS 苹方/冬青黑体/宋体，Linux Noto Sans CJK/思源黑体/文泉驿/DejaVu/Liberation。egui 的 `FontId` 没有字重字段，Medium/Bold 各注册为独立字族（`harmony-medium` / `harmony-bold`）。字体许可见 `src-egui/assets/font/LICENSE.txt`，分发时请保留（`scripts/build.py` 会复制为 `dist/<版本号>/HarmonyOS-Sans-LICENSE.txt`）。
- `src-pkgtool` 刻意**留在 workspace 之外**：它的 release profile 放宽了 LTO 与优化级别（`opt-level = 2` / `lto = false` / `codegen-units = 16`），用于规避特定环境下编译 `windows-sys` 的偶发崩溃。workspace 的 profile 全局唯一、成员无法覆盖，把它纳进来会让这份规避失效，顺带把发布产物 `launcherclient.exe` 的 LTO/strip 一起退化。
- 更新器只做「下载 → 校验 → 解压 → 拉起」，**不做包签名校验**；分发链路请自行保证 HTTPS 与 CDN 可信。
  `require_sha256`（默认开启）挡住的是「传输损坏 + 无完整性证据的包」，不是伪造者——他能同时改掉服务器上的包和哈希，
  SHA256 就形同不存在。真正的来源可信只能靠 HTTPS 证书链与代码签名；签名步骤已在 `pybuild.py` 预留（见 3.4），
  等待发布机提供 `.pfx` 与时间戳服务。
- 仓库根 `update.json` 是**示例文件**，但同名文件会被真实读取，且优先级高于编译期烘焙值（命令行 > update.json > 烘焙）。
  发布时不要把仓库根这份带开发期 manifest 地址的 `update.json` 一起打进包，否则会覆盖烘焙结果。
- 打包工具目前不含：文件 diff 勾选/筛选、发布通道概念、压缩级别与 SHA-256 开关、构建历史持久化与回滚、日志级别筛选。

---

## 8. 相关文档

| 文档 | 内容 |
| --- | --- |
| [`CHANGELOG.md`](CHANGELOG.md) | 版本变更记录 |
| [`docs/选型决策记录.md`](docs/选型决策记录.md) | 为什么选 egui 而非 Tauri |
| [`docs/Tauri-vs-egui性能对比.md`](docs/Tauri-vs-egui性能对比.md) | 两版体积/启动/内存对比 |
| [`docs/egui界面复刻说明.md`](docs/egui界面复刻说明.md) | 八套模板与阶段画面的复刻说明 |
| [`docs/更新包打包工具-界面重设计.html`](docs/更新包打包工具-界面重设计.html) | pkgtool 界面设计稿（浏览器打开） |
| [`docs/ui-drafts-下载校验解压.html`](docs/ui-drafts-下载校验解压.html) | 下载/校验/解压阶段界面草稿 |
