//! 构建脚本：将 `icon.rc`（exe 图标 + 文件版本信息）嵌入可执行文件。
//!
//! 参照 `src-egui/build.rs` 与 `SupboseService/QsLtdService` 的 `build.rs` / `version.rc`
//! 模式：图标与文件信息（公司名、描述、版本、版权等）统一在 `icon.rc` 管理，
//! 由 `embed_resource` 调用系统 `rc.exe` 编译嵌入；非 Windows 平台跳过。

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=icon.rc");
    println!("cargo:rerun-if-changed=../package.json");

    // 工具版本号统一取自工作区 package.json（与发布版本单一来源），
    // 注入编译期常量 PKG_VERSION 供「关于」页展示
    let pkg = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../package.json");
    let text = std::fs::read_to_string(&pkg).expect("读取 package.json 失败");
    let version = extract_json_string(&text, "version").expect("package.json 缺少 version 字段");
    println!("cargo:rustc-env=PKG_VERSION={version}");

    #[cfg(target_os = "windows")]
    {
        embed_resource::compile("icon.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("嵌入 exe 图标与版本信息失败");
    }
}

/// 从 JSON 文本中提取 `"key": "value"` 的字符串值（package.json 为扁平结构，够用）
fn extract_json_string(text: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let at = text.find(&pat)? + pat.len();
    let rest = &text[at..];
    let colon = rest.find(':')?;
    let after = &rest[colon + 1..];
    let open = after.find('"')?;
    let close = after[open + 1..].find('"')?;
    Some(after[open + 1..open + 1 + close].to_string())
}
