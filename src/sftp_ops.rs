//! SFTP 高层操作：目录浏览、流式上传/下载（带进度）。
//! Warp 没自己实现 SFTP（它 shell-out `sftp` 命令），所以本模块协议层用 russh-sftp，
//! UI 状态机参考 `warp/app/src/terminal/view/ssh_file_upload.rs` 的 FileUploadStatus。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileType, OpenFlags};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::ssh_session::SshHandle;

/// SFTP 建链超时。半死连接（对端无 FIN）时 channel_open 会无限挂起，
/// 套超时让其快速失败，避免文件区/编辑器无限 loading（中断重连体验）。
pub const SFTP_OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// 在已有 SSH handle 上开 SFTP subsystem channel（不需要持有 SshSession）。
/// 用于 UI 层：拿到主 PTY session 的 handle clone 后，自己开 SFTP。
pub async fn open_sftp_on_handle(handle: &SshHandle) -> Result<SftpSession, String> {
    let open = async {
        let channel = handle
            .channel_open_session()
            .await
            .map_err(|error| format!("SFTP channel open failed: {error}"))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|error| format!("SFTP subsystem request failed: {error}"))?;
        SftpSession::new(channel.into_stream())
            .await
            .map_err(|error| format!("SFTP handshake failed: {error}"))
    };
    tokio::time::timeout(SFTP_OPEN_TIMEOUT, open)
        .await
        .map_err(|_| "SFTP 连接超时（远端无响应，可能已断开）".to_string())?
}

/// 单条目元数据，给 UI 用。
#[derive(Clone, Debug)]
pub struct RemoteEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub permissions: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Dir,
    File,
    Symlink,
    Other,
}

impl EntryKind {
    fn from_file_type(ft: FileType) -> Self {
        match ft {
            FileType::Dir => EntryKind::Dir,
            FileType::File => EntryKind::File,
            FileType::Symlink => EntryKind::Symlink,
            _ => EntryKind::Other,
        }
    }
}

/// 单次传输的进度事件，通过 mpsc 推给 UI。
#[derive(Clone, Debug)]
pub enum TransferEvent {
    Started { total: Option<u64> },
    Progress { transferred: u64 },
    Completed { transferred: u64 },
    Failed { error: String },
}

/// 分块大小：32 KiB。SFTP 协议单包最大 32768 字节，再大会被服务端拒。
const CHUNK_SIZE: usize = 32 * 1024;

pub async fn list_dir(sftp: &SftpSession, path: &str) -> Result<Vec<RemoteEntry>, String> {
    let entries = sftp
        .read_dir(path)
        .await
        .map_err(|error| format!("read_dir({path}) failed: {error}"))?;

    let mut out = Vec::new();
    for entry in entries {
        let meta = entry.metadata();
        out.push(RemoteEntry {
            name: entry.file_name(),
            kind: EntryKind::from_file_type(meta.file_type()),
            size: meta.len(),
            modified: meta.modified().ok(),
            permissions: meta.permissions,
        });
    }
    Ok(out)
}

pub async fn canonicalize(sftp: &SftpSession, path: &str) -> Result<String, String> {
    sftp.canonicalize(path)
        .await
        .map_err(|error| format!("canonicalize({path}) failed: {error}"))
}

pub async fn create_dir(sftp: &SftpSession, path: &str) -> Result<(), String> {
    sftp.create_dir(path)
        .await
        .map_err(|error| format!("create_dir({path}) failed: {error}"))
}

/// mkdir -p 等价：path 已存在或父目录都存在时静默成功。
/// Warp 的 SFTP 走 shell `put -r` 由 sftp 客户端自己 mkdir；我们这里手工等价。
pub async fn ensure_remote_dir(sftp: &SftpSession, path: &str) -> Result<(), String> {
    if path.is_empty() || path == "/" {
        return Ok(());
    }
    if let Ok(meta) = sftp.metadata(path).await {
        if matches!(meta.file_type(), FileType::Dir) {
            return Ok(());
        }
        return Err(format!(
            "ensure_remote_dir: {path} exists and is not a directory"
        ));
    }
    // 父目录递归
    if let Some(idx) = path.trim_end_matches('/').rfind('/') {
        let parent = if idx == 0 { "/" } else { &path[..idx] };
        if parent != path {
            Box::pin(ensure_remote_dir(sftp, parent)).await?;
        }
    }
    match sftp.create_dir(path).await {
        Ok(()) => Ok(()),
        Err(_) => {
            // race / 已存在则当成功
            if let Ok(meta) = sftp.metadata(path).await {
                if matches!(meta.file_type(), FileType::Dir) {
                    return Ok(());
                }
            }
            Err(format!("create_dir({path}) failed"))
        }
    }
}

