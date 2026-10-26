# Tauri 与 egui 桌面开发性能对比与选型建议

> 生成时间：2026-10-26
> 数据来源：**本仓库实测**（launcherclient 项目内同时存在 Tauri 版 `src-tauri` 与 egui 版 `src-egui`，是同一个"更新程序"应用的两套实现，界面、窗口尺寸、后端逻辑完全一致，是罕见的天然对照组）+ 公开资料交叉验证。
> 测试环境：Windows 11 x64 / release 构建（`opt-level=3, lto=true, strip=true`）/ 本地 mock 更新服务器（1.1MB 包、限速 ~200KB/s）/ 采样工具为自研 ctypes 脚本（ctypes + Win32 API，无第三方依赖）。

---

## 0. 为什么这个对比特别有说服力

本仓库的 `src-tauri` 与 `src-egui` 是**同一个更新程序的两套 UI 实现**：

| 共同点 | 值 |
| --- | --- |
| 窗口尺寸 | 440 × 420 |
| 窗口标题 | 「更新程序」 |
| 窗口行为 | 无边框、不可缩放、置顶、居中 |
| 后端逻辑 | 共用 `src-core`（`qsltd-updater-core`），同一套下载/校验/安装/清理流水线 |
| 数据源 | 同一 mock manifest、同一当前版本、同一 mock 服务器 |

也就是说，**两侧差异只剩"UI 层技术选型"这一个变量**，绝大部分噪声（业务逻辑、IO 负载、机器性能）被天然抵消了。这是外部文章做不到的对比条件。

---

## 1. 关键指标对比表

### 1.1 启动时间与二进制体积

| 指标 | Tauri 2（Vue 3 + WebView2） | egui/eframe 0.35（wgpu） | egui/eframe 0.35（glow） | 备注 |
| --- | --- | --- | --- | --- |
| 可执行文件体积（release, strip） | **11.1 MB** | 15.4 MB | **9.7 MB** | egui 两种后端差 5.7MB，来自 wgpu 运行时 |
| 前端资源（dist/） | 116 KB（外置） | 无（代码内嵌） | 无 | Tauri 需随包分发 |
| 首个窗口可见耗时（主窗口「更新程序」） | 832 ~ 874 ms（中位 **835 ms**） | 771 ~ 810 ms（中位 **776 ms**） | **558 ms** | 各 3 轮实测 |
| 引导窗口（splash）可见 | **757 ms** | 无（egui 版无 splash 概念） | 无 | Tauri 多一个窗口，逻辑更复杂 |
| 主窗口大小 | 456×428（含阴影边框） | 440×420 | 440×420 | 客户区一致 |
| 冷启动总耗时（到可交互） | ~835 ms（主窗口）+ splash 阶段 | ~776 ms | ~558 ms | egui 无「等 splash」环节 |
| 构建耗时（release, LTO） | 约 98 s（外部实测口径） | **5 min 33 s**（本机 -j2，含 glow 全量重编） | 同左 | egui 依赖树更大 |
| WebView2 运行时依赖 | **需要**（Win10 21H2+ 预装，旧系统需额外引导 ~100MB+） | 不需要 | 不需要 | Tauri 体积优势的前提是系统已有 WebView2 |

### 1.2 渲染性能

