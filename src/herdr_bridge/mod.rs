//! herdr（终端多路复用器）桥接：跟随本 tab 里那个 herdr client 的焦点
//! workspace 的 cwd，供 git / 文件面板使用。
//!
//! nexshell 的 pty 子进程只是 herdr **client**，真实 shell 挂在 herdr server
//! 下、进程树不相连，拿不到 cwd，只能走 Unix socket 问 server。
//!
//! # 为什么不用 `pane.current`
//! herdr 的焦点是 **per-client** 的（官方 concepts：多 client 时各看各的
//! workspace 和 tab），而 socket API 的 `pane.current` / `focused_*` 只反映
//! 「前台 client」。用户在别的终端里再挂一个 herdr client，nexshell 这个
//! client 切 workspace 时 server 的全局焦点根本不动，也没有任何 per-client
//! 查询接口。
//!
//! # 标题反查方案
//! herdr client 会往宿主 pty 写 OSC 0/2 窗口标题，默认模板
//! `{hostname}: {workspace}`，nexshell 已经把它解析进
//! `TerminalRuntimeState.title`。所以本模块拉 `session.snapshot` 建索引，
//! 由 `resolve_cwd(title)` 用标题里的 workspace label 反查 workspace，
//! 再取该 workspace 焦点 pane 的 cwd。
//!
//! # 已知局限
//! - **label 可能重名**：label 是目录 basename，本机就有两个 `impl-tools`。
//!   重名时先看这些 workspace 的焦点目录是不是同一个——同一个仓库开多个
//!   workspace 时目录往往一样，那就没有歧义、直接用；只有**目录确实不同**
//!   才回退 server 全局焦点。
//! - **依赖默认标题模板**：用户把 `ui.window_title` 改成不含 `{workspace}`
//!   时反查不出来，同样回退全局焦点（行为退化成改造前，不会更差）。
//! - **只支持单一 socket 路径**：`HERDR_SOCKET_PATH` → `~/.config/herdr/herdr.sock`。
//!   命名 session / 多 server 实例尚未支持。

pub mod client;
pub mod focus_state;
pub mod protocol;
pub mod snapshot;

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use focus_state::{event_marks_dirty, next_backoff, should_fetch_now, MIN_FETCH_INTERVAL};
use protocol::Line;
use snapshot::SnapshotIndex;

/// snapshot 变化时用来戳 UI 重绘的回调（tab 的 pty wakeup sender）。
pub type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Leases {
    next_id: u64,
    wakers: HashMap<u64, Option<Waker>>,
}

pub struct HerdrBridge {
    index: Mutex<Option<SnapshotIndex>>,
    /// snapshot 内容每变一次 +1。
    generation: AtomicU64,
    leases: Mutex<Leases>,
    /// 每次启动后台线程 +1；线程只在 epoch 未变时写状态，避免旧线程回写。
    epoch: AtomicU64,
    worker: Mutex<Option<WorkerHandle>>,
    /// 测试用非单例实例置 false：只跑引用计数逻辑，不产生任何真实 I/O。
    spawn_worker: bool,
}

struct WorkerHandle {
    /// 这一轮 worker 的 epoch。不变量：**slot 为 Some 时它必须等于
    /// `HerdrBridge::epoch`**——两者只在 worker 锁的同一临界区里一起改。
    epoch: u64,
    stop: Arc<StopSignal>,
    shutdown: Arc<Mutex<client::ShutdownHandle>>,
}

/// 可打断的休眠信号：stop 置位即唤醒退避中的线程。
#[derive(Default)]
struct StopSignal {
    flag: AtomicBool,
    lock: Mutex<()>,
    cv: Condvar,
}

impl StopSignal {
    fn stop(&self) {
        self.flag.store(true, Ordering::SeqCst);
        let _guard = self.lock.lock();
        self.cv.notify_all();
    }

