//! 自动重连（docs/plans/2026-10-10-suppress-output-mouse-cursor.md §3）：断开原因分类、
//! 跨重连保留的会话状态、心跳看门狗、睡眠唤醒信号。

use std::fmt;
use std::io;
use std::sync::OnceLock;
use std::time::Duration;

use ironrdp_connector::{ConnectorError, ConnectorErrorKind, NetworkCharacteristicsSync};
use ironrdp_pdu::rdp::autodetect::AutoDetectRequest;
use ironrdp_pdu::rdp::session_info::ServerAutoReconnect;
use tokio::sync::watch;
use tokio::time::Instant;

use super::RdpCertMismatch;

/// 重连次数上限，同 .rdp 文件 `autoreconnect max retries` 的默认值。
pub(super) const MAX_ATTEMPTS: u32 = 20;
/// 第一次立即重试，之后的间隔（参照 Windows App）。
pub(super) const RETRY_INTERVAL: Duration = Duration::from_secs(3);
/// 连接阶段（TCP → TLS → NLA → 激活）总时限；断网时读写会一直挂着，靠它跳到下一次尝试。
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
/// 连续这么多次走到 NLA 却没进活动期就停：服务端认证失败时可能直接断 TCP，
/// 看起来像网络错误，一直重试会锁账户。
pub(super) const MAX_LOGON_ATTEMPTS: u32 = 3;
/// 唤醒前后这么久内断开，算睡眠导致（UI 换提示语）。
const WAKE_WINDOW: Duration = Duration::from_secs(60);

/// 会话正常结束，不重连。
pub(super) enum SessionEnd {
    /// 本地关闭（关 tab、主动断开）。
    Closed,
    /// 服务端结束会话（Terminate）：注销、管理员断开、被别处登录挤掉等。
    Server(String),
}

impl fmt::Display for SessionEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("session ended"),
            Self::Server(reason) => write!(f, "terminated: {reason}"),
        }
    }
}

/// 会话出错。只有 Network 会自动重连：协议、认证、证书错误重试也没用，
/// 认证失败反复重试还可能锁账户。
#[derive(Debug)]
pub(super) enum SessionError {
    Network(String),
    Fatal(String),
    /// 证书与已信任指纹不符：致命，UI 单独展示并让用户决定是否信任。
    CertificateChanged(RdpCertMismatch),
}

impl From<String> for SessionError {
    fn from(reason: String) -> Self {
        Self::Fatal(reason)
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(reason) | Self::Fatal(reason) => f.write_str(reason),
            Self::CertificateChanged(m) => write!(
                f,
                "RDP server certificate for {} changed (expected {}, got {})",
                m.endpoint, m.expected, m.actual
            ),
        }
    }
}

/// map_err 用：标成网络类错误。
pub(super) fn net<E: fmt::Display>(what: &'static str) -> impl FnOnce(E) -> SessionError {
    move |e| SessionError::Network(format!("{what}: {e}"))
}

/// 读写错误：InvalidData 是帧解析失败（含 TLS 记录损坏），其余（断开、重置、超时）算网络类。
pub(super) fn io_error(what: &'static str) -> impl FnOnce(io::Error) -> SessionError {
    move |e| {
        let reason = format!("{what}: {e}");
        if e.kind() == io::ErrorKind::InvalidData {
            SessionError::Fatal(reason)
        } else {
            SessionError::Network(reason)
        }
    }
}

/// connector 错误：认证、协商失败算致命；错误链里有 I/O 错误的按 [`io_error`] 规则分。
pub(super) fn connector_error(what: &'static str) -> impl FnOnce(ConnectorError) -> SessionError {
    move |e| {
        if matches!(
            e.kind(),
            ConnectorErrorKind::Credssp(_)
                | ConnectorErrorKind::AccessDenied
                | ConnectorErrorKind::Negotiation(_)
        ) {
            return SessionError::Fatal(format!("{what}: {e}"));
        }
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&e);
        while let Some(error) = source {
            if let Some(io) = error.downcast_ref::<io::Error>() {
                if io.kind() != io::ErrorKind::InvalidData {
                    return SessionError::Network(format!("{what}: {e}"));
                }
            }
            source = error.source();
        }
        SessionError::Fatal(format!("{what}: {e}"))
    }
}

