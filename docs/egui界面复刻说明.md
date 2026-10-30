# src-egui：更新程序界面的 egui/eframe 复刻说明

> **状态：生产启动器壳（自 2026-10-26 起）**
>
> 经 Tauri vs egui 实测对比与产品定位评估，本 `src-egui` 已定为 **生产启动器壳**；
> `src-tauri`（Tauri 2 + Vue 3）与 `src-zsui`（zsui 原生 UI）均已于 2026-10-26 删除，仓库仅剩 `src-egui` 单一 UI 实现 + 共享 `src-core`。
> 决策依据与对比数据见 [`Tauri-vs-egui性能对比.md`](./Tauri-vs-egui性能对比.md) 与 [`选型决策记录.md`](./选型决策记录.md)。

本文说明 `src-egui`（egui/eframe 0.35 原生 UI）与已删除的 Tauri + Vue 前端（`src/` + `src-tauri/`，均于 2026-10-26 删除）
的**对应关系、实现差异与已知限制**（保留作为历史对照与设计溯源）。

## 〇、检测更新时的加载状态（懒加载窗口）

更新器的「检测更新」有两种触发，UI 反馈策略不同：

- **重试检测**（错误态点「重试」）：`retry()` → `run_check_flow` 首行即推送 `Stage::Checking`，八套模板（A~H）均已渲染 **spinner 旋转 + "正在检查更新..." 文字**；`app.rs` 对 `Checking` 阶段做 33ms 高频重绘保证动画，加载态完整。
- **首次检测**（双击启动器后的 manifest 拉取）：原本在窗口创建前同步 await 完成、UI 不可见。**现改为后台线程异步检测 + 懒加载窗口**（`main.rs` 起后台线程跑 `run_startup`，`app.rs` 据 300ms 阈值决定是否 `ViewportCommand::Visible`）：
  - 检测 **≤300ms** 完成：无更新则静默拉起目标程序退出（用户无感，保住「秒开」）、有更新则直接弹出"发现新版本"；
  - 检测 **>300ms** 未完成（弱网/慢 manifest，最坏受 `client.rs` 检测超时约束）：弹出"正在检查更新"窗口给用户反馈，完成后转入对应状态；
  - 这样内网/快 CDN 场景零感知、公网/弱网场景也有明确加载反馈，互不拖累。

## 一、交付物清单

| 文件 | 行数 | 职责 |
| --- | --- | --- |
| `src-egui/Cargo.toml` | — | 新 crate `qsltdupdater-egui`，依赖 eframe/egui 0.37 + tokio + 共享 core |
| `src-egui/src/main.rs` | 212 | 命令行预处理（`--preview` / `--stage`）、`--check-only` 退出码、启动决策、进入事件循环 |
| `src-egui/src/app.rs` | 505 | `App` 实现：阶段快照轮询、标题栏动作、关闭确认模态、完成/重试派发 |
| `src-egui/src/theme.rs` | 121 | 对 `qsltd-ui` 色板/字体工具的转出层 |
| `src-egui/src/widgets.rs` | 563 | 自绘控件库：背景渐变、标题栏、进度条、环形进度、spinner、状态点、pill、日志卡、按钮 |
| `src-egui/src/templates.rs` | 1474 | 八套模板 A~H 布局；A~E 与 `UpdateTemplateA~E.vue` 一一对应，F/G/H 为后续新增的三套设计方向 |
| `src-egui/src/preview.rs` | 270 | 预览面板数据与渲染（`Preview.vue` 的 8 个阶段预设 + 模板切换） |
| `src-core/src/phase.rs` | 768 | **新增共享模块**：阶段载荷 + 派生视图 + 跨线程快照句柄 |
| `src-core/src/flow.rs` | 1368 | **新增共享模块**：启动决策 + 完整更新编排（下载/校验/安装/启动/重试） |
| `src-core/src/lib.rs` | — | 追加 `pub mod flow;` / `pub mod phase;` |

## 二、复用改造：先把业务逻辑抽成共享模块

