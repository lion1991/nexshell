//! RDP 会话运行时统计：协议线程累加，UI 侧只读差分算率。全原子无锁。
//! 速率不在此算（协议层不知刷新节奏），UI 每秒读一次做差分。macOS 专有。

use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// Arc 共享给 UI 的统计集。
pub struct RdpStats {
    /// read_pdu 成功累加的 payload 字节数（含 drain 循环）。
    bytes_received: AtomicU64,
    /// publish_frame 实发一帧 +1。
    frames_published: AtomicU64,
    /// 见到 FrameMarker 即置位：true=RemoteFX 帧边界，false=传统位图回退。
    marker_mode: AtomicBool,
    /// 渲染管线：0=legacy 位图，1=RemoteFX 帧边界，2=EGFX 图形管线。UI 面板据此显示。
    pipeline: AtomicU8,
    /// 连接建立时刻，算会话时长。
    connected_at: Instant,
    /// TLS 升级前对 TcpStream dup 的 fd，仅供 getsockopt 读 srtt；Drop 时 close。-1=无。
    raw_fd: AtomicI32,
}

impl RdpStats {
    pub fn new() -> Self {
        Self {
            bytes_received: AtomicU64::new(0),
            frames_published: AtomicU64::new(0),
            marker_mode: AtomicBool::new(false),
            pipeline: AtomicU8::new(0),
            connected_at: Instant::now(),
            raw_fd: AtomicI32::new(-1),
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
    pub fn capture_fd(&self, stream_fd: RawFd) {
        // SAFETY: dup 一个当前有效的 socket fd；失败返回 -1，由 rtt_ms 忽略。
        let dup = unsafe { libc::dup(stream_fd) };
        self.raw_fd.store(dup, Ordering::Relaxed);
    }

    /// 读 TCP 平滑 RTT（macOS `TCP_CONNECTION_INFO.tcpi_srtt`，单位 ms）。失败/无 fd 返回 None。
    pub fn rtt_ms(&self) -> Option<f64> {
        let fd = self.raw_fd.load(Ordering::Relaxed);
        if fd < 0 {
            return None;
        }
        // SAFETY: fd 为本会话 dup 的有效 TCP socket；info 为 POD、zeroed 合法，
        // len 恰为其尺寸，符合 getsockopt 契约。
        let info = unsafe {
            let mut info: libc::tcp_connection_info = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::tcp_connection_info>() as libc::socklen_t;
            let ret = libc::getsockopt(
                fd,
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
}

impl Drop for RdpStats {
    fn drop(&mut self) {
        let fd = self.raw_fd.load(Ordering::Relaxed);
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