#[derive(Clone, Debug)]
pub struct RemoteFileEntry {
    pub rel: String,
    pub size: u64,
}

#[derive(Clone, Debug, Default)]
pub struct RemoteTree {
    pub dirs: Vec<String>,
    pub files: Vec<RemoteFileEntry>,
    pub total_bytes: u64,
}

/// 单段路径名校验：拒绝空 / "." / ".." / 含 '/' 或控制字符，防路径穿越。
pub fn is_safe_path_segment(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.chars().any(|c| c == '\0' || c.is_control())
}

/// 路径任一 '/' 分段是否为 ".."（防穿越）。
pub fn path_has_dotdot(path: &str) -> bool {
    path.split('/').any(|seg| seg == "..")
}

/// 广度优先递归远端目录；rel 为相对 root 的 POSIX 路径（不含前导斜杠）。
/// 跳过软链 / 特殊文件，避免环路。
pub async fn walk_remote_dir(sftp: &SftpSession, root: &str) -> Result<RemoteTree, String> {
    let mut tree = RemoteTree::default();
    let mut queue: Vec<String> = vec![String::new()];
    while let Some(rel) = queue.pop() {
        let abs = if rel.is_empty() {
            root.to_string()
        } else if root.ends_with('/') {
            format!("{root}{rel}")
        } else {
            format!("{root}/{rel}")
        };
        let entries = sftp
            .read_dir(&abs)
            .await
            .map_err(|error| format!("read_dir({abs}) failed: {error}"))?;
        for entry in entries {
            let name = entry.file_name();
            // 防御异常/恶意服务端返回带 '/' 或 ".." 的条目名（正常文件名不含 '/')
            if !is_safe_path_segment(&name) {
                continue;
            }
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let meta = entry.metadata();
            match meta.file_type() {
                FileType::Dir => {
                    tree.dirs.push(child_rel.clone());
                    queue.push(child_rel);
                }
                FileType::File => {
                    tree.total_bytes = tree.total_bytes.saturating_add(meta.len());
                    tree.files.push(RemoteFileEntry {
                        rel: child_rel,
                        size: meta.len(),
                    });
                }
                _ => {}
            }
        }
    }
    Ok(tree)
}

#[derive(Clone, Debug)]
pub struct LocalFileEntry {
    pub abs: PathBuf,
    pub rel: String,
    pub size: u64,
}

#[derive(Clone, Debug, Default)]
pub struct LocalTree {
    pub dirs: Vec<String>,
    pub files: Vec<LocalFileEntry>,
    pub total_bytes: u64,
}

/// 递归遍历本地目录；rel 用 POSIX 风格分隔符，方便拼远端路径。
pub async fn walk_local_dir(root: &Path) -> Result<LocalTree, String> {
    let mut tree = LocalTree::default();
    let mut queue: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    while let Some((abs, rel)) = queue.pop() {
        let mut rd = fs::read_dir(&abs)
            .await
            .map_err(|error| format!("read_dir({}) failed: {error}", abs.display()))?;
        while let Some(entry) = rd
            .next_entry()
            .await
            .map_err(|error| format!("read_dir next: {error}"))?
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let ft = match entry.file_type().await {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ft.is_dir() {
                tree.dirs.push(child_rel.clone());
                queue.push((entry.path(), child_rel));
            } else if ft.is_file() {
                let size = fs::metadata(entry.path())
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                tree.total_bytes = tree.total_bytes.saturating_add(size);
                tree.files.push(LocalFileEntry {
                    abs: entry.path(),
                    rel: child_rel,
                    size,
                });
            }
        }
    }
    Ok(tree)
}

pub async fn remove_file(sftp: &SftpSession, path: &str) -> Result<(), String> {
    sftp.remove_file(path)
        .await
        .map_err(|error| format!("remove_file({path}) failed: {error}"))
}

pub async fn remove_dir(sftp: &SftpSession, path: &str) -> Result<(), String> {
    sftp.remove_dir(path)
        .await
        .map_err(|error| format!("remove_dir({path}) failed: {error}"))
}

