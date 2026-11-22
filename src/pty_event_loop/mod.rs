//! PTY event loop, ported from Warp.
//!
//! This module is a one-to-one Rust port of
//! `warp/crates/warp_terminal/src/local_tty/event_loop.rs` (and its sibling
//! `mio_channel.rs`), adapted to drive a [`portable_pty`] master file
//! descriptor instead of Warp's hand-rolled `Pty`.
//!
//! Why this exists:
//! - The pre-Warp spike read the PTY on a blocking thread with an 8 KB
//!   buffer and woke the UI through an 8 ms `Timer::after` poll. That
//!   produced typing latency jitter and held the state mutex 1 lock per
//!   8 KB. Warp's loop reads up to 256 KB per wakeup, only locks at the
//!   end (or on a `MAX_LOCKED_READ` boundary), and yields the lock to
//!   waiting threads with `FairMutexGuard::bump` so the UI can read a
//!   snapshot mid-burst. We follow the exact same recipe.
//! - PTY readability, PTY writability, channel input, and SIGCHLD all
//!   wait on the same `mio::Poll`, mirroring Warp's source layout
//!   (CHANNEL_TOKEN / PTY_TOKEN / SIGNALS_TOKEN).
//! - Wakeups land on an `async_channel::Sender<()>` that the UI throttles
//!   to 60 Hz, just like Warp's `WAKEUP_THROTTLE_PERIOD`
//!   (`app/src/terminal/view.rs`).

pub mod mio_channel;

use std::{borrow::Cow, io, sync::Arc, thread::JoinHandle, time::Instant};

#[cfg(unix)]
use std::{
    collections::VecDeque,
    fs::File,
    io::{ErrorKind, Read, Write},
    mem::ManuallyDrop,
    os::unix::io::{FromRawFd, RawFd},
    thread,
};
#[cfg(windows)]
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::RecvTimeoutError,
    },
    thread,
};

#[cfg(unix)]
use mio::{unix::SourceFd, Events, Interest, Poll, Token};
use parking_lot::FairMutex;
#[cfg(unix)]
use parking_lot::FairMutexGuard;
use portable_pty::{ChildKiller, MasterPty, PtySize};
#[cfg(unix)]
use signal_hook::consts::SIGCHLD;
#[cfg(unix)]
use signal_hook_mio::v1_0::Signals;

/// Mirrors `READ_BUFFER_SIZE` in `warp/crates/warp_terminal/src/local_tty/event_loop.rs`.
#[cfg(any(unix, windows))]
const READ_BUFFER_SIZE: usize = 0x4_0000;

/// Mirrors `MAX_LOCKED_READ` in `warp/crates/warp_terminal/src/local_tty/event_loop.rs`.
#[cfg(unix)]
const MAX_LOCKED_READ: usize = 0x1_0000;

// 同步输出（`?2026h` … `?2026l`）：vte 只在收到 ESU 或缓冲攒满 2 MiB 时结束同步，
// 不会自己按时间收尾。程序发了 BSU 却没发 ESU（画到一半被杀）时，读循环要在
// `PtySink::sync_output_deadline` 到点后调 `finish_expired_sync_output`，否则标签页一直不刷新。
// 做法同 Warp 的 `crates/warp_terminal/src/local_tty/event_loop.rs` poll 超时后 finish_sync_output。

#[cfg(unix)]
const CHANNEL_TOKEN: Token = Token(0);
#[cfg(unix)]
const PTY_TOKEN: Token = Token(1);
#[cfg(unix)]
const SIGNALS_TOKEN: Token = Token(2);

#[cfg(unix)]
fn pty_debug_key_log(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("NEXSHELL_DEBUG_KEYS").is_some() {
        eprintln!("[nexshell key-debug] {args}");
    }
}

#[cfg(unix)]
fn pty_debug_bytes(bytes: &[u8]) -> String {
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    let text = String::from_utf8_lossy(bytes);
    format!(
        "len={} hex=[{hex}] text=\"{}\"",
        bytes.len(),
        text.escape_debug()
    )
}

