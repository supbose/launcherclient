























## v0.2.6-beta.0 (2026-10-30)
### 🚀 新增功能

- [589cfbb](/-/commit/589cfbb2304e0e327a5bb2994a69a9341798b859) 新增(src-core): 支持更新检查失败时提供官网下载入口 (README.md, launcherclient-guide/launcherclient-guide.html, src-core/src/config.rs, src-core/src/flow.rs, src-core/src/phase.rs, src-egui/src/widgets.rs, src-pkgtool/Cargo.lock)

### 📦 构建系统

- [c5ecbae](/-/commit/c5ecbae18490492db8673242f96277b8744e78af) 构建(src-pkgtool): 更新版本号至 0.2.5 (Cargo.lock, Cargo.toml, src-egui/version.rc, src-pkgtool/Cargo.toml, src-pkgtool/icon.rc)

## v0.2.5 (2026-10-30)
### 🚀 新增功能

- [9cdb081](/-/commit/9cdb081d04f453a5fbcf61c2c4ef94dac40fd15b) 新增(README.md): 新增首次安装的更新处理逻辑 (README.md, src-core/src/flow.rs)

### 🩹 修复 Bug

- [feb3e1f](/-/commit/feb3e1fbc95de405df555d7fe379b3b248b64e96) 修复(core): 校验 app_name/version 可作单级路径分量，杜绝路径穿越 (src-core/src/appdir.rs, src-core/src/client.rs, src-core/src/flow.rs, src-core/src/manifest.rs, src-egui/src/main.rs)
- [c3b8da2](/-/commit/c3b8da24e87e3be12587130dc7738f9adcc55845) 修复(core): 安装改为先解压到临时目录再交换，避免解压失败毁掉现有版本 (src-core/src/install.rs)
- [fe583ca](/-/commit/fe583ca34359f7fe7c0d26342f7b10f89d4f4dd0) 修复(egui): 修复隐藏启动的显示门闩，避免首检结果晚到后窗口永不显示 (src-egui/src/app.rs)
- [6102c79](/-/commit/6102c79714ef9046b0ef5219509d7d7b67a49253) 修复(core): 校验失败归入下载环节并复用已算出的 SHA256 (src-core/src/download.rs, src-core/src/flow.rs)

### 🔧 工具变更

- [5a87d69](/-/commit/5a87d6952c9caec3b847bb17d15572b5f2acdcdf) 工具(src-pkgtool): 将 0.2.4 版本铺开到 Cargo 清单与 rc 资源 (Cargo.lock, Cargo.toml, src-egui/version.rc, src-pkgtool/Cargo.lock, src-pkgtool/Cargo.toml, src-pkgtool/icon.rc)

## v0.2.4 (2026-10-30)
### 📦 构建系统

- [78ad7e1](/-/commit/78ad7e11097253442b074c0e24980068a8bead9c) 构建(src-pkgtool): 所有版本号更新至0.2.3 (Cargo.lock, Cargo.toml, src-egui/version.rc, src-pkgtool/Cargo.lock, src-pkgtool/Cargo.toml, src-pkgtool/icon.rc)

## v0.2.3 (2026-10-30)
### 🚀 新增功能

- [87b7f1a](/-/commit/87b7f1a94e34be78beae69a6c163cc9028e7ce2b) 新增(src-egui): 新增共享 UI 原语 crate qsltd-ui，供 launcherclient 与 pkgtool 共用 (.cnb.yml, .gitignore, Cargo.lock, Cargo.toml, README.md, launcherclient-guide/launcherclient-guide.html, package.json, src-core/examples/gen_cli_docs.rs, src-core/src/cli.rs, src-egui/Cargo.toml, src-egui/src/templates.rs, src-egui/src/theme.rs, src-egui/src/widgets.rs, src-egui/version.rc, src-pkgtool/Cargo.lock, src-pkgtool/Cargo.toml, src-pkgtool/icon.rc, src-pkgtool/src/main.rs, src-ui/Cargo.toml, src-ui/src/lib.rs, src-ui/src/theme.rs, src-ui/src/widgets.rs)

