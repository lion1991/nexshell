//! 内置编辑器的远程文件一次性 SFTP 读 / 写（ADR 0005）。
//! 复用已认证 SSH handle，每个操作起短命 OS 线程 + current-thread tokio runtime
//! （russh-sftp 需 tokio reactor），与文件面板 worker 解耦。结果经 async_channel 回 UI，
//! 由 ctx.spawn_stream_local 消费。套路同 host_overview::spawn_remote_exec。

use std::thread;
use std::time::{Duration, SystemTime};

use crate::sftp_ops;
use crate::ssh_session::SshHandle;

/// 编辑器单次读 / 写整体超时（含建链）。4MB 上限下足够，半死连接则快速失败。
const SFTP_IO_TIMEOUT: Duration = Duration::from_secs(30);

/// 远程文件基线元数据 (size, modified)；冲突检测的对比基准。modified 可能缺失。
pub type RemoteMeta = (u64, Option<SystemTime>);

/// 远程读结果。超大 / 二进制 / 非 UTF-8 由调用方据此回退（提示下载）。
pub enum RemoteReadOutcome {
    Text { content: String, meta: RemoteMeta },
    TooLarge,
    Binary,
    NotUtf8,
    Error(String),
}

/// 远程保存结果。
pub enum RemoteSaveOutcome {
    Saved { meta: RemoteMeta },
    Conflict { current: RemoteMeta },
    Error(String),
}

fn build_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("runtime: {error}"))
}

/// 起线程读远程文本文件到内存（最多 max_bytes+1 字节判超大）。
pub fn spawn_remote_read(
    handle: SshHandle,
    path: String,
    max_bytes: usize,
) -> async_channel::Receiver<RemoteReadOutcome> {
    let (tx, rx) = async_channel::bounded(1);
    // spawn 失败兜底：否则 tx drop → UI 永停 loading（review B）。
    let err_tx = tx.clone();
    if let Err(error) = thread::Builder::new()
        .name("nexshell-editor-read".to_string())
        .spawn(move || {
            let runtime = match build_runtime() {
                Ok(rt) => rt,
                Err(error) => {
                    let _ = tx.try_send(RemoteReadOutcome::Error(error));
                    return;
                }
            };
            runtime.block_on(async {
                let outcome =
                    match tokio::time::timeout(SFTP_IO_TIMEOUT, remote_read(&handle, &path, max_bytes))
                        .await
                    {
                        Ok(outcome) => outcome,
                        Err(_) => RemoteReadOutcome::Error(
                            "读取超时（远端无响应，可能已断开）".to_string(),
                        ),
                    };
                let _ = tx.send(outcome).await;
            });
        })
    {
        let _ = err_tx.try_send(RemoteReadOutcome::Error(format!("spawn read thread: {error}")));
    }
    rx
}

async fn remote_read(handle: &SshHandle, path: &str, max_bytes: usize) -> RemoteReadOutcome {
    let sftp = match sftp_ops::open_sftp_on_handle(handle).await {
        Ok(s) => s,
        Err(error) => return RemoteReadOutcome::Error(error),
    };
    let (bytes, meta) = match sftp_ops::read_file_to_memory(&sftp, path, max_bytes).await {
        Ok(b) => b,
        Err(error) => return RemoteReadOutcome::Error(error),
    };
    if bytes.len() > max_bytes {
        return RemoteReadOutcome::TooLarge;
    }
    if warp_util::file_type::is_buffer_binary(&bytes) {
        return RemoteReadOutcome::Binary;
    }
    match String::from_utf8(bytes) {
        Ok(content) => {
            // meta 与内容来自同一 open 句柄（review E）；缺失则退化为内容长度。
            let meta = meta.unwrap_or((content.len() as u64, None));
            RemoteReadOutcome::Text { content, meta }
        }
        Err(_) => RemoteReadOutcome::NotUtf8,
    }
}

/// 起线程把内存内容覆盖写回远程文件。除非 force，先 stat 与 expected 比对做冲突检测。
pub fn spawn_remote_save(
    handle: SshHandle,
    path: String,
    content: Vec<u8>,
    expected: Option<RemoteMeta>,
    force: bool,
) -> async_channel::Receiver<RemoteSaveOutcome> {
    let (tx, rx) = async_channel::bounded(1);
    // spawn 失败兜底：否则 tx drop → saving 永真、tab 锁死（review B）。
    let err_tx = tx.clone();
    if let Err(error) = thread::Builder::new()
        .name("nexshell-editor-save".to_string())
        .spawn(move || {
            let runtime = match build_runtime() {
                Ok(rt) => rt,
                Err(error) => {
                    let _ = tx.try_send(RemoteSaveOutcome::Error(error));
                    return;
                }
            };
            runtime.block_on(async {
                let outcome = match tokio::time::timeout(
                    SFTP_IO_TIMEOUT,
                    remote_save(&handle, &path, &content, expected, force),
                )
                .await
                {
                    Ok(outcome) => outcome,
                    Err(_) => {
                        RemoteSaveOutcome::Error("保存超时（远端无响应，可能已断开）".to_string())
                    }
                };
                let _ = tx.send(outcome).await;
            });
        })
    {
        let _ = err_tx.try_send(RemoteSaveOutcome::Error(format!("spawn save thread: {error}")));
    }
    rx
}

async fn remote_save(
    handle: &SshHandle,
    path: &str,
    content: &[u8],
    expected: Option<RemoteMeta>,
    force: bool,
) -> RemoteSaveOutcome {
    let sftp = match sftp_ops::open_sftp_on_handle(handle).await {
        Ok(s) => s,
        Err(error) => return RemoteSaveOutcome::Error(error),
    };
    if !force {
        if let Some(exp) = expected {
            // stat 失败（文件被删等）不阻断保存：按新建写回。
            if let Some(current) = stat_meta(&sftp, path).await {
                if current != exp {
                    return RemoteSaveOutcome::Conflict { current };
                }
            }
        }
    }
    if let Err(error) = sftp_ops::write_file_from_memory(&sftp, path, content).await {
        return RemoteSaveOutcome::Error(error);
    }
    let meta = stat_meta(&sftp, path)
        .await
        .unwrap_or((content.len() as u64, None));
    RemoteSaveOutcome::Saved { meta }
}

async fn stat_meta(sftp: &russh_sftp::client::SftpSession, path: &str) -> Option<RemoteMeta> {
    sftp_ops::stat_file(sftp, path)
        .await
        .ok()
        .map(|e| (e.size, e.modified))
}