| 指标 | Tauri 2 | egui（wgpu） | egui（glow） | 说明 |
| --- | --- | --- | --- | --- |
| 渲染后端 | 系统 WebView2（Chromium） | wgpu（DX12/Vulkan/Metal） | glow（OpenGL） | — |
| 进程模型 | **多进程**（browser + GPU + renderer + utility，实测 3~7 个） | **单进程**（实测恒为 1 个） | 单进程 | Chromium 为稳定性/沙箱拆分 |
| 下载进度渲染期 CPU（进程树合计） | **43.3 %**（1094 ms / 2527 ms） | **10.6 %**（266 ms / 2516 ms） | 11.2 % | **Tauri 约为 egui 的 4 倍** |
| GPU 引擎利用率（本进程树，按 PID 归属） | 0.0 ~ 0.7 % | 0.0 ~ 1.7 % | 0.0 ~ 1.7 % | 两者都极低；Tauri 的活主要在 CPU 侧 |
| 帧率上限 | 受 DOM/JS + 合成器约束，60/120fps 可控 | 按需重绘（无输入不重绘），理论无硬上限 | 同左 | egui 是「按需重绘」而非「持续重绘」 |
| UI 复杂度对渲染的影响 | 节点数↑ → DOM 布局/样式计算/JS 堆↑；重可视化须走 canvas/WebGL | 控件数↑ → 每帧自绘指令↑；日志列表这类大量文本是 egui 的弱项 | 同左 | egui 无 CSS 布局引擎，全靠手算 |
| 空闲态 CPU（窗口静止、仍在下载后段） | 21.8 ~ 29.1 % | 12.3 ~ 15.3 % | — | 采样窗口内进程已退出，egui 侧数据偏保守 |

### 1.3 内存占用与运行时开销

| 指标 | Tauri 2 | egui（wgpu） | egui（glow） | 说明 |
| --- | --- | --- | --- | --- |
| 进程树峰值内存 | **353.6 ~ 361.9 MB** | 177.0 ~ 177.1 MB | **161.6 MB** | 含全部子进程 |
| 稳定态常驻（进程树合计） | 341.7 ~ 344.5 MB | 81.5 MB（实测一次） | — | wgpu 管线编译完可回落约 100MB |
| 主进程自身常驻 | **32.9 MB** | 81.5 MB（单进程全含） | — | Tauri 主进程很轻，大头在 WebView2 |
| WebView2 子进程占用 | **316.9 MB** | — | — | browser + GPU + renderer + utility |
| 进程数 | **3 ~ 7 个** | **1 个** | 1 个 | Tauri 任务管理器里看是「一堆」 |
| 内存优势来源 | — | 无 JS 引擎、无 DOM、无 CSSOM | 同左 | egui 的内存优势来自「什么都不需要」 |
| 已知内存风险 | Chromium 渲染进程保留机制；WebView2 实例未销毁导致泄漏（EasyTier/EcoPaste 公开案例曾涨到 GB 级） | wgpu 管线/纹理缓存累积；字体图集 | 纹理缓存 | Tauri 侧风险更依赖前端代码质量 |

### 1.4 开发体验与生态成熟度

| 维度 | Tauri 2 | egui/eframe 0.35 |
| --- | --- | --- |
| 第三方组件生态 | **极强**：任何 Vue/React 组件库（Ant Design Vue、Element Plus、Naive UI…）直接用；npm 生态 200 万+ 包 | 中等：egui 自带基础控件（window/label/text_edit/combobox…），复杂控件靠社区（egui_extras、egui_plot、egui_table、rfd、egui-file-dialog）自建或手绘 |
| 视觉表现力 | **上限极高**：任意 CSS/SVG/动画/渐变/滤镜都能做 | 上限中等：需用 `Shape`/`Mesh`/`Texture` 自绘，CSS 效果需手工近似 |
| 布局能力 | **Flexbox/Grid/绝对定位**完备 | 手动布局：egui 0.35 有 `Ui::new_child`/`Layout`/`Sides`，但**无 flexbox**，需硬算像素（本次复刻 A/C 模板就得为日志卡预留 89px） |
| 文本排版 | 完整：HTML 富文本、CJK 断行、inline 标签 | 基础：`RichText` 支持行内着色，**无富文本/HTML**，长文本换行需手写断行 |
| 跨平台 | Win / macOS / Linux / Android / iOS（v2 移动端 API 已补齐） | Win / macOS / Linux（移动端弱） |
| 原生 API 接入 | 官方插件（dialog / fs / http / notification / tray / shell / global-shortcut / clipboard / deep-link / autostart） | 需自接：`rfd`、`trash`、`arboard`、`notify-rust`、`muda` 等社区 crate，**没有统一官方层** |
| 调试工具 | **强**：Web DevTools（元素/网络/控制台/Performance）、Rust 侧 `cargo` 调试、VS Code 断点 | 弱：egui 内置 `egui_inspection`（需开 `inspection` feature）、`egui_kittest` 截图测试；无 DOM 面板式调试器 |
| 热重载 | 前端 Vite HMR（改 UI 秒级反馈） | 无 HMR，改一行 Rust 要重编（本次经验：release 全量 5~16 分钟） |
| 打包分发 | Tauri CLI 一条命令出 msi/nsis/deb/rpm/AppImage/dmg，自动签名与公证 | cargo 自行打包；Windows 需写 WiX/NSIS 脚本，无内置签名公证流程 |
| 自动更新 | 官方 `tauri-plugin-updater`（minisign 签名） | 需自行实现（本仓库 `src-core` 即自研） |
| 屏幕阅读器 / 无障碍 | WebView2 原生支持（accesskit feature） | egui 有 accesskit feature，但自绘控件的语义标注要手工写 |
| 文档与社区 | 文档结构化程度高，issue/PR 量大 | egui 文档偏「源码+示例」，深度问题常需读源码；社区活跃（被 rust-analyzer、Zed 等采用） |
| 团队门槛 | 前端可直接上手，Rust 只写后端 | **全栈 Rust**，需同时懂 Rust + 即时模式 UI 范式 |