/// rm -rf 等价：先 walk 出所有文件/子目录，自底向上删。
/// Warp 在 sftp 客户端里走 `rm -r`，我们用协议手工等价。
pub async fn remove_dir_recursive(sftp: &SftpSession, path: &str) -> Result<(), String> {
    let tree = walk_remote_dir(sftp, path).await?;
    for file in &tree.files {
        let abs = format!("{}/{}", path.trim_end_matches('/'), file.rel);
        sftp.remove_file(&abs)
            .await
            .map_err(|error| format!("remove_file({abs}) failed: {error}"))?;
    }
    let mut dirs = tree.dirs.clone();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.matches('/').count()));
    for d in &dirs {
        let abs = format!("{}/{}", path.trim_end_matches('/'), d);
        sftp.remove_dir(&abs)
            .await
            .map_err(|error| format!("remove_dir({abs}) failed: {error}"))?;
    }
    sftp.remove_dir(path)
        .await
        .map_err(|error| format!("remove_dir({path}) failed: {error}"))
}

/// touch 等价：用 CREATE | TRUNCATE 打开然后立即关闭，得到 0 字节文件。
pub async fn create_empty_file(sftp: &SftpSession, path: &str) -> Result<(), String> {
    let mut f = sftp
        .open_with_flags(
            path,
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
        )
        .await
        .map_err(|error| format!("create file {path} failed: {error}"))?;
    f.shutdown()
        .await
        .map_err(|error| format!("close {path} failed: {error}"))
}

pub async fn rename(sftp: &SftpSession, from: &str, to: &str) -> Result<(), String> {
    sftp.rename(from, to)
        .await
        .map_err(|error| format!("rename({from} -> {to}) failed: {error}"))
}

/// 上传本地文件到远端，按块写并通过 `progress` 推送进度。
/// `progress` 用 try_send，UI 消费不过来时会丢失中间帧，最终的 Completed 仍会送达。
pub async fn put_file(
    sftp: &SftpSession,
    local: &Path,
    remote: &str,
    progress: mpsc::UnboundedSender<TransferEvent>,
) -> Result<(), String> {
    static NO_CANCEL: AtomicBool = AtomicBool::new(false);
    let total = fs::metadata(local).await.ok().map(|m| m.len());
    let _ = progress.send(TransferEvent::Started { total });
    let transferred = put_file_stream(sftp, local, remote, &progress, 0, &NO_CANCEL).await?;
    let _ = progress.send(TransferEvent::Completed { transferred });
    Ok(())
}

/// 仅做"上传一个文件"的字节级流转，不发 Started/Completed。
/// 用于递归上传时把多个文件累计到同一个 transfer 上：
/// `progress_base` 是该文件开始前已累计的字节数，进度推送的是 `base + 本文件已传`。
/// `cancel` 在 chunk 边界检查，true 时立刻返回 Err("cancelled")。
pub async fn put_file_stream(
    sftp: &SftpSession,
    local: &Path,
    remote: &str,
    progress: &mpsc::UnboundedSender<TransferEvent>,
    progress_base: u64,
    cancel: &AtomicBool,
) -> Result<u64, String> {
    let mut local_file = fs::File::open(local)
        .await
        .map_err(|error| format!("open local {} failed: {error}", local.display()))?;

    let mut remote_file = sftp
        .open_with_flags(
            remote,
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
        )
        .await
        .map_err(|error| format!("open remote {remote} failed: {error}"))?;

    let result = async {
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut transferred: u64 = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".to_string());
            }
            let n = local_file
                .read(&mut buf)
                .await
                .map_err(|error| format!("local read failed: {error}"))?;
            if n == 0 {
                break;
            }
            remote_file
                .write_all(&buf[..n])
                .await
                .map_err(|error| format!("remote write failed: {error}"))?;
            transferred += n as u64;
            let _ = progress.send(TransferEvent::Progress {
                transferred: progress_base + transferred,
            });
        }
        remote_file
            .flush()
            .await
            .map_err(|error| format!("remote flush failed: {error}"))?;
        remote_file
            .shutdown()
            .await
            .map_err(|error| format!("remote close failed: {error}"))?;
        Ok(transferred)
    }
    .await;

    if result.is_err() {
        // 失败/取消时清理半截远端文件，避免残留损坏文件冒充完整上传
        drop(remote_file);
        let _ = sftp.remove_file(remote).await;
    }
    result
}

/// 下载远端文件到本地。
pub async fn get_file(
    sftp: &SftpSession,
    remote: &str,
    local: &Path,
    progress: mpsc::UnboundedSender<TransferEvent>,
) -> Result<(), String> {
    static NO_CANCEL: AtomicBool = AtomicBool::new(false);
    let total = sftp.metadata(remote).await.ok().map(|m| m.len());
    let _ = progress.send(TransferEvent::Started { total });
    let transferred = get_file_stream(sftp, remote, local, &progress, 0, &NO_CANCEL).await?;
    let _ = progress.send(TransferEvent::Completed { transferred });
    Ok(())
}

