//! 远端文件 → Finder（ADR 0016 第 4 步）：剪贴板里的 file URL 被读取时，
//! 把远端这次复制的文件整份下载到临时目录，下完再把各顶层条目的路径交给辅助进程。

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ironrdp_cliprdr::pdu::{
    ClipboardFileAttributes, FileContentsFlags, FileContentsRequest, FileContentsResponse,
    FileDescriptor,
};

use super::super::{ClipboardTransfer, RdpEvent};
use super::wire::{Data, Format};
use super::{helper, trace};

/// 每次 RANGE 请求的字节数。
const CHUNK: u32 = 1 << 20;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
/// 1601-01-01 到 1970-01-01 的秒数。
const FILETIME_UNIX_OFFSET: u64 = 11_644_473_600;

/// stream ID 进程内唯一：取消的下载可能还有应答在路上，不能被新下载认领。
static NEXT_STREAM: AtomicU32 = AtomicU32::new(1);

struct Entry {
    index: i32,
    path: PathBuf,
    is_dir: bool,
    size: Option<u64>,
    modified: Option<u64>,
}

struct Running {
    entry: usize,
    file: File,
    offset: u64,
    size: Option<u64>,
    stream: u32,
}

enum State {
    Idle,
    Running(Running),
    Done,
    Failed,
}

pub(super) struct Download {
    session: u64,
    dir: PathBuf,
    lock: Option<u32>,
    entries: Vec<Entry>,
    roots: Vec<PathBuf>,
    state: State,
    /// 等路径的辅助进程请求：(请求 ID, 顶层序号)。
    waiters: Vec<(u64, u32)>,
    done: u64,
    total: u64,
    events: async_channel::Sender<RdpEvent>,
    last_progress: Option<Instant>,
}

impl Download {
    /// 远端路径逐段校验后落到本次的临时目录下；没有可用条目时返回 None。
    pub(super) fn new(
        session: u64,
        generation: u64,
        files: &[FileDescriptor],
        lock: Option<u32>,
        events: async_channel::Sender<RdpEvent>,
    ) -> Option<Self> {
        let dir = cache_root().join(format!("{}-{session}-{generation}", std::process::id()));
        let mut entries = Vec::new();
        let mut roots = Vec::new();
        for (index, file) in files.iter().enumerate() {
            let parts: Vec<&str> = file
                .relative_path
                .iter()
                .flat_map(|p| p.split('\\'))
                .chain([file.name.as_str()])
                .collect();
            if !parts.iter().all(|p| valid_name(p)) {
                trace(|| format!("skip remote file {:?}", file.name));
                continue;
            }
            let root = dir.join(parts[0]);
            if !roots.contains(&root) {
                roots.push(root);
            }
            let is_dir = file
                .attributes
                .is_some_and(|a| a.contains(ClipboardFileAttributes::DIRECTORY));
            entries.push(Entry {
                index: i32::try_from(index).ok()?,
                path: parts.iter().fold(dir.clone(), |p, part| p.join(part)),
                is_dir,
                size: if is_dir { Some(0) } else { file.file_size },
                modified: file.last_write_time,
            });
        }
        if roots.is_empty() {
            return None;
        }
        let total = entries.iter().filter_map(|e| e.size).sum();
        Some(Self {
            session,
            dir,
            lock,
            entries,
            roots,
            state: State::Idle,
            waiters: Vec::new(),
            done: 0,
            total,
            events,
            last_progress: None,
        })
    }

    pub(super) fn roots(&self) -> u32 {
        u32::try_from(self.roots.len()).unwrap_or(u32::MAX)
    }

    /// 辅助进程要第 root 个顶层条目的路径。第一次请求时开始下载，返回要发给远端的请求。
    pub(super) fn request(&mut self, id: u64, root: u32) -> Option<FileContentsRequest> {
        match self.state {
            State::Done => {
                helper::reply(id, self.path_reply(root));
                None
            }
            State::Failed => {
                helper::reply(id, None);
                None
            }
            State::Running(_) => {
                self.waiters.push((id, root));
                None
            }
            State::Idle => {
                self.waiters.push((id, root));
                self.start()
            }
        }
    }

    pub(super) fn on_response(
        &mut self,
        response: &FileContentsResponse<'_>,
    ) -> Option<FileContentsRequest> {
        let State::Running(running) = &mut self.state else {
            return None;
        };
        if running.stream != response.stream_id() {
            return None;
        }
        if response.is_error() {
            trace(|| format!("download entry {} failed", running.entry));
            return self.fail();
        }
        match running.size {
            None => match response.data_as_size() {
                Ok(size) => {
                    running.size = Some(size);
                    self.total += size;
                }
                Err(_) => return self.fail(),
            },
            Some(size) => {
                let data = response.data();
                let take = data
                    .len()
                    .min(usize::try_from(size - running.offset).unwrap_or(usize::MAX));
                if take == 0 || running.file.write_all(&data[..take]).is_err() {
                    return self.fail();
                }
                running.offset += take as u64;
                self.done += take as u64;
            }
        }
        let State::Running(running) = &mut self.state else {
            return None;
        };
        if running.size.is_some_and(|size| running.offset >= size) {
            let entry = running.entry;
            set_modified(&running.file, self.entries[entry].modified);
            return self.advance(entry + 1);
        }
        let next = self.next_request();
        self.progress(false);
        next
    }

    /// 用户取消或会话结束：等待中的读取都拿到空结果，删掉下了一半的文件。
    pub(super) fn cancel(&mut self) {
        if matches!(self.state, State::Idle | State::Running(_)) {
            self.fail();
        }
    }