为了避免「UI 换一套、业务重写一遍」，本次先把散落在 `src-tauri/src/updater.rs`（Tauri 版，已于 2026-10-26 删除）、`src-zsui/src/updater.rs`（zsui 版，已于 2026-10-26 删除）以及前端 `src/composables/usePhase.js`（Vue 版，随 Tauri 壳一并删除）的内容上提到 `src-core`：

| 抽取内容 | 原位置 | 现位置 | 说明 |
| --- | --- | --- | --- |
| `PhasePayload`（阶段载荷字段与 serde 名） | Tauri `emit("update-phase", …)` 载荷 | `phase::PhasePayload` | 字段名与 Tauri 版完全一致，未来其它推送式 UI 也可改用同一结构 |
| `percent/size_text/speed_text/eta_text/show_progress/step` 派生 | `usePhase.js` | `phase::PhaseView` | UI 无关的显示层计算，任一 UI 直接复用 |
| `format_bytes` / `fmt_mb` / `STEP_LABELS` / `resolve_template_key` | `usePhase.js` / `templates/index.js` | `phase.rs` | 含 `show_progress` 按 CSS 语义修正（见第四节第 6 条） |
| `resolve_config` / `run_startup` / `run_update_flow` / `run_download_parts` / `SpeedTracker` / `resolve_launch_exe` / `launch_target` / `finish_and_launch` / `retry` | `src-tauri/src/updater.rs` + `src-zsui/src/updater.rs`（两版均已于 2026-10-26 删除） | `flow.rs` | 两版合并去重，`src-egui` 直接调用同一条流水线 |

受益面：

- `src-egui` 后端业务流程与 Tauri 版**逐行相同**，不存在第二套实现；
- `src-tauri` 与 `src-zsui` 均已于 2026-10-26 删除；`src-core` 为共享逻辑，与外壳无关，保留；
- 阶段/流程相关的纯逻辑进入 `src-core` 单测（本次 135 条全绿）。

## 三、对应关系

### 3.1 文件 / 模块级

| Tauri + Vue 侧 | egui 侧 | 说明 |
| --- | --- | --- |
| `src/App.vue` | `src-egui/src/app.rs` | 阶段状态归一维护（`applyPhase` ↔ `apply_phase`）、完成/重试/关闭派发、窗口兜底兜数据 |
| `src/components/MinTitleBar.vue` | `widgets::draw_titlebar` | 32px 标题栏、最小化/关闭 46×32、拖拽区（排除按钮）、关闭 hover 变红 |
| `src/components/templates/UpdateTemplateA.vue` | `templates::template_a` | 信息密集型：标题行 + spinner 状态行 + 进度 + meta + 日志卡 |
| `…/UpdateTemplateB.vue` | `templates::template_b` | 96px 环形进度居左 + 右侧信息列 + 日志卡（96px 上限） |
| `…/UpdateTemplateC.vue` | `templates::template_c` | 默认模板：顶部四步步骤条（下载→校验→解压→完成）+ 大号百分比 + 渐变进度条 |
| `…/UpdateTemplateD.vue` | `templates::template_d` | 极简居中：4px 细进度条 + 无边框内联日志（带分隔线） |
| `…/UpdateTemplateE.vue` | `templates::template_e` | 日志优先：顶部状态条（LED + 百分比）+ 主日志区 + 底部分隔行 + 空态 |
| `src/composables/usePhase.js` | `src-core/src/phase.rs` | `STAGE` 常量、格式化函数、步骤/进度/子标题派生 |
| `src/components/templates/index.js` | `phase::resolve_template_key` + `preview::TEMPLATE_META` | `DEFAULT_TEMPLATE='C'`、A~E 键名归一化、模板元信息 |
| `src/Preview.vue` / `preview.html` / `preview.js` | `src-egui/src/preview.rs` + `--preview` | 模板 chips、阶段 chips、8 个阶段预设载荷数值完全相同 |
| `src/main.js` | `src-egui/src/main.rs` | 应用装配：预览模式 vs 真实模式 |
| `src-tauri/src/lib.rs`（CloseRequested 拦截 + 确认对话框） | `app::draw_close_confirm` + `logic()` 中的 `CancelClose` | 关键阶段关闭确认，文案与按钮一致（中断更新？/ 取消 / 确定） |
| `src-tauri/tauri.conf.json`（windows.main） | `app::run_ui` 中 `ViewportBuilder` | 440×420、resizable=false、decorations=false、alwaysOnTop |
| `src-tauri/src/updater.rs`（阶段 emit） | `phase::PhaseHandle` 快照 + `logic()` 每帧轮询 | 见第四节第 1 条 |