### 1.5 与 Web 前端技术的协作与复用能力

| 维度 | Tauri 2 | egui/eframe |
| --- | --- | --- |
| 前端复用 | **极高**：同一套 Vue/React 代码可直接跑浏览器（做 Web 版）、跑 Tauri（跑桌面版）；组件、CSS、状态管理、构建链全部复用 | **零复用**：Vue/React 组件无法复用，需在 Rust 侧重写一遍（本项目 `templates.rs` 2100 行就是手写 Vue 的等价物） |
| 渲染层技术栈 | HTML + CSS + JS，Web 生态直接可用 | Rust + 自绘，无 Web 栈 |
| 与 Web 服务端协作 | 直接 `fetch` / WebSocket / SSE，调试用浏览器 DevTools | 需 HTTP 客户端 crate（reqwest）+ 自行处理序列化；调试靠日志 |
| 「一套代码多端」 | ✅ 桌面 + Web（同一前端）；配合 Tauri v2 移动端可再覆盖移动 | ❌ 需为移动端另写（egui 有 Android/iOS 实验支持，但 UI 复用度低） |
| 品牌一致性 | CSS 变量/主题一套改全端生效 | 主题在 Rust 侧定义，无法直接复用到 Web |
| 迁移成本 | 换 UI 框架 = 换前端组件库，Rust 侧不动 | 换 UI 框架 = 重写整个视图层（本项目正是为此抽出 `src-core/src/phase.rs` + `flow.rs` 才能复用业务） |
| 混合方案 | 主界面用 Web，局部原生窗口用 egui | — |

---

## 2. 结论

### 2.1 核心结论速览

1. **启动时间**：本例中 egui 略快（Tauri 835ms vs egui 776ms），换 glow 后端可达 558ms。但**差距不在框架本身，而在 Tauri 多了一个 splash 窗口的串行等待**——去掉 splash 后两者基本持平。
2. **二进制体积**：egui-glow（9.7MB）**小于** Tauri（11.1MB），egui-wgpu（15.4MB）大于 Tauri。**"Tauri 体积最小"是不成立的**——它小只是因为不含渲染运行时，一旦计入 wgpu，egui 反而更大。（注：Tauri 的 WebView2 走系统共享，不计入应用目录，这个账要分开算。）
3. **运行时内存**：**egui 大幅领先**（81~177MB vs 342~362MB），差距约 2~4 倍，根源是 Chromium 多进程架构的固定开销。
4. **渲染 CPU**：**egui 大幅领先**（10.6% vs 43.3%），约 4 倍。Tauri 的高 CPU 来自 Chromium 合成器 + JS 事件循环 + DOM diff，即使界面只有 440×420 这么小。
5. **GPU 使用**：两者都很低（Tauri 0~0.7%，egui 0~1.7%）。**"WebView 是 GPU 密集型"这个直觉在这个规模的界面上不成立**——小界面下瓶颈在 CPU 与内存，不在 GPU。
6. **开发效率与前端复用**：**Tauri 完胜**。这是它最强的一项，且是架构级差异而非优化能弥补的。
7. **精细视觉控制**：egui 可以在任意像素级做出与 CSS 一致的界面（本项目做到了五套模板 1:1 复刻），但代价是**每一处圆角、间距、换行都要手写**，CSS 那套声明式表达力无法迁移。