### 🔧 工具变更

- [4154bbd](/-/commit/4154bbd8affc4875d1f7eff1cd4acf2c32e2d638) 工具(scripts): 删除编译缓存和崩溃日志文件 (scripts/__pycache__/build_egui.cpython-314.pyc, scripts/__pycache__/pybuild.cpython-312.pyc, scripts/__pycache__/pybuild.cpython-314.pyc, src-core/Cargo.lock, src-egui/Cargo.lock, src-pkgtool/bt.log, src-pkgtool/crash.log)

## v0.2.2 (2026-10-30)
### 📦 构建系统

- [02dd0f0](/-/commit/02dd0f04b97cb650518c5da7849cbb925bd57112) 构建(src-core): 重构工作区配置，统一版本管理与构建 profile (.cnb.yml, Cargo.lock, Cargo.toml, README.md, package.json, scripts/__pycache__/pybuild.cpython-314.pyc, scripts/build.py, scripts/pybuild.py, src-core/Cargo.toml, src-core/build.rs, src-core/src/appdir.rs, src-core/src/cleanup.rs, src-core/src/cli.rs, src-core/src/config.rs, src-core/src/download.rs, src-core/src/elevation.rs, src-core/src/flow.rs, src-core/src/install.rs, src-core/src/instance.rs, src-core/src/lib.rs, src-core/src/local_manifest.rs, src-core/src/manifest.rs, src-core/src/phase.rs, src-core/src/services.rs, src-core/src/shell.rs, src-egui/Cargo.toml, src-egui/build.rs, src-egui/src/app.rs, src-egui/src/main.rs, src-egui/src/preview.rs, src-egui/src/templates.rs, src-egui/src/theme.rs, src-egui/src/widgets.rs, src-pkgtool/Cargo.lock, src-pkgtool/Cargo.toml, src-pkgtool/icon.rc, src-pkgtool/src/main.rs)
- [e477838](/-/commit/e477838fe3d268be958a0c217560ab88180bfb76) 构建(src-egui): 更新版本号至0.2.1.0 (src-egui/version.rc)

## v0.2.1 (2026-10-30)
### 🚀 新增功能

- [3db78f3](/-/commit/3db78f39274aa55ac621e79964161e76e8e9d5e2) 新增(src-core): 内聚自研能力库，新增单实例互斥与包完整性校验机制 (README.md, launcherclient-guide/launcherclient-guide.html, src-core/Cargo.lock, src-core/Cargo.toml, src-core/src/appdir.rs, src-core/src/cli.rs, src-core/src/config.rs, src-core/src/download.rs, src-core/src/elevation.rs, src-core/src/flow.rs, src-core/src/install.rs, src-core/src/instance.rs, src-core/src/lib.rs, src-core/src/local_manifest.rs, src-core/src/manifest.rs, src-core/src/shell.rs, src-egui/Cargo.lock, src-egui/Cargo.toml, src-egui/src/main.rs, update.json)

## v0.2.0 (2026-10-30)
## v0.1.39 (2026-10-30)
### 🚀 新增功能

- [e040465](/-/commit/e040465ec53ac819aaa99f60d7f07b9da6d59485) 新增(src-core): 扩展界面模板至A~H并新增强制更新阶段 (README.md, docs/egui界面复刻说明.md, launcherclient-guide/launcherclient-guide.html, package.json, scripts/pybuild.py, src-core/src/cli.rs, src-core/src/config.rs, src-core/src/flow.rs, src-core/src/lib.rs, src-core/src/phase.rs, src-egui/src/main.rs, src-egui/src/templates.rs, update.json)
- [d10c637](/-/commit/d10c63721780bd759504b22b06f5ce3adabf31ec) 新增(src-core): 支持通过命令行/配置文件设置启动器标题 (README.md, docs/egui界面复刻说明.md, launcherclient-guide/launcherclient-guide.html, src-core/src/cli.rs, src-core/src/config.rs, src-core/src/elevation.rs, src-core/src/flow.rs, src-egui/src/app.rs, src-egui/src/main.rs, src-egui/src/theme.rs, src-egui/src/widgets.rs, update.json)

