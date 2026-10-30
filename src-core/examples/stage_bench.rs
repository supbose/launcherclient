//! 更新流程各阶段耗时基准（本地回环 HTTP + 临时目录）
//!
//! 目的：在决定 P2（免合并直读分卷 / zlib-rs）之前，先量出「下载、逐卷校验、
//! 合并、整包校验、解压、清理」各自占多少时间，避免凭感觉优化。
//!
//! 用法：
//! ```text
//! cargo run --release -p qsltd-updater-core --example stage_bench -- --mb 512
//! cargo run --release -p qsltd-updater-core --example stage_bench -- --mb 512 --stages extract
//! ```
//!
//! **请用 `--release`**：debug 构建的 SHA256 / deflate 慢一个数量级，数字没有参考价值。
//!
//! - 数据目录：`target/stage_bench/<mb>mb/`，分卷与 package.zip 会复用，
//!   换压缩后端后只跑 `--stages extract` 即可对比解压吞吐。
//! - 下载走 127.0.0.1 回环，测到的是「本机磁盘 + 协议开销」的上限，
//!   真实 CDN 速率请以线上为准。
//! - 分卷内容是随机字节（模拟不可压缩包），package.zip 里 90% 是可压缩
//!   文本、10% 随机数据：解压阶段会真正跑 inflate，而不是全程 memcpy。
//!
//! 输出为 `stage / bytes / secs / MB/s`，并按 2.3 GB 线性外推。
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    clippy::print_stderr
)]

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use qsltd_updater_core::{download, install};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// 目标包大小（2.3 GB），用于把实测吞吐外推到真实包
const TARGET_PACKAGE_BYTES: u64 = 2_300_000_000;

fn main() {
    if let Err(e) = run() {
        eprintln!("[bench] 失败: {e}");
        std::process::exit(1);
    }
}

#[derive(Debug, Clone)]
struct Args {
    mb: u64,
    parts: usize,
    concurrency: usize,
    stages: Stages,
}

#[derive(Debug, Clone, Default)]
struct Stages {
    download_single: bool,
    download_parts: bool,
    merge: bool,
    copy_only: bool,
    verify: bool,
    extract: bool,
    cleanup: bool,
}

impl Stages {
    fn all() -> Self {
        Self {
            download_single: true,
            download_parts: true,
            merge: true,
            copy_only: true,
            verify: true,
            extract: true,
            cleanup: true,
        }
    }

    fn parse(spec: &str) -> Result<Self, String> {
        if spec.trim() == "all" {
            return Ok(Self::all());
        }
        let mut s = Self::default();
        for item in spec.split(',') {
            match item.trim() {
                "" => {}
                "download-single" => s.download_single = true,
                "download-parts" => s.download_parts = true,
                "merge" => s.merge = true,
                "copy" => s.copy_only = true,
                "verify" => s.verify = true,
                "extract" => s.extract = true,
                "cleanup" => s.cleanup = true,
                other => {
                    return Err(format!(
                        "未知阶段 {other}（可用：all / download-single / download-parts / merge / copy / verify / extract / cleanup）"
                    ))
                }
            }
        }
        if !s.download_single
            && !s.download_parts
            && !s.merge
            && !s.copy_only
            && !s.verify
            && !s.extract
            && !s.cleanup
        {
            return Err("没有选中任何阶段".to_string());
        }
        Ok(s)
    }
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut args = Self {
            mb: 512,
            parts: 8,
            concurrency: 3,
            stages: Stages::all(),
        };
        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            let mut value = || {
                it.next()
                    .ok_or_else(|| format!("{flag} 缺少取值"))
                    .and_then(|v| {
                        v.parse::<u64>()
                            .map_err(|e| format!("{flag} 取值非法: {e}"))
                    })
            };
            match flag.as_str() {
                "--mb" => args.mb = value()?.max(1),
                "--parts" => args.parts = (value()? as usize).clamp(1, 64),
                "--concurrency" => args.concurrency = (value()? as usize).clamp(1, 8),
                "--stages" => {
                    let spec = it.next().ok_or_else(|| "--stages 缺少取值".to_string())?;
                    args.stages = Stages::parse(&spec)?;
                }
                "--help" | "-h" => {
                    println!(
                        "用法: stage_bench [--mb 512] [--parts 8] [--concurrency 3] [--stages all|extract|download-parts,merge,...]"
                    );
                    std::process::exit(0);
                }
                other => return Err(format!("未知参数: {other}")),
            }
        }
        Ok(args)
    }
}