### 3.2 窗口与布局常量

| 项 | Tauri/Vue | egui | 一致性 |
| --- | --- | --- | --- |
| 窗口标题 | `tauri.conf.json` 静态标题（Vue 侧标题栏左侧不显示文字） | 运行时解析 `config::resolve_launcher_name`（`--name` > update.json `name` > 烘焙默认 `defaults::DEFAULT_LAUNCHER_NAME`），同一值喂给 `ViewportBuilder::with_title` 与自绘标题栏 `widgets::draw_titlebar` | egui 侧多显示一段标题文字（有意增强） |
| 客户区尺寸 | 440 × 420 | `WINDOW_SIZE = (440.0, 420.0)` | 一致 |
| 预览窗口高度 | 590（`preview.html`） | 420 + `PREVIEW_EXTRA_HEIGHT(150)` | 一致 |
| 标题栏 | `.min-title-bar` 高 32px，`padding-top:32px` | `TITLEBAR_HEIGHT = 32.0` | 一致 |
| 标题栏按钮 | 46 × 32 | `TITLEBAR_BTN_WIDTH = 46.0`，高 32 | 一致 |
| 标题文字排布 | Vue 侧标题栏无文字 | 左侧留 `TITLE_INSET = 12.0` 起排，`TITLE_V_NUDGE = 1.0` 补偿 CJK 字形相对基线的垂直偏移（与按钮视觉中线对齐）；可用宽度不够时由 `widgets::ellipsize_to_width` 按字体度量逐字回退补「…」，右侧预留 1px 度量余量，避免硬裁切削到笔画 | egui 自绘，无 CSS `text-overflow: ellipsis` 可用 |
| 是否可缩放 | `resizable: false` | `.with_resizable(false)` | 一致 |
| 边框 | `decorations: false` | `.with_decorations(false)` | 一致 |
| 置顶 | `alwaysOnTop: true` | `.with_always_on_top()` | 一致 |
| 内容内边距 | `padding: 16px 28px 22px` | `content(ui, (16,28,22,28), …)` | 一致 |
| 步骤标签 | 下载 / 校验 / 解压 / 完成 | `STEP_LABELS` | 一致 |

### 3.3 主题色

| 语义 | CSS | `theme.rs` |
| --- | --- | --- |
| 背景渐变起止 | `#1e2430` → `#171c26` | `BG_TOP` / `BG_BOTTOM` |
| 主文本 | `#e8eaf0` | `TEXT` |
| 强调蓝 | `#4a9eff` | `ACCENT`（主按钮、百分比、日志点、进度条起点） |
| 成功绿 | `#35c26e` | `SUCCESS`（安装中 spinner、步骤完成点、进度条终点） |
| 关闭 hover 红 | `#e81123` | `DANGER` |
| 错误文本 | `#ff5b66` | `ERROR_TEXT` |
| 环形内底色 | `#1a2029` | `RING_INNER` |
| 白色蒙版层 0.04/0.08/0.10/0.12/0.14/0.18/0.20/0.10-hover | `rgba(255,255,255,x)` | `W04…W20`、`W10_HOVER` |
| 文本透明度 | `rgba(255,255,255,0.65/0.7)` | `text_alpha(166)` / `text_alpha(178)` |

### 3.4 控件

