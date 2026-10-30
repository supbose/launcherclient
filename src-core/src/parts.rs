//! 分卷拼接读取器：把按序排列的分卷当成一个连续文件读取
//!
//! 分卷已逐卷校验时，安装可以直接从分卷流式解压，省掉「合并成完整 zip」
//! 这一步的整包写盘（以及等量磁盘峰值）。zip 解析需要 `Read + Seek`，
//! 这里把全局偏移映射成「第几卷 + 卷内偏移」：顺序解压时读取基本都在
//! 卷内前进，跨卷只在边界发生一次。
//!
//! 只读、不缓存整卷：更新包动辄 GB 级，按块读取让内存占用与文件大小脱钩。

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

/// 一卷分卷：文件句柄 + 它在拼接结果中的起始偏移与长度
struct Part {
    file: File,
    /// 拼接结果中的起始偏移
    start: u64,
    /// 本卷长度（字节）
    len: u64,
}

/// 分卷拼接读取器（`Read + Seek`）
pub(crate) struct PartsReader {
    parts: Vec<Part>,
    /// 所有分卷长度之和（虚拟文件大小）
    total: u64,
    /// 当前虚拟偏移
    pos: u64,
    /// 当前卷文件内的游标；与目标偏移一致时省掉一次 seek
    file_pos: u64,
    /// `pos` 所在的卷（只向前推进，定位/Seek 时重置）
    index: usize,
}

impl PartsReader {
    /// 打开分卷列表并建立偏移表（顺序即拼接顺序）
    ///
    /// 打不开/读不到长度都直接失败：缺卷时任何「跳过继续」都会装出
    /// 缺文件的半套版本，不如当场报错。
    pub(crate) fn open(paths: &[PathBuf]) -> Result<Self, String> {
        if paths.is_empty() {
            return Err("分卷列表为空".to_string());
        }
        let mut parts = Vec::with_capacity(paths.len());
        let mut start: u64 = 0;
        for path in paths {
            let file =
                File::open(path).map_err(|e| format!("打开分卷失败 {}: {}", path.display(), e))?;
            let len = file
                .metadata()
                .map_err(|e| format!("读取分卷大小失败 {}: {}", path.display(), e))?
                .len();
            parts.push(Part { file, start, len });
            start = start.saturating_add(len);
        }
        Ok(Self {
            parts,
            total: start,
            pos: 0,
            file_pos: u64::MAX,
            index: 0,
        })
    }

    /// 虚拟文件大小
    #[cfg(test)]
    fn len(&self) -> u64 {
        self.total
    }

    /// 把 `index` 推进到 `pos` 所在的非空卷
    ///
    /// 零长度分卷直接跳过（它不占虚拟文件的任何字节）。索引只会向前走；
    /// 需要回退时由 [`Seek`] 重置为 0。
    fn locate(&mut self) {
        let before = self.index;
        while self.index < self.parts.len() {
            let part = &self.parts[self.index];
            let end = part.start.saturating_add(part.len);
            if part.len > 0 && self.pos >= part.start && self.pos < end {
                break;
            }
            if self.pos < part.start {
                break;
            }
            self.index += 1;
        }
        if self.index != before {
            // 换了文件，卷内游标失效
            self.file_pos = u64::MAX;
        }
    }
}

impl Read for PartsReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.pos >= self.total {
            return Ok(0);
        }
        self.locate();
        let Some(part) = self.parts.get_mut(self.index) else {
            return Ok(0);
        };
        let offset = self.pos - part.start;
        let available = (part.len - offset).min(buf.len() as u64) as usize;
        if available == 0 {
            return Ok(0);
        }
        if self.file_pos != offset {
            part.file.seek(SeekFrom::Start(offset))?;
        }
        let n = part.file.read(&mut buf[..available])?;
        if n == 0 {
            // 元数据说有这么多字节却读不到：文件被截断/外部改动
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "分卷 {} 提前结束（偏移 {} / {}）",
                    self.index + 1,
                    offset,
                    part.len
                ),
            ));
        }
        self.pos += n as u64;
        self.file_pos = offset + n as u64;
        Ok(n)
    }
}