#[derive(Debug, Clone)]
struct SourceInfo {
    part_paths: Vec<PathBuf>,
    part_sha: Vec<String>,
    overall_sha: String,
    total: u64,
    uncompressed: u64,
    package: PathBuf,
}

fn run() -> Result<(), String> {
    let args = Args::parse()?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("取不到 workspace 根目录")?;
    let work = workspace
        .join("target")
        .join("stage_bench")
        .join(format!("{}mb", args.mb));
    fs::create_dir_all(&work).map_err(|e| format!("创建基准目录失败 {}: {e}", work.display()))?;

    let total = args.mb * 1024 * 1024;
    println!(
        "[bench] 参数: package={} MiB parts={} concurrency={} stages={:?}",
        args.mb, args.parts, args.concurrency, args.stages
    );
    if cfg!(debug_assertions) {
        println!(
            "[bench] 警告: 当前是 debug 构建，SHA256/deflate 会慢一个数量级，请改用 --release"
        );
    }
    println!("[bench] 工作目录: {}", work.display());

    let setup_start = Instant::now();
    let source = ensure_source(&work, total, args.parts)?;
    let package_sha = download::sha256_of_file(&source.package)?;
    println!(
        "[bench] 数据准备完成（{} MiB 分卷 + package {} MiB）耗时 {:.2}s",
        source.total / 1024 / 1024,
        fs::metadata(&source.package)
            .map(|m| m.len() / 1024 / 1024)
            .unwrap_or(0),
        setup_start.elapsed().as_secs_f64()
    );

    let port = start_server(&source, &work)?;
    println!("[bench] 回环服务: http://127.0.0.1:{port}\n");

    let mut results: Vec<StageResult> = Vec::new();
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("创建 tokio 运行时失败: {e}"))?;

    if args.stages.download_single {
        let dest = work.join("dl_single").join("update_bench.zip");
        remove_dir_all_quietly(&work.join("dl_single"));
        fs::create_dir_all(work.join("dl_single")).map_err(|e| format!("创建下载目录失败: {e}"))?;
        let url = format!("http://127.0.0.1:{port}/package.zip");
        let size = fs::metadata(&source.package)
            .map_err(|e| format!("读取 package 大小失败: {e}"))?
            .len();
        let started = Instant::now();
        let (_path, sha) = rt
            .block_on(download::download_file(&url, &dest, |_| {}))
            .map_err(|e| format!("下载 package 失败: {e}"))?;
        let elapsed = started.elapsed();
        if sha != package_sha {
            return Err("单包下载后的 SHA256 与源文件不一致".to_string());
        }
        results.push(StageResult::new("download_single", size, elapsed));
    }

    let mut downloaded_parts: Option<Vec<PathBuf>> = None;
    if args.stages.download_parts {
        let dir = work.join("dl_parts");
        remove_dir_all_quietly(&dir);
        let urls: Vec<String> = (1..=source.part_paths.len())
            .map(|i| format!("http://127.0.0.1:{port}/part_{i:03}"))
            .collect();
        let started = Instant::now();
        let parts = rt
            .block_on(download::download_parts(
                &urls,
                Some(&source.part_sha),
                &dir,
                Some(source.total),
                args.concurrency,
                |_, _, _| {},
            ))
            .map_err(|e| format!("分卷下载失败: {e}"))?;
        let elapsed = started.elapsed();
        downloaded_parts = Some(parts.part_paths.clone());
        results.push(StageResult::new("download_parts", source.total, elapsed));
    }

    let merged = work.join("merged.zip");
    if args.stages.merge {
        let parts = match downloaded_parts.clone() {
            Some(p) => p,
            None => {
                // 只跑 merge 时，把源分卷当作刚下载好的结果
                source.part_paths.clone()
            }
        };
        remove_file_quietly(&merged);
        let started = Instant::now();
        let sha = rt
            .block_on(download::merge_parts(
                &parts,
                &merged,
                &Some(source.overall_sha.clone()),
                |_, _| {},
            ))
            .map_err(|e| format!("分卷合并失败: {e}"))?;
        let elapsed = started.elapsed();
        if sha != source.overall_sha {
            return Err("合并产物 SHA256 与源数据不一致".to_string());
        }
        results.push(StageResult::new("merge(read+write)", source.total, elapsed));
    }

    if args.stages.copy_only {
        // 只做「读分卷 + 写合并包」，不算 SHA256：用来量化合并里哈希占多少
        let dest = work.join("copy_only.zip");
        remove_file_quietly(&dest);
        let started = Instant::now();
        copy_parts(&source.part_paths, &dest)?;
        let elapsed = started.elapsed();
        remove_file_quietly(&dest);
        results.push(StageResult::new("copy(read+write)", source.total, elapsed));
    }

    if args.stages.verify {
        // 模拟「不使用流式哈希时，整读一遍校验」的成本，作为对照
        if !merged.exists() {
            return Err("verify 需要先跑 merge（或用 --stages all）".to_string());
        }
        let started = Instant::now();
        let _ = download::sha256_of_file(&merged)?;
        let elapsed = started.elapsed();
        results.push(StageResult::new("verify_read", source.total, elapsed));
    }

    if args.stages.extract {
        let install_dir = work.join("install").join("app_bench");
        for run_no in 1..=2 {
            remove_dir_all_quietly(&work.join("install"));
            let started = Instant::now();
            rt.block_on(install::install(
                &source.package,
                &install_dir,
                Arc::new(|_: &str, _: Option<f64>| {}),
            ))
            .map_err(|e| format!("解压安装失败: {e}"))?;
            let elapsed = started.elapsed();
            results.push(StageResult::new(
                &format!("extract_run{run_no}"),
                source.uncompressed,
                elapsed,
            ));
        }
    }

    if args.stages.cleanup {
        // 清理的是本轮产生的副本（下载包 + 合并包 + 解压产物），不含源数据
        let produced = source.total + source.total + source.uncompressed;
        let started = Instant::now();
        remove_dir_all_quietly(&work.join("dl_single"));
        remove_dir_all_quietly(&work.join("dl_parts"));
        remove_dir_all_quietly(&work.join("install"));
        remove_file_quietly(&merged);
        let elapsed = started.elapsed();
        results.push(StageResult::new("cleanup", produced, elapsed));
    }

    println!();
    print_table(&results);
    Ok(())
}

