

























## v0.1.23 (2026-10-27)
### 📝 文档更新

- [6143176](/-/commit/61431765bcd7c52b2b92173ece5cb7d31258b980) 文档(.workbuddy): 新增启动器项目完整 README 文档 (.workbuddy/memory/2026-10-27.md, README.md)

## v0.1.22 (2026-10-27)
### 🚀 新增功能

- [37d18b4](/-/commit/37d18b4be10b047f8c45dd1bd0a3a62fe23c3568) 新增(pkgtool): 打包阶段进度条与协作式取消（P1） (src-pkgtool/src/main.rs)
- [d483a79](/-/commit/d483a796fa428a2be5136cc2ff43e0bbad565df6) 新增(pkgtool): 新增打包进度回调与取消功能，修复界面假死 (.workbuddy/memory/2026-10-27.md, src-egui/version.rc, src-pkgtool/src/main.rs)
- [fcf23bc](/-/commit/fcf23bcf47cda1dda442ea90b9a0497cb1df1bc1) 新增(pkgtool): 按设计稿落地 P0 视觉改造（标签置顶/等宽/导航分组） (docs/更新包打包工具-界面重设计.html, src-pkgtool/src/main.rs)
- [3e898f3](/-/commit/3e898f3f54fcbd15e5a3b32ad33df947c71b10db) 新增(pkgtool): 打磨界面视觉并新增启动窗口居中 (src-pkgtool/src/main.rs)
- [17f0b37](/-/commit/17f0b37a17472f93c273b8c5a784f435e2aa6095) 新增(pkgtool): UI 重构为左导航多面板（方案B） (src-pkgtool/src/main.rs)

### 🩹 修复 Bug

- [514bb96](/-/commit/514bb966c2a24adfe48f00993d3a2e165584696d) 修复(pkgtool): 修复点击打包导致界面假死 (src-pkgtool/src/main.rs)

### 📝 文档更新

- [f424712](/-/commit/f42471286c518707ccfcec6784783e553f1c9b0a) 文档(.workbuddy): 更新 2026-10-27 工作记录，补充打包进度与取消功能细节 (.workbuddy/memory/2026-10-27.md)
- [41e3b82](/-/commit/41e3b826be678b87062939a3dc7931a57b2e99fa) 文档(.workbuddy): 添加 2026-10-27 工作日志记录 (.workbuddy/memory/2026-10-27.md)

### 💄 代码格式

- [ea15d90](/-/commit/ea15d909ed87de27083dd07daa64c6b4b949839d) 格式(pkgtool): 界面清爽化（白底卡片、淡蓝选中态、历史构建对齐去裁切） (src-pkgtool/src/main.rs)

### 🔧 工具变更

- [e54a36e](/-/commit/e54a36e83ab08f552393f6ae6124457b4fe6edc9) 工具(deps): 升级 egui/eframe 0.35 → 0.37（两个 crate 同步） (src-egui/Cargo.lock, src-egui/Cargo.toml, src-egui/src/app.rs, src-egui/src/widgets.rs, src-pkgtool/Cargo.lock, src-pkgtool/Cargo.toml)

## v0.1.21 (2026-10-27)
### 🚀 新增功能

- [1867438](/-/commit/186743836ce4bd09f7e7cc3495887aae19ad9724) 新增(pkgtool): 新增 egui 更新包打包工具，取代 versionTools.py (scripts/versionTools.py, src-pkgtool/Cargo.lock, src-pkgtool/Cargo.toml, src-pkgtool/assets/icon.png, src-pkgtool/build.rs, src-pkgtool/icon.rc, src-pkgtool/src/main.rs)

### 🔧 工具变更

- [f9fe1bc](/-/commit/f9fe1bc0910e789e37278798820c999c39347862) 工具(pkgtool): 在 package.json 增加 pkgtool 启动脚本 (package.json)

## v0.1.20 (2026-10-27)
### 🚀 新增功能