    fn stopped(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// 睡 `dur`，期间被 stop 打断则提前返回。用 `wait_timeout_while` 而非裸
    /// `wait_timeout`：stop 若在拿锁之前就置位，谓词会立刻为假直接返回，
    /// 不会丢掉那次唤醒。
    fn sleep(&self, dur: Duration) {
        let Ok(guard) = self.lock.lock() else { return };
        let _ = self
            .cv
            .wait_timeout_while(guard, dur, |_| !self.flag.load(Ordering::SeqCst));
    }
}

/// RAII 租约：drop 时自动 release，tab 关闭 / runtime drop 不会漏计数。
pub struct HerdrLease {
    bridge: &'static HerdrBridge,
    id: u64,
}

impl Drop for HerdrLease {
    fn drop(&mut self) {
        self.bridge.release(self.id);
    }
}

impl HerdrBridge {
    fn new(spawn_worker: bool) -> Self {
        Self {
            index: Mutex::new(None),
            generation: AtomicU64::new(0),
            leases: Mutex::new(Leases::default()),
            epoch: AtomicU64::new(0),
            worker: Mutex::new(None),
            spawn_worker,
        }
    }

    pub fn global() -> &'static HerdrBridge {
        static INSTANCE: OnceLock<HerdrBridge> = OnceLock::new();
        INSTANCE.get_or_init(|| Self::new(true))
    }