/// 给连接阶段的一步加总时限，超时算网络类错误。
pub(super) async fn within<T>(
    deadline: Instant,
    future: impl std::future::Future<Output = T>,
) -> Result<T, SessionError> {
    tokio::time::timeout_at(deadline, future)
        .await
        .map_err(|_| SessionError::Network("connection timed out".to_string()))
}

/// 跨重连保留的会话状态，只在内存：cookie 等同会话凭据（MS-RDPBCGR 5.5）。
#[derive(Default)]
pub(super) struct SessionMemo {
    /// 最近一次 Auto-Reconnect Cookie；服务端每次连接与每小时更换，被拒后丢弃。
    pub arc_cookie: Option<ServerAutoReconnect>,
    /// 服务端最近测得的带宽（kbps）与平均 RTT（ms），重连时经 NETCHAR_SYNC 回传。
    bandwidth_kbps: Option<u32>,
    rtt_ms: Option<u32>,
    /// 本轮连接进过活动期，重连计数据此清零。
    pub activated: bool,
    /// 曾进过活动期；首连失败不重连。
    pub ever_activated: bool,
    /// 上次进活动期以来走到 NLA 的次数。
    pub logon_attempts: u32,
    /// 最近一次系统唤醒。
    pub woke_at: Option<Instant>,
    /// 上次发出的远端光标 cache_key，连续同指针不重发。每轮连接清零。
    pub last_pointer_key: Option<u64>,
    /// 本轮连接的心跳看门狗。
    pub liveness: Liveness,
}

impl SessionMemo {
    /// 新一轮连接开始：清掉只属于上一条连接的状态。
    pub fn begin_connection(&mut self) {
        self.activated = false;
        self.last_pointer_key = None;
        self.liveness = Liveness::default();
    }

    pub fn on_activated(&mut self) {
        self.activated = true;
        self.ever_activated = true;
        self.logon_attempts = 0;
    }

    /// 记下服务端下发的网络特征结果。
    pub fn on_network_characteristics(&mut self, request: &AutoDetectRequest) {
        if let AutoDetectRequest::NetworkCharacteristicsResult {
            bandwidth_kbps,
            average_rtt_ms,
            ..
        } = request
        {
            if bandwidth_kbps.is_some() {
                self.bandwidth_kbps = *bandwidth_kbps;
            }
            self.rtt_ms = Some(*average_rtt_ms);
        }
    }

    /// 重连时回传的网络特征；带宽、RTT 都测到过才有。
    pub fn network_sync(&self) -> Option<NetworkCharacteristicsSync> {
        Some(NetworkCharacteristicsSync {
            bandwidth_kbps: self.bandwidth_kbps?,
            rtt_ms: self.rtt_ms?,
        })
    }

    /// 断开是否发生在系统唤醒前后。
    pub fn after_sleep(&self) -> bool {
        self.woke_at.is_some_and(|at| at.elapsed() < WAKE_WINDOW)
    }
}

/// 心跳看门狗：服务端声明心跳后，period × (count1 + count2) 秒内 TCP 上收不到任何数据即判定断链。
#[derive(Default)]
pub(super) struct Liveness {
    params: Option<(u8, u8, u8)>,
    period: Option<Duration>,
    timeout: Option<Duration>,
    deadline: Option<Instant>,
}

impl Liveness {
    /// 返回参数是否有变化（首个心跳也算）。
    pub fn on_heartbeat(&mut self, period: u8, count1: u8, count2: u8) -> bool {
        let changed =
            self.params.replace((period, count1, count2)) != Some((period, count1, count2));
        let missed = u64::from(count1) + u64::from(count2);
        let period = (period > 0).then(|| Duration::from_secs(u64::from(period)));
        self.period = period;
        self.timeout = period.filter(|_| missed > 0).map(|p| p * missed as u32);
        self.on_traffic();
        changed
    }

    pub fn on_traffic(&mut self) {
        self.deadline = self.timeout.map(|timeout| Instant::now() + timeout);
    }