- [0a1003a](/-/commit/0a1003abf293e5356313d716fff9b50447a942e4) 新增(src-pkgtool): 新增Rust GUI版更新包打包工具替代Python脚本 (scripts/versionTools.py, src-egui/version.rc, src-pkgtool/Cargo.lock, src-pkgtool/Cargo.toml, src-pkgtool/assets/icon.ico, src-pkgtool/assets/icon.png, src-pkgtool/src/main.rs)

## v0.1.19 (2026-10-27)
### 🚀 新增功能

- [d292638](/-/commit/d2926386e214de1776ffb9be8294e0fa439e2b6f) 新增(egui): 添加应用名称参数到 run_ui 和 UpdaterApp (src-egui/src/main.rs)
- [0868125](/-/commit/086812509535204881722bf61d3e9f96db6580be) 新增(src-egui): 添加 egui 检测中极简屏及打包脚本 (package.json, scripts/__pycache__/build_egui.cpython-314.pyc, scripts/build_egui.py, src-egui/src/app.rs, src-egui/src/main.rs, src-egui/src/templates.rs, src-egui/version.rc)

## v0.1.18 (2026-10-26)
### 🚀 新增功能

- [444ed4d](/-/commit/444ed4df21482b39689acb51738eda960db58761) 新增(src-egui): 实现更新检测的懒加载窗口与加载状态反馈 (docs/egui界面复刻说明.md, src-egui/src/app.rs, src-egui/src/main.rs)

### ♻️ 代码重构

- [79cf4be](/-/commit/79cf4be17d9d47ba59ba3ee28f838a0ba8bbaebd) 重构(src-egui): 修复步骤条裁剪与下载标题冗余问题，更新Windows资源管理 (docs/egui界面复刻说明.md, src-core/src/flow.rs, src-egui/build.rs, src-egui/src/templates.rs, src-egui/version.rc)
- [5dbd09a](/-/commit/5dbd09af1e206268f9c9e5ffdc710cdd232dbaa1) 重构(egui): 重构步骤条绘制逻辑并优化进度连接线样式 (src-egui/src/templates.rs)

### 📝 文档更新

- [fa4fd46](/-/commit/fa4fd4690a846b46b065a5a1926c183a4681c027) 文档(.workbuddy): 更新版本信息与修复步骤条及下载标题问题 (.workbuddy/memory/2026-10-26.md, src-egui/version.rc)
- [67c44d4](/-/commit/67c44d44dc861d2640c57b4291dce3848758e1ee) 文档(.workbuddy): 更新步骤条实现说明及添加模板美化优化记录 (.workbuddy/memory/2026-10-26.md, docs/egui界面复刻说明.md)

### 📦 构建系统

- [da9001c](/-/commit/da9001ccd2bb50baa9f9466bd0f5305d5dbf2b0a) 构建(egui): 修改二进制名称为 launcherclient (.workbuddy/memory/2026-10-26.md, src-egui/Cargo.toml)

## v0.1.17 (2026-10-26)
### 🚀 新增功能

- [a42fb1f](/-/commit/a42fb1fe5daf7ba67f9eb0b25f122602be947bcb) 新增(src-egui): 添加应用图标支持（嵌入exe与运行时窗口图标） (src-egui/Cargo.lock, src-egui/Cargo.toml, src-egui/assets/icon.ico, src-egui/assets/icon.png, src-egui/build.rs, src-egui/src/main.rs)

## v0.1.16 (2026-10-26)
### ♻️ 代码重构