/// Messages the UI thread sends into the PTY event loop. Same shape as
/// Warp's `crate::terminal::writeable_pty::Message`.
pub enum Message {
    Input(Cow<'static, [u8]>),
    Resize(PtySize),
    ClearVisibleScreen {
        preserve_prompt_prefix: bool,
    },
    Shutdown,
    /// Windows 读线程 → 事件线程：开始了一段同步输出，事件线程据此改按截止时间等待。
    #[cfg(windows)]
    SyncOutputStarted,
}

/// Things the PTY event loop pushes to the UI side. We separate
/// "wakeup" (terminal grid changed, UI should rerender) from "lifecycle"
/// (child exited, write error) because Warp throttles wakeups but
/// processes lifecycle events immediately.
#[derive(Debug, Clone)]
pub enum PtyEvent {
    ChildExited,
    Disconnected(String),
}

/// Implemented by whatever owns the terminal grid + ANSI parser. The
/// event loop locks `Arc<FairMutex<S>>`, calls `process_output(...)`,
/// and writes any reply bytes the sink returns back to the PTY in the
/// same loop iteration. This matches Warp's
/// `state.parser.parse_bytes(terminal.deref_mut(), bytes, &mut writer)`
/// call shape — parser + writer + handler all line up while the lock is
/// held.
pub trait PtySink: Send + 'static {
    /// Process `bytes` through the ANSI parser and return any reply
    /// bytes that should be written back to the PTY. The event loop
    /// pushes these onto `State::write_list` so they go out with the
    /// other queued writes.
    fn process_output(&mut self, bytes: &[u8]) -> Vec<Cow<'static, [u8]>>;
    fn handle_resize(&mut self, size: PtySize);
    fn clear_visible_screen(&mut self, preserve_prompt_prefix: bool);
    fn mark_disconnected(&mut self, status: String);
    /// 同步输出（`?2026h`）的截止时间；读循环的等待不能超过它。
    fn sync_output_deadline(&self) -> Option<Instant>;
    /// 截止时间已过仍没等到 `?2026l`：强制结束同步。没到点返回 None，否则返回要写回的应答字节。
    fn finish_expired_sync_output(&mut self, now: Instant) -> Option<Vec<Cow<'static, [u8]>>>;
}

/// Wakeup channel handle returned to the UI thread.
pub type WakeupReceiver = async_channel::Receiver<()>;

/// PTY event channel handle returned to the UI thread (lifecycle
/// notifications).
pub type PtyEventReceiver = async_channel::Receiver<PtyEvent>;

/// Handle to the running event loop, owned by `LocalTerminalRuntime`.
pub struct EventLoopHandle {
    pub message_tx: mio_channel::Sender<Message>,
    /// wakeup 发送端的**弱**引用：外部数据源（如 herdr bridge）能戳一次 UI 重绘，
    /// 又不延长通道寿命——子进程退出、event loop 线程结束后强引用全部 drop，
    /// 通道照旧关闭让 UI 的 throttle 流收尾。
    pub weak_wakeup_tx: async_channel::WeakSender<()>,
    pub wakeup_rx: WakeupReceiver,
    pub event_rx: PtyEventReceiver,
    pub thread: Option<JoinHandle<()>>,
    pub killer: Box<dyn ChildKiller + Send + Sync>,
}

impl Drop for EventLoopHandle {
    fn drop(&mut self) {
        // 1:1 与 Warp `terminal_manager.rs` 的 `shutdown_event_loop` 对齐：
        // 只发 Shutdown + join，这里不 kill。子进程还活着时，由事件循环线程退出前
        // kill 掉（EventLoop::run 末尾，同 Warp event_loop 的 `self.pty.kill()`）。
        if let Err(e) = self.message_tx.send(Message::Shutdown) {
            log::info!("Failed to send Shutdown {e:?}");
        }
        if let Some(handle) = self.thread.take() {
            if handle.join().is_err() {
                log::error!("Failed to join PTY event loop handle");
            }
        } else {
            log::error!("No event loop handle to join when dropping PTY event loop.");
        }
    }
}

