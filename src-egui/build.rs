//! 构建脚本：将 assets/icon.ico 作为 Windows 资源嵌入可执行文件（exe 图标）
//!
//! 调用系统 rc.exe（embed-resource 自动定位 Windows SDK），非 Windows 平台跳过。

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let icon = manifest_dir.join("assets").join("icon.ico");
    // rc.exe 的相对路径依赖工作目录，这里写绝对路径（正斜杠避免转义问题）
    let icon_path = icon.display().to_string().replace('\\', "/");
    let out_dir = std::env::var("OUT_DIR").expect("缺少 OUT_DIR");
    let rc_path = Path::new(&out_dir).join("app.rc");
    std::fs::write(&rc_path, format!("1 ICON \"{icon_path}\"\n")).expect("写入图标资源脚本失败");

    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("嵌入 exe 图标失败");
}