- [e111f3d](/-/commit/e111f3dca79400f503da46549a74c84920553453) 重构(src-tauri): 删除已废弃的 Tauri 和 zsui 启动器壳，仅保留 egui 作为生产壳 (.workbuddy/memory/2026-10-26.md, docs/Tauri-vs-egui性能对比.md, docs/egui界面复刻说明.md, docs/选型决策记录.md, index.html, package.json, pnpm-lock.yaml, pnpm-workspace.yaml, preview.html, public/splash.html, src-core/src/config.rs, src-core/src/defaults.rs, src-core/src/flow.rs, src-core/src/lib.rs, src-core/src/phase.rs, src-core/src/services.rs, src-egui/Cargo.toml, src-egui/src/main.rs, src-tauri/Cargo.lock, src-tauri/Cargo.toml, src-tauri/build.rs, src-tauri/capabilities/default.json, src-tauri/icons/128x128.png, src-tauri/icons/128x128@2x.png, src-tauri/icons/32x32.png, src-tauri/icons/64x64.png, src-tauri/icons/Square107x107Logo.png, src-tauri/icons/Square142x142Logo.png, src-tauri/icons/Square150x150Logo.png, src-tauri/icons/Square284x284Logo.png, src-tauri/icons/Square30x30Logo.png, src-tauri/icons/Square310x310Logo.png, src-tauri/icons/Square44x44Logo.png, src-tauri/icons/Square71x71Logo.png, src-tauri/icons/Square89x89Logo.png, src-tauri/icons/StoreLogo.png, src-tauri/icons/icon.icns, src-tauri/icons/icon.ico, src-tauri/icons/icon.png, src-tauri/src/lib.rs, src-tauri/src/main.rs, src-tauri/src/updater.rs, src-tauri/tauri.conf.json, src-zsui/Cargo.lock, src-zsui/Cargo.toml, src-zsui/README.md, src-zsui/src/main.rs, src-zsui/src/ui.rs, src-zsui/src/updater.rs, src/App.vue, src/Preview.vue, src/components/MinTitleBar.vue, src/components/templates/UpdateTemplateA.vue, src/components/templates/UpdateTemplateB.vue, src/components/templates/UpdateTemplateC.vue, src/components/templates/UpdateTemplateD.vue, src/components/templates/UpdateTemplateE.vue, src/components/templates/index.js, src/components/templates/usePhase.js, src/main.js, src/preview.css, src/preview.js, vite.config.js)

## v0.1.15 (2026-10-26)
### 📝 文档更新

- [b4d2fb4](/-/commit/b4d2fb4a1b4db131c566afaf34d9334aad75c079) 文档(docs): 弃用 zsui 原生 UI 实现并修复更新日志未传递问题 (.workbuddy/memory/2026-10-26.md, docs/Tauri-vs-egui性能对比.md, docs/egui-preview-shots/egui_C_changelog.png, docs/egui-preview-shots/egui_C_no-changelog.png, docs/egui界面复刻说明.md, src-core/src/flow.rs, src-core/src/lib.rs, src-core/src/phase.rs, src-zsui/README.md, src-zsui/src/main.rs)

## v0.1.14 (2026-10-26)
### 🚀 新增功能

- [d4da70a](/-/commit/d4da70a608e07ded1cbaaf771710415dffc02e49) 新增(docs): 新增 egui 原生 UI 实现更新程序界面 (.workbuddy/memory/2026-10-26.md, docs/egui-preview-shots/egui_A_dl.png, docs/egui-preview-shots/egui_A_err.png, docs/egui-preview-shots/egui_B_dl.png, docs/egui-preview-shots/egui_B_done.png, docs/egui-preview-shots/egui_B_inst.png, docs/egui-preview-shots/egui_C_check.png, docs/egui-preview-shots/egui_C_done.png, docs/egui-preview-shots/egui_C_inst.png, docs/egui-preview-shots/egui_C_verify.png, docs/egui-preview-shots/egui_D_dl.png, docs/egui-preview-shots/egui_D_done.png, docs/egui-preview-shots/egui_E_done.png, docs/egui-preview-shots/egui_E_verify.png, docs/egui-preview-shots/egui_b2.png, docs/egui-preview-shots/egui_b3.png, docs/egui-preview-shots/egui_b_preview.png, docs/egui-preview-shots/egui_tplC_dl.png, docs/egui-preview-shots/real-flow-error.png, docs/egui界面复刻说明.md, package.json, src-core/src/flow.rs, src-core/src/lib.rs, src-core/src/phase.rs, src-egui/Cargo.lock, src-egui/Cargo.toml, src-egui/src/app.rs, src-egui/src/main.rs, src-egui/src/preview.rs, src-egui/src/templates.rs, src-egui/src/theme.rs, src-egui/src/widgets.rs)