/// `warp_terminal::local_tty::event_loop::State`.
#[cfg(unix)]
struct State {
    write_list: VecDeque<Cow<'static, [u8]>>,
    writing: Option<Writing>,
    pending_clear_visible_screen: Option<bool>,
    /// 解析器同步输出的截止时间，每次读完输出时从 sink 抄下来，poll 前不用再锁 sink。
    sync_deadline: Option<Instant>,
}

#[cfg(unix)]
impl Default for State {
    fn default() -> Self {
        Self {
            write_list: VecDeque::new(),
            writing: None,
            pending_clear_visible_screen: None,
            sync_deadline: None,
        }
    }
}

#[cfg(unix)]
impl State {
    #[inline]
    fn ensure_next(&mut self) {
        if self.writing.is_none() {
            self.goto_next();
        }
    }

    #[inline]
    fn goto_next(&mut self) {
        self.writing = self.write_list.pop_front().map(Writing::new);
    }

    #[inline]
    fn take_current(&mut self) -> Option<Writing> {
        self.writing.take()
    }

    #[inline]
    fn needs_write(&self) -> bool {
        self.writing.is_some() || !self.write_list.is_empty()
    }

    #[inline]
    fn set_current(&mut self, new: Option<Writing>) {
        self.writing = new;
    }
}

#[cfg(unix)]
struct Writing {
    source: Cow<'static, [u8]>,
    written: usize,
}

#[cfg(unix)]
impl Writing {
    fn new(source: Cow<'static, [u8]>) -> Self {
        Self { source, written: 0 }
    }

    fn advance(&mut self, n: usize) {
        self.written += n;
    }

    fn remaining_bytes(&self) -> &[u8] {
        &self.source[self.written..]
    }

    fn finished(&self) -> bool {
        self.written >= self.source.len()
    }
}

#[cfg(unix)]
enum ChannelResult {
    Continue,
    TerminateLoop,
}

/// Owned by the PTY event loop thread.
#[cfg(unix)]
struct EventLoop<S: PtySink> {
    poll: Poll,
    sink: Arc<FairMutex<S>>,
    /// Master fd, wrapped in `ManuallyDrop<File>` so reads/writes go
    /// through `std::io::Read`/`Write` without us accidentally closing
    /// the fd (the [`MasterPty`] still owns it).
    pty_io: ManuallyDrop<File>,
    pty_fd: RawFd,
    master: Box<dyn MasterPty + Send>,
    rx: mio_channel::Receiver<Message>,
    signals: Signals,
    wakeup_tx: async_channel::Sender<()>,
    event_tx: async_channel::Sender<PtyEvent>,
    /// Captures `signal_hook_mio` `next_child_event`-style state. We
    /// query the [`portable_pty::Child`] via a `Sender<()>` because
    /// `try_wait` lives on the child, not the master. See
    /// [`spawn_event_loop`].
    has_child_exited: Arc<parking_lot::Mutex<bool>>,
    /// 循环退出后在本线程 kill 子进程（Warp `event_loop.rs:471-477`）。
    killer: Box<dyn ChildKiller + Send + Sync>,
}

