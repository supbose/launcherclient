//! 构建脚本：生成产品线编译期默认值覆盖常量。
//!
//! `scripts/pybuild.py` 把命令行构建参数转成 `QSLTD_*` 环境变量传给 cargo；
//! 本脚本将其写入 `$OUT_DIR/build_overrides.rs`（Option 常量），
//! `defaults.rs` / `config.rs` 优先采用覆盖值，未传参时回落内置 qsm 默认。
//! 环境变量变化会触发重新生成与重编译（rerun-if-env-changed）。

// 构建脚本没有调用方可接收错误：失败时唯一正确的动作就是立即 panic 让 cargo 报错停下，
// 所以这里对来自 [lints.clippy] 的 expect_used 开口子（业务代码仍然维持 warn/deny）。
#![allow(clippy::expect_used)]

use std::env;
use std::fmt::Write as _;
use std::path::Path;

/// (构建参数环境变量, 生成常量名, 是否 '|' 分隔列表, 是否数值)
const OVERRIDES: &[(&str, &str, bool, bool)] = &[
    ("QSLTD_APP_NAME", "BUILD_APP_NAME", false, false),
    ("QSLTD_NAME", "BUILD_NAME", false, false),
    ("QSLTD_MANIFEST_URLS", "BUILD_MANIFEST_URLS", true, false),
    ("QSLTD_HOMEPAGE", "BUILD_HOMEPAGE", false, false),
    (
        "QSLTD_LAUNCHER_TEMPLATE",
        "BUILD_LAUNCHER_TEMPLATE",
        false,
        false,
    ),
    ("QSLTD_RETRY_TIMES", "BUILD_RETRY_TIMES", false, true),
    ("QSLTD_MAX_VERSIONS", "BUILD_MAX_VERSIONS", false, true),
    ("QSLTD_SERVICES", "BUILD_SERVICES", true, false),
];

fn main() {
    for (var, _, _, _) in OVERRIDES {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-changed=build.rs");

    let mut src =
        String::from("// 由 build.rs 依据 QSLTD_* 环境变量生成：构建期默认值覆盖常量（勿手改）\n");
    for &(var, name, is_list, is_num) in OVERRIDES {
        let value = env::var(var)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let rendered = match (&value, is_list, is_num) {
            (None, false, false) => format!("pub const {name}: Option<&str> = None;"),
            (None, true, false) => format!("pub const {name}: Option<&[&str]> = None;"),
            (None, false, true) => format!("pub const {name}: Option<u32> = None;"),
            (Some(raw), false, false) => format!("pub const {name}: Option<&str> = Some({raw:?});"),
            (Some(raw), true, false) => {
                let items: Vec<String> = raw
                    .split('|')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("{s:?}"))
                    .collect();
                format!(
                    "pub const {name}: Option<&[&str]> = Some(&[{}]);",
                    items.join(", ")
                )
            }
            (Some(raw), false, true) => {
                let n: u32 = raw
                    .parse()
                    .unwrap_or_else(|e| panic!("环境变量 {var} 需要数值，实际为 {raw:?}: {e}"));
                format!("pub const {name}: Option<u32> = Some({n});")
            }
            (_, true, true) => unreachable!("覆盖参数不存在既列表又数值的项"),
        };
        writeln!(src, "{rendered}").expect("内存写入不应失败");
    }

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR 应由 cargo 提供");
    let path = Path::new(&out_dir).join("build_overrides.rs");
    std::fs::write(&path, src).unwrap_or_else(|e| panic!("写入 {} 失败: {e}", path.display()));
}