### 🩹 修复 Bug

- [9acce4c](/-/commit/9acce4c9a3bf07cb9a06b1dbab5a99c5db83bb95) 修复(egui): 修复更新完成后窗口不关闭及启动器未居中问题 (.workbuddy/memory/2026-10-26.md)

## v0.1.13 (2026-10-25)
### 🩹 修复 Bug

- [0cd0377](/-/commit/0cd037787273d58801d7c8d2bc10d4ef9a3bfa4b) 修复(updater): 修复服务卸载报错无法定位问题 (.workbuddy/memory/2026-10-25.md, src-core/src/services.rs)

## v0.1.12 (2026-10-24)
### 🩹 修复 Bug

- [8d9328c](/-/commit/8d9328c82e72c03b59d0ec690c03248b7acccced) 修复(src-core): 修复 launch 留空时无更新/安装完成不自动拉起应用的问题 (.workbuddy/memory/2026-10-24.md, src-core/src/cli.rs, src-core/src/config.rs, src-tauri/src/updater.rs)

## v0.1.11 (2026-10-23)
### 🚀 新增功能

- [ed5c986](/-/commit/ed5c986a2a2fcebcdcd168ef8858541a11a4d1ee) 新增(docs): 实现五套启动器界面模板及配置支持 (.workbuddy/memory/2026-10-22.md, docs/preview-shots/tpl-A-done.png, docs/preview-shots/tpl-A-downloading.png, docs/preview-shots/tpl-A-error.png, docs/preview-shots/tpl-A-installing.png, docs/preview-shots/tpl-A-verifying.png, docs/preview-shots/tpl-B-done.png, docs/preview-shots/tpl-B-downloading.png, docs/preview-shots/tpl-B-error.png, docs/preview-shots/tpl-B-installing.png, docs/preview-shots/tpl-B-verifying.png, docs/preview-shots/tpl-C-done.png, docs/preview-shots/tpl-C-downloading.png, docs/preview-shots/tpl-C-error.png, docs/preview-shots/tpl-C-installing.png, docs/preview-shots/tpl-C-verifying.png, docs/preview-shots/tpl-D-done.png, docs/preview-shots/tpl-D-downloading.png, docs/preview-shots/tpl-D-error.png, docs/preview-shots/tpl-D-installing.png, docs/preview-shots/tpl-D-verifying.png, docs/preview-shots/tpl-E-done.png, docs/preview-shots/tpl-E-downloading.png, docs/preview-shots/tpl-E-error.png, docs/preview-shots/tpl-E-installing.png, docs/preview-shots/tpl-E-verifying.png, docs/ui-drafts-下载校验解压.html, preview.html, src-core/src/cli.rs, src-core/src/config.rs, src-core/src/defaults.rs, src-tauri/src/updater.rs, src-tauri/tauri.conf.json, src/App.vue, src/Preview.vue, src/components/templates/UpdateTemplateA.vue, src/components/templates/UpdateTemplateB.vue, src/components/templates/UpdateTemplateC.vue, src/components/templates/UpdateTemplateD.vue, src/components/templates/UpdateTemplateE.vue, src/components/templates/index.js, src/components/templates/usePhase.js, src/preview.css, src/preview.js, update.json)

## v0.1.10 (2026-10-20)
### 🩹 修复 Bug

- [d0f233f](/-/commit/d0f233f70c2cbe0deb2e7e2e3195c49224354061) 修复(tauri): 更新后启动新版本二进制，修复交接模式启动旧版/静默失败 (src-tauri/src/updater.rs)

### 📝 文档更新

- [d0f34d1](/-/commit/d0f34d186d5be740dfe14ce0896577b67875ba70) 文档(.workbuddy): 工作日志追加「更新→启动链路专项检测」记录 (.workbuddy/memory/2026-10-20.md)

## v0.1.9 (2026-10-20)
### 🚀 新增功能