#[cfg(unix)]
impl<S: PtySink> EventLoop<S> {
    fn drain_recv_channel(&mut self, state: &mut State) -> ChannelResult {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Message::Input(input) => {
                    pty_debug_key_log(format_args!("pty queue input {}", pty_debug_bytes(&input)));
                    state.write_list.push_back(input);
                }
                Message::ClearVisibleScreen {
                    preserve_prompt_prefix,
                } => {
                    state.pending_clear_visible_screen = Some(
                        state.pending_clear_visible_screen.unwrap_or(false)
                            || preserve_prompt_prefix,
                    );
                }
                Message::Resize(size) => {
                    if let Err(error) = self.master.resize(size) {
                        let _ = self
                            .event_tx
                            .try_send(PtyEvent::Disconnected(format!("pty resize error: {error}")));
                    } else {
                        self.sink.lock().handle_resize(size);
                    }
                }
                Message::Shutdown => return ChannelResult::TerminateLoop,
            }
        }
        ChannelResult::Continue
    }

    /// Mirrors `event_loop.rs`'s `fn pty_read`. The lock acquisition
    /// dance with `try_lock` -> "fill more buffer" -> blocking lock on
    /// `>= READ_BUFFER_SIZE` is exactly Warp's, including the
    /// `FairMutexGuard::bump` to yield the lock to a waiting UI thread
    /// every `MAX_LOCKED_READ` bytes.
    fn pty_read(
        &mut self,
        state: &mut State,
        buf: &mut [u8],
        can_read: &mut bool,
    ) -> io::Result<()> {
        let mut bytes_in_buffer = 0;
        let mut bytes_processed = 0;

        let mut sink: Option<FairMutexGuard<'_, S>> = None;

        loop {
            match self.pty_io.read(&mut buf[bytes_in_buffer..]) {
                Ok(0) if bytes_in_buffer == 0 => {
                    *can_read = false;
                    break;
                }
                Ok(got) => bytes_in_buffer += got,
                Err(err) => match err.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                        if err.kind() == ErrorKind::WouldBlock {
                            *can_read = false;
                        }
                        if bytes_in_buffer == 0 {
                            break;
                        }
                    }
                    _ => return Err(err),
                },
            }

            let sink = match &mut sink {
                Some(sink) => sink,
                None => sink.insert(match self.sink.try_lock() {
                    None if bytes_in_buffer >= READ_BUFFER_SIZE => self.sink.lock(),
                    None => continue,
                    Some(sink) => sink,
                }),
            };

            // process_output owns the parser internally; reply bytes
            // (DSR, cursor-position reports, OSC clipboard, etc.) come
            // back so we can queue them on `state.write_list` and let
            // the same iteration drain them through `pty_write`.
            let replies = sink.process_output(&buf[..bytes_in_buffer]);
            for reply in replies {
                state.write_list.push_back(reply);
            }

            bytes_processed += bytes_in_buffer;
            bytes_in_buffer = 0;

            if bytes_processed >= MAX_LOCKED_READ {
                break;
            }

            FairMutexGuard::bump(sink);
        }

        if let Some(sink) = &sink {
            state.sync_deadline = sink.sync_output_deadline();
        }

        // 同步缓冲期间也照常唤醒 UI（画面不变，只是多一次重绘）；
        // Warp 在 `bytes_processed > sync_output_buffer_len` 时才唤醒。
        if bytes_processed > 0 {
            let _ = self.wakeup_tx.try_send(());
        }

        Ok(())
    }

    fn finish_expired_sync_output(&mut self, state: &mut State) {
        let now = Instant::now();
        if !state.sync_deadline.is_some_and(|deadline| now >= deadline) {
            return;
        }
        let mut sink = self.sink.lock();
        if let Some(replies) = sink.finish_expired_sync_output(now) {
            state.write_list.extend(replies);
            let _ = self.wakeup_tx.try_send(());
        }
        state.sync_deadline = sink.sync_output_deadline();
    }

    /// Mirrors `event_loop.rs`'s `fn pty_write`.
    fn pty_write(&mut self, state: &mut State, can_write: &mut bool) -> io::Result<()> {
        state.ensure_next();

        'write_many: while let Some(mut current) = state.take_current() {
            'write_one: loop {
                let remaining = current.remaining_bytes();
                match self.pty_io.write(remaining) {
                    Ok(0) => {
                        state.set_current(Some(current));
                        *can_write = false;
                        break 'write_many;
                    }
                    Ok(n) => {
                        pty_debug_key_log(format_args!(
                            "pty write {}",
                            pty_debug_bytes(&remaining[..n])
                        ));
                        current.advance(n);
                        if current.finished() {
                            state.goto_next();
                            break 'write_one;
                        }
                    }
                    Err(err) => {
                        state.set_current(Some(current));
                        match err.kind() {
                            ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                                if err.kind() == ErrorKind::WouldBlock {
                                    *can_write = false;
                                }
                                break 'write_many;
                            }
                            _ => return Err(err),
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn run(mut self) {
        let mut state = State::default();
        let mut buf = vec![0u8; READ_BUFFER_SIZE];

        let mut can_read = false;
        let mut can_write = false;

        // Channel + PTY + SIGCHLD all share the same Poll, just like
        // Warp's `event_loop.rs`.
        if let Err(err) =
            self.poll
                .registry()
                .register(&mut self.rx, CHANNEL_TOKEN, Interest::READABLE)
        {
            log::error!("PTY event loop: failed to register channel: {err}");
            return;
        }
        if let Err(err) = self.poll.registry().register(
            &mut SourceFd(&self.pty_fd),
            PTY_TOKEN,
            Interest::READABLE | Interest::WRITABLE,
        ) {
            log::error!("PTY event loop: failed to register pty fd: {err}");
            return;
        }
        if let Err(err) =
            self.poll
                .registry()
                .register(&mut self.signals, SIGNALS_TOKEN, Interest::READABLE)
        {
            log::error!("PTY event loop: failed to register signals: {err}");
            return;
        }

        let mut events = Events::with_capacity(1024);
        let mut child_exited = false;

        'event_loop: loop {
            events.clear();

            let timeout = state
                .sync_deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()));
            if let Err(err) = self.poll.poll(&mut events, timeout) {
                match err.kind() {
                    ErrorKind::Interrupted => continue,
                    _ => {
                        log::error!("PTY event loop: poll error: {err}");
                        break 'event_loop;
                    }
                }
            }

            for event in events.iter() {
                match event.token() {
                    CHANNEL_TOKEN => match self.drain_recv_channel(&mut state) {
                        ChannelResult::Continue => {}
                        ChannelResult::TerminateLoop => break 'event_loop,
                    },

                    SIGNALS_TOKEN => {
                        let saw_sigchld = self.signals.pending().any(|signal| signal == SIGCHLD);
                        if saw_sigchld && *self.has_child_exited.lock() {
                            self.sink
                                .lock()
                                .mark_disconnected("shell process exited".to_string());
                            child_exited = true;
                            let _ = self.wakeup_tx.try_send(());
                            let _ = self.event_tx.try_send(PtyEvent::ChildExited);
                            break 'event_loop;
                        }
                    }

                    PTY_TOKEN => {
                        if event.is_read_closed() || event.is_write_closed() {
                            // Don't try to do I/O on a dead PTY; loop
                            // back to wait for the SIGCHLD that's surely
                            // coming.
                            continue;
                        }
                        if event.is_readable() {
                            can_read = true;
                        }
                        if event.is_writable() {
                            can_write = true;
                        }
                    }

                    _ => {}
                }
            }

            self.finish_expired_sync_output(&mut state);

            while can_read || (state.needs_write() && can_write) {
                if can_read {
                    if let Err(err) = self.pty_read(&mut state, &mut buf, &mut can_read) {
                        // Linux: `read` on the leader side can return
                        // EIO if the follower hangs up. Warp's
                        // `event_loop.rs` swallows it and waits for
                        // SIGCHLD. We do the same.
                        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                        if err.kind() == ErrorKind::Other {
                            continue;
                        }
                        log::error!("PTY event loop: read error: {err}");
                        let _ = self
                            .event_tx
                            .try_send(PtyEvent::Disconnected(format!("pty read error: {err}")));
                        break 'event_loop;
                    }
                }

                if state.needs_write() && can_write {
                    if let Err(err) = self.pty_write(&mut state, &mut can_write) {
                        log::error!("PTY event loop: write error: {err}");
                        let _ = self
                            .event_tx
                            .try_send(PtyEvent::Disconnected(format!("pty write error: {err}")));
                        break 'event_loop;
                    }
                }
            }

            if let Some(preserve_prompt_prefix) = state.pending_clear_visible_screen.take() {
                self.sink
                    .lock()
                    .clear_visible_screen(preserve_prompt_prefix);
                let _ = self.wakeup_tx.try_send(());
            }
        }

        let _ = self.poll.registry().deregister(&mut self.rx);
        let _ = self.poll.registry().deregister(&mut SourceFd(&self.pty_fd));
        let _ = self.poll.registry().deregister(&mut self.signals);

        // 非本进程发起关闭时，在本线程终止 PTY 进程（同 Warp `event_loop.rs`）。
        if !child_exited {
            if let Err(err) = self.killer.kill() {
                log::error!("Failed to kill PTY process {err:?}");
            }
            // Final notification mirrors Warp's `event_loop.rs`.
            self.sink
                .lock()
                .mark_disconnected("pty disconnected".to_string());
            let _ = self.wakeup_tx.try_send(());
        }
    }
}

