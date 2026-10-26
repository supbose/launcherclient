//! 构建脚本：将 `icon.rc`（exe 图标 + 文件版本信息）嵌入可执行文件。
//!
//! 参照 `src-egui/build.rs` 与 `SupboseService/QsLtdService` 的 `build.rs` / `version.rc`
//! 模式：图标与文件信息（公司名、描述、版本、版权等）统一在 `icon.rc` 管理，
//! 由 `embed_resource` 调用系统 `rc.exe` 编译嵌入；非 Windows 平台跳过。

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=icon.rc");

    #[cfg(target_os = "windows")]
    {
        embed_resource::compile("icon.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("嵌入 exe 图标与版本信息失败");
    }
}