impl Seek for PartsReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::Current(n) => self.pos as i128 + n as i128,
            SeekFrom::End(n) => self.total as i128 + n as i128,
        };
        if target < 0 || target > self.total as i128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "分卷读取器定位越界",
            ));
        }
        self.pos = target as u64;
        // 允许定位到末尾（EOF）；卷索引与卷内游标全部失效，按需重建
        self.index = 0;
        self.file_pos = u64::MAX;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::path::Path;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "qsltd_parts_test_{}_{}_{}",
            tag,
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 把 `chunks` 依次写成 part_001、part_002…，返回路径列表
    fn write_parts(dir: &Path, chunks: &[&[u8]]) -> Vec<PathBuf> {
        chunks
            .iter()
            .enumerate()
            .map(|(i, data)| {
                let path = dir.join(format!("part_{:03}", i + 1));
                let mut f = File::create(&path).expect("建分卷");
                f.write_all(data).expect("写分卷");
                path
            })
            .collect()
    }

    fn read_all(reader: &mut PartsReader) -> Vec<u8> {
        let mut out = Vec::new();
        io::copy(reader, &mut out).expect("读全部分卷");
        out
    }

    #[test]
    fn reads_across_part_boundaries() {
        let dir = temp_dir("all");
        let paths = write_parts(&dir, &[b"AAA", b"BBBB", b"CC"]);
        let mut reader = PartsReader::open(&paths).expect("打开分卷");
        assert_eq!(reader.len(), 9);
        assert_eq!(read_all(&mut reader), b"AAABBBBCC");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_in_small_chunks_across_boundaries() {
        // 逐字节读也要能跨卷：模拟解压时的小块读取
        let dir = temp_dir("chunks");
        let paths = write_parts(&dir, &[b"12", b"345", b"6"]);
        let mut reader = PartsReader::open(&paths).expect("打开分卷");
        let mut out = Vec::new();
        let mut buf = [0u8; 1];
        loop {
            let n = reader.read(&mut buf).expect("读分卷");
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        assert_eq!(out, b"123456");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seek_start_current_and_end() {
        let dir = temp_dir("seek");
        let paths = write_parts(&dir, &[b"abc", b"defgh"]);
        let mut reader = PartsReader::open(&paths).expect("打开分卷");

        assert_eq!(reader.seek(SeekFrom::Start(4)).expect("绝对定位"), 4);
        let mut two = [0u8; 2];
        reader.read_exact(&mut two).expect("读两字节");
        assert_eq!(&two, b"ef");

        assert_eq!(reader.seek(SeekFrom::Current(-3)).expect("相对定位"), 3);
        let mut three = [0u8; 3];
        reader.read_exact(&mut three).expect("读三字节");
        assert_eq!(&three, b"def");

        // 从末尾回退两字节：落在第二卷内
        assert_eq!(reader.seek(SeekFrom::End(-2)).expect("末尾定位"), 6);
        let mut tail = [0u8; 2];
        reader.read_exact(&mut tail).expect("读末尾");
        assert_eq!(&tail, b"gh");

        // 定位到末尾再读应为 EOF
        reader.seek(SeekFrom::End(0)).expect("定位末尾");
        let mut one = [0u8; 1];
        assert_eq!(reader.read(&mut one).expect("末尾读"), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seek_out_of_bounds_is_rejected() {
        let dir = temp_dir("bounds");
        let paths = write_parts(&dir, &[b"abc"]);
        let mut reader = PartsReader::open(&paths).expect("打开分卷");
        assert!(reader.seek(SeekFrom::Start(4)).is_err(), "超出末尾应报错");
        assert!(reader.seek(SeekFrom::Current(-10)).is_err(), "负偏移应报错");
        // 报错后游标不应被破坏：仍能从头读
        reader.seek(SeekFrom::Start(0)).expect("回到开头");
        assert_eq!(read_all(&mut reader), b"abc");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_part_fails_to_open() {
        let dir = temp_dir("missing");
        let paths = vec![dir.join("part_001"), dir.join("part_002")];
        std::fs::write(&paths[0], b"abc").expect("写第一卷");
        let err = PartsReader::open(&paths).err().expect("缺卷应失败");
        assert!(
            err.contains("打开分卷失败"),
            "错误应指出分卷打开失败: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_part_list_fails_to_open() {
        let err = PartsReader::open(&[]).err().expect("空列表应失败");
        assert!(err.contains("分卷列表为空"), "错误应说明空列表: {err}");
    }

    #[test]
    fn zero_length_part_is_transparent() {
        // 空卷不占用虚拟文件字节，读取不中断
        let dir = temp_dir("zero");
        let paths = write_parts(&dir, &[b"ab", b"", b"cd"]);
        let mut reader = PartsReader::open(&paths).expect("打开分卷");
        assert_eq!(reader.len(), 4);
        assert_eq!(read_all(&mut reader), b"abcd");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seek_back_to_start_after_reading_to_end() {
        // 索引只前进，回退必须重置；验证 seek(Start) 后还能正确读
        let dir = temp_dir("rewind");
        let paths = write_parts(&dir, &[b"ab", b"cd", b"ef"]);
        let mut reader = PartsReader::open(&paths).expect("打开分卷");
        assert_eq!(read_all(&mut reader), b"abcdef");
        reader.seek(SeekFrom::Start(3)).expect("回退");
        let mut buf = [0u8; 3];
        reader.read_exact(&mut buf).expect("读回退位置");
        assert_eq!(&buf, b"def");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
