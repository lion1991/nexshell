//! RDP 会话运行时统计：协议线程累加，UI 侧只读差分算率。计数全是原子量；RTT 采样用的 fd
//! 放在锁里，重连换 fd 时不会和 UI 线程的读取交错。
//! 速率不在此算（协议层不知刷新节奏），UI 每秒读一次做差分。
//! TCP RTT 通过 macOS 原生 fd 采样；其他平台保留通用统计并返回无 RTT 样本。

#[cfg(target_os = "macos")]
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// Arc 共享给 UI 的统计集。
pub struct RdpStats {
    /// read_pdu 与 UDP 隧道收到的 payload 字节数（含 drain 循环）。
    bytes_received: AtomicU64,
    /// publish_frame 实发一帧 +1。
    frames_published: AtomicU64,
    /// 见到 FrameMarker 即置位：true=RemoteFX 帧边界，false=传统位图回退。
    marker_mode: AtomicBool,
    /// 渲染管线：0=legacy 位图，1=RemoteFX 帧边界，2=EGFX 图形管线。UI 面板据此显示。
    pipeline: AtomicU8,
    /// 已有动态通道经 Soft-Sync 迁到可靠 UDP 隧道（docs/adr/0014）。
    udp_active: AtomicBool,
    /// 连接建立时刻，算会话时长。
    connected_at: Instant,
    /// TLS 升级前对 TcpStream dup 的 fd，仅供 getsockopt 读 srtt；换新 fd 或 Drop 时 close。-1=无。
    #[cfg(target_os = "macos")]
    raw_fd: parking_lot::Mutex<RawFd>,
}

impl RdpStats {
    pub fn new() -> Self {
        Self {
            bytes_received: AtomicU64::new(0),
            frames_published: AtomicU64::new(0),
            marker_mode: AtomicBool::new(false),
            pipeline: AtomicU8::new(0),
            udp_active: AtomicBool::new(false),
            connected_at: Instant::now(),
            #[cfg(target_os = "macos")]
            raw_fd: parking_lot::Mutex::new(-1),
        }
    }

    pub fn add_bytes(&self, n: u64) {
        self.bytes_received.fetch_add(n, Ordering::Relaxed);
    }
    pub fn inc_frame(&self) {
        self.frames_published.fetch_add(1, Ordering::Relaxed);
    }
    pub fn set_marker_mode(&self) {
        self.marker_mode.store(true, Ordering::Relaxed);
        // 未升到 EGFX 时标记为 RemoteFX 帧边界（1）。
        let _ = self
            .pipeline
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed);
    }
    /// EGFX 能力协商成功：管线升到 EGFX（2），覆盖 legacy/marker。
    pub fn set_pipeline_egfx(&self) {
        self.pipeline.store(2, Ordering::Relaxed);
    }
    /// 渲染管线：0=legacy 位图，1=RemoteFX 帧边界，2=EGFX。
    pub fn pipeline(&self) -> u8 {
        self.pipeline.load(Ordering::Relaxed)
    }

    pub fn set_udp_active(&self) {
        self.udp_active.store(true, Ordering::Relaxed);
    }
    pub fn udp_active(&self) -> bool {
        self.udp_active.load(Ordering::Relaxed)
    }

    pub fn bytes(&self) -> u64 {
        self.bytes_received.load(Ordering::Relaxed)
    }
    pub fn frames(&self) -> u64 {
        self.frames_published.load(Ordering::Relaxed)
    }
    pub fn is_marker_mode(&self) -> bool {
        self.marker_mode.load(Ordering::Relaxed)
    }
    pub fn connected_at(&self) -> Instant {
        self.connected_at
    }

    /// TLS 升级前 dup 一份底层 TCP fd 存入（原 stream 照常被 TLS 吃掉）。失败存 -1。
    /// 整个会话共用一个 RdpStats，每次重连都会再调一次：先关掉上一轮的 dup，
    /// 否则漏一个 fd，旧连接的 socket 也被它吊着关不掉。
    #[cfg(target_os = "macos")]
    pub fn capture_fd(&self, stream_fd: RawFd) {
        // SAFETY: dup 一个当前有效的 socket fd；失败返回 -1，由 rtt_ms 忽略。
        let dup = unsafe { libc::dup(stream_fd) };
        let old = std::mem::replace(&mut *self.raw_fd.lock(), dup);
        if old >= 0 {
            // SAFETY: old 是上一轮 dup 出来、由本 stats 独占的 fd，已从锁里换出，只 close 这一次。
            unsafe {
                libc::close(old);
            }
        }
    }

    /// 读 TCP 平滑 RTT（macOS `TCP_CONNECTION_INFO.tcpi_srtt`，单位 ms）。失败/无 fd 返回 None。
    #[cfg(target_os = "macos")]
    pub fn rtt_ms(&self) -> Option<f64> {
        // 读的全程持锁，capture_fd 不会在中途把这个 fd 关掉、让编号被别的文件复用。
        let fd = self.raw_fd.lock();
        if *fd < 0 {
            return None;
        }
        // SAFETY: fd 为本会话 dup 的有效 TCP socket；info 为 POD、zeroed 合法，
        // len 恰为其尺寸，符合 getsockopt 契约。
        let info = unsafe {
            let mut info: libc::tcp_connection_info = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::tcp_connection_info>() as libc::socklen_t;
            let ret = libc::getsockopt(
                *fd,
                libc::IPPROTO_TCP,
                libc::TCP_CONNECTION_INFO,
                &mut info as *mut _ as *mut libc::c_void,
                &mut len,
            );
            if ret != 0 {
                return None;
            }
            info
        };
        Some(info.tcpi_srtt as f64)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn rtt_ms(&self) -> Option<f64> {
        None
    }
}

