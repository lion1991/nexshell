use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use russh::client;
use russh::keys::key::PublicKey;
use russh::keys::known_hosts::{check_known_hosts_path, learn_known_hosts_path};
use russh::{Channel, ChannelMsg, Disconnect};
use russh_sftp::client::SftpSession;
use ssh_key::Certificate;
use tokio::sync::Mutex;

/// known_hosts 校验结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyVerdict {
    /// 已记录且一致。
    Known,
    /// 未记录，已按 accept-new 写入。
    Learned,
    /// 已记录但与远端不一致，必须拒绝。
    Changed,
}

/// 默认 known_hosts 路径（~/.ssh/known_hosts）。
fn default_known_hosts_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.is_empty())?;
    Some(PathBuf::new().join(home).join(".ssh").join("known_hosts"))
}

/// 对指定 known_hosts 文件做 TOFU 校验：一致放行，未知写入后放行，变化则拒绝。
pub fn verify_host_key_at(
    path: &Path,
    host: &str,
    port: u16,
    key: &PublicKey,
) -> Result<HostKeyVerdict, String> {
    match check_known_hosts_path(host, port, key, path) {
        Ok(true) => Ok(HostKeyVerdict::Known),
        Ok(false) => {
            learn_known_hosts_path(host, port, key, path)
                .map_err(|error| format!("known_hosts write failed: {error}"))?;
            Ok(HostKeyVerdict::Learned)
        }
        Err(russh::keys::Error::KeyChanged { .. }) => Ok(HostKeyVerdict::Changed),
        Err(error) => Err(format!("known_hosts check failed: {error}")),
    }
}

/// 连接期共享的 host key 校验状态；失败原因回传给 connect() 组装可读提示。
#[derive(Clone)]
pub struct ClientHandler {
    host: String,
    port: u16,
    reject_reason: Arc<StdMutex<Option<String>>>,
}

impl ClientHandler {
    fn new(host: &str, port: u16) -> Self {
        Self {
            host: host.to_string(),
            port,
            reject_reason: Arc::new(StdMutex::new(None)),
        }
    }

    fn record_reject(&self, reason: String) {
        if let Ok(mut slot) = self.reject_reason.lock() {
            *slot = Some(reason);
        }
    }

    fn take_reject_reason(&self) -> Option<String> {
        self.reject_reason
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }
}

/// 暴露给 UI 用的 SSH handle 别名。`client::Handle` 内部持有 unbounded receiver
/// 和 JoinHandle 所以不能 Clone，这里用 Arc 包一层共享所有权。
/// 持有者可以 `channel_open_session` + `request_subsystem("sftp")` 开 SFTP channel，
/// 跟 PTY channel 在同一 TCP 连接上多路复用。
pub type SshHandle = Arc<client::Handle<ClientHandler>>;

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        let Some(path) = default_known_hosts_path() else {
            self.record_reject(rust_i18n::t!("ssh_known_hosts_unavailable").to_string());
            return Ok(false);
        };
        match verify_host_key_at(&path, &self.host, self.port, server_public_key) {
            Ok(HostKeyVerdict::Known) | Ok(HostKeyVerdict::Learned) => Ok(true),
            Ok(HostKeyVerdict::Changed) => {
                self.record_reject(
                    rust_i18n::t!(
                        "ssh_host_key_changed",
                        host = self.host,
                        path = path.display().to_string()
                    )
                    .to_string(),
                );
                Ok(false)
            }
            Err(error) => {
                self.record_reject(error);
                Ok(false)
            }
        }
    }
}

pub enum ChannelRequest {
    Data(Vec<u8>),
    Resize(u32, u32),
    Close,
}

pub struct SshConnectOptions {
    pub keep_alive_enabled: bool,
    pub keep_alive_interval_secs: u16,
    pub keep_alive_max_failures: u8,
}

pub struct SshSession {
    /// Arc 让认证完成后可以把 handle clone 给 UI 用（开 SFTP channel）。
    /// 认证期间用 Arc::get_mut 取 &mut；那时只有 SshSession 一个所有者。
    handle: SshHandle,
    channel: Mutex<Option<Channel<client::Msg>>>,
}