/// 见 [`put_file_stream`]：递归下载用的字节级流转。
pub async fn get_file_stream(
    sftp: &SftpSession,
    remote: &str,
    local: &Path,
    progress: &mpsc::UnboundedSender<TransferEvent>,
    progress_base: u64,
    cancel: &AtomicBool,
) -> Result<u64, String> {
    let mut remote_file = sftp
        .open(remote)
        .await
        .map_err(|error| format!("open remote {remote} failed: {error}"))?;

    if let Some(parent) = local.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent).await;
        }
    }
    let mut local_file = fs::File::create(local)
        .await
        .map_err(|error| format!("create local {} failed: {error}", local.display()))?;

    let result = async {
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut transferred: u64 = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".to_string());
            }
            let n = remote_file
                .read(&mut buf)
                .await
                .map_err(|error| format!("remote read failed: {error}"))?;
            if n == 0 {
                break;
            }
            local_file
                .write_all(&buf[..n])
                .await
                .map_err(|error| format!("local write failed: {error}"))?;
            transferred += n as u64;
            let _ = progress.send(TransferEvent::Progress {
                transferred: progress_base + transferred,
            });
        }
        local_file
            .flush()
            .await
            .map_err(|error| format!("local flush failed: {error}"))?;
        Ok(transferred)
    }
    .await;

    if result.is_err() {
        // 失败/取消时清理半截本地文件，避免残留损坏文件
        drop(local_file);
        let _ = fs::remove_file(local).await;
    }
    result
}

/// 把远端文件读进内存，供内置编辑器（ADR 0005）。最多读到 `max_bytes + 1` 字节即停：
/// 返回 Vec 长度 > max_bytes 说明超大，调用方据此回退（不再续读，限制内存）。
/// 同时在**同一 open 句柄**上 fstat 取 (size, modified) 作冲突检测基线，避免 read→stat 的 TOCTOU（review E）。
pub async fn read_file_to_memory(
    sftp: &SftpSession,
    path: &str,
    max_bytes: usize,
) -> Result<(Vec<u8>, Option<(u64, Option<SystemTime>)>), String> {
    let mut remote_file = sftp
        .open(path)
        .await
        .map_err(|error| format!("open remote {path} failed: {error}"))?;
    let mut out = Vec::new();
    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        let n = remote_file
            .read(&mut buf)
            .await
            .map_err(|error| format!("remote read failed: {error}"))?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() > max_bytes {
            out.truncate(max_bytes + 1);
            break;
        }
    }
    let meta = remote_file
        .metadata()
        .await
        .ok()
        .map(|m| (m.len(), m.modified().ok()));
    Ok((out, meta))
}

/// 把内存内容覆盖写回远端文件，供内置编辑器保存（ADR 0005）。
/// 直接 TRUNCATE 覆盖，无备份（同本地 0003 的 fs::write）；失败不删原文件，编辑器侧仍持脏内容可重试。
pub async fn write_file_from_memory(
    sftp: &SftpSession,
    path: &str,
    content: &[u8],
) -> Result<(), String> {
    let mut remote_file = sftp
        .open_with_flags(
            path,
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
        )
        .await
        .map_err(|error| format!("open remote {path} failed: {error}"))?;
    for chunk in content.chunks(CHUNK_SIZE) {
        remote_file
            .write_all(chunk)
            .await
            .map_err(|error| format!("remote write failed: {error}"))?;
    }
    remote_file
        .flush()
        .await
        .map_err(|error| format!("remote flush failed: {error}"))?;
    remote_file
        .shutdown()
        .await
        .map_err(|error| format!("remote close failed: {error}"))
}

/// 取单个远端文件元数据，供保存前冲突检测（ADR 0005）。
pub async fn stat_file(sftp: &SftpSession, path: &str) -> Result<RemoteEntry, String> {
    let meta = sftp
        .metadata(path)
        .await
        .map_err(|error| format!("stat({path}) failed: {error}"))?;
    let name = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string();
    Ok(RemoteEntry {
        name,
        kind: EntryKind::from_file_type(meta.file_type()),
        size: meta.len(),
        modified: meta.modified().ok(),
        permissions: meta.permissions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_kind_classifies_file_types() {
        assert_eq!(EntryKind::from_file_type(FileType::Dir), EntryKind::Dir);
        assert_eq!(EntryKind::from_file_type(FileType::File), EntryKind::File);
        assert_eq!(
            EntryKind::from_file_type(FileType::Symlink),
            EntryKind::Symlink
        );
    }
}