### 🩹 修复 Bug

- [87d3438](/-/commit/87d343896a4b25979349f4a214b16e6cbb058f35) 修复(src-core): 更新默认的清单 URL 和主页地址 (src-core/src/defaults.rs)

### 🔥 性能优化

- [e2108c9](/-/commit/e2108c913ccfd09aa5133e1ffb0b49fb4dc092bc) 优化(widgets): 补偿 CJK 标题垂直偏移，对齐按钮视觉中线 (.dumate/inbox/image.png, .dumate/inbox/paste-image(1).png, src-egui/src/widgets.rs, src-egui/version.rc)

### 📦 构建系统

- [f5e2e22](/-/commit/f5e2e228aa3cc3b0dc604cbab1ce88d4eb7b73b9) 构建(package.json): 为egui构建脚本添加中文应用名称 (package.json)

## v0.1.38 (2026-10-29)
### 🚀 新增功能

- [cd146f8](/-/commit/cd146f83f270841d5fd06966c3d385ca46b678b7) 新增(src-egui): 支持编译期自定义启动器窗口标题 (README.md, scripts/__pycache__/pybuild.cpython-312.pyc, scripts/pybuild.py, src-core/build.rs, src-core/src/defaults.rs, src-egui/src/app.rs, src-egui/src/theme.rs, src-egui/src/widgets.rs)

## v0.1.37-beta.0 (2026-10-29)
### 🔧 工具变更

- [b5713c5](/-/commit/b5713c5fb961251abcd26ddd0e0d3fe93749507c) 工具(.dumate): 添加粘贴的图片文件 (.dumate/inbox/paste-image.png)

## v0.1.36 (2026-10-29)
### 📦 构建系统

- [35c0513](/-/commit/35c05136a6b973ddeb3769a1650cc6ec8078e62c) 构建(package.json): 更新版本号并修改构建脚本参数 (package.json, src-egui/version.rc)

## v0.1.35 (2026-10-29)
### 🚀 新增功能

- [892b135](/-/commit/892b13519e3c0ad33a5a8df9a3255c37c842a886) 新增(src-core): 新增构建期产品线参数烘焙功能 (.workbuddy/memory/2026-10-29.md, README.md, package.json, scripts/pybuild.py, src-core/build.rs, src-core/src/config.rs, src-core/src/defaults.rs, src-egui/version.rc)

## v0.1.34 (2026-10-29)
### 📦 构建系统

- [84098d8](/-/commit/84098d8e2ef233dc2693e22f9712405e20b860c5) 构建(package.json): 新增构建脚本用于构建egui的qsm模块 (package.json)

## v0.1.33 (2026-10-29)
### 📦 构建系统

- [352b905](/-/commit/352b905c47dd571c89eb87a8d541cb867d5eda9d) 构建(scripts): 统一构建脚本为 pybuild 并新增全量打包脚本 (.workbuddy/memory/2026-10-29.md, README.md, package.json, scripts/__pycache__/pybuild.cpython-314.pyc, scripts/build.py, scripts/pybuild.py, src-egui/version.rc, src-pkgtool/icon.rc)

## v0.1.32 (2026-10-29)
### 📝 文档更新

- [1083731](/-/commit/1083731ea985d75beb6a9b80f0b3769a78b65c98) 文档(launcherclient-guide): 添加 launcherclient 项目使用说明文档及配套资源 (.trae-html-share-packages/launcherclient-guide/launcherclient-guide.html.zip, launcherclient-guide/_shared/fonts/JetBrainsMono-Bold.ttf, launcherclient-guide/_shared/fonts/JetBrainsMono-Regular.ttf, launcherclient-guide/_shared/fonts/WorkSans-Bold.ttf, launcherclient-guide/_shared/fonts/WorkSans-Regular.ttf, launcherclient-guide/_shared/js/mermaid.min.js, launcherclient-guide/launcherclient-guide.html)

