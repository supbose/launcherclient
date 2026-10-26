//! 构建脚本：将 `version.rc`（exe 图标 + 文件版本信息）嵌入可执行文件。
//!
//! 参照 `SupboseService/QsLtdService` 的 `build.rs` / `version.rc` 模式：
//! 图标与文件信息（公司名、描述、版本、版权等）统一在 `version.rc` 管理，
//! 由 `embed_resource` 调用系统 `rc.exe` 编译嵌入；非 Windows 平台跳过。

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