- [831808a](/-/commit/831808aecfd541f699981f3519ad7a963c2da761) 新增(tauri): 错误态新增「重试」按钮，弱网/服务端故障可自助恢复 (src-tauri/src/lib.rs, src-tauri/src/updater.rs, src/App.vue)
- [eb23412](/-/commit/eb23412ffacb6c85f7684adc4a6d56e616124cfd) 新增(tauri): 关键阶段关窗弹确认，防「服务已卸、app.json 未写」不一致态 (src-tauri/Cargo.lock, src-tauri/Cargo.toml, src-tauri/capabilities/default.json, src-tauri/src/lib.rs, src-tauri/src/updater.rs)

### 🩹 修复 Bug

- [9909e27](/-/commit/9909e273d604005ca2369703f50af71660540c24) 修复(tauri): 交接模式 splash 零闪现（create:false + setup 按配置构建） (public/splash.html, src-tauri/src/lib.rs, src-tauri/src/updater.rs, src-tauri/tauri.conf.json)
- [4b4cf4a](/-/commit/4b4cf4a2f1c98404034ae4592ef4a03b6a4fd042) 修复(tauri): 安装成功后清理下载缓存，downloads 回归暂存区语义 (src-tauri/src/updater.rs)
- [e8a7f5e](/-/commit/e8a7f5ee8fd79dc8be37c3cdd7741d6068ff30cb) 修复(install): 解压加三道闸防 zip bomb，总量 8GB / 单条目 512MB 硬顶 (src-core/src/install.rs)
- [9639ca9](/-/commit/9639ca97a83c27735b889941480ebe400a0b7145) 修复(core): 服务卸载失败返回错误，tauri 版安装前终止更新 (src-core/src/services.rs, src-tauri/src/updater.rs, src-zsui/src/updater.rs)
- [fbcea7c](/-/commit/fbcea7cb9b409c1506d0efe38d4be3b51ae0cd02) 修复(tauri): finish_and_launch 加重入守卫，防止启动两个应用实例 (src-tauri/src/updater.rs)
- [0beafd1](/-/commit/0beafd1e6d54ac0e82d706027bdb693dcbb8e12d) 修复(ui): current_phase 快照仅兜底不覆盖，修复界面阶段回退 (src/App.vue)
- [9823bf3](/-/commit/9823bf3d3bbdf09d1bd3099ae87c4b960c539dba) 修复(install): 临时目录命名唯一化，消除跨版本共名 (src-core/src/install.rs)
- [cc4f059](/-/commit/cc4f05907135fdb5aa80d52fd8f566fd223cfc80) 修复(core): 本地包复用前必须有校验值，杜绝半包/损坏包被当有效包 (src-core/src/download.rs, src-tauri/src/updater.rs, src-zsui/src/updater.rs)

### 🔥 性能优化

- [8af0db9](/-/commit/8af0db97a4e5da4e39c3c4fd1f18f5f5e606a97e) 优化(core): manifest 拉取加 5 分钟 TTL 缓存，无更新路径免联网 (src-core/src/manifest.rs, src-tauri/src/updater.rs)
- [9417636](/-/commit/94176360d808ebf03fc2823c5babfad3bd97c008) 优化(core): 共享 reqwest Client，消除每请求新建的连接/TLS 握手浪费 (src-core/src/client.rs, src-core/src/download.rs, src-core/src/lib.rs, src-core/src/manifest.rs)

### 📝 文档更新

- [fffb78f](/-/commit/fffb78f45b8df9c99c78c338dece53ff99e4abc4) 文档(.workbuddy): 工作日志追加第三批（P3 体验 5 项）执行记录与踩坑 (.workbuddy/memory/2026-10-20.md)
- [35c5353](/-/commit/35c535342e77ed5e83daa6f11f412101342f93de) 文档(.workbuddy): 工作日志追加第二批（P2 安全卫生 2 项）执行记录 (.workbuddy/memory/2026-10-20.md)
- [12ab1a9](/-/commit/12ab1a9e02dad8a6bf075806ca918c1a396b8e9d) 文档(zsui): 声明实验版冻结，主维护版收敛为 src-tauri (src-zsui/README.md)