pub struct SshExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_status: Option<u32>,
}

impl SshSession {
    pub async fn connect(
        host: &str,
        port: u16,
        options: SshConnectOptions,
    ) -> Result<Self, String> {
        let mut cfg = client::Config::default();
        if options.keep_alive_enabled {
            cfg.keepalive_interval = Some(Duration::from_secs(u64::from(
                options.keep_alive_interval_secs,
            )));
            // Some SSH servers and middleboxes do not reply to OpenSSH global
            // keepalive requests even though the channel is still usable. Keep
            // sending protocol-level probes, but let the channel event loop's
            // periodic no-op window-change detect real write failures instead
            // of closing a healthy idle session from russh's timeout counter.
            let _ = options.keep_alive_max_failures;
            cfg.keepalive_max = 0;
        } else {
            cfg.keepalive_interval = None;
            cfg.keepalive_max = 0;
        }

        let addr = format!("{host}:{port}");
        let handler = ClientHandler::new(host, port);
        let reason_probe = handler.clone();
        let handle = client::connect(Arc::new(cfg), &addr, handler)
            .await
            .map_err(|error| match reason_probe.take_reject_reason() {
                Some(reason) => reason,
                None => format!("SSH connection failed: {error}"),
            })?;

        Ok(Self {
            handle: Arc::new(handle),
            channel: Mutex::new(None),
        })
    }

    /// 认证阶段需要 &mut Handle；调用前不应该 handle()，否则 Arc::get_mut 会失败。
    fn handle_mut(&mut self) -> Result<&mut client::Handle<ClientHandler>, String> {
        Arc::get_mut(&mut self.handle).ok_or_else(|| {
            "SSH handle already shared (auth must complete before handle())".to_string()
        })
    }

    pub async fn auth_password(&mut self, username: &str, password: &str) -> Result<(), String> {
        let handle = self.handle_mut()?;
        let auth_ok = handle
            .authenticate_password(username, password)
            .await
            .map_err(|error| format!("Password auth failed: {error}"))?;

        if !auth_ok {
            return Err("Password authentication rejected by server".to_string());
        }
        Ok(())
    }

    pub async fn auth_key(
        &mut self,
        username: &str,
        key_data: &str,
        key_passphrase: Option<&str>,
        ca_cert: Option<&str>,
    ) -> Result<(), String> {
        let key_pair = russh::keys::decode_secret_key(key_data, key_passphrase)
            .map_err(|error| format!("Invalid private key: {error}"))?;
        let handle = self.handle_mut()?;
        let auth_ok = if let Some(cert_raw) = ca_cert {
            let cert = Certificate::from_openssh(cert_raw.trim())
                .map_err(|error| format!("Invalid OpenSSH certificate: {error}"))?;
            handle
                .authenticate_openssh_cert(username, Arc::new(key_pair), cert)
                .await
                .map_err(|error| format!("Certificate auth failed: {error}"))?
        } else {
            handle
                .authenticate_publickey(username, Arc::new(key_pair))
                .await
                .map_err(|error| format!("Key auth failed: {error}"))?
        };

        if !auth_ok {
            return Err("Public key authentication rejected by server".to_string());
        }
        Ok(())
    }

