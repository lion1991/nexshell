//! 剪贴板辅助进程的主进程一侧（ADR 0016 决策 5）：远端第一次复制时拉起辅助进程，
//! 把它转来的读取请求派给对应会话，再把会话从远端取回的数据写回去。

use std::collections::HashMap;
use std::io::{self, BufReader, BufWriter};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, LazyLock, Mutex};
use std::{env, thread};

use super::wire::{self, Data, Offer, Request, ToHelper};
use super::{lock, trace};

/// 带这个参数启动的进程进入辅助模式（见 main）。
pub const HELPER_ARG: &str = "--pasteboard-helper";

/// 远端内容由辅助进程延迟提供。
pub(super) const LAZY: bool = true;

struct Client {
    id: u64,
    tx: mpsc::Sender<ToHelper>,
}

static CLIENT: Mutex<Option<Client>> = Mutex::new(None);
static SESSIONS: LazyLock<Mutex<HashMap<u64, async_channel::Sender<Request>>>> =
    LazyLock::new(Default::default);

pub(super) fn offer(offer: Offer) -> bool {
    send(ToHelper::Offer(offer), true)
}

pub(super) fn reply(id: u64, data: Option<Data>) {
    send(ToHelper::Reply { id, data }, false);
}

/// start 为 false 时辅助进程不在就丢弃：应答和清空只对还活着的那个有意义。
fn send(msg: ToHelper, start: bool) -> bool {
    let mut client = lock(&CLIENT);
    let mut msg = msg;
    if let Some(c) = client.as_ref() {
        match c.tx.send(msg) {
            Ok(()) => return true,
            Err(mpsc::SendError(back)) => msg = back,
        }
    }
    *client = None;
    if !start {
        return false;
    }
    match spawn() {
        Ok(c) => {
            let sent = c.tx.send(msg).is_ok();
            *client = Some(c);
            sent
        }
        Err(e) => {
            eprintln!("[rdp-clip] start pasteboard helper failed: {e}");
            false
        }
    }
}

fn spawn() -> io::Result<Client> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut child = Command::new(env::current_exe()?)
        .arg(HELPER_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err(io::Error::other("helper pipes missing"));
    };
    let pid = child.id();
    let (tx, rx) = mpsc::channel::<ToHelper>();
    // 写失败说明辅助进程已退出；线程结束时丢掉 rx，下次 send 失败就重新拉起。
    thread::Builder::new()
        .name("nexshell-pb-write".into())
        .spawn(move || {
            let mut out = BufWriter::new(stdin);
            for msg in rx {
                if wire::write_to_helper(&mut out, &msg).is_err() {
                    break;
                }
            }
        })?;
    thread::Builder::new()
        .name("nexshell-pb-read".into())
        .spawn(move || {
            let mut input = BufReader::new(stdout);
            while let Ok(request) = wire::read_request(&mut input) {
                route(request);
            }
            let mut client = lock(&CLIENT);
            if client.as_ref().is_some_and(|c| c.id == id) {
                *client = None;
            }
            drop(client);
            let _ = child.wait();
            trace(|| format!("pasteboard helper {pid} exited"));
        })?;
    trace(|| format!("pasteboard helper {pid} started"));
    Ok(Client { id, tx })
}

fn route(request: Request) {
    trace(|| format!("helper wants {:?} #{}", request.kind, request.index));
    let delivered = lock(&SESSIONS)
        .get(&request.session)
        .is_some_and(|tx| tx.try_send(request).is_ok());
    if !delivered {
        reply(request.id, None);
    }
}

/// 一个会话收辅助进程读取请求的入口；会话结束时注销，剪贴板还是它写的就清空。
pub(in crate::rdp_session) struct PasteRequests {
    session: u64,
    rx: async_channel::Receiver<Request>,
}

impl PasteRequests {
    pub(super) fn register(session: u64) -> Self {
        let (tx, rx) = async_channel::unbounded();
        lock(&SESSIONS).insert(session, tx);
        Self { session, rx }
    }

    pub(in crate::rdp_session) async fn recv(&self) -> Option<Request> {
        self.rx.recv().await.ok()
    }
}

impl Drop for PasteRequests {
    fn drop(&mut self) {
        lock(&SESSIONS).remove(&self.session);
        // 已派发还没处理的请求也要应答，否则辅助进程一直等。
        while let Ok(request) = self.rx.try_recv() {
            reply(request.id, None);
        }
        send(
            ToHelper::Clear {
                session: self.session,
            },
            false,
        );
    }
}