| Vue 控件 / CSS 类 | egui 实现 | 关键参数 |
| --- | --- | --- |
| `.tpl-c__steps`（步骤条） | `templates::draw_steps` | 首尾对称内缩（`margin = dot_r+8`，不再贴右边界被裁剪）；三段连接线从圆点右缘贯穿到下一圆点左缘（完成段=绿、当前=蓝+光晕+外环描边、未达=灰） |
| `.tpl-c__pct` 大号百分比 | `templates::metrics_row` | 数字 26px + `%` 14px |
| `.tpl-c__bar` 进度条 | `widgets::progress_track` | 轨道 8px / 圆角 4 / `W10` 底 + 蓝→绿渐变填充 |
| `.tpl-b__ring` 环形进度 | `widgets::progress_ring` | 外径 96px、描边 9px、内盘 `#1a2029`、中心百分比 + 副标题 |
| `.tpl-x__spin` spinner | `widgets::spinner` | 灰环 + 90° 彩色弧，0.9s/圈；安装态用绿色 |
| `.tpl-c__log` 日志卡 | `widgets::log_card` + `log_list` | 圆角 8、`W04` 底 + `W08` 描边、蓝点 + 12.5px 文本（行高 1.9） |
| 模板 D 内联日志 | `widgets::inline_log` | 无卡片容器，条目间 1px 分隔线 |
| `.tpl-x__pill` 标签 | `widgets::pill` | 圆角 999、`W10` 底、`W20` 描边 |
| `.tpl-x__btn` 按钮 | `widgets::button` | padding 7/26、圆角 6、主按钮蓝底白字、次按钮描边、禁用降低透明度 |
| 操作行 | `widgets::actions_row` → `Action{Retry,Close,Finish}` | 右对齐，含间距 8px |
| 关闭确认对话框 | `app::draw_close_confirm` + `modal_button` | 330×148 居中卡片、半透明遮罩吞点击 |

### 3.5 状态 / 交互

| 状态或交互 | Vue | egui |
| --- | --- | --- |
| 阶段归一化与模板切换 | `applyPhase` | `UpdaterApp::apply_phase` |
| 错误态保留已到达步骤 | 由组件局部状态实现 | `last_step` 字段（`step()==-2` 表示沿用） |
| 「校验」步骤点亮条件 | 是否进入过 download 阶段 | `has_downloaded` 字段，语义相同 |
| 重试按钮禁用态 | `retrying` ref | `UpdaterApp::retrying`（收到任意非 error 阶段即复位） |
| 完成 → 启动目标程序 → 退出 | `FINISH_CALLED` 守卫 + invoke | `finish_triggered` 守卫 + `EXIT_AFTER_FINISH` 原子位 |
| 关闭拦截 | Tauri `CloseRequested` + WebView 弹窗 | `CancelClose` + 自绘遮罩模态 |
| 重绘节奏 | 事件驱动 | 动画/下载中 33ms 重绘，静止态 150ms 轮询 |
| 标题栏拖拽 | `data-tauri-drag-region` | `ViewportCommand::StartDrag`（按钮矩形剔除） |

### 3.6 业务逻辑

`src-egui` **不实现任何业务逻辑**，全部经 `qsltd_updater_core::flow` 调用：

| 能力 | 调用入口 |
| --- | --- |
| 配置解析（CLI > update.json > 默认值，含 `launcher_template`） | `flow::resolve_config` |
| 纯检测（`--check-only`，退出码 0/1/2） | `flow::check_only_exit` |
| 窗口前启动决策（无更新 + launch 模式静默退出） | `flow::run_startup` |
| 完整更新流水线（已取过 manifest 的场景） | `flow::spawn_flow_with` |
| 重试 | `flow::retry` |
| 完成并启动目标程序 | `flow::finish_and_launch` |

## 四、实现差异

1. **数据通道：`emit` 改为共享快照轮询。**
   egui 没有 WebView IPC，后台线程通过 `PhaseHandle`（`Arc<Mutex<Option<PhasePayload>>>`）
   写入，UI 在 `logic()` 每帧 `snapshot()` 读取。载荷结构保持与 Tauri 一致，因此将来把 Tauri 侧
   也切到 `PhaseHandle::set` 即为一行替换。代价是刷新由帧驱动（静止态 150ms）而非瞬时，
   对更新程序这种低频 UI 无感知差异。