/// 单阶段结果
struct StageResult {
    name: String,
    bytes: u64,
    seconds: f64,
}

impl StageResult {
    fn new(name: &str, bytes: u64, elapsed: Duration) -> Self {
        Self {
            name: name.to_string(),
            bytes,
            seconds: elapsed.as_secs_f64(),
        }
    }

    fn mb_per_s(&self) -> f64 {
        if self.seconds <= 0.0 {
            return 0.0;
        }
        self.bytes as f64 / 1024.0 / 1024.0 / self.seconds
    }
}

fn print_table(results: &[StageResult]) {
    println!(
        "[bench] {:<20} {:>12} {:>9} {:>10} {:>14}",
        "stage", "bytes", "secs", "MB/s", "2.3GB 外推"
    );
    for r in results {
        let estimate = if r.mb_per_s() > 0.0 {
            format!(
                "{:.1}s",
                TARGET_PACKAGE_BYTES as f64 / 1024.0 / 1024.0 / r.mb_per_s()
            )
        } else {
            "-".to_string()
        };
        println!(
            "[bench] {:<20} {:>12} {:>9.3} {:>10.1} {:>14}",
            r.name,
            r.bytes,
            r.seconds,
            r.mb_per_s(),
            estimate
        );
    }
    println!(
        "\n[bench] 说明：merge 的 MB/s 按「处理字节数」计，实际 I/O 为读一遍 + 写一遍；\n\
         [bench] extract 按解压后字节计；download 走 127.0.0.1，是协议+磁盘的上限，不代表 CDN 速率。"
    );
}