## v0.1.31 (2026-10-29)
### 🚀 新增功能

- [68073fe](/-/commit/68073fe33bcbe61df9667d3971749ef2d8ace805) 新增(src-pkgtool): 新增关于页面并统一版本号来源 (THIRD-PARTY-LICENSES.md, package.json, src-pkgtool/Cargo.toml, src-pkgtool/build.rs, src-pkgtool/src/main.rs)

## v0.1.30 (2026-10-29)
### 🚀 新增功能

- [e35e125](/-/commit/e35e1252ebb3aed1a6d4991a05d899953aa039f5) 新增(pkgtool): 打包完成后同步版本号到当前应用 (src-pkgtool/src/main.rs)

## v0.1.29 (2026-10-29)
### 🚀 新增功能

- [889047a](/-/commit/889047a516d44230d874c37180edbed9aad3f071) 新增(src-egui): 添加升级日志展示组件 (src-egui/src/templates.rs)

## v0.1.28 (2026-10-29)
### 🚀 新增功能

- [86c03b8](/-/commit/86c03b8d2be60b16f8c611cb57e8ac5f2474bd39) 新增(launcherclient): 将 store_url 迁移至服务端 manifest 并补全打包工具配置 (.workbuddy/memory/2026-10-28.md)
- [239d550](/-/commit/239d5508a961a0d3180da5cd00a4e02cedd7dbe8) 新增(pkgtool): 打包工具支持配置并产出 force_update/min_version/store_url (src-pkgtool/src/main.rs)

### ♻️ 代码重构

- [444bd05](/-/commit/444bd0563bd18c6c2f91c230d339ea5c624b8a5a) 重构(src-core): store_url 迁至服务端 manifest，清除本地配置残留 (src-core/src/config.rs, src-core/src/elevation.rs, src-core/src/flow.rs, src-core/src/phase.rs, src-egui/src/app.rs, src-egui/src/templates.rs, update.json, version.json.example)

## v0.1.27-beta.1 (2026-10-28)
### 🚀 新增功能

- [2c36790](/-/commit/2c367906e341e99336627305d56e02ef26d63bc4) 新增(src-core): force_update 改为本地开关并默认 false，普通更新可跳过进应用 (src-core/src/flow.rs, update.json)

### ♻️ 代码重构

- [550e560](/-/commit/550e560190fb898ea41f3f7f99712b91af0bbc59) 重构(src-core): 将 force_update 逻辑统一移至服务端 manifest，移除本地开关 (.workbuddy/memory/2026-10-28.md, src-core/src/config.rs, src-core/src/elevation.rs, src-core/src/flow.rs, src-core/src/manifest.rs, update.json, version.json.example)

## v0.1.27-beta.0 (2026-10-28)
### 🚀 新增功能

- [48f34a5](/-/commit/48f34a5a347e3b8a74f755d59097ecb3a470a0b1) 新增(src-egui): 全链路落地强制更新功能 (.workbuddy/memory/2026-10-28.md, src-core/src/config.rs, src-core/src/elevation.rs, src-core/src/flow.rs, src-core/src/manifest.rs, src-core/src/phase.rs, src-egui/src/app.rs, src-egui/src/main.rs, src-egui/src/preview.rs, src-egui/src/templates.rs, src-egui/src/theme.rs, src-egui/src/widgets.rs, update.json, version.json.example)

## v0.1.26 (2026-10-28)
### 🚀 新增功能

- [f8cbcaf](/-/commit/f8cbcafb5bf593b7a47ff4456b9d2500a3f22840) 新增(src-core): 新增Windows提权机制，更新服务时主动触发UAC提权 (.workbuddy/memory/2026-10-28.md, src-core/src/cli.rs, src-core/src/elevation.rs, src-core/src/flow.rs, src-core/src/lib.rs)

