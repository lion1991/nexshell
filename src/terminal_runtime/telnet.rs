//! Telnet 传输：std TcpStream + 独立线程。读线程把收到的字节转进同一个请求通道，
//! 主循环独占 codec 状态，无轮询延迟。协议编解码见 `crate::telnet`。

use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::Instant;

use super::*;
use crate::telnet::{TelnetAutoLogin, TelnetCodec, KEEPALIVE};

/// 与 SSH PTY 请求的 TERM 一致。
const TELNET_TERM: &str = "xterm-256color";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TelnetRuntimeConfig {
    pub host: String,
    pub port: u16,
    /// 非空则自动应答一次登录提示。
    pub username: String,
    pub password: String,
    pub connect_timeout_secs: u16,
    pub keep_alive_enabled: bool,
    pub keep_alive_interval_secs: u16,
    pub term_encoding: String,
}

enum TelnetRequest {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
    /// 读线程 → 主循环
    Received(Vec<u8>),
    ReadEnd(String),
}

pub(super) struct TelnetEventLoopHandle {
    request_tx: mpsc::Sender<TelnetRequest>,
    _thread: Option<JoinHandle<()>>,
}

impl TelnetEventLoopHandle {
    pub(super) fn send_data(&self, bytes: Vec<u8>) {
        let _ = self.request_tx.send(TelnetRequest::Data(bytes));
    }

    pub(super) fn resize(&self, cols: u16, rows: u16) {
        let _ = self.request_tx.send(TelnetRequest::Resize(cols, rows));
    }
}

impl Drop for TelnetEventLoopHandle {
    fn drop(&mut self) {
        let _ = self.request_tx.send(TelnetRequest::Close);
    }
}

impl LocalTerminalRuntime {
    pub fn spawn_telnet_or_failed(
        session_id: &str,
        config: TelnetRuntimeConfig,
        cols: u16,
        rows: u16,
    ) -> Self {
        Self::spawn_telnet(session_id, config, cols, rows).unwrap_or_else(|error| {
            Self::failed(
                session_id,
                format!("failed to start telnet session: {error}"),
            )
        })
    }