// ---------------------------------------------------------------- 数据准备

fn ensure_source(work: &Path, total: u64, parts: usize) -> Result<SourceInfo, String> {
    let parts_dir = work.join("src_parts");
    let package = work.join("package.zip");
    let meta = work.join("source.meta");
    if let Some(info) = load_source(&meta, &parts_dir, &package, total, parts) {
        return Ok(info);
    }

    println!("[bench] 生成源数据（首次运行会稍慢，之后复用）...");
    remove_dir_all_quietly(&parts_dir);
    remove_file_quietly(&package);
    remove_file_quietly(&meta);
    fs::create_dir_all(&parts_dir)
        .map_err(|e| format!("创建分卷目录失败 {}: {e}", parts_dir.display()))?;

    let (part_paths, part_sha, overall_sha) = gen_parts(&parts_dir, total, parts)?;
    let readme_bytes = 1024 * 1024;
    let uncompressed = total + readme_bytes;
    build_package(&part_paths, &package, total, readme_bytes)?;
    save_meta(&meta, total, parts, uncompressed, &overall_sha, &part_sha)?;

    Ok(SourceInfo {
        part_paths,
        part_sha,
        overall_sha,
        total,
        uncompressed,
        package,
    })
}

fn load_source(
    meta: &Path,
    parts_dir: &Path,
    package: &Path,
    total: u64,
    parts: usize,
) -> Option<SourceInfo> {
    let text = fs::read_to_string(meta).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "bench-meta v2" {
        return None;
    }
    let header = lines.next()?;
    let expected = format!("mb_bytes={total} parts={parts}");
    if !header.starts_with(&expected) {
        return None;
    }
    let uncompressed: u64 = header
        .split_whitespace()
        .find_map(|kv| kv.strip_prefix("uncompressed="))?
        .parse()
        .ok()?;
    let overall_sha = lines.next()?.strip_prefix("overall=")?.to_string();
    let mut part_sha = Vec::new();
    for line in lines {
        part_sha.push(line.strip_prefix("part=")?.to_string());
    }
    if part_sha.len() != parts || !package.is_file() {
        return None;
    }
    let mut part_paths = Vec::with_capacity(parts);
    let mut sum = 0u64;
    for i in 1..=parts {
        let path = parts_dir.join(format!("part_{i:03}"));
        let len = fs::metadata(&path).ok()?.len();
        sum += len;
        part_paths.push(path);
    }
    if sum != total {
        return None;
    }
    Some(SourceInfo {
        part_paths,
        part_sha,
        overall_sha,
        total,
        uncompressed,
        package: package.to_path_buf(),
    })
}

fn save_meta(
    meta: &Path,
    total: u64,
    parts: usize,
    uncompressed: u64,
    overall_sha: &str,
    part_sha: &[String],
) -> Result<(), String> {
    let mut text = String::new();
    text.push_str("bench-meta v2\n");
    text.push_str(&format!(
        "mb_bytes={total} parts={parts} uncompressed={uncompressed}\n"
    ));
    text.push_str(&format!("overall={overall_sha}\n"));
    for sha in part_sha {
        text.push_str(&format!("part={sha}\n"));
    }
    fs::write(meta, text).map_err(|e| format!("写入元数据失败 {}: {e}", meta.display()))
}

