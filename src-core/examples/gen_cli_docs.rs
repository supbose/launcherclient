//! 生成 / 校验 CLI 文档（README §4.2 与使用手册 4.1 的用法行 + 参数表）。
//!
//! 用法（仓库根目录）：
//!   cargo run -p qsltd-updater-core --example gen_cli_docs -- --write   # 按 cli.rs 重写两处文档
//!   cargo run -p qsltd-updater-core --example gen_cli_docs -- --check   # 只校验，不一致退出码 1（CI 硬门禁）
//!
//! 单一来源是 `qsltd_updater_core::cli::CLI_SPECS`：解析逻辑与文档同住一个文件，
//! 新增参数时改那里 + `parse_from`，再跑一次 `--write`，不会出现「代码改了文档忘了」。

#![allow(clippy::print_stdout)] // CLI 工具，产出就是打印结果

use std::fs;
use std::path::{Path, PathBuf};

use qsltd_updater_core::cli;

/// 生成内容在两份文档里的定界注释（Markdown 与 HTML 都合法）
const BEGIN: &str = "<!-- cli-doc:begin -->";
const END: &str = "<!-- cli-doc:end -->";

/// 文档里被生成的两段：用法行（代码块）+ 参数表
const README_PATH: &str = "README.md";
const GUIDE_PATH: &str = "launcherclient-guide/launcherclient-guide.html";

fn main() {
    let write = std::env::args().any(|a| a == "--write");
    let check = std::env::args().any(|a| a == "--check");
    if !write && !check {
        eprintln!("用法: gen_cli_docs --check | --write");
        std::process::exit(2);
    }

    let root = repo_root();
    let readme_block = format!(
        "```\n{}\n```\n\n{}",
        cli::usage_text(),
        cli::table_markdown()
    );
    let guide_block = format!(
        "<pre><code>{}</code></pre>\n    {}",
        html_escape(&cli::usage_text()),
        cli::table_html()
    );

    let jobs = [(README_PATH, readme_block), (GUIDE_PATH, guide_block)];
    let mut drifted = Vec::new();

    for (rel, block) in &jobs {
        let path = root.join(rel);
        let original = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[cli-doc] 读取 {} 失败: {}", rel, e);
                std::process::exit(1);
            }
        };
        let updated = match splice(&original, block) {
            Some(s) => s,
            None => {
                eprintln!("[cli-doc] {} 缺少定界注释 {} / {}", rel, BEGIN, END);
                std::process::exit(1);
            }
        };
        if updated == original {
            println!("[cli-doc] {} 已是最新", rel);
            continue;
        }
        drifted.push(*rel);
        if write {
            if let Err(e) = fs::write(&path, &updated) {
                eprintln!("[cli-doc] 写入 {} 失败: {}", rel, e);
                std::process::exit(1);
            }
            println!("[cli-doc] 已重写 {}", rel);
        }
    }

    if check && !drifted.is_empty() {
        eprintln!(
            "[cli-doc] 以下文档与 src-core/src/cli.rs 的 CLI_SPECS 不一致：{}\n\
             [cli-doc] 请运行 `cargo run -p qsltd-updater-core --example gen_cli_docs -- --write` 后提交",
            drifted.join(", ")
        );
        std::process::exit(1);
    }
}

/// 仓库根目录：example 的 CARGO_MANIFEST_DIR 是 src-core
fn repo_root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().map(Path::to_path_buf).unwrap_or_else(|| {
        eprintln!("[cli-doc] 定位仓库根目录失败");
        std::process::exit(1);
    })
}

/// 把 BEGIN..END 之间的内容替换成 `block`（两端注释保留）
fn splice(text: &str, block: &str) -> Option<String> {
    let i = text.find(BEGIN)? + BEGIN.len();
    let j = i + text[i..].find(END)?;
    let mut out = String::with_capacity(text.len() + block.len());
    out.push_str(&text[..i]);
    out.push('\n');
    out.push_str(block.trim_end());
    out.push('\n');
    out.push_str(&text[j..]);
    Some(out)
}

/// 代码块内的 HTML 转义（`<名称>` 之类）
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