    pub fn spawn_telnet(
        session_id: &str,
        config: TelnetRuntimeConfig,
        cols: u16,
        rows: u16,
    ) -> Result<Self, String> {
        if config.host.trim().is_empty() {
            return Err("主机地址为空".to_string());
        }
        let status = format!("connecting Telnet: {}:{}", config.host.trim(), config.port);
        let mut runtime_state = TerminalRuntimeState::new(session_id, true, status, cols, rows);
        runtime_state.bootstrapped = true;
        let state = Arc::new(FairMutex::new(runtime_state));
        let (wakeup_tx, wakeup_rx) = async_channel::bounded::<()>(1);
        let (event_tx, event_rx) = async_channel::unbounded::<PtyEvent>();
        let (request_tx, request_rx) = mpsc::channel::<TelnetRequest>();
        let thread = thread::Builder::new()
            .name(format!("nexshell-telnet-{session_id}"))
            .spawn({
                let state = Arc::clone(&state);
                let request_tx = request_tx.clone();
                move || {
                    run_telnet_event_loop(
                        state, config, request_tx, request_rx, wakeup_tx, event_tx, cols, rows,
                    )
                }
            })
            .map_err(|error| format!("spawn telnet thread: {error}"))?;

        Ok(Self {
            state,
            event_loop: None,
            remote_event_loop: None,
            serial_event_loop: None,
            telnet_event_loop: Some(TelnetEventLoopHandle {
                request_tx,
                _thread: Some(thread),
            }),
            ssh_handle_rx: None,
            wakeup_rx: Some(wakeup_rx),
            event_rx: Some(event_rx),
            pty_fd: None,
            shell_is_foreground: Arc::new(AtomicBool::new(true)),
            herdr_lease: std::sync::Mutex::new(HerdrLeaseState::default()),
            last_resize_request: std::sync::Mutex::new(None),
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn run_telnet_event_loop(
    state: Arc<FairMutex<TerminalRuntimeState>>,
    config: TelnetRuntimeConfig,
    request_tx: mpsc::Sender<TelnetRequest>,
    request_rx: mpsc::Receiver<TelnetRequest>,
    wakeup_tx: async_channel::Sender<()>,
    event_tx: async_channel::Sender<PtyEvent>,
    cols: u16,
    rows: u16,
) {
    let target = format!("{}:{}", config.host.trim(), config.port);
    remote_process_output(
        &state,
        &wakeup_tx,
        format!("Trying {target}...\r\n").as_bytes(),
    );
    let mut stream = match telnet_connect(&config) {
        Ok(stream) => stream,
        Err(error) => {
            remote_mark_disconnected(&state, &wakeup_tx, &event_tx, error);
            return;
        }
    };
    let reader = match stream.try_clone() {
        Ok(reader) => reader,
        Err(error) => {
            remote_mark_disconnected(&state, &wakeup_tx, &event_tx, format!("telnet: {error}"));
            return;
        }
    };
    thread::spawn(move || telnet_read_loop(reader, request_tx));
    remote_update_status(&state, &wakeup_tx, format!("connected Telnet: {target}"));

    let mut codec = TelnetCodec::new(TELNET_TERM, cols, rows);
    let mut encoding = RemoteTerminalEncoding::new(&config.term_encoding);
    let mut auto_login = TelnetAutoLogin::new(config.username.trim(), &config.password);
    let keepalive = config
        .keep_alive_enabled
        .then(|| Duration::from_secs(u64::from(config.keep_alive_interval_secs.clamp(10, 300))));
    let mut last_write = Instant::now();
    let start = codec.start();
    let disconnect_status = match write(&mut stream, &start, &mut last_write) {
        Err(error) => error,
        Ok(()) => loop {
            let request = match keepalive {
                Some(interval) => {
                    let wait = interval.saturating_sub(last_write.elapsed());
                    match request_rx.recv_timeout(wait) {
                        Ok(request) => request,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if let Err(error) = write(&mut stream, &KEEPALIVE, &mut last_write) {
                                break error;
                            }
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break closed(),
                    }
                }
                None => match request_rx.recv() {
                    Ok(request) => request,
                    Err(_) => break closed(),
                },
            };
            let result = match request {
                TelnetRequest::Data(bytes) => {
                    let encoded = encoding.encode_input(&bytes);
                    write(&mut stream, &codec.send(&encoded), &mut last_write)
                }
                TelnetRequest::Resize(cols, rows) => {
                    remote_handle_resize(&state, &wakeup_tx, cols, rows);
                    match codec.resize(cols, rows) {
                        Some(naws) => write(&mut stream, &naws, &mut last_write),
                        None => Ok(()),
                    }
                }
                TelnetRequest::Received(bytes) => {
                    let input = codec.receive(&bytes);
                    let mut out = input.reply;
                    if !input.data.is_empty() {
                        let decoded = encoding.decode_output(&input.data).into_owned();
                        if let Some(line) = auto_login.feed(&String::from_utf8_lossy(&decoded)) {
                            out.extend(codec.send(&encoding.encode_input(line.as_bytes())));
                        }
                        for reply in remote_process_output(&state, &wakeup_tx, &decoded) {
                            out.extend(codec.send(&encoding.encode_input(&reply)));
                        }
                    }
                    write(&mut stream, &out, &mut last_write)
                }
                TelnetRequest::ReadEnd(status) => break status,
                TelnetRequest::Close => break closed(),
            };
            if let Err(error) = result {
                break error;
            }
        },
    };

    // 让读线程的阻塞 read 立即返回
    let _ = stream.shutdown(Shutdown::Both);
    remote_mark_disconnected(&state, &wakeup_tx, &event_tx, disconnect_status);
}

fn write(stream: &mut TcpStream, bytes: &[u8], last_write: &mut Instant) -> Result<(), String> {
    if bytes.is_empty() {
        return Ok(());
    }
    *last_write = Instant::now();
    stream
        .write_all(bytes)
        .map_err(|error| format!("telnet write error: {error}"))
}

fn closed() -> String {
    "Telnet session closed".to_string()
}

fn telnet_connect(config: &TelnetRuntimeConfig) -> Result<TcpStream, String> {
    let host = config.host.trim();
    let timeout = Duration::from_secs(u64::from(config.connect_timeout_secs.clamp(5, 60)));
    let addrs = (host, config.port)
        .to_socket_addrs()
        .map_err(|error| format!("resolve {host}: {error}"))?;
    let mut last_error = format!("resolve {host}: no address");
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(error) => last_error = format!("connect {addr}: {error}"),
        }
    }
    Err(last_error)
}

fn telnet_read_loop(mut reader: TcpStream, request_tx: mpsc::Sender<TelnetRequest>) {
    let mut buf = [0_u8; 4096];
    let end = loop {
        match reader.read(&mut buf) {
            Ok(0) => break "Connection closed by foreign host".to_string(),
            Ok(n) => {
                if request_tx
                    .send(TelnetRequest::Received(buf[..n].to_vec()))
                    .is_err()
                {
                    return;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => break format!("telnet read error: {error}"),
        }
    };
    let _ = request_tx.send(TelnetRequest::ReadEnd(end));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    const WAIT: Duration = Duration::from_secs(5);

    fn read_until(stream: &mut TcpStream, got: &mut Vec<u8>, needle: &[u8]) {
        let deadline = Instant::now() + WAIT;
        let mut buf = [0_u8; 1024];
        while !got.windows(needle.len()).any(|w| w == needle) {
            assert!(
                Instant::now() < deadline,
                "等不到 {needle:?}，已收到 {got:?}"
            );
            match stream.read(&mut buf) {
                Ok(0) => panic!("客户端提前断开，已收到 {got:?}"),
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(error) => panic!("read: {error}"),
            }
        }
        got.clear();
    }

    /// 带超时的 accept：客户端没连上时测试快速失败而不是挂死。
    fn accept(listener: &TcpListener) -> TcpStream {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + WAIT;
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    return stream;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "客户端没有连上来");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    }

    fn wait_for_screen(runtime: &LocalTerminalRuntime, text: &str) {
        let deadline = Instant::now() + WAIT;
        loop {
            let snapshot = runtime.snapshot();
            if snapshot.lines.iter().any(|line| line.contains(text)) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "屏幕上等不到 {text:?}：{:?} / {}",
                snapshot.lines,
                snapshot.status
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn config(port: u16) -> TelnetRuntimeConfig {
        TelnetRuntimeConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "admin".to_string(),
            password: "pw".to_string(),
            connect_timeout_secs: 5,
            keep_alive_enabled: false,
            keep_alive_interval_secs: 30,
            term_encoding: "gbk".to_string(),
        }
    }

    #[test]
    fn negotiates_logs_in_and_relays_input_resize_and_gbk_output() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let mut s = accept(&listener);
            s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
            let mut got = Vec::new();
            // 同意客户端的 NAWS、开远端回显；应立刻收到 80x24
            s.write_all(&[255, 253, 31, 255, 251, 1]).unwrap();
            read_until(&mut s, &mut got, &[255, 250, 31, 0, 80, 0, 24, 255, 240]);
            s.write_all(b"Username: ").unwrap();
            read_until(&mut s, &mut got, b"admin\r\0");
            s.write_all(b"\r\nPassword: ").unwrap();
            read_until(&mut s, &mut got, b"pw\r\0");
            let (welcome, _, _) = encoding_rs::GBK.encode("\r\n欢迎 router> ");
            s.write_all(&welcome).unwrap();
            read_until(&mut s, &mut got, b"show\r\0");
            read_until(&mut s, &mut got, &[255, 250, 31, 0, 100, 0, 30, 255, 240]);
        });

        let runtime = LocalTerminalRuntime::spawn_telnet("telnet-test", config(port), 80, 24)
            .expect("spawn telnet");
        wait_for_screen(&runtime, "router>");
        wait_for_screen(&runtime, "欢迎");
        runtime.send_input(b"show\r".to_vec());
        runtime.resize(100, 30);
        server.join().expect("server 断言失败");
    }

    #[test]
    fn server_close_marks_disconnected() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let mut s = accept(&listener);
            s.write_all(b"bye\r\n").unwrap();
        });
        let runtime = LocalTerminalRuntime::spawn_telnet("telnet-close", config(port), 80, 24)
            .expect("spawn telnet");
        server.join().unwrap();
        wait_for_screen(&runtime, "bye");
        let deadline = Instant::now() + WAIT;
        while runtime.is_connected() {
            assert!(Instant::now() < deadline, "服务端关闭后应标记断开");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