const CHUNK: usize = 1024 * 1024;

/// 简单 xorshift64，避免为基准引入 rand 依赖
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

fn fill_random(buf: &mut [u8], rng: &mut Rng) {
    let mut i = 0;
    while i + 8 <= buf.len() {
        buf[i..i + 8].copy_from_slice(&rng.next_u64().to_le_bytes());
        i += 8;
    }
    if i < buf.len() {
        let tail = buf.len() - i;
        let bytes = rng.next_u64().to_le_bytes();
        buf[i..].copy_from_slice(&bytes[..tail]);
    }
}

fn gen_parts(
    dir: &Path,
    total: u64,
    parts: usize,
) -> Result<(Vec<PathBuf>, Vec<String>, String), String> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut overall = Sha256::new();
    let mut paths = Vec::with_capacity(parts);
    let mut hashes = Vec::with_capacity(parts);
    let base = total / parts as u64;
    let remainder = total % parts as u64;
    let mut buf = vec![0u8; CHUNK];

    for i in 0..parts {
        let size = base + if i + 1 == parts { remainder } else { 0 };
        let path = dir.join(format!("part_{:03}", i + 1));
        let mut file =
            File::create(&path).map_err(|e| format!("创建分卷失败 {}: {e}", path.display()))?;
        let mut part_hasher = Sha256::new();
        let mut left = size;
        while left > 0 {
            let n = left.min(CHUNK as u64) as usize;
            fill_random(&mut buf[..n], &mut rng);
            file.write_all(&buf[..n])
                .map_err(|e| format!("写分卷失败 {}: {e}", path.display()))?;
            part_hasher.update(&buf[..n]);
            overall.update(&buf[..n]);
            left -= n as u64;
        }
        file.flush()
            .map_err(|e| format!("刷新分卷失败 {}: {e}", path.display()))?;
        hashes.push(hex::encode(part_hasher.finalize()));
        paths.push(path);
    }
    Ok((paths, hashes, hex::encode(overall.finalize())))
}

/// 构造一个可解压的 zip：90% 可压缩文本 + 10% 随机数据 + 1 MiB readme
fn build_package(
    parts: &[PathBuf],
    package: &Path,
    total: u64,
    readme_bytes: u64,
) -> Result<(), String> {
    let file = File::create(package)
        .map_err(|e| format!("创建 package 失败 {}: {e}", package.display()))?;
    let mut writer = ZipWriter::new(file);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    let text_bytes = total.saturating_mul(9) / 10;
    let random_bytes = total - text_bytes;

    writer
        .start_file("app.exe", deflated)
        .map_err(|e| format!("写入 zip 条目 app.exe 失败: {e}"))?;
    write_text(&mut writer, text_bytes, 1)?;

    writer
        .start_file("resources/pack.dat", stored)
        .map_err(|e| format!("写入 zip 条目 pack.dat 失败: {e}"))?;
    // 用分卷的字节填充随机段，避免再生成一份随机数据
    copy_from_parts(&mut writer, parts, random_bytes)?;

    writer
        .start_file("readme.txt", deflated)
        .map_err(|e| format!("写入 zip 条目 readme.txt 失败: {e}"))?;
    write_text(&mut writer, readme_bytes, 7)?;

    writer.finish().map_err(|e| format!("收尾 zip 失败: {e}"))?;
    Ok(())
}

