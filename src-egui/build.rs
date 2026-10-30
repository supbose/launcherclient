//! 构建脚本：将 `version.rc`（exe 图标 + 文件版本信息）嵌入可执行文件。
//!
//! 字体不在这里处理：`assets/font/` 下的 ttf 由 `src/app.rs` 用 `include_bytes!`
//! 直接编进 exe，无需复制到 `target/<profile>/fonts/`。

// 构建脚本没有调用方可接收错误：资源编译失败时唯一正确的动作就是 panic 让 cargo 停下，
// 所以放行来自 [lints.clippy] 的 expect_used（业务代码仍维持 warn/deny）。
#![allow(clippy::expect_used)]

fn main() {
    println!("cargo:rerun-if-changed=version.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    #[cfg(target_os = "windows")]
    {
        embed_resource::compile("version.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("嵌入 exe 资源（图标 + 版本信息）失败");
    }
}
