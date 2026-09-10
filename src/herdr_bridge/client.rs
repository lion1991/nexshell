//! herdr Unix socket I/O：一次性请求 + 订阅长连接。阻塞式 std::os::unix::net。

use std::io;
use std::path::Path;

use super::protocol::Response;

#[cfg(unix)]
mod imp {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    use crate::herdr_bridge::protocol::{self, Line};

    /// 单次请求超时；herdr 本地响应是毫秒级，3s 足够宽松。
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
    /// 单行上限，防御 server 异常输出把内存吃光。
    const MAX_LINE_BYTES: u64 = 1024 * 1024;

    /// 一连接一请求：连、写一行、读一行、关。herdr 在同一连接上收第二个
    /// 普通请求会直接断开（BrokenPipe），所以绝不复用。
    pub fn request_once(path: &Path, line: &str) -> io::Result<Response> {
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
        stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
        let mut writer = stream.try_clone()?;
        writer.write_all(line.as_bytes())?;
        writer.flush()?;
        let mut reader = BufReader::new(stream);
        let (payload, complete) = read_framed_line(&mut reader)?;
        if !complete {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "herdr: response line exceeds limit",
            ));
        }
        match protocol::parse_line(&payload) {
            Some(Line::Response(response)) => Ok(response),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "herdr: unexpected line for request",
            )),
        }
    }

    /// 读一行（不含换行判定由调用方看 `complete`）。非 UTF-8 走 lossy，
    /// 绝不因为一个坏字节把整条订阅断掉。
    fn read_framed_line(reader: &mut BufReader<UnixStream>) -> io::Result<(String, bool)> {
        let mut buf = Vec::new();
        let read = reader.take(MAX_LINE_BYTES).read_until(b'\n', &mut buf)?;
        if read == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "herdr: eof"));
        }
        let complete = buf.last() == Some(&b'\n');
        Ok((String::from_utf8_lossy(&buf).into_owned(), complete))
    }

    /// 订阅长连接：写一次 subscribe，之后只读事件。
    pub struct Subscription {
        reader: BufReader<UnixStream>,
        shutdown: UnixStream,
    }

    impl Subscription {
        pub fn open(path: &Path, subscribe_line: &str) -> io::Result<Self> {
            let stream = UnixStream::connect(path)?;
            stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
            let mut writer = stream.try_clone()?;
            writer.write_all(subscribe_line.as_bytes())?;
            writer.flush()?;
            let shutdown = stream.try_clone()?;
            Ok(Self {
                reader: BufReader::new(stream),
                shutdown,
            })
        }

        /// 阻塞读下一行；EOF / 出错返回 None。超长行整条丢弃并跳到下一个
        /// 换行，避免半截 JSON 让后续所有帧错位。
        pub fn next_line(&mut self) -> Option<String> {
            loop {
                let (payload, complete) = read_framed_line(&mut self.reader).ok()?;
                if complete {
                    return Some(payload);
                }
                log::debug!("herdr bridge: oversized line dropped");
                self.discard_to_newline()?;
            }
        }

        fn discard_to_newline(&mut self) -> Option<()> {
            loop {
                let (_, complete) = read_framed_line(&mut self.reader).ok()?;
                if complete {
                    return Some(());
                }
            }
        }

        /// BufReader 里是否还压着没消费的字节。用来合并同一批连发的焦点事件：
        /// 还有缓冲就先攒着，别对每条都打一次 `pane.current`。
        pub fn has_buffered_data(&self) -> bool {
            !self.reader.buffer().is_empty()
        }

        /// 可跨线程持有的关闭句柄，让阻塞中的读立刻返回。
        pub fn shutdown_handle(&self) -> ShutdownHandle {
            ShutdownHandle {
                stream: self.shutdown.try_clone().ok(),
            }
        }
    }

    #[derive(Debug, Default)]
    pub struct ShutdownHandle {
        stream: Option<UnixStream>,
    }

    impl ShutdownHandle {
        pub fn shutdown(&self) {
            if let Some(stream) = &self.stream {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use super::*;

    fn unsupported() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "herdr bridge is unix-only")
    }

    pub fn request_once(_path: &Path, _line: &str) -> io::Result<Response> {
        Err(unsupported())
    }

    pub struct Subscription;

    impl Subscription {
        pub fn open(_path: &Path, _subscribe_line: &str) -> io::Result<Self> {
            Err(unsupported())
        }

        pub fn next_line(&mut self) -> Option<String> {
            None
        }

        pub fn has_buffered_data(&self) -> bool {
            false
        }

        pub fn shutdown_handle(&self) -> ShutdownHandle {
            ShutdownHandle
        }
    }

    #[derive(Debug, Default)]
    pub struct ShutdownHandle;

    impl ShutdownHandle {
        pub fn shutdown(&self) {}
    }
}

pub use imp::{ShutdownHandle, Subscription};

/// 发一行已编码的请求并返回响应；error 信封转成 io::Error。
/// `what` 只用于错误文案。请求一律由 `protocol::encode_*` 生成，避免生产路径
/// 手搓 params（`pane.current` 不能带 caller_pane_id）。
pub fn request(path: &Path, line: &str, what: &str) -> io::Result<Response> {
    let response = imp::request_once(path, line)?;
    if let Some(error) = &response.error {
        return Err(io::Error::other(format!(
            "herdr {}: {} ({})",
            what, error.message, error.code
        )));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr_bridge::protocol;
    use crate::herdr_bridge::snapshot::SnapshotIndex;

    #[test]
    fn missing_socket_is_an_error_not_a_panic() {
        let path = Path::new("/tmp/nexshell-herdr-does-not-exist.sock");
        let line = protocol::encode_session_snapshot("t");
        assert!(request(path, &line, "session.snapshot").is_err());
    }

    /// 真机用例：需要本机跑着 herdr server。`cargo test -- --ignored`
    #[test]
    #[ignore = "需要本机 herdr server 在跑"]
    fn live_session_snapshot_indexes_workspaces() {
        let path = protocol::resolve_socket_path().expect("socket path");
        let line = protocol::encode_session_snapshot("nexshell-test");
        let response =
            request(&path, &line, "session.snapshot").expect("session.snapshot should succeed");
        assert!(response.error.is_none());
        let value = protocol::snapshot_from_response(&response).expect("result.snapshot");
        let index = SnapshotIndex::from_json(value);
        // 至少有一个 workspace 能查到 cwd，且全局焦点可解析。
        assert!(
            index.global_focused_cwd().is_some(),
            "global focused cwd should resolve"
        );
        assert_ne!(index, SnapshotIndex::default(), "index should not be empty");
    }

    /// 真机用例：订阅连接建起来后第一行应是 subscription_started，
    /// 顺带验证扩大后的订阅集合被 server 接受。
    #[test]
    #[ignore = "需要本机 herdr server 在跑"]
    fn live_subscription_starts() {
        let path = protocol::resolve_socket_path().expect("socket path");
        let mut sub = Subscription::open(&path, &protocol::encode_subscribe("nexshell-test"))
            .expect("subscribe should succeed");
        let line = sub.next_line().expect("first line");
        let Some(protocol::Line::Response(response)) = protocol::parse_line(&line) else {
            panic!("expected subscription ack, got {line}");
        };
        assert!(response.error.is_none(), "server rejected subscriptions");
        assert_eq!(
            response.result.as_ref().and_then(|r| r["type"].as_str()),
            Some("subscription_started")
        );
    }
}