    pub async fn request_pty(&self, cols: u32, rows: u32) -> Result<(), String> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|error| format!("Channel open failed: {error}"))?;

        channel
            .request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
            .await
            .map_err(|error| format!("PTY request failed: {error}"))?;

        channel
            .request_shell(true)
            .await
            .map_err(|error| format!("Shell request failed: {error}"))?;

        let mut ch = self.channel.lock().await;
        *ch = Some(channel);
        Ok(())
    }

    pub async fn take_channel(&self) -> Option<Channel<client::Msg>> {
        self.channel.lock().await.take()
    }

    pub async fn exec_command(
        &self,
        command: &str,
        timeout: Duration,
    ) -> Result<SshExecOutput, String> {
        tokio::time::timeout(timeout, async {
            let mut channel = self
                .handle
                .channel_open_session()
                .await
                .map_err(|error| format!("Exec channel open failed: {error}"))?;
            channel
                .exec(true, command)
                .await
                .map_err(|error| format!("Exec request failed: {error}"))?;

            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut exit_status = None;

            while let Some(msg) = channel.wait().await {
                match msg {
                    ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                    ChannelMsg::ExtendedData { data, .. } => stderr.extend_from_slice(&data),
                    ChannelMsg::ExitStatus { exit_status: code } => exit_status = Some(code),
                    _ => {}
                }
            }

            if exit_status.unwrap_or(0) != 0 {
                let detail = String::from_utf8_lossy(&stderr).trim().to_string();
                return Err(if detail.is_empty() {
                    format!("Exec command exited with status {:?}", exit_status)
                } else {
                    detail
                });
            }

            Ok(SshExecOutput {
                stdout,
                stderr,
                exit_status,
            })
        })
        .await
        .map_err(|_| "Exec command timed out".to_string())?
    }

    /// 测一次 SSH 协议层 RTT：对 channel_open_session 的服务端确认计时（≈1 个网络往返），
    /// 开完即关，不产生远端进程。
    pub async fn measure_rtt(&self, timeout: Duration) -> Result<Duration, String> {
        tokio::time::timeout(timeout, async {
            let started = std::time::Instant::now();
            let channel = self
                .handle
                .channel_open_session()
                .await
                .map_err(|error| format!("RTT channel open failed: {error}"))?;
            let rtt = started.elapsed();
            let _ = channel.close().await;
            Ok(rtt)
        })
        .await
        .map_err(|_| "RTT probe timed out".to_string())?
    }

    /// 暴露 client handle（Arc clone 廉价）。
    /// UI 拿到后可以并发开多个 channel（PTY / SFTP / exec），共享 TCP 连接。
    /// 注意：一旦调用过此方法，handle 会被多方持有，后续不能再走 auth_* 路径。
    pub fn handle(&self) -> SshHandle {
        Arc::clone(&self.handle)
    }

    /// 在现有连接上开一个 SFTP subsystem channel。
    /// 与 PTY channel 并发，互不影响。
    pub async fn open_sftp(&self) -> Result<SftpSession, String> {
        let channel = self
            .handle
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
    }

    pub async fn close(&self) {
        if let Some(channel) = self.channel.lock().await.take() {
            let _ = channel.eof().await;
            let _ = channel.close().await;
        }
        let _ = self
            .handle
            .disconnect(Disconnect::ByApplication, "bye", "en")
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::key::KeyPair;

    fn public_key() -> PublicKey {
        KeyPair::generate_ed25519().clone_public_key().unwrap()
    }

    #[test]
    fn unknown_host_is_learned_then_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let key = public_key();

        assert_eq!(
            verify_host_key_at(&path, "example.com", 22, &key).unwrap(),
            HostKeyVerdict::Learned
        );
        assert_eq!(
            verify_host_key_at(&path, "example.com", 22, &key).unwrap(),
            HostKeyVerdict::Known
        );
    }

    #[test]
    fn non_default_port_is_recorded_with_brackets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let key = public_key();

        assert_eq!(
            verify_host_key_at(&path, "example.com", 2222, &key).unwrap(),
            HostKeyVerdict::Learned
        );
        let recorded = std::fs::read_to_string(&path).unwrap();
        assert!(recorded.contains("[example.com]:2222"));
        // 端口不同视为不同条目，22 端口仍是未知。
        assert_eq!(
            verify_host_key_at(&path, "example.com", 22, &key).unwrap(),
            HostKeyVerdict::Learned
        );
    }

    #[test]
    fn changed_host_key_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let first = public_key();
        let second = public_key();

        assert_eq!(
            verify_host_key_at(&path, "example.com", 22, &first).unwrap(),
            HostKeyVerdict::Learned
        );
        let before = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            verify_host_key_at(&path, "example.com", 22, &second).unwrap(),
            HostKeyVerdict::Changed
        );
        // 拒绝时不得改写 known_hosts。
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }
}