### 🔧 工具变更

- [175bdf6](/-/commit/175bdf64911bff4d5b07b4f602c2175b4797cf30) 工具(.workbuddy): 同步 Cargo.lock 版本号（0.1.8）+ 更新工作日志 (.workbuddy/memory/2026-10-20.md, src-core/Cargo.lock, src-tauri/Cargo.lock, src-zsui/Cargo.lock)
- [10f2583](/-/commit/10f258318c582c6ca59d32830cf18c2af3300aa1) 工具(src-tauri): 统一版本号为 0.1.8（消除 package.json/tauri.conf/Cargo.toml 三处不一致） (src-core/Cargo.toml, src-tauri/Cargo.toml, src-tauri/tauri.conf.json, src-zsui/Cargo.toml)

### ✅ 测试用例

- [599636d](/-/commit/599636d3d5560256de4bde05939ee2a423842c24) 测试(core): 单测从 33 扩到 92，覆盖此前零测试的五个模块 (src-core/src/cleanup.rs, src-core/src/cli.rs, src-core/src/config.rs, src-core/src/download.rs, src-core/src/manifest.rs)

## v0.1.8 (2026-10-20)
### 🚀 新增功能

- [a97ec4c](/-/commit/a97ec4ce102c2b7c0fa48ef8f4b21754f6238226) 新增(.workbuddy): 对齐 qsm 方案优化 splash 启动加载逻辑 (.workbuddy/memory/2026-10-19.md, .workbuddy/memory/2026-10-20.md)
- [5c6139c](/-/commit/5c6139ca6ef004104d079ceeffa6433ac975e814) 新增(tauri): splash 改静态声明对齐 qsm，无更新补足最短展示 (public/splash.html, src-tauri/src/lib.rs, src-tauri/src/updater.rs, src-tauri/tauri.conf.json)

### 📦 构建系统

- [e309a88](/-/commit/e309a8851698686d1d5c51d8f66ef6f9930e8fc3) 构建(release): 加固 release profile 降低杀软启发式误报率 (src-tauri/Cargo.toml)

## v0.1.7-beta.0 (2026-10-20)
### 🚀 新增功能

- [09baec3](/-/commit/09baec346ada18a934d1130600f5843795a25bd5) 新增(.workbuddy): 为启动页添加动态 splash 加载逻辑 (.workbuddy/memory/2026-10-19.md)
- [dfbd360](/-/commit/dfbd360f5a4b9a8de097f4f7353b77025ac84094) 新增(tauri): 引入 qsm 同款 splash 启动加载页，消除检测期无界面空档 (public/splash.html, src-tauri/src/lib.rs, src-tauri/src/updater.rs)

## v0.1.6 (2026-10-20)
### 🚀 新增功能

- [36f80ad](/-/commit/36f80ad6dea89d54fa548c46d23f85736ffe1302) 新增(tauri): 直接运行时检测阶段显示「正在检查更新」窗口 (src-tauri/src/updater.rs)

### 🩹 修复 Bug

- [567da42](/-/commit/567da42f5e19e78cd383f16bf0a12751ef94f210) 修复(tauri): 手动模式点「完成」启动更新后的应用，并修复阶段事件竞态 (src-core/src/install.rs, src-tauri/Cargo.toml, src-tauri/src/lib.rs, src-tauri/src/updater.rs, src/App.vue)

### ♻️ 代码重构