    /// 独立实例（不启后台线程、不碰 socket），只给单测用。
    #[cfg(test)]
    fn leaked_for_test() -> &'static HerdrBridge {
        Box::leak(Box::new(Self::new(false)))
    }

    /// 登记一个使用者；首个使用者启动后台线程。`waker` 用于 snapshot 变化时戳 UI。
    pub fn acquire(&'static self, waker: Option<Waker>) -> HerdrLease {
        let Ok(mut leases) = self.leases.lock() else {
            return HerdrLease {
                bridge: self,
                id: 0,
            };
        };
        leases.next_id += 1;
        let id = leases.next_id;
        leases.wakers.insert(id, waker);
        let first = leases.wakers.len() == 1;
        drop(leases);
        if first {
            self.start_worker();
        }
        HerdrLease { bridge: self, id }
    }

    fn release(&'static self, id: u64) {
        let empty = {
            let Ok(mut leases) = self.leases.lock() else {
                return;
            };
            leases.wakers.remove(&id);
            leases.wakers.is_empty()
        };
        if empty {
            self.stop_worker();
            self.clear_index();
        }
    }

    /// 本 tab 该跟随的 cwd。`title` 是该 tab 的终端标题（herdr client 写的
    /// OSC 0/2），用来反查它当前在哪个 workspace。
    pub fn resolve_cwd(&self, title: Option<&str>) -> Option<PathBuf> {
        self.index.lock().ok()?.as_ref()?.resolve_cwd(title)
    }

    /// snapshot 内容每变一次 +1。
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    fn set_index(&self, next: SnapshotIndex) {
        {
            let Ok(mut slot) = self.index.lock() else {
                return;
            };
            if slot.as_ref() == Some(&next) {
                return;
            }
            *slot = Some(next);
        }
        self.generation.fetch_add(1, Ordering::Relaxed);
        log::debug!("herdr bridge: snapshot updated");
        self.wake_all();
    }

    fn clear_index(&self) {
        let changed = match self.index.lock() {
            Ok(mut slot) => slot.take().is_some(),
            Err(_) => return,
        };
        if changed {
            self.generation.fetch_add(1, Ordering::Relaxed);
            log::debug!("herdr bridge: snapshot cleared");
            self.wake_all();
        }
    }

    fn wake_all(&self) {
        let wakers: Vec<Waker> = match self.leases.lock() {
            Ok(leases) => leases.wakers.values().flatten().cloned().collect(),
            Err(_) => return,
        };
        for waker in wakers {
            waker();
        }
    }

    /// 不变量：**`epoch` 的每一次推进都在 `worker` 锁的临界区内完成**（这里、
    /// `stop_worker`、`clear_worker_slot` 三处）。否则 stop/start 交错时可能出现
    /// 「新线程拿到的 epoch 已被旧的 stop 推走」，slot 被置空却没人再启动，
    /// 或者旧 slot 永久残留。
    fn start_worker(&'static self) {
        let Ok(mut slot) = self.worker.lock() else {
            return;
        };
        if slot.is_some() {
            return;
        }
        let epoch = self.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let stop = Arc::new(StopSignal::default());
        let shutdown = Arc::new(Mutex::new(client::ShutdownHandle::default()));
        *slot = Some(WorkerHandle {
            epoch,
            stop: Arc::clone(&stop),
            shutdown: Arc::clone(&shutdown),
        });
        // 测试实例走完全一样的 slot/epoch 簿记，只是不真起线程、不碰 socket。
        if !self.spawn_worker {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("herdr-bridge".to_string())
            .spawn(move || {
                // worker 里 panic 也必须把 slot 清掉，否则永远不再重启。
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    self.worker_loop(epoch, stop, shutdown)
                }));
                if result.is_err() {
                    log::debug!("herdr bridge: worker panicked");
                }
                self.clear_worker_slot(epoch);
            });
        if spawned.is_err() {
            log::debug!("herdr bridge: failed to spawn worker thread");
            *slot = None;
        }
    }

    /// 线程退出（正常或 panic）时收回 slot；epoch 已被别人推进则说明
    /// slot 已归新一轮所有，不能动。epoch 的读与 slot 的写在同一临界区。
    fn clear_worker_slot(&self, epoch: u64) {
        let Ok(mut slot) = self.worker.lock() else {
            return;
        };
        // 只清自己那一轮的 slot：epoch 不同说明期间已被 stop + 重新 start，
        // 现在这个 slot 归新一轮所有。判定与写入同临界区。
        if slot.as_ref().is_some_and(|handle| handle.epoch == epoch) {
            *slot = None;
        }
    }

    fn stop_worker(&self) {
        let handle = {
            let Ok(mut slot) = self.worker.lock() else {
                return;
            };
            let handle = slot.take();
            // 推进 epoch 必须与 slot.take() 同临界区：否则并发的 start_worker 可能
            // 在两步之间插进来占住 slot，随后被这次 fetch_add 作废，slot 永久残留。
            // 推进后即使旧线程还在跑（比如卡在请求超时里）也不会再回写。
            self.epoch.fetch_add(1, Ordering::SeqCst);
            handle
        };
        if let Some(handle) = handle {
            handle.stop.stop();
            if let Ok(shutdown) = handle.shutdown.lock() {
                shutdown.shutdown();
            }
        }
    }

    fn alive(&self, epoch: u64, stop: &StopSignal) -> bool {
        !stop.stopped() && self.epoch.load(Ordering::SeqCst) == epoch
    }

    fn worker_loop(
        &'static self,
        epoch: u64,
        stop: Arc<StopSignal>,
        shutdown: Arc<Mutex<client::ShutdownHandle>>,
    ) {
        let mut backoff: Option<Duration> = None;
        while self.alive(epoch, &stop) {
            let opened_at = self.run_session(epoch, &stop, &shutdown);
            if !self.alive(epoch, &stop) {
                break;
            }
            let delay = next_backoff(backoff, opened_at, Instant::now());
            stop.sleep(delay);
            backoff = Some(delay);
        }
        log::debug!("herdr bridge: worker exit");
    }

    /// 一次完整会话：拉一次 snapshot → 订阅 → 事件标脏后重拉。
    /// 返回订阅建立的时刻（没连上则 None）。
    fn run_session(
        &self,
        epoch: u64,
        stop: &StopSignal,
        shutdown: &Arc<Mutex<client::ShutdownHandle>>,
    ) -> Option<Instant> {
        let path = protocol::resolve_socket_path()?;

        // 启动 / 重连后先拉一次：订阅没有事件回放。
        if !self.fetch_snapshot(epoch, &path) {
            return None;
        }
        let mut last_fetch = Instant::now();

        let mut sub = match client::Subscription::open(&path, &protocol::encode_subscribe("sub")) {
            Ok(sub) => sub,
            Err(error) => {
                log::debug!("herdr bridge: subscribe failed: {error}");
                return None;
            }
        };
        let opened_at = Instant::now();
        log::debug!("herdr bridge: subscribed to {}", path.display());
        if let Ok(mut slot) = shutdown.lock() {
            *slot = sub.shutdown_handle();
        }

        // 同一批连发的事件先攒进 dirty，缓冲读空后只拉一次 snapshot；再加
        // MIN_FETCH_INTERVAL 频率闸挡住稀疏但持续的事件流。
        let mut dirty = false;
        while self.alive(epoch, stop) {
            let Some(line) = sub.next_line() else {
                log::debug!("herdr bridge: subscription closed");
                break;
            };
            if let Some(Line::Event(event)) = protocol::parse_line(&line) {
                if event_marks_dirty(&event) {
                    dirty = true;
                }
            }
            if !dirty || sub.has_buffered_data() {
                continue;
            }
            let elapsed = last_fetch.elapsed();
            if !should_fetch_now(dirty, false, elapsed) {
                // 还没到间隔：睡掉剩余时间。期间到达的事件留在 socket 缓冲里，
                // 醒来后一并并进这一次拉取（dirty 仍为 true，不会漏）。
                if let Some(wait) = MIN_FETCH_INTERVAL.checked_sub(elapsed) {
                    stop.sleep(wait);
                }
                if !self.alive(epoch, stop) {
                    break;
                }
            }
            self.fetch_snapshot(epoch, &path);
            last_fetch = Instant::now();
            dirty = false;
        }
        Some(opened_at)
    }

    /// 拉一次 `session.snapshot` 并替换索引。返回是否拿到响应。
    fn fetch_snapshot(&self, epoch: u64, path: &Path) -> bool {
        let line = protocol::encode_session_snapshot("snapshot");
        match client::request(path, &line, "session.snapshot") {
            Ok(response) => {
                if let Some(value) = protocol::snapshot_from_response(&response) {
                    let index = SnapshotIndex::from_json(value);
                    // epoch 变了说明这轮线程已被弃用，不许回写。
                    if self.epoch.load(Ordering::SeqCst) == epoch {
                        self.set_index(index);
                    }
                }
                true
            }
            Err(error) => {
                log::debug!("herdr bridge: session.snapshot failed: {error}");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("testdata/session_snapshot.json");

    fn index() -> SnapshotIndex {
        let envelope: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        SnapshotIndex::from_json(&envelope["result"]["snapshot"])
    }

    /// 引用计数用独立实例跑：不启后台线程、不连 socket，零真实 I/O。
    #[test]
    fn lease_refcount_returns_to_zero_without_io() {
        let bridge = HerdrBridge::leaked_for_test();
        let a = bridge.acquire(None);
        let b = bridge.acquire(None);
        assert_eq!(bridge.leases.lock().unwrap().wakers.len(), 2);
        // 首个租约占住 slot（测试实例不真起线程）。
        assert!(bridge.worker.lock().unwrap().is_some());
        drop(a);
        assert_eq!(bridge.leases.lock().unwrap().wakers.len(), 1);
        drop(b);
        assert_eq!(bridge.leases.lock().unwrap().wakers.len(), 0);
        // 归零后 slot 必须交还，否则下次 acquire 起不来。
        assert!(bridge.worker.lock().unwrap().is_none());
        assert_eq!(bridge.resolve_cwd(Some("host: nexshell")), None);
    }

    /// 不变量回归：**slot 为 Some 时其 epoch 必须等于 bridge 的 epoch**。
    /// 若 `stop_worker` 把 epoch 推进挪出 worker 锁，并发 start/stop 交错就会
    /// 出现「slot 里的 worker 已被作废却没人清」——那台 bridge 从此再也起不来
    /// （后续 acquire 看到 slot.is_some() 直接返回）。
    ///
    /// 每个线程都是「拿一个租约就立刻放掉」，refcount 频繁在 0/1 之间跳，
    /// 于是 start_worker 与 stop_worker 高频并发；两处不变量各查一次。
    #[test]
    fn concurrent_acquire_release_never_strands_the_worker_slot() {
        let bridge = HerdrBridge::leaked_for_test();
        let threads: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(move || {
                    for _ in 0..20_000 {
                        let lease = bridge.acquire(None);
                        if let Some(handle) = bridge.worker.lock().unwrap().as_ref() {
                            assert_eq!(
                                handle.epoch,
                                bridge.epoch.load(Ordering::SeqCst),
                                "worker slot outlived its epoch"
                            );
                        }
                        drop(lease);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().expect("worker churn thread");
        }
        assert_eq!(bridge.leases.lock().unwrap().wakers.len(), 0);
        // 没有租约了，slot 必须已交还——残留就说明上面的交错发生过。
        assert!(
            bridge.worker.lock().unwrap().is_none(),
            "slot stranded after concurrent churn"
        );
        // 交还后还能正常再起一轮。
        let lease = bridge.acquire(None);
        assert!(bridge.worker.lock().unwrap().is_some());
        drop(lease);
        assert!(bridge.worker.lock().unwrap().is_none());
    }

    #[test]
    fn releasing_the_last_lease_clears_the_index_and_wakes() {
        let bridge = HerdrBridge::leaked_for_test();
        let hits = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&hits);
        let lease = bridge.acquire(Some(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })));

        bridge.set_index(index());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(bridge.generation(), 1);
        assert_eq!(
            bridge.resolve_cwd(Some("Matts-MacBook-Pro: taskops")),
            Some(PathBuf::from("/Users/matt/SynologyDrive/code/work/taskops"))
        );

        // 内容没变就不推 generation、不唤醒（herdr 事件很吵）。
        bridge.set_index(index());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(bridge.generation(), 1);

        drop(lease);
        assert_eq!(bridge.resolve_cwd(Some("host: taskops")), None);
        assert_eq!(bridge.generation(), 2);
    }

    /// 没有 snapshot 时任何查询都返回 None，不 panic。
    #[test]
    fn resolve_without_a_snapshot_is_none() {
        let bridge = HerdrBridge::leaked_for_test();
        assert_eq!(bridge.resolve_cwd(None), None);
        assert_eq!(bridge.resolve_cwd(Some("host: nexshell")), None);
    }

    /// 事件 → 标脏 → 合并拉取，这条链路的判定组合。
    #[test]
    fn event_to_dirty_to_coalesced_fetch() {
        use protocol::EventEnvelope;

        const LONG_AGO: Duration = Duration::from_secs(5);

        let mut dirty = false;
        let batch = [
            ("workspace_focused", true),
            ("tab_focused", true),
            ("pane_agent_status_changed", false),
        ];
        // 前两条同批到达（缓冲里还有数据）→ 只标脏不拉取。
        for (kind, marks) in batch {
            let ev = EventEnvelope {
                event: kind.to_string(),
                data_type: kind.to_string(),
            };
            assert_eq!(event_marks_dirty(&ev), marks, "{kind}");
            if event_marks_dirty(&ev) {
                dirty = true;
            }
            assert!(!should_fetch_now(dirty, true, LONG_AGO));
        }
        // 缓冲读空且间隔已过 → 这一批只拉一次。
        assert!(dirty);
        assert!(!should_fetch_now(dirty, false, Duration::ZERO));
        assert!(should_fetch_now(dirty, false, LONG_AGO));
        dirty = false;
        // 之后只来无关事件 → 不再拉。
        let ev = EventEnvelope {
            event: "pane_output_matched".to_string(),
            data_type: "pane_output_matched".to_string(),
        };
        assert!(!event_marks_dirty(&ev));
        assert!(!should_fetch_now(dirty, false, LONG_AGO));
    }
}