/// Spawn the PTY event loop. Mirrors how `EventLoop::spawn` is called
/// from `warp/app/src/terminal/local_tty/terminal_manager.rs`.
#[cfg(unix)]
pub fn spawn_event_loop<S: PtySink>(
    sink: Arc<FairMutex<S>>,
    master: Box<dyn MasterPty + Send>,
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    killer: Box<dyn ChildKiller + Send + Sync>,
) -> io::Result<EventLoopHandle> {
    let pty_fd = master.as_raw_fd().ok_or_else(|| {
        io::Error::new(
            ErrorKind::Unsupported,
            "portable-pty master does not expose a raw fd on this platform",
        )
    })?;

    set_nonblocking(pty_fd)?;

    let pty_io = ManuallyDrop::new(unsafe { File::from_raw_fd(pty_fd) });

    let poll = Poll::new()?;

    let signals = Signals::new([SIGCHLD])?;

    let (message_tx, rx) = mio_channel::channel::<Message>();
    // Bounded(1) wakeup channel — coalesce multiple wakeups into one,
    // since the UI throttles at 60 Hz anyway. `async_channel::bounded(1)
    // .try_send` returns Full quietly when the slot is occupied, which
    // is exactly the coalesce-on-overflow behaviour Warp comments on in
    // `crates/warp_terminal/src/event_listener.rs`.
    let (wakeup_tx, wakeup_rx) = async_channel::bounded::<()>(1);
    let (event_tx, event_rx) = async_channel::unbounded::<PtyEvent>();

    let has_child_exited = Arc::new(parking_lot::Mutex::new(false));

    // Background thread to translate `child.wait()` into the SIGCHLD
    // observable from the main event loop. portable-pty's `Child` does
    // not expose `try_wait` on the same fd we mio-register, so we stand
    // up a small reaper; SIGCHLD still lands in the main poll, the
    // reaper just flips the `has_child_exited` flag the SIGCHLD handler
    // checks.
    {
        let has_child_exited = has_child_exited.clone();
        thread::Builder::new()
            .name("PTY child reaper".into())
            .spawn(move || {
                let _ = child.wait();
                *has_child_exited.lock() = true;
                // libc::raise on SIGCHLD so the main poll wakes up
                // even if SIGCHLD was already delivered before signals
                // got registered.
                unsafe {
                    libc::raise(SIGCHLD);
                }
            })
            .map_err(io::Error::other)?;
    }

    // EventLoop 在本线程退出后 kill；EventLoopHandle 保留一个独立 killer
    // 仅为不动 windows 构造/结构体签名（unix Drop 已不再用它）。
    let loop_killer = killer.clone_killer();
    let event_loop = EventLoop {
        poll,
        sink,
        pty_io,
        pty_fd,
        master,
        rx,
        signals,
        wakeup_tx: wakeup_tx.clone(),
        event_tx,
        has_child_exited,
        killer: loop_killer,
    };

    let thread = thread::Builder::new()
        .name("PTY event loop".into())
        .spawn(move || event_loop.run())
        .map_err(io::Error::other)?;

    Ok(EventLoopHandle {
        message_tx,
        weak_wakeup_tx: wakeup_tx.downgrade(),
        wakeup_rx,
        event_rx,
        thread: Some(thread),
        killer,
    })
}