## v0.1.25 (2026-10-27)
### 📝 文档更新

- [b842e84](/-/commit/b842e84af314f3bb29cb8733f2f1f115c9d9bbb0) 文档(src-pkgtool): 添加 pkgtool UI 原型预览页面和崩溃日志 (.trae-html-share-packages/pkgtool-ui-prototype/pkgtool-ui-prototype.html.zip, pkgtool-ui-prototype/pkgtool-ui-prototype.html, src-pkgtool/bt.log, src-pkgtool/crash.log, src-pkgtool/src/main.rs)

### 📦 构建系统

- [4cde2c1](/-/commit/4cde2c17deef9292083afa6480ca86a3298d4b88) 构建(src-egui): 更新版本号至0.1.24 (src-egui/version.rc)

## v0.1.24 (2026-10-27)
### 🚀 新增功能

- [0b7810e](/-/commit/0b7810ed3b9c07b910ea8eab43f0247144e48a4a) 新增(src-core): 新增F/G/H三套启动器界面模板并默认切换为F (package.json, src-core/src/config.rs, src-core/src/defaults.rs, src-core/src/flow.rs, src-core/src/phase.rs, src-core/src/shell.rs, src-egui/src/main.rs, src-egui/src/preview.rs, src-egui/src/templates.rs, src-egui/version.rc, update.json)
- [b30ca4c](/-/commit/b30ca4cbaf7365f020a590f640b806c54ceca561) 新增(src-core): 下载与解压校验失败自动重试 (CHANGELOG.md, README.md, src-core/src/config.rs, src-core/src/defaults.rs, src-core/src/flow.rs, src-core/src/phase.rs, src-egui/src/preview.rs, src-egui/src/templates.rs, src-egui/src/widgets.rs, update.json)
- [8dc62cf](/-/commit/8dc62cf32ad17b59284f4fcff34ad14212fb8906) 新增(egui): 下载地址全挂时可跳转官网首页 (README.md, src-core/src/config.rs, src-core/src/defaults.rs, src-core/src/download.rs, src-core/src/flow.rs, src-core/src/lib.rs, src-core/src/phase.rs, src-core/src/shell.rs, src-egui/src/app.rs, src-egui/src/preview.rs, src-egui/src/templates.rs, src-egui/src/widgets.rs, update.json)

### 🩹 修复 Bug

- [290218f](/-/commit/290218fd0fa9f408bacb7ec70615d80b62055735) 修复(egui): 检测中加载动画居中并放大 (src-egui/src/app.rs, src-egui/src/main.rs, src-egui/src/templates.rs)

### ♻️ 代码重构

- [3725561](/-/commit/372556189a2e87d15e42dfce1d07547b3cec6483) 重构(egui): 重构日志卡片为固定高度内部滚动，优化布局稳定性 (src-egui/src/templates.rs, src-egui/src/widgets.rs)

### 📝 其他

- [8489dc3](/-/commit/8489dc349785a6d63b0c91cb0521e12a800fce32) 合并来自: 合并来自 auto/retry-download-983f 的合并请求 #5
- [af8ceed](/-/commit/af8ceeda5c9563dad6fc27a9ccdb2ef9072418c4) 合并来自: 合并来自 auto/loading-center-1024 的合并请求 #4
- [0f03991](/-/commit/0f03991f9508b4b8c7c8a0512979265ec1a7f40c) 合并来自: 合并来自 auto/homepage-fallback-f216 的合并请求 #2

## 未发布
### 🚀 新增功能

- 下载 / 解压校验环节自动重试：默认 3 次（含首次），失败后 1s→2s→3s 递增等待（封顶 5s），重试前清理失败产物；新增配置项 `retry_times`
- 重试期间界面标题标出「第 N/M 次尝试」并保留失败原因，流程未终结不亮错误态
- 错误态补充重试说明行：写明失败环节与已试次数；下载环节重试用尽才出现「前往官网重新下载」入口（原「前往官网」）

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