- [9bc579b](/-/commit/9bc579bb5e8fd36139359168c44c8c46b12aa326) 重构(core): 内置默认值收敛到 src-core/defaults.rs 单一来源 (src-core/src/config.rs, src-core/src/defaults.rs, src-core/src/lib.rs, src-tauri/src/updater.rs, src-zsui/src/updater.rs)
- [b74c6d8](/-/commit/b74c6d8d87f6cccf2421f09f4237847ffd74df6e) 重构(core): 抽离共享核心 crate src-core，消除两套实现的后端重复 (src-core/Cargo.lock, src-core/Cargo.toml, src-core/src/cleanup.rs, src-core/src/cli.rs, src-core/src/config.rs, src-core/src/download.rs, src-core/src/install.rs, src-core/src/lib.rs, src-core/src/manifest.rs, src-core/src/services.rs, src-tauri/Cargo.lock, src-tauri/Cargo.toml, src-tauri/src/lib.rs, src-tauri/src/updater.rs, src-zsui/Cargo.lock, src-zsui/Cargo.toml, src-zsui/src/cleanup.rs, src-zsui/src/cli.rs, src-zsui/src/config.rs, src-zsui/src/download.rs, src-zsui/src/install.rs, src-zsui/src/main.rs, src-zsui/src/manifest.rs, src-zsui/src/services.rs, src-zsui/src/updater.rs)

### 📝 文档更新

- [1a1d034](/-/commit/1a1d0347299a1ff0f712ca90c554be92c0cd5d5f) 文档(.workbuddy): 添加2026-10-19项目分析与重构备忘文档 (.workbuddy/memory/2026-10-19.md)

### ✅ 测试用例

- [21f799f](/-/commit/21f799fdad8c4430a380c40cff38d58288c56d99) 测试(services): 抽象 ScRunner 隔离破坏性测试，补 22 个纯单元测试 (src-core/Cargo.toml, src-core/src/services.rs, src-tauri/src/updater.rs, src-zsui/src/updater.rs)

## v0.1.5 (2026-10-19)
### 🚀 新增功能

- [a815696](/-/commit/a8156964fd5b8263e574f0c6fdcb09c2dc810efb) 新增(src-zsui): 新增更新前后Windows服务自动管理功能 (src-tauri/src/config.rs, src-tauri/src/lib.rs, src-tauri/src/services.rs, src-tauri/src/updater.rs, src-zsui/README.md, src-zsui/src/config.rs, src-zsui/src/main.rs, src-zsui/src/services.rs, src-zsui/src/updater.rs)

### ♻️ 代码重构

- [138d4b4](/-/commit/138d4b483ce07c7ac403625a3f608c465716bdb6) 重构(src-zsui): 将更新前服务处理改为卸载而非停止并恢复 (src-tauri/src/config.rs, src-tauri/src/services.rs, src-tauri/src/updater.rs, src-zsui/README.md, src-zsui/src/config.rs, src-zsui/src/services.rs, src-zsui/src/updater.rs)

## v0.1.4 (2026-10-18)
## v0.1.3-beta.0 (2026-10-18)
### 🚀 新增功能

- [888ab9e](/-/commit/888ab9e6e232a4fd78f834b13376085fbd78ccca) 新增(src-tauri): 添加纯检测模式并优化网络超时配置 (src-tauri/Cargo.lock, src-tauri/Cargo.toml, src-tauri/src/cli.rs, src-tauri/src/config.rs, src-tauri/src/download.rs, src-tauri/src/lib.rs, src-tauri/src/manifest.rs, src-tauri/src/updater.rs, src-zsui/README.md, src-zsui/src/cli.rs, src-zsui/src/config.rs, src-zsui/src/download.rs, src-zsui/src/main.rs, src-zsui/src/manifest.rs, src-zsui/src/updater.rs)

## v0.1.2 (2026-10-18)
### 🚀 新增功能

- [afd4785](/-/commit/afd47859acb66b02cdcb9992338c90f8f937ad8f) 新增(src-zsui): 重构更新核心库，增加合并与安装进度回调支持 (src-tauri/Cargo.lock, src-tauri/Cargo.toml, src-tauri/src/download.rs, src-tauri/src/install.rs, src-tauri/src/updater.rs, src-zsui/Cargo.lock, src-zsui/Cargo.toml, src-zsui/src/download.rs, src-zsui/src/install.rs, src-zsui/src/ui.rs, src-zsui/src/updater.rs, src/App.vue)

### 🩹 修复 Bug

