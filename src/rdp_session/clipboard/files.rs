//! Mac 文件 → 远端（ADR 0016 第 3 步）：展开 Finder 复制的文件和目录，按远端请求读大小和字节段。

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ironrdp_cliprdr::pdu::{ClipboardFileAttributes, FileDescriptor, MAX_FILE_COUNT};

/// cFileName 共 260 个 UTF-16 单元，含结尾 NUL。
const MAX_WIRE_NAME: usize = 259;
/// 单次 RANGE 请求的读取上限，免得按远端给的长度分配过大的内存。
const MAX_RANGE: u32 = 32 << 20;
/// 1601-01-01 到 1970-01-01 的秒数。
const FILETIME_UNIX_OFFSET: u64 = 11_644_473_600;

#[derive(Debug)]
pub(super) struct LocalFile {
    path: PathBuf,
    is_dir: bool,
}

pub(super) type FileList = Arc<[LocalFile]>;

/// 一次文件复制：本地路径与发给远端的描述符按序号一一对应。
#[derive(Debug)]
pub(in crate::rdp_session) struct FileOffer {
    pub(super) files: FileList,
    pub(super) descriptors: Vec<FileDescriptor>,
}

/// 展开顶层路径，目录在前、内容随后。没有可发的条目或超过协议上限时返回 None。
pub(super) fn collect(roots: &[PathBuf]) -> Option<FileOffer> {
    let mut collector = Collector::default();
    for root in roots {
        let Some(name) = root.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        collector.add(root, name, None)?;
    }
    if collector.files.is_empty() {
        return None;
    }
    Some(FileOffer {
        files: collector.files.into(),
        descriptors: collector.descriptors,
    })
}

#[derive(Default)]
struct Collector {
    files: Vec<LocalFile>,
    descriptors: Vec<FileDescriptor>,
    /// 已展开过的目录（规范化路径），挡住符号链接成环。
    visited: HashSet<PathBuf>,
}

impl Collector {
    /// 只在条目数超过协议上限时返回 None；读不到的条目直接跳过。
    fn add(&mut self, path: &Path, name: &str, parent: Option<&str>) -> Option<()> {
        // Finder 的视图配置，带到 Windows 上没用。
        if name == ".DS_Store" {
            return Some(());
        }
        let Ok(meta) = fs::metadata(path) else {
            return Some(());
        };
        let name = windows_name(name);
        let wire = match parent {
            Some(parent) => format!("{parent}\\{name}"),
            None => name.clone(),
        };
        if wire.encode_utf16().count() > MAX_WIRE_NAME {
            return Some(());
        }
        let is_dir = meta.is_dir();
        if !is_dir && !meta.is_file() {
            return Some(());
        }
        if is_dir {
            let Ok(real) = fs::canonicalize(path) else {
                return Some(());
            };
            if !self.visited.insert(real) {
                return Some(());
            }
        }
        if self.files.len() >= MAX_FILE_COUNT {
            return None;
        }
        let mut descriptor = FileDescriptor::new(name).with_attributes(if is_dir {
            ClipboardFileAttributes::DIRECTORY
        } else {
            ClipboardFileAttributes::NORMAL
        });
        if let Some(time) = meta.modified().ok().and_then(filetime) {
            descriptor = descriptor.with_last_write_time(time);
        }
        if !is_dir {
            descriptor = descriptor.with_file_size(meta.len());
        }
        if let Some(parent) = parent {
            descriptor = descriptor.with_relative_path(parent);
        }
        self.files.push(LocalFile {
            path: path.to_owned(),
            is_dir,
        });
        self.descriptors.push(descriptor);
        if is_dir {
            let Ok(entries) = fs::read_dir(path) else {
                return Some(());
            };
            let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            children.sort();
            for child in &children {
                let Some(child_name) = child.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                self.add(child, child_name, Some(&wire))?;
            }
        }
        Some(())
    }
}

/// Windows 文件名里不能出现的字符换成 `_`；`\` 和 `/` 还会被当成路径分隔。
fn windows_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

fn filetime(time: SystemTime) -> Option<u64> {
    let since = time.duration_since(UNIX_EPOCH).ok()?;
    (since.as_secs() + FILETIME_UNIX_OFFSET)
        .checked_mul(10_000_000)?
        .checked_add(u64::from(since.subsec_nanos() / 100))
}