    /// 唤醒后连接多半已断：一个心跳周期（加 2 s 余量）内没数据就重连，不等满整个超时。
    pub fn on_wake(&mut self) {
        let Some(period) = self.period else {
            return;
        };
        let soon = Instant::now() + period + Duration::from_secs(2);
        self.deadline = Some(self.deadline.map_or(soon, |deadline| deadline.min(soon)));
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }
}

fn wake_sender() -> &'static watch::Sender<()> {
    static WAKE: OnceLock<watch::Sender<()>> = OnceLock::new();
    WAKE.get_or_init(|| watch::channel(()).0)
}

/// 系统从睡眠唤醒（AppCallbacks::on_cpu_awakened 调）。
pub fn notify_system_wake() {
    wake_sender().send_replace(());
}

/// 订阅唤醒信号，只收订阅之后的。
pub(super) fn wake_events() -> watch::Receiver<()> {
    wake_sender().subscribe()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_errors_split_on_invalid_data() {
        let network = io_error("read")(io::Error::from(io::ErrorKind::ConnectionReset));
        assert!(matches!(network, SessionError::Network(_)));
        let fatal = io_error("read")(io::Error::from(io::ErrorKind::InvalidData));
        assert!(matches!(fatal, SessionError::Fatal(_)));
    }

    #[test]
    fn connector_errors_are_network_only_with_an_io_cause() {
        let io = ironrdp_connector::custom_err!(
            "read frame by hint",
            io::Error::from(io::ErrorKind::UnexpectedEof)
        );
        assert!(matches!(
            connector_error("connect")(io),
            SessionError::Network(_)
        ));
        let denied = ConnectorError::new("logon", ConnectorErrorKind::AccessDenied);
        assert!(matches!(
            connector_error("connect")(denied),
            SessionError::Fatal(_)
        ));
        let general = ironrdp_connector::general_err!("bad state");
        assert!(matches!(
            connector_error("connect")(general),
            SessionError::Fatal(_)
        ));
    }

    #[test]
    fn heartbeat_timeout_covers_both_counts() {
        let mut liveness = Liveness::default();
        assert!(liveness.deadline().is_none());
        let before = Instant::now();
        assert!(liveness.on_heartbeat(5, 2, 4));
        assert!(!liveness.on_heartbeat(5, 2, 4));
        let deadline = liveness.deadline().unwrap();
        assert!(deadline >= before + Duration::from_secs(30));
        assert!(deadline <= Instant::now() + Duration::from_secs(30));
    }

    #[test]
    fn wake_shortens_the_deadline_to_one_period() {
        let mut liveness = Liveness::default();
        liveness.on_heartbeat(5, 2, 4);
        liveness.on_wake();
        assert!(liveness.deadline().unwrap() <= Instant::now() + Duration::from_secs(7));
    }

    #[test]
    fn zero_period_disables_the_watchdog() {
        let mut liveness = Liveness::default();
        liveness.on_heartbeat(0, 2, 4);
        liveness.on_wake();
        assert!(liveness.deadline().is_none());
    }

    #[test]
    fn network_sync_needs_bandwidth_and_rtt() {
        let mut memo = SessionMemo::default();
        let rtt_only = AutoDetectRequest::NetworkCharacteristicsResult {
            sequence_number: 1,
            request_type: 0x0840,
            base_rtt_ms: Some(40),
            bandwidth_kbps: None,
            average_rtt_ms: 68,
        };
        memo.on_network_characteristics(&rtt_only);
        assert_eq!(memo.network_sync(), None);
        let all = AutoDetectRequest::NetworkCharacteristicsResult {
            sequence_number: 2,
            request_type: 0x08C0,
            base_rtt_ms: Some(40),
            bandwidth_kbps: Some(5000),
            average_rtt_ms: 70,
        };
        memo.on_network_characteristics(&all);
        memo.on_network_characteristics(&rtt_only);
        assert_eq!(
            memo.network_sync(),
            Some(NetworkCharacteristicsSync {
                bandwidth_kbps: 5000,
                rtt_ms: 68
            })
        );
    }
}