#[cfg(target_os = "macos")]
impl Drop for RdpStats {
    fn drop(&mut self) {
        let fd = *self.raw_fd.get_mut();
        if fd >= 0 {
            // SAFETY: fd 为本 stats 独占持有的 dup fd，仅此处 close 一次。
            unsafe {
                libc::close(fd);
            }
        }
    }
}

/// 接收字节差分 → Mbps（bit/s ÷ 1e6）。dt<=0 返回 0。
pub fn mbps(delta_bytes: u64, dt_secs: f64) -> f64 {
    if dt_secs <= 0.0 {
        return 0.0;
    }
    (delta_bytes as f64 * 8.0) / dt_secs / 1_000_000.0
}

/// 发布帧差分 → fps。dt<=0 返回 0。
pub fn fps(delta_frames: u64, dt_secs: f64) -> f64 {
    if dt_secs <= 0.0 {
        return 0.0;
    }
    delta_frames as f64 / dt_secs
}

/// 会话时长 → `H:MM:SS`（<1h 省时段为 `M:SS`）。
pub fn format_duration_hms(d: Duration) -> String {
    let total = d.as_secs();
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn capture_fd_again_releases_previous_dup() {
        use std::io::Read;
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;

        let stats = RdpStats::new();
        let (first, mut first_peer) = UnixStream::pair().expect("socketpair");
        first_peer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set_read_timeout");
        stats.capture_fd(first.as_raw_fd());
        drop(first);
        // 重连：换成第二条连接。第一条的 dup 若还开着，对端就读不到 EOF。
        let (second, _second_peer) = UnixStream::pair().expect("socketpair");
        stats.capture_fd(second.as_raw_fd());

        let mut buf = [0u8; 1];
        let n = first_peer
            .read(&mut buf)
            .expect("第一条连接应已全部关闭（读到 EOF），而不是超时");
        assert_eq!(n, 0);
    }

    #[test]
    fn mbps_diffs_over_one_second() {
        // 1_250_000 字节/s = 10 Mbps。
        assert!((mbps(1_250_000, 1.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn fps_diffs_over_interval() {
        assert!((fps(90, 1.5) - 60.0).abs() < 1e-9);
    }

    #[test]
    fn zero_or_negative_dt_is_zero() {
        assert_eq!(mbps(1000, 0.0), 0.0);
        assert_eq!(fps(30, -1.0), 0.0);
    }

    #[test]
    fn duration_formats_with_and_without_hours() {
        assert_eq!(format_duration_hms(Duration::from_secs(5)), "0:05");
        assert_eq!(format_duration_hms(Duration::from_secs(65)), "1:05");
        assert_eq!(format_duration_hms(Duration::from_secs(3661)), "1:01:01");
    }

    #[test]
    fn atomic_counters_accumulate() {
        let s = RdpStats::new();
        s.add_bytes(100);
        s.add_bytes(50);
        s.inc_frame();
        assert_eq!(s.bytes(), 150);
        assert_eq!(s.frames(), 1);
        assert!(!s.is_marker_mode());
        s.set_marker_mode();
        assert!(s.is_marker_mode());
    }
}