2. **渐变用 Mesh 近似。**
   CSS `linear-gradient(160deg, …)` 在 egui 中没有直接对应，采用带顶点色的三角网格（gouraud）
   做纵向渐变；进度条 `#4a9eff → #35c26e` 同理（左右渐变 + 圆角矩形裁剪）。160deg 的斜向分量被
   简化为纯纵向：在 420px 高的窗口里两者肉眼差异可忽略。

3. **环形进度用描边弧逼近 `conic-gradient`。**
   `conic-gradient` 无原生支持，改为分段折线 + `PathShape` 描边（64 段），并叠 `#1a2029` 内盘
   形成圆环；视觉宽度对齐 CSS 的 ring + inner 组合（96px / 78px）。

4. **字体必须手工注入，优先华为鸿蒙字体。**
   launcherclient 关闭了 eframe 的 `default_fonts`（不再打包 egui 内置字体），egui 自身没有 CJK 字形，
   不注入会直接豆腐块、甚至因字族为空而 panic。`src-ui/src/fonts.rs` 提供两种入口：launcherclient 走
   `fonts::install_embedded`，三个 `HarmonyOSSansSC-{Regular,Medium,Bold}.ttf`（`src-egui/assets/font/`）
   由 `include_bytes!` 直接编进 exe，单文件分发；`fonts::install`（pkgtool 等）则从 exe 同级 `fonts/`
   或源码目录加载。都缺失时逐字重回退系统字体：常规按平台为 Windows `msyh → msyhl → simhei → simsun → Deng → segoeui → arial`、
   macOS `PingFang → Hiragino Sans GB → STHeiti → Songti`、Linux `NotoSansCJK → SourceHanSans → wqy → DejaVu/Liberation`；
   粗体同理（`msyhbd` / `STHeiti Medium` / `NotoSansCJK-Bold` 等）。Medium/Bold 各自注册为独立字族
   （`harmony-medium` / `harmony-bold`），因为 egui 的 `FontId` 没有字重字段。
   pkgtool 仍启用默认字体，同一函数只是把字体插到原字体链里，行为不变。

5. **控件全部自绘，而非使用 egui 默认 `Button`/`ProgressBar`。**
   egui 默认控件的圆角、padding、hover 动效与 CSS 不一致，因此用 `Painter` 直接画。
   好处是与 CSS 逐像素对齐；代价是没有原生 focus ring / 键盘可达性 / DPI 自适应文本布局。

6. **`show_progress` 语义按 CSS 修正。**
   抽取过程中发现判据在两版间不一致：本次统一为 CSS 的语义——下载/安装恒显，
   检测阶段（`checking`）依赖 `sub` 是否非空。这样 `--stage` 预览中「检测中（带下载进度）」
   才会与 Vue 预览页行为一致。

7. **布局系统差异导致两处高度硬预留。**
   egui 是即时模式布局，没有 flexbox 的 `flex:1 + min-height:0`。模板 A/C 底部的日志卡若直接占满
   剩余高度，会把操作按钮挤出窗口；因此按「是否有操作行」预留 89px / 42px（`log_card(Some(h))`
   自身还有约 42px 卡片装饰）。同理 metrics/meta 行用固定右对齐宽度而非 `space-between`。

8. **预览模式做了工程化增强：`--stage <0-7|名称>`。**
   WebView 预览页靠点击 chips 切换；原生窗口需要自动化验证，故新增该参数，可在启动时直接进入
   指定阶段，便于无交互地批量截图/回归。它不属于核心 CLI，会在交给 `cli::parse_from` 前剥离。

9. **eframe 0.35 的 API 形态与常见示例不同。**
   本 crate 按 0.35 实际 API 编写：`App` 拆为 `logic()` + `ui(&mut Ui, …)`（`update` 已不存在）、
   无 `clear_color`、`ui.new_child(UiBuilder)`、`painter_at()` 返回值、`Shape::mesh()`、
   `Color32::from_rgba_unmultiplied_const`、`all_styles_mut(|s| …)`、`ViewportBuilder::default()`、
   creator 返回 `Result<Box<dyn App>, Box<dyn Error>>`。

## 五、已知限制