#[cfg(windows)]
pub fn spawn_event_loop<S: PtySink>(
    sink: Arc<FairMutex<S>>,
    master: Box<dyn MasterPty + Send>,
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    killer: Box<dyn ChildKiller + Send + Sync>,
) -> io::Result<EventLoopHandle> {
    let mut reader = master.try_clone_reader().map_err(io::Error::other)?;
    let mut writer = master.take_writer().map_err(io::Error::other)?;

    let (message_tx, rx) = mio_channel::channel::<Message>();
    let (wakeup_tx, wakeup_rx) = async_channel::bounded::<()>(1);
    let (event_tx, event_rx) = async_channel::unbounded::<PtyEvent>();
    let shutdown = Arc::new(AtomicBool::new(false));

    {
        let sink = Arc::clone(&sink);
        let wakeup_tx = wakeup_tx.clone();
        let event_tx = event_tx.clone();
        let message_tx = message_tx.clone();
        let shutdown = Arc::clone(&shutdown);
        thread::Builder::new()
            .name("PTY reader".into())
            .spawn(move || {
                let mut buf = vec![0u8; READ_BUFFER_SIZE];
                let mut announced_sync_deadline = None;
                loop {
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }

                    match reader.read(&mut buf) {
                        Ok(0) => {
                            if !shutdown.swap(true, Ordering::Relaxed) {
                                sink.lock()
                                    .mark_disconnected("pty disconnected".to_string());
                                let _ = wakeup_tx.try_send(());
                                let _ = event_tx.try_send(PtyEvent::Disconnected(
                                    "pty disconnected".to_string(),
                                ));
                                let _ = message_tx.send(Message::Shutdown);
                            }
                            break;
                        }
                        Ok(got) => {
                            let (replies, sync_deadline) = {
                                let mut sink = sink.lock();
                                let replies = sink.process_output(&buf[..got]);
                                (replies, sink.sync_output_deadline())
                            };
                            for reply in replies {
                                let _ = message_tx.send(Message::Input(reply));
                            }
                            // 每段同步的截止时间都不同，只在出现新的一段时叫醒事件线程。
                            if sync_deadline.is_some() && sync_deadline != announced_sync_deadline {
                                let _ = message_tx.send(Message::SyncOutputStarted);
                            }
                            announced_sync_deadline = sync_deadline;
                            let _ = wakeup_tx.try_send(());
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(error) => {
                            if !shutdown.swap(true, Ordering::Relaxed) {
                                let status = format!("pty read error: {error}");
                                sink.lock().mark_disconnected(status.clone());
                                let _ = wakeup_tx.try_send(());
                                let _ = event_tx.try_send(PtyEvent::Disconnected(status));
                                let _ = message_tx.send(Message::Shutdown);
                            }
                            break;
                        }
                    }
                }
            })
            .map_err(io::Error::other)?;
    }

    {
        let sink = Arc::clone(&sink);
        let wakeup_tx = wakeup_tx.clone();
        let event_tx = event_tx.clone();
        let message_tx = message_tx.clone();
        let shutdown = Arc::clone(&shutdown);
        thread::Builder::new()
            .name("PTY child reaper".into())
            .spawn(move || {
                let _ = child.wait();
                if !shutdown.swap(true, Ordering::Relaxed) {
                    sink.lock()
                        .mark_disconnected("shell process exited".to_string());
                    let _ = wakeup_tx.try_send(());
                    let _ = event_tx.try_send(PtyEvent::ChildExited);
                    let _ = message_tx.send(Message::Shutdown);
                }
            })
            .map_err(io::Error::other)?;
    }

    let thread = {
        let sink = Arc::clone(&sink);
        let wakeup_tx = wakeup_tx.clone();
        let event_tx = event_tx.clone();
        let shutdown = Arc::clone(&shutdown);
        thread::Builder::new()
            .name("PTY event loop".into())
            .spawn(move || {
                'events: while let Ok(batch) = recv_or_finish_sync(&rx, &sink, &wakeup_tx) {
                    for msg in batch {
                        match msg {
                            Message::Input(input) => {
                                if let Err(error) = writer.write_all(input.as_ref()) {
                                    if !shutdown.swap(true, Ordering::Relaxed) {
                                        let status = format!("pty write error: {error}");
                                        sink.lock().mark_disconnected(status.clone());
                                        let _ = wakeup_tx.try_send(());
                                        let _ = event_tx.try_send(PtyEvent::Disconnected(status));
                                    }
                                    break 'events;
                                }
                            }
                            Message::Resize(size) => {
                                if let Err(error) = master.resize(size) {
                                    let _ = event_tx.try_send(PtyEvent::Disconnected(format!(
                                        "pty resize error: {error}"
                                    )));
                                } else {
                                    sink.lock().handle_resize(size);
                                }
                            }
                            Message::ClearVisibleScreen {
                                preserve_prompt_prefix,
                            } => {
                                sink.lock().clear_visible_screen(preserve_prompt_prefix);
                                let _ = wakeup_tx.try_send(());
                            }
                            Message::Shutdown => break 'events,
                            Message::SyncOutputStarted => {}
                        }
                    }

                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                }
                shutdown.store(true, Ordering::Relaxed);
            })
            .map_err(io::Error::other)?
    };

    Ok(EventLoopHandle {
        message_tx,
        weak_wakeup_tx: wakeup_tx.downgrade(),
        wakeup_rx,
        event_rx,
        thread: Some(thread),
        killer,
    })
}