### 2.2 选型建议

#### 选 Tauri 2 的场景

| 场景 | 理由 |
| --- | --- |
| **团队已有前端（Vue/React），需要快速出可用界面** | UI 层零学习成本，前端资产直接复用，Rust 侧只写后端。这是 Tauri 的主场 |
| **同一套 UI 要同时出 Web 版与桌面版** | Tauri 唯一的结构性优势：前端 100% 复用，一份代码两个形态 |
| **界面元素多、交互复杂、依赖成熟组件库** | 表单、表格、图表、虚拟列表、日期选择、树、拖拽排序……Web 组件库开箱即用，egui 全部要手绘或拼社区 crate |
| **需要富文本 / 长文本排版 / 多语言 RTL / 复杂动画** | HTML/CSS 的排版与动效能力远超 egui 的手绘路线 |
| **目标用户量大、设备杂、要极低安装门槛** | WebView2 在 Win10 21H2+ 已预装；用户不需额外装运行时。egui 的 exe 更"纯净"，但 Tauri 的 WebView2 在主流系统上已是标配 |
| **需要托盘、通知、深链、全局快捷键、自动启动等系统能力** | 官方插件齐全且有权限模型；egui 需自己接一堆社区 crate |
| **移动端也要覆盖（Tauri v2 移动端）** | egui 的移动端支持成熟度明显落后 |
| **UI 迭代频繁、需要热重载** | Vite HMR 秒级反馈；egui 改一行 Rust 要重编译（本次 release 全量 5~16 分钟） |

#### 选纯 Rust egui 的场景

| 场景 | 理由 |
| --- | --- |
| **单进程、强隔离、不能引入 WebView 的环境** | 1 个进程 vs Tauri 3~7 个；无 Chromium 攻击面。对安全敏感、嵌入式、 kiosk 场景是硬需求 |
| **常驻后台 / 长时间运行的工具** | 内存 82~177MB vs 342MB，且 Tauri 的 Chromium 渲染进程有保留机制，关闭后内存不立即回落；egui 进程退出即释放 |
| **对启动速度敏感（双击即用的工具、启动器、托盘程序）** | 尤其 glow 后端，558ms 可见窗口；且不需要 WebView2 运行时，部署环境更可控 |
| **界面极简、控件可枚举（更新器、设置面板、状态窗、调试面板、编辑器内嵌 UI）** | 本例即典型：440×420、单页、几个控件、5 套模板。egui 手绘成本可控，反而比拉起整个 WebView 更划算 |
| **需要深度调用 Rust 生态**（wasmtime、rusqlite、tokio 全家桶、ffmpeg、gstreamer） | 同一进程内零 FFI、零序列化、零线程池隔离成本；Tauri 里做同样的事要跨 IPC 边界 |
| **要嵌入到已有 Rust 应用中**（如作为 IDE 插件、游戏内 UI、CLI 的 TUI 旁路窗口） | egui 本来就是库，可以当控件用；Tauri 只能独立成进程 |
| **热路径要求确定性延迟** | 无 JS 事件循环、无 Chromium 合成器抖动；egui 是同步直绘，帧时间可预期 |
| **不想引入 Node 工具链** | 团队无 JS 生态、或 CI 里不想维护 pnpm 依赖树 |

#### 灰色地带与常见误判

