




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