1. **无富文本 / HTML 排版能力。** changelog 条目只能按「点 + 单行文本」渲染，不解析 HTML 标签；
   超长条目依赖 egui 的换行策略而非 CSS `line-height` 精确排版。当前后端为空行数限制的单行文案，
   实际不受影响。
2. **无 DPI 感知的自适应弹性布局。** 窗口固定 440×420 且不缩放，因此与 Tauri 版一致地不具备响应式；
   在高 DPI 缩放下egui 会整体缩放而非重排，某些模板的长文本可能出现轻微换行差异。
3. **字体内嵌进 exe，体积增加约 5.1 MB。** 三个字重的华为 HarmonyOS Sans SC 随编译产物分发，
   不再依赖 exe 旁边的 `fonts/` 目录；磁盘目录与系统字体只作为兜底（末尾的 segoeui/arial 兜底在标准 Windows 上恒存在）。
   woff2 是 Brotli 容器，egui（skrifa）不认，必须使用 ttf/otf/ttc。
4. **渐变/环形为近似实现。** 160deg 斜向渐变 → 纵向近似；`conic-gradient` → 64 段描边近似；
   spinner 的 `animation-timing-function` 复刻为匀速旋转。
5. **仅 Windows 验证。** 与已删除的 Tauri 版相同，服务端操作（`sc`、`services`）、路径规则、
   字体加载路径均为 Windows 语义；未在 Linux/macOS 上编译或运行。
6. **无 WebView 层带来的能力缺失。** 没有 console、`--devtools`、热更新；调试需依赖 stderr 日志。
   若将来要在原生 UI 内嵌远端 HTML（如富文本更新公告），egui 版做不到，Tauri 版可以。
7. **预览模式与真实模式的数据来源不同。** 预览由 `preview::payload` 生成静态预设（数值照抄
   `Preview.vue`），不触碰网络与磁盘；真实流程的全部副作用由 `src-core::flow` 承担。
8. **release 构建较慢。** release profile 继承自既有约定（`lto=true`、`codegen-units=1`、`strip`），
   egui 依赖树较大，全量 release 约需 16 分钟；日常开发建议用 debug。

## 六、验证记录

> 本节是**首次复刻交付时（2026-10-26）的记录快照**：当时只有 A~E 五套模板、`src-core` 单测 134 个；
> 之后新增 F/G/H 三套（合计八套，默认模板改为 F），单测数量也随之增长，最新数字见 README 与
> `cargo test --lib` 当场输出。历史行保留原样，不作为当前状态描述。

| 项 | 命令 | 结果 |
| --- | --- | --- |
| 共享模块单测 | `cargo test --lib`（src-core） | **134 passed / 0 failed** |
| 既有 Tauri 应用（src-tauri） | 已于 2026-10-26 删除，不再验证 |
| 既有 zsui 原生版 | 已于 2026-10-26 删除，不再验证 |
| egui debug 构建 | `cargo build -j 2`（src-egui） | Finished（55MB exe） |
| egui release 构建 | `cargo build -j 2 --release` | Finished（15MB exe，16m41s） |
| 可执行文件启动 | `.exe --check-only --app-name qsm`（debug + release 均跑） | 正常拉 manifest → 「已是最新版本: 0.4.27」→ 退出码 0 |
| 五套模板 × 多阶段 | `--preview --template <A~E> --stage <0-7>` + GDI 截图 | 13 张截图，见 `docs/egui-preview-shots/` |
| 真实更新全流程 | 本地 mock 更新服务（8MB zip + 限速 + sha256） | manifest → 发现新版本 → 下载 → 校验通过 → 安装 → 写 app.json → 清理缓存/旧版本 → 启动失败转错误态 → 重试可重跑（均符合预期） |

> 说明：出于安全考虑，未对线上 manifest 执行真实更新；真实链路验证改用本地 mock 服务 +
> `--check-only`，二者覆盖的流程节点与产出物一致。

## 七、运行方式