    fn start(&mut self) -> Option<FileContentsRequest> {
        remove_session_dirs(self.session, Some(&self.dir));
        let _ = fs::remove_dir_all(&self.dir);
        if fs::create_dir_all(&self.dir).is_err() {
            return self.fail();
        }
        trace(|| {
            format!(
                "download {} entries ({} bytes) to {}",
                self.entries.len(),
                self.total,
                self.dir.display()
            )
        });
        self.progress(true);
        self.advance(0)
    }

    /// 从第 from 个条目往后建目录、开文件，遇到要下载的文件就返回它的第一个请求。
    fn advance(&mut self, from: usize) -> Option<FileContentsRequest> {
        for i in from..self.entries.len() {
            let entry = &self.entries[i];
            if entry.is_dir {
                if fs::create_dir_all(&entry.path).is_err() {
                    return self.fail();
                }
                continue;
            }
            let created = entry
                .path
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| File::create(&entry.path));
            let Ok(file) = created else {
                return self.fail();
            };
            if entry.size == Some(0) {
                set_modified(&file, entry.modified);
                continue;
            }
            self.state = State::Running(Running {
                entry: i,
                file,
                offset: 0,
                size: entry.size,
                stream: NEXT_STREAM.fetch_add(1, Ordering::Relaxed),
            });
            self.progress(false);
            return self.next_request();
        }
        self.state = State::Done;
        trace(|| format!("download done: {} bytes", self.done));
        for (id, root) in std::mem::take(&mut self.waiters) {
            helper::reply(id, self.path_reply(root));
        }
        self.send_progress(None);
        None
    }

    fn next_request(&mut self) -> Option<FileContentsRequest> {
        let State::Running(running) = &self.state else {
            return None;
        };
        let index = self.entries[running.entry].index;
        let request = match running.size {
            None => FileContentsRequest {
                stream_id: running.stream,
                index,
                flags: FileContentsFlags::SIZE,
                position: 0,
                requested_size: 8,
                data_id: self.lock,
            },
            Some(size) => FileContentsRequest {
                stream_id: running.stream,
                index,
                flags: FileContentsFlags::RANGE,
                position: running.offset,
                requested_size: u32::try_from((size - running.offset).min(u64::from(CHUNK)))
                    .unwrap_or(CHUNK),
                data_id: self.lock,
            },
        };
        Some(request)
    }

    fn fail(&mut self) -> Option<FileContentsRequest> {
        self.state = State::Failed;
        for (id, _) in std::mem::take(&mut self.waiters) {
            helper::reply(id, None);
        }
        let _ = fs::remove_dir_all(&self.dir);
        self.send_progress(None);
        None
    }

    fn path_reply(&self, root: u32) -> Option<Data> {
        let path = self.roots.get(root as usize)?.to_str()?;
        Some(Data {
            format: Format::Path,
            bytes: path.as_bytes().into(),
        })
    }

    fn progress(&mut self, force: bool) {
        let now = Instant::now();
        if !force
            && self
                .last_progress
                .is_some_and(|last| now.duration_since(last) < PROGRESS_INTERVAL)
        {
            return;
        }
        self.last_progress = Some(now);
        self.send_progress(Some(ClipboardTransfer {
            files: self.entries.iter().filter(|e| !e.is_dir).count(),
            done: self.done,
            total: self.total,
        }));
    }

    fn send_progress(&self, progress: Option<ClipboardTransfer>) {
        let _ = self.events.try_send(RdpEvent::ClipboardTransfer(progress));
    }
}

impl Drop for Download {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Windows 那边已校验过路径，这里再挡一遍会逃出临时目录的名字。
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\0'])
}

/// 文件下完补上远端的修改时间。
fn set_modified(file: &File, filetime: Option<u64>) {
    if let Some(time) = filetime.and_then(filetime_to_system) {
        let _ = file.set_modified(time);
    }
}

fn filetime_to_system(filetime: u64) -> Option<SystemTime> {
    let secs = (filetime / 10_000_000).checked_sub(FILETIME_UNIX_OFFSET)?;
    let nanos = (filetime % 10_000_000) * 100;
    UNIX_EPOCH.checked_add(Duration::new(secs, u32::try_from(nanos).ok()?))
}

fn cache_root() -> PathBuf {
    static CLEAN_STALE: Once = Once::new();
    let root = std::env::temp_dir().join("NexShell-rdp-clipboard");
    CLEAN_STALE.call_once(|| remove_stale_dirs(&root));
    root
}

/// 删掉本会话以前下载的目录（keep 除外）。
pub(super) fn remove_session_dirs(session: u64, keep: Option<&Path>) {
    let prefix = format!("{}-{session}-", std::process::id());
    let Ok(entries) = fs::read_dir(cache_root()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with(&prefix) && Some(path.as_path()) != keep
        {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// 进程异常退出时留下的目录：目录名开头的 pid 已不在运行就删掉。
fn remove_stale_dirs(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let pid = name
            .to_str()
            .and_then(|n| n.split('-').next())
            .and_then(|p| p.parse::<u32>().ok());
        if pid.is_some_and(|pid| pid != std::process::id() && !process_alive(pid)) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: 信号 0 只检查进程是否存在，不发送信号。
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    alive || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_converts_back_to_unix_time() {
        assert_eq!(
            filetime_to_system(116_444_736_000_000_000),
            Some(UNIX_EPOCH)
        );
        assert_eq!(
            filetime_to_system(116_444_736_000_000_000 + 15_000_000),
            UNIX_EPOCH.checked_add(Duration::from_millis(1500))
        );
        assert_eq!(filetime_to_system(0), None);
    }

    #[test]
    fn names_that_escape_the_cache_are_rejected() {
        for bad in ["", ".", "..", "a/b", "a\0b"] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        assert!(valid_name("报告 v2.docx"));
    }
}