- [3976fe4](/-/commit/3976fe45d7859d9f35bb6194a11bfb18d1543619) 修复(updater): 仅检测模式下静默退出不显示主窗口，添加启动环境变量标记 (src-tauri/src/updater.rs)

## v0.1.1-beta.4 (2026-10-18)
### 🚀 新增功能

- [6aae593](/-/commit/6aae5938a7ec8d76fb3d8db0efb0fe2a620d8d57) 新增(updater): 添加防重入启动标记环境变量 (src-zsui/README.md, src-zsui/src/updater.rs)

## v0.1.1-beta.3 (2026-10-18)
### 🚀 新增功能

- [445a2da](/-/commit/445a2da17ce0b3d30e892ef87d17359cad25b2e4) 新增(src-zsui): 新增 zsui 原生 UI 版独立更新程序实现 (.gitignore, package.json, src-zsui/Cargo.lock, src-zsui/Cargo.toml, src-zsui/README.md, src-zsui/src/cleanup.rs, src-zsui/src/cli.rs, src-zsui/src/config.rs, src-zsui/src/download.rs, src-zsui/src/install.rs, src-zsui/src/main.rs, src-zsui/src/manifest.rs, src-zsui/src/ui.rs, src-zsui/src/updater.rs)

## v0.1.1-beta.2 (2026-10-18)
### 🚀 新增功能

- [f3df1b3](/-/commit/f3df1b3184f620ced37c7a37b458a6a87daf7297) 新增(src-tauri): 为更新程序添加下载详情显示与配置模板生成 (src-tauri/Cargo.toml, src-tauri/src/config.rs, src-tauri/src/download.rs, src-tauri/src/manifest.rs, src-tauri/src/updater.rs, src-tauri/tauri.conf.json, src/App.vue)

## v0.1.1-beta.1 (2026-10-18)
# Changelog

## [0.1.1-beta.0] - 2026-10-18
### 🚀 新增功能

- [50d4d28](/-/commit/50d4d2816a9085dcb0005999561a198f90d69a03) 新增(updater): 支持分卷下载与多地址故障转移更新 (pnpm-lock.yaml, pnpm-workspace.yaml, src-tauri/Cargo.lock, src-tauri/src/cleanup.rs, src-tauri/src/config.rs, src-tauri/src/download.rs, src-tauri/src/install.rs, src-tauri/src/lib.rs, src-tauri/src/manifest.rs, src-tauri/src/updater.rs, src/App.vue, update.json, version.json.example)
- [ebd9f82](/-/commit/ebd9f82d60ce7abd80f84c8b532db7ce018fea15) 新增(src-tauri): 新增 Tauri 架构的 QsLtd 更新程序初始代码 (.gitignore, index.html, package.json, src-tauri/Cargo.lock, src-tauri/Cargo.toml, src-tauri/build.rs, src-tauri/capabilities/default.json, src-tauri/icons/128x128.png, src-tauri/icons/128x128@2x.png, src-tauri/icons/32x32.png, src-tauri/icons/64x64.png, src-tauri/icons/Square107x107Logo.png, src-tauri/icons/Square142x142Logo.png, src-tauri/icons/Square150x150Logo.png, src-tauri/icons/Square284x284Logo.png, src-tauri/icons/Square30x30Logo.png, src-tauri/icons/Square310x310Logo.png, src-tauri/icons/Square44x44Logo.png, src-tauri/icons/Square71x71Logo.png, src-tauri/icons/Square89x89Logo.png, src-tauri/icons/StoreLogo.png, src-tauri/icons/icon.icns, src-tauri/icons/icon.ico, src-tauri/icons/icon.png, src-tauri/src/cli.rs, src-tauri/src/config.rs, src-tauri/src/download.rs, src-tauri/src/lib.rs, src-tauri/src/main.rs, src-tauri/src/manifest.rs, src-tauri/src/updater.rs, src-tauri/tauri.conf.json, src/App.vue, src/components/MinTitleBar.vue, src/main.js, update.json, vite.config.js)