```bash
# 预览八套模板（默认阶段下载中）
cargo run -p qsltdupdater-egui -- --preview
cargo run -p qsltdupdater-egui -- --preview --template B --stage 2
cargo run -p qsltdupdater-egui -- --preview --template F --name 聚合音乐 --stage 4

# 纯检测（不建窗，退出码 0=无更新 1=失败 2=有更新可跳过 3=必须更新）
cargo run -p qsltdupdater-egui -- --check-only --app-name qsm

# 真实更新（走共享 flow 流水线）
cargo run -p qsltdupdater-egui -- --app-name qsm
```

`--stage` 取值（11 个预设，也可传中文名称，未知值回落 `1`）：`0` 检测中 / `1` 下载中（分卷） /
`2` 校验中 / `3` 解压·安装中 / `4` 发现新版本 / `5` 已是最新 / `6` 更新完成 / `7` 错误 /
`8` 重试中 / `9` 强制更新（阻断） / `10` 强制更新（可稍后升级）。

## 八、实测反馈修复记录（2026-10-26）

真实使用反馈的两个问题及修复：

| 问题 | 根因 | 修复 |
| --- | --- | --- |
| 安装完成后目标程序已启动，但更新器窗口停在「更新完成」不关闭 | 共享 `flow.rs` 自动拉起目标程序成功后只返回 `Ok(())`，无退出信号（Tauri 版在自家 updater.rs 里是 `app.exit(0)`，egui 版走了共享 flow 就缺了这一步） | `PhaseHandle` 新增 `request_exit()` / `exit_requested()` 退出信号；`flow.rs` 三处启动成功后置位（`run_update_flow` 完成分支、`run_check_flow` 无更新分支、`finish_and_launch`）；`app.rs` 帧循环轮询到信号即发 `ViewportCommand::Close`，原 `EXIT_AFTER_FINISH` 静态标志随之移除 |
| 更新器窗口未水平垂直居中 | `tauri.conf.json` 有 `"center": true`，而 egui 的 `ViewportBuilder` 无 `with_center`，winit 用默认级联位置 | `app.rs` 首帧（`logic()`）读 `ViewportInfo.monitor_size` + `outer_rect`，按屏幕中心发 `ViewportCommand::OuterPosition`；窗口改为 `with_visible(false)` 隐藏启动、居中完成后再显示，避免「先在默认位置闪现再跳中心」；约 1s 拿不到显示器信息时兜底直接显示 |
| 生产环境下「更新日志」整段消失 | 共享 `flow.rs` 从未把 `manifest.changelog` 写进阶段载荷（`PhasePayload::new` 恒 `changelog: Vec::new()`），而 Tauri 版靠 `set_changelog()` + `push_phase()` 统一注入 —— 走共享 flow 的 egui 版就漏了这一整环 | `phase.rs` 增加进程级更新日志副本 `set_changelog()` / `changelog_snapshot()`（与 Tauri 版同语义），`PhasePayload::new` / `PhasePayload::download` 构造时自动携带；`flow.rs` 在 `run_startup` / `run_check_flow` 拿到 manifest 后登记一次，此后「发现新版本 → 下载 → 安装 → 完成」全程可见更新日志 |

验证（debug 构建）：

- 居中：1920×1080 下窗口实测中心 `(960,540)`，偏差 `(0,0)`，窗口可见。
- 自动关窗：真实端点全流程（发现 0.4.27 → 28 分卷下载 → 合并 → SHA256 → 服务跳过 → 安装 → 清理 → 启动 `qsm.exe`）后更新器进程 **51.1s 自动退出，退出码 0**；日志确认 `QSLTD_FROM_UPDATER=1` 已注入（目标程序跳过更新检测）。
- 更新日志：mock manifest（3 条 changelog）跑真实流程，用**像素对照实验**判定（因无法直接读图）：
  「有 changelog」vs「无 changelog」的 Available 阶段截图差异 5187px，其中 **5096px 连续集中在 y233-398**（日志卡片区域）；
  「无 changelog」vs「无 changelog」基线差异 **0px**（画面完全确定，无动画噪声干扰）。
  截图存 `docs/egui-preview-shots/egui_C_changelog.png` 与 `egui_C_no-changelog.png`。
  生产 manifest 确认带 `changelog` 字段（`["修复了某些bugs"]`），故问题在客户端而非服务端。