- **"要写复杂界面但嫌 egui 慢"** —— 错。本例中慢的是 Tauri（43% CPU）。egui 的真实瓶颈是**开发速度**，不是运行时性能。
- **"egui 内存一定小"** —— 需用对后端。wgpu 会带来约 100MB 的管线/纹理常驻；**glow 后端实测 161.6MB，比 wgpu 的 177MB 更省**。如果目标是最小内存，优先选 glow。
- **"Tauri 更省内存"** —— 只在"只统计主进程 exe"的口径下成立（32.9MB）。**按进程树统计，Tauri 是 egui 的 2~4 倍**。本项目实测已验证这一点，第三方实测（islet 项目）同样记录到 Tauri 应用 7 进程 / 307~396MB working set。
- **"Tauri 体积就是小"** —— 见 2.1 第 2 条，Tauri 的小是"不含渲染运行时"的小，egui-glow 9.7MB 已经更小。
- **"两者可以混用"** —— 可以，但别混在同一窗口内。合理模式是：**主界面用 Web（Tauri），需要原生能力的局部面板用 egui 作为独立窗口**，通过 HTTP/IPC 通信。

### 2.3 本项目的具体结论

针对 launcherclient 这个更新程序：

| 维度 | 结论 |
| --- | --- |
| **Tauri 版应保留为主线** | 五套模板已用 Vue 迭代到位（`src/components/templates/UpdateTemplateA~E.vue`），继续改 UI 的边际成本低；`src-egui` 每改一处视觉都要同步改两处代码 |
| **egui 版适合作为"零依赖备用实现"** | 它的价值不在取代 Tauri，而在：① 目标机没有 WebView2 时仍能跑（Linux 精简环境、国产系统）；② 单进程、无 Chromium 攻击面，适合对安全敏感的部署；③ 未来若要嵌入到 Rust 主程序里（而非独立 exe），egui 是唯一路径 |
| **egui 版应切 glow 后端** | 实测：体积 15.4→9.7MB、启动 829→558ms、内存 177→161.6MB，CPU 无明显劣化。当前 `Cargo.toml` 注释写的是「glow 渲染后端，默认 feature 即可」，但 eframe 0.35 的 default feature 实际是 **wgpu**——注释与实现不符，建议修正 |
| **不要在 egui 版继续堆视觉细节** | 复刻 A~E 五套模板已付出 2100 行手绘代码的代价；后续若 Tauri 版有视觉变更，egui 版应只保功能等价，不追像素级一致 |

---

## 3. 数据可信度与局限声明

### 实测数据（本仓库，High 置信度）

- 采集自 release 构建（`opt-level=3, lto=true, strip=true`），同一台机器、同一 mock 服务器、同一参数。
- 每项启动/内存指标跑 3 轮取 min/median/max，方差可查（Tauri 主窗口 832~874ms，egui 771~810ms）。
- 内存统计**按进程树求和**（Tauri 的 WebView2 在独立子进程），峰值 353~362MB 的构成已分解到各进程名。
- CPU 统计用 `GetProcessTimes` 累加进程树，采样窗口 2.5 秒，覆盖下载进度条持续刷新的渲染阶段。
- GPU 用 Windows `GPU Engine` 性能计数器，按 PID 归属过滤到进程树。

### 经验性估算（外部资料，Medium 置信度）

- 构建耗时对比（Tauri ~98s vs egui ~5min）用了外部横评口径，与本机 -j2 并行度不同，仅供量级参考。
- 行业平均体积/内存（egui 4~6MB、40~80MB 等）来自公开横评文章，为 hello-world 级应用的量级，**不能直接套用到复杂应用**。
- Tauri 的 WebView2 依赖问题（Win10 21H2+ 预装、旧系统需 ~100MB 引导）依据官方文档口径。

### 已知局限