/// 应答时现取大小，文件在复制后改过也以当前为准；目录为 0。
pub(super) fn size(file: &LocalFile) -> Option<u64> {
    if file.is_dir {
        return Some(0);
    }
    fs::metadata(&file.path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
}

/// 应答线程专用：连续的 RANGE 请求多半读同一个文件，留着上次打开的句柄。
#[derive(Default)]
pub(super) struct RangeReader {
    open: Option<(PathBuf, File)>,
}

impl RangeReader {
    pub(super) fn read(&mut self, file: &LocalFile, position: u64, len: u32) -> Option<Vec<u8>> {
        if file.is_dir {
            return Some(Vec::new());
        }
        if self
            .open
            .as_ref()
            .is_none_or(|(path, _)| *path != file.path)
        {
            self.open = None;
            self.open = Some((file.path.clone(), File::open(&file.path).ok()?));
        }
        let (_, handle) = self.open.as_mut()?;
        if handle.seek(SeekFrom::Start(position)).is_err() {
            self.open = None;
            return None;
        }
        let mut buf = vec![0; len.min(MAX_RANGE) as usize];
        let mut filled = 0;
        while filled < buf.len() {
            match handle.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => {
                    self.open = None;
                    return None;
                }
            }
        }
        buf.truncate(filled);
        Some(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(d: &FileDescriptor) -> String {
        match &d.relative_path {
            Some(p) => format!("{p}\\{}", d.name),
            None => d.name.clone(),
        }
    }

    #[test]
    fn collect_expands_directories_in_order() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("相册");
        fs::create_dir_all(dir.join("子目录")).unwrap();
        fs::write(dir.join("b.txt"), b"bb").unwrap();
        fs::write(dir.join("子目录").join("a.txt"), b"a").unwrap();
        fs::write(dir.join(".DS_Store"), b"x").unwrap();
        let single = root.path().join("单个.bin");
        fs::write(&single, [0u8; 5]).unwrap();

        let offer = collect(&[dir.clone(), single]).unwrap();
        let names: Vec<_> = offer.descriptors.iter().map(wire).collect();
        assert_eq!(
            names,
            [
                "相册",
                "相册\\b.txt",
                "相册\\子目录",
                "相册\\子目录\\a.txt",
                "单个.bin"
            ]
        );
        assert_eq!(offer.files.len(), offer.descriptors.len());
        let d = &offer.descriptors;
        assert_eq!(d[0].attributes, Some(ClipboardFileAttributes::DIRECTORY));
        assert_eq!(d[0].file_size, None);
        assert_eq!(d[1].file_size, Some(2));
        assert_eq!(d[4].file_size, Some(5));
        assert!(d[4].last_write_time.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn collect_stops_at_symlink_loops() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("loop");
        fs::create_dir(&dir).unwrap();
        std::os::unix::fs::symlink(&dir, dir.join("self")).unwrap();
        let offer = collect(&[dir]).unwrap();
        let names: Vec<_> = offer.descriptors.iter().map(wire).collect();
        assert_eq!(names, ["loop"]);
    }

    #[test]
    fn windows_name_replaces_reserved_characters() {
        assert_eq!(windows_name("a:b\\c/d?.txt"), "a_b_c_d_.txt");
        assert_eq!(windows_name("正常 名字.txt"), "正常 名字.txt");
    }

    #[test]
    fn range_reader_reads_slices_and_eof() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("data");
        fs::write(&path, b"0123456789").unwrap();
        let file = LocalFile {
            path,
            is_dir: false,
        };
        let mut reader = RangeReader::default();
        assert_eq!(reader.read(&file, 2, 3).unwrap(), b"234");
        assert_eq!(reader.read(&file, 8, 10).unwrap(), b"89");
        assert_eq!(reader.read(&file, 20, 4).unwrap(), b"");
        assert_eq!(size(&file), Some(10));
    }

    #[test]
    fn filetime_counts_from_1601() {
        assert_eq!(filetime(UNIX_EPOCH), Some(116_444_736_000_000_000));
    }
}