/// 生成 32 符号表内的伪随机文本：约 5 bit/字节，deflate 压缩率 ~60%，
/// 字面量为主、匹配较少，比重复短语更接近真实二进制的 inflate 负载。
fn write_text<W: Write>(writer: &mut W, mut bytes: u64, seed: u64) -> Result<(), String> {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz012345";
    let mut rng = Rng(seed ^ 0xA5A5_5A5A_1234_5678);
    let mut buf = vec![0u8; CHUNK];
    while bytes > 0 {
        let n = bytes.min(CHUNK as u64) as usize;
        let mut word = rng.next_u64();
        for chunk in buf[..n].chunks_mut(8) {
            for (j, b) in chunk.iter_mut().enumerate() {
                *b = ALPHABET[(word >> (j * 8)) as usize & 31];
            }
            word = rng.next_u64();
        }
        writer
            .write_all(&buf[..n])
            .map_err(|e| format!("写文本失败: {e}"))?;
        bytes -= n as u64;
    }
    Ok(())
}

fn copy_from_parts<W: Write>(
    writer: &mut W,
    parts: &[PathBuf],
    mut bytes: u64,
) -> Result<(), String> {
    let mut buf = vec![0u8; CHUNK];
    for part in parts {
        if bytes == 0 {
            break;
        }
        let mut file =
            File::open(part).map_err(|e| format!("打开 {} 失败: {e}", part.display()))?;
        let mut left = bytes;
        while left > 0 {
            let n = left.min(CHUNK as u64) as usize;
            let read = file
                .read(&mut buf[..n])
                .map_err(|e| format!("读取 {} 失败: {e}", part.display()))?;
            if read == 0 {
                break;
            }
            writer
                .write_all(&buf[..read])
                .map_err(|e| format!("写入随机段失败: {e}"))?;
            left -= read as u64;
            bytes -= read as u64;
        }
    }
    if bytes > 0 {
        return Err("源分卷不足以填充随机段".to_string());
    }
    Ok(())
}

/// 纯拼接分卷（读 + 写，不算 SHA256），用于对照 merge 的哈希开销
fn copy_parts(parts: &[PathBuf], dest: &Path) -> Result<(), String> {
    use std::io::BufWriter;
    const BUF: usize = 256 * 1024;
    let out = File::create(dest).map_err(|e| format!("创建 {} 失败: {e}", dest.display()))?;
    let mut out = BufWriter::with_capacity(BUF, out);
    let mut buf = vec![0u8; BUF];
    for part in parts {
        let mut file =
            File::open(part).map_err(|e| format!("打开 {} 失败: {e}", part.display()))?;
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| format!("读取 {} 失败: {e}", part.display()))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])
                .map_err(|e| format!("写入 {} 失败: {e}", dest.display()))?;
        }
    }
    out.flush()
        .map_err(|e| format!("刷新 {} 失败: {e}", dest.display()))
}

// ---------------------------------------------------------------- 回环 HTTP

fn start_server(source: &SourceInfo, _work: &Path) -> Result<u16, String> {
    let mut files: HashMap<String, PathBuf> = HashMap::new();
    for (i, path) in source.part_paths.iter().enumerate() {
        files.insert(format!("/part_{:03}", i + 1), path.clone());
    }
    files.insert("/package.zip".to_string(), source.package.clone());
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("绑定回环端口失败: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("读取回环端口失败: {e}"))?
        .port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let files = files.clone();
            thread::spawn(move || {
                let _ = serve(stream, &files);
            });
        }
    });
    Ok(port)
}

fn serve(mut stream: TcpStream, files: &HashMap<String, PathBuf>) -> io::Result<()> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.windows(4).any(|w| w == b"\r\n\r\n") || raw.len() > 16 * 1024 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let path = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");

    let Some(file) = files.get(path) else {
        stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )?;
        return Ok(());
    };
    let len = fs::metadata(file)?.len();
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(header.as_bytes())?;
    let mut f = File::open(file)?;
    io::copy(&mut f, &mut stream)?;
    Ok(())
}

// ---------------------------------------------------------------- 小工具

fn remove_dir_all_quietly(path: &Path) {
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
}

fn remove_file_quietly(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}