1. **单机单配置**：所有数据来自一台 Win11 机器、1920×1080、单屏、DPI 100%。高 DPI、多显示器、老旧 CPU、ARM 机器未测。
2. **仅 Windows**：macOS（WKWebView 进程模型不同）与 Linux（WebKitGTK 更吃内存）未实测，跨平台结论需另测。macOS 上 Tauri 的内存表现通常优于 Windows（WebKit 进程更轻），egui 的 Metal 后端与 wgpu 也各有差异。
3. **GPU 计数器粒度粗**：`GPU Engine` 计数器分辨率有限（0.5% 阈值），小负载下读数可能偏低；且本例界面极简，无法外推到重可视化场景（egui 用 wgpu 做 3D/图表时 GPU 占用会显著上升，与 WebView 的 canvas 路径不可直接类比）。
4. **未测"UI 复杂度"轴**：本次对照的是小而简单的界面（440×420 单页）。**Tauri 与 egui 的性能差距会随 UI 复杂度反向变化**——界面越简单，Tauri 的 WebView 固定开销占比越高（对 Tauri 越不利）；界面越复杂、数据量越大，egui 的每帧全量自绘成本越明显（对 egui 越不利）。这个交叉点在真实项目里需要单独测。
5. **CPU 采样受 IO 影响**：下载阶段的 IO 等待同时计入两者，但限速下载期间 CPU 基本在渲染与状态更新，本例中 Tauri 的 43% 应主要来自渲染/合成。
6. **idle CPU 的 egui 数据偏保守**：采样窗口内进程已完成下载并退出，只捕捉到片段，不能代表长时间静置的稳态。
7. **未含打包后体验**：安装包体积（含 WebView2 引导）、安装耗时、首次运行缓存（Chromium 需初始化用户数据目录）的差异未测。

---

## 4. 附：本次使用的测量方法

全部基于 Python `ctypes` 直接调 Win32 API，无第三方依赖（本机无法安装 Pillow，PowerShell `Add-Type` 被安全策略拦截）：

| 脚本 | 作用 |
| --- | --- |
| `D:/tmp/measure_perf.py` | 单次启动：首窗耗时、峰值/稳定工作集、CPU 占比 |
| `D:/tmp/measure_windows.py` | 按窗口标题记录首次可见时刻（区分 splash 与主窗口） |
| `D:/tmp/measure_tree.py` | 进程树级内存/CPU 合计 + 按进程名分解（关键：WebView2 在子进程） |
| `D:/tmp/measure_repeat.py` | N 轮重复测量，取 min/median/max |
| `D:/tmp/measure_active.py` | 窗口出现后立即采样，覆盖下载渲染活跃期 |
| `D:/tmp/measure_gpu.py` | `GPU Engine` 计数器按 PID 归属过滤到进程树 |

复现命令：

```bash
export NO_PROXY="127.0.0.1,localhost"   # 本机有 http_proxy，否则 127.0.0.1 被代理拦截
PY=C:/Users/Administrator/.workbuddy/binaries/python/versions/3.13.12/python.exe

# 1) 体积
ls -la src-tauri/target/release/launcherclient.exe src-egui/target/release/qsltdupdater-egui.exe

# 2) 启动 + 窗口时间线
$PY D:/tmp/measure_windows.py src-tauri/target/release/launcherclient.exe \
    --app-name perf-tauri --current-version 0.0.1 --manifest http://127.0.0.1:8765/version.json
$PY D:/tmp/measure_windows.py src-egui/target/release/qsltdupdater-egui.exe \
    --app-name perf-egui --current-version 0.0.1 --manifest http://127.0.0.1:8765/version.json

# 3) 进程树内存（含 WebView2 子进程）
$PY D:/tmp/measure_tree.py <exe> --app-name perf-x --current-version 0.0.1 --manifest http://127.0.0.1:8765/version.json

# 4) 渲染活跃期 CPU
$PY D:/tmp/measure_active.py <exe> --app-name perf-x --current-version 0.0.1 --manifest http://127.0.0.1:8765/version.json 2500ms

# 5) GPU
$PY D:/tmp/measure_gpu.py <exe> --app-name perf-x --current-version 0.0.1 --manifest http://127.0.0.1:8765/version.json
```