- **五套模板逐个验证更新日志**（真实流程 + 像素对照，「有/无 changelog」差异）：

  | 模板 | 日志呈现方式 | 差异像素 | 下半部差异 | 判定 |
  | --- | --- | --- | --- | --- |
  | A | 吸底日志卡（动态高度） | 7198 | 4456 | 已渲染 |
  | B | 固定 96px 日志卡（列表滚动） | 5584 | 5071 | 已渲染 |
  | C | 吸底日志卡（默认模板） | 6700 | 5156 | 已渲染 |
  | D | 内联日志（无卡片，分隔线） | 5107 | 3268 | 已渲染 |
  | E | 标题 + 「共 N 项」+ 列表/空态 | 5215 | 781 | 已渲染 |

  E 的差异分散在主体区（每行条目一行差异），故下半部计数小；D 为内联样式，同理。
- `cargo test --lib`（src-core）135 passed；src-egui clippy 无警告。（`src-tauri` / `src-zsui` 已于 2026-10-26 删除，不再纳入验证。）

注：manifest 覆盖参数为 `--manifest <地址>`（非 `--manifest-url`）。
本机存在 `http_proxy` 代理变量，访问 127.0.0.1 的 mock 服务需设 `NO_PROXY=127.0.0.1,localhost`。

## 修正记录（2026-10-26 后续）

1. **C 模板步骤条最右「完成」节点被裁剪**：原 `draw_steps` 节点圆心公式 `(2*i+1)*seg` 使第 4 个节点落在 `area.right()`（区域右边界），而 `painter_at(area)` 按区域矩形裁剪，圆点与标签超出右界被切（只露半截「完」字）。改为**首尾对称内缩**（`margin = dot_r + 8`、中间等分），连接线仍从圆点右缘连到下一圆点左缘，两端不再贴边、不被裁剪。
2. **下载标题与徽标重复**：分卷下载时 `flow.rs` 曾同时把「分卷 x/y」写进标题 message 与右上角 badge，导致 C 模板标题「正在下载更新包（分卷 1/28）」与徽标「分卷 1/28」冗余拥挤。已将分卷下载的标题统一改为「正在下载更新包...」，分卷进度仅由徽标承载（与单包模式标题语义一致）。`src-core` 单测 135 passed 无回归。

3. **检测更新态圆点"没显示完整"**：Checking 阶段 4 个节点均为 todo 态，原 todo 圆点仅用 `W12`（白色 12% 填充）+ `W18` 细描边，在 `#1e2430` 深色背景上填充近乎不可见，呈现"空心/没画满"的观感。已将 todo 圆点改为 `W18` 实心填充 + `text_alpha(80)` 更亮的环，使其清晰显示为完整圆。另将间距计算改为基于 `area.width()`（实际分配宽度）、圆点中心下移 3px，规避任何贴边裁切风险。

## Windows 资源（图标 + 文件版本信息）

参照 `SupboseService/QsLtdService` 的 `version.rc` / `build.rs` 模式，本启动器（src-egui）的 exe 图标与文件信息统一在 **`src-egui/version.rc`** 管理：

- `VS_VERSION_INFO VERSIONINFO`：公司名（QsLtd）、文件描述（QsLtd 更新程序）、文件/产品版本（取自 Cargo `0.1.8` → `0.1.8.0`）、内部名/原始文件名（`launcherclient` / `launcherclient.exe`）、版权。
- `StringFileInfo` 用 `080404b0`（中文 + Unicode 代码页 1200），与界面中文一致。
- 图标：`1 ICON "assets/icon.ico"`（ID 1，保证被 Explorer 作为 exe 图标）。

`src-egui/build.rs` 在 Windows 下调 `embed_resource::compile("version.rc", NONE).manifest_optional()` 编译嵌入；非 Windows 跳过。`rerun-if-changed` 已覆盖 `version.rc` 与 `assets/icon.ico`，改图标/版本无需手动清理缓存。

> 旧实现是在 `build.rs` 运行时于 `$OUT_DIR` 生成 `app.rc`（仅图标一行）；现已由 `version.rc` 源文件取代，信息更完整。