/// Windows 事件线程取下一批消息。同步输出（`?2026h`）没收尾时最多等到截止时间，
/// 到点强制结束同步，解析器的应答当作输入消息交回去写进 PTY。
#[cfg(windows)]
fn recv_or_finish_sync<S: PtySink>(
    rx: &mio_channel::Receiver<Message>,
    sink: &FairMutex<S>,
    wakeup_tx: &async_channel::Sender<()>,
) -> Result<Vec<Message>, ()> {
    let Some(deadline) = sink.lock().sync_output_deadline() else {
        return rx.recv().map(|msg| vec![msg]).map_err(drop);
    };
    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(msg) => Ok(vec![msg]),
        Err(RecvTimeoutError::Timeout) => {
            let replies = sink.lock().finish_expired_sync_output(Instant::now());
            let Some(replies) = replies else {
                return Ok(Vec::new());
            };
            let _ = wakeup_tx.try_send(());
            Ok(replies.into_iter().map(Message::Input).collect())
        }
        Err(RecvTimeoutError::Disconnected) => Err(()),
    }
}

#[cfg(not(any(unix, windows)))]
pub fn spawn_event_loop<S: PtySink>(
    _sink: Arc<FairMutex<S>>,
    _master: Box<dyn MasterPty + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    _killer: Box<dyn ChildKiller + Send + Sync>,
) -> io::Result<EventLoopHandle> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NexShell local PTY event loop is not wired for this platform yet",
    ))
}

#[cfg(unix)]
fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // Same trick as `set_nonblocking` in `warp/crates/warp_terminal/src/local_tty/unix.rs`.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
