//! 构建脚本：
//! 1. 将 `version.rc`（exe 图标 + 文件版本信息）嵌入可执行文件；
//! 2. 把 `assets/font/`（华为 HarmonyOS Sans SC 三种字重 + 许可）复制到
//!    `target/<profile>/fonts/`，让开发期 `cargo run` 也能走内置字体探测路径。

// 构建脚本没有调用方可接收错误：资源编译/复制失败时唯一正确的动作就是 panic 让 cargo 停下，
// 所以放行来自 [lints.clippy] 的 expect_used（业务代码仍维持 warn/deny）。
#![allow(clippy::expect_used)]

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=version.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    copy_fonts();

    #[cfg(target_os = "windows")]
    {
        embed_resource::compile("version.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("嵌入 exe 资源（图标 + 版本信息）失败");
    }
}

/// 把字体内置资源复制到 `target/<profile>/fonts/`
///
/// `OUT_DIR` 形如 `target/<profile>/build/<crate>-<hash>/out`，向上第 3 级即
/// 可执行文件所在目录；运行期 `theme::install_fonts` 优先探测 `<exe 同级>/fonts/`。
fn copy_fonts() {
    println!("cargo:rerun-if-changed=assets/font");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR 未设置");
    let exe_dir = Path::new(&out_dir)
        .ancestors()
        .nth(3)
        .expect("无法从 OUT_DIR 定位 target/<profile> 目录");
    let dest = exe_dir.join("fonts");
    std::fs::create_dir_all(&dest).expect("创建 target/<profile>/fonts 失败");

    let entries = std::fs::read_dir("assets/font").expect("读取 assets/font 失败");
    for entry in entries {
        let entry = entry.expect("读取 assets/font 条目失败");
        let src = entry.path();
        if src.is_file() {
            std::fs::copy(&src, dest.join(entry.file_name())).expect("复制字体资源失败");
        }
    }
}
