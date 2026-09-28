//! 可靠 UDP 旁路（MS-RDPEUDP2 + MS-RDPEMT，docs/adr/0014）。
//! 服务端发 Initiate Multitransport Request 后建 UDP+TLS 隧道；DVC 经 Soft-Sync 迁入后，
//! 该通道双向都走隧道。任何一步失败都回 E_ABORT，会话照常走 TCP。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ironrdp_async::FramedWrite;
use ironrdp_connector::MultitransportResult;
use ironrdp_dvc::pdu::SoftSyncTunnelType;
use ironrdp_dvc::DvcMessageBatch;
use ironrdp_pdu::gcc::MultiTransportFlags;
use ironrdp_pdu::rdp::multitransport::{
    MultitransportRequestPdu, MultitransportResponsePdu, RequestedProtocol,
};
use ironrdp_rdpemt::TunnelConfig;
use ironrdp_rdpeudp_tokio::{UdpTlsConfig, UdpTransport, UdpTransportConfig};
use ironrdp_session::ActiveStage;

use super::stats::RdpStats;

// 连接期握手是内联等待的：UDP 被防火墙挡时每次连接都要白等这么久，故远短于上游默认 10s。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);
const TUNNEL_TIMEOUT: Duration = Duration::from_secs(5);
// 证书走 TOFU 回调、不弹窗，无需上游为人工确认预留的 130s。
const TLS_TIMEOUT: Duration = Duration::from_secs(10);
/// 接收窗口 2^8=256 包（约 315 KB）。上游默认 64 包，RTT 150ms 时下行被卡在约 4 Mbps。
const LOG_RECV_WINDOW: u8 = 8;
/// 握手包须补零到协商 MTU；上游默认 1232 时 IP 包 1260 字节，过不了 MTU 1160 的隧道
/// （实测分片在途中被丢，表现为握手超时）。取协议下限 1132，代价是每包少约 100 字节载荷。
const UDP_MTU: u16 = 1132;
/// 版本 3（RDPEUDP2）的 MTU 固定 1232，与上面的 1132 冲突，实测 Windows 11 收到后不应答；
/// 版本 2 按协商 MTU 走 MS-RDPEUDP 可靠传输。
const UDP_VERSION: ironrdp_rdpeudp::pdu::v1_syn::UdpVersion =
    ironrdp_rdpeudp::pdu::v1_syn::UdpVersion::V2;

fn default_enable_udp_from_env(disable_udp: Option<std::ffi::OsString>) -> bool {
    disable_udp.is_none()
}

/// 可靠 UDP 默认开启；NEXSHELL_RDP_DISABLE_UDP=1 退回纯 TCP。
pub fn default_enable_udp() -> bool {
    default_enable_udp_from_env(std::env::var_os("NEXSHELL_RDP_DISABLE_UDP"))
}

/// GCC 多传输声明：只报可靠 UDP + Soft-Sync（有损 UDP/DTLS 上游未实现）。
pub(super) fn multitransport_flags(enable: bool) -> Option<MultiTransportFlags> {
    enable.then_some(
        MultiTransportFlags::TRANSPORT_TYPE_UDP_FECR | MultiTransportFlags::SOFT_SYNC_TCP_TO_UDP,
    )
}

#[derive(Debug, PartialEq, Eq)]
enum RequestDecision {
    Attempt,
    Reject(&'static str),
}

/// 同一协议只试一次；无 Soft-Sync 时 DVC 无法迁移，建了也用不上。
fn decide(
    attempted: &[RequestedProtocol],
    protocol: RequestedProtocol,
    soft_sync: bool,
) -> RequestDecision {
    if attempted.contains(&protocol) {
        RequestDecision::Reject("duplicate request")
    } else if !soft_sync {
        RequestDecision::Reject("server did not negotiate Soft-Sync")
    } else if protocol != RequestedProtocol::UdpFecR {
        RequestDecision::Reject("only reliable UDP is supported")
    } else {
        RequestDecision::Attempt
    }
}

pub(super) struct UdpSideband {
    peer: SocketAddr,
    server_name: String,
    tls: UdpTlsConfig,
    stats: Arc<RdpStats>,
    transport: Option<UdpTransport>,
    attempted: Vec<RequestedProtocol>,
    /// 服务端发完 Soft-Sync 请求即可能在 UDP 上发数据，而请求本身走 TCP、可能后到。
    /// 先到的这一条暂存并停读 UDP，等 Soft-Sync 生效后再处理，保证通道内顺序。
    pending: Option<Vec<u8>>,
}

impl UdpSideband {
    pub(super) fn new(
        peer: SocketAddr,
        server_name: String,
        tls: UdpTlsConfig,
        stats: Arc<RdpStats>,
    ) -> Self {
        Self {
            peer,
            server_name,
            tls,
            stats,
            transport: None,
            attempted: Vec::new(),
            pending: None,
        }
    }

    pub(super) fn has_transport(&self) -> bool {
        self.transport.is_some()
    }

    /// 处理一条多传输请求，返回应答结果与是否新建了隧道。
    pub(super) async fn handle_request(
        &mut self,
        request: &MultitransportRequestPdu,
        soft_sync: bool,
    ) -> (MultitransportResult, bool) {
        let protocol = request.requested_protocol;
        let decision = decide(&self.attempted, protocol, soft_sync);
        self.attempted.push(protocol);
        if let RequestDecision::Reject(reason) = decision {
            eprintln!("[rdp-udp] reject multitransport {protocol:?}: {reason}");
            return (
                MultitransportResult::Failure(MultitransportResponsePdu::E_ABORT),
                false,
            );
        }
        match self.connect(request).await {
            Ok(transport) => {
                eprintln!("[rdp-udp] reliable UDP tunnel established to {}", self.peer);
                self.transport = Some(transport);
                (MultitransportResult::Success, true)
            }
            Err(error) => {
                eprintln!("[rdp-udp] bootstrap failed, staying on TCP: {error}");
                (
                    MultitransportResult::Failure(MultitransportResponsePdu::E_ABORT),
                    false,
                )
            }
        }
    }

    async fn connect(
        &self,
        request: &MultitransportRequestPdu,
    ) -> Result<UdpTransport, ironrdp_rdpeudp_tokio::UdpTransportError> {
        let tunnel = TunnelConfig {
            request_id: request.request_id,
            security_cookie: request.security_cookie,
        };
        let mut config = UdpTransportConfig::new(self.peer, self.server_name.clone(), tunnel);
        config.connection_config.log_window_size = LOG_RECV_WINDOW;
        config.connection_config.upstream_mtu = UDP_MTU;
        config.connection_config.downstream_mtu = UDP_MTU;
        config.connection_config.offer_version = UDP_VERSION;
        config.handshake_timeout = HANDSHAKE_TIMEOUT;
        config.tunnel_timeout = TUNNEL_TIMEOUT;
        config.tls_timeout = TLS_TIMEOUT;
        config.tls = self.tls.clone();
        ironrdp_rdpeudp_tokio::connect_udp(config).await
    }

    /// 读下一条隧道数据；无隧道或有暂存时永不就绪（供 select! 常驻分支）。
    async fn recv(&mut self) -> Option<Vec<u8>> {
        match (self.transport.as_mut(), self.pending.is_none()) {
            (Some(transport), true) => transport.recv().await,
            _ => std::future::pending().await,
        }
    }

    /// 处理 recv 结果：已迁移则交给 DRDYNVC，返回待回发的批次。
    async fn on_recv(
        &mut self,
        active_stage: &mut ActiveStage,
        payload: Option<Vec<u8>>,
    ) -> Result<Option<DvcMessageBatch>, String> {
        let Some(payload) = payload else {
            let reason = self.close_reason().await;
            if active_stage.reliable_udp_dvc_tunnel_in_use() {
                return Err(format!("reliable UDP tunnel closed: {reason}"));
            }
            eprintln!("[rdp-udp] tunnel closed before Soft-Sync, staying on TCP: {reason}");
            active_stage
                .disable_reliable_udp_dvc_tunnel()
                .map_err(|e| format!("disable UDP tunnel failed: {e}"))?;
            return Ok(None);
        };
        if payload.is_empty() {
            return Ok(None);
        }
        self.stats.add_bytes(payload.len() as u64);
        if !active_stage.reliable_udp_dvc_tunnel_in_use() {
            self.pending = Some(payload);
            return Ok(None);
        }
        self.process(active_stage, &payload).map(Some)
    }

    /// 隧道已关：回收后台任务，取出驱动 / 泵的退出原因（recv 只给 None）。
    async fn close_reason(&mut self) -> String {
        let Some(transport) = self.transport.take() else {
            return "no transport".to_string();
        };
        match transport.shutdown().await {
            Ok(()) => "closed without error".to_string(),
            Err(error) => error.report().to_string(),
        }
    }

    /// Soft-Sync 生效后取出暂存数据处理。
    fn take_ready_pending(
        &mut self,
        active_stage: &mut ActiveStage,
    ) -> Result<Option<DvcMessageBatch>, String> {
        if self.pending.is_none() || !active_stage.reliable_udp_dvc_tunnel_in_use() {
            return Ok(None);
        }
        let payload = self.pending.take().unwrap_or_default();
        self.process(active_stage, &payload).map(Some)
    }

    fn process(
        &self,
        active_stage: &mut ActiveStage,
        payload: &[u8],
    ) -> Result<DvcMessageBatch, String> {
        self.stats.set_udp_active();
        active_stage
            .process_dvc_tunnel(SoftSyncTunnelType::RELIABLE_UDP, payload)
            .map_err(|e| {
                format!(
                    "process UDP tunnel data failed ({} bytes, header {:02x?}): {}",
                    payload.len(),
                    &payload[..payload.len().min(8)],
                    e.report()
                )
            })
    }
}

/// select! 分支用：未启用 UDP 时永不就绪。
pub(super) async fn recv(udp: &mut Option<UdpSideband>) -> Option<Vec<u8>> {
    match udp.as_mut() {
        Some(udp) => udp.recv().await,
        None => std::future::pending().await,
    }
}

/// 处理一次 UDP 收包并回发产生的 DVC 应答。
pub(super) async fn handle_recv<W: FramedWrite>(
    udp: &mut Option<UdpSideband>,
    active_stage: &mut ActiveStage,
    framed: &mut W,
    payload: Option<Vec<u8>>,
) -> Result<(), String> {
    let Some(sideband) = udp.as_mut() else {
        return Ok(());
    };
    if let Some(batch) = sideband.on_recv(active_stage, payload).await? {
        route_dvc_batch(udp.as_ref(), active_stage, framed, batch).await?;
    }
    Ok(())
}

/// 处理 Soft-Sync 前暂存的数据；有处理返回 true。
pub(super) async fn drain_pending<W: FramedWrite>(
    udp: &mut Option<UdpSideband>,
    active_stage: &mut ActiveStage,
    framed: &mut W,
) -> Result<bool, String> {
    let Some(sideband) = udp.as_mut() else {
        return Ok(false);
    };
    let Some(batch) = sideband.take_ready_pending(active_stage)? else {
        return Ok(false);
    };
    route_dvc_batch(udp.as_ref(), active_stage, framed, batch).await?;
    Ok(true)
}

/// DVC 批次按通道所在隧道分流：已迁 UDP 的走隧道（无帧编码），其余照旧走 TCP。
pub(super) async fn route_dvc_batch<W: FramedWrite>(
    udp: Option<&UdpSideband>,
    active_stage: &mut ActiveStage,
    framed: &mut W,
    batch: DvcMessageBatch,
) -> Result<(), String> {
    let channel_id = batch.channel_id();
    let messages = batch.into_messages();
    if active_stage.dvc_tunnel_for_channel(channel_id) == Some(SoftSyncTunnelType::RELIABLE_UDP) {
        let transport = udp
            .and_then(|u| u.transport.as_ref())
            .ok_or_else(|| "reliable UDP tunnel unavailable for a Soft-Sync channel".to_string())?;
        for message in messages {
            let payload = message
                .encode_unframed_pdu()
                .map_err(|e| format!("encode tunneled DVC message failed: {e}"))?;
            transport
                .send(payload)
                .await
                .map_err(|e| format!("write UDP tunnel failed: {e}"))?;
        }
        return Ok(());
    }
    let frame = active_stage
        .encode_dvc_messages(messages)
        .map_err(|e| format!("encode DVC messages failed: {e}"))?;
    framed
        .write_all(&frame)
        .await
        .map_err(|e| format!("write DVC messages failed: {e}"))
}

/// 会话期到达的多传输请求：尝试建隧道（未启用 UDP 则直接拒绝），按需在 TCP 上应答。
pub(super) async fn answer_requests<W: FramedWrite>(
    udp: &mut Option<UdpSideband>,
    active_stage: &mut ActiveStage,
    framed: &mut W,
    requests: Vec<MultitransportRequestPdu>,
    soft_sync: bool,
) -> Result<(), String> {
    for request in requests {
        let (result, established) = match udp.as_mut() {
            Some(sideband) => sideband.handle_request(&request, soft_sync).await,
            None => (
                MultitransportResult::Failure(MultitransportResponsePdu::E_ABORT),
                false,
            ),
        };
        if let Some(response) = result.response_pdu(request.request_id, soft_sync) {
            let frame = active_stage
                .encode_multitransport_response(&response)
                .map_err(|e| format!("encode multitransport response failed: {e}"))?;
            framed
                .write_all(&frame)
                .await
                .map_err(|e| format!("write multitransport response failed: {e}"))?;
        }
        if established {
            active_stage
                .enable_reliable_udp_dvc_tunnel()
                .map_err(|e| format!("enable UDP tunnel failed: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_gate_disables_udp_only_when_set() {
        assert!(default_enable_udp_from_env(None));
        assert!(!default_enable_udp_from_env(Some("1".into())));
    }

    #[test]
    fn flags_advertise_reliable_udp_with_soft_sync() {
        assert_eq!(multitransport_flags(false), None);
        let flags = multitransport_flags(true).unwrap();
        assert!(flags.contains(MultiTransportFlags::TRANSPORT_TYPE_UDP_FECR));
        assert!(flags.contains(MultiTransportFlags::SOFT_SYNC_TCP_TO_UDP));
        assert!(!flags.contains(MultiTransportFlags::TRANSPORT_TYPE_UDP_PREFERRED));
    }

    #[test]
    fn decide_attempts_reliable_udp_once_with_soft_sync() {
        let fec_r = RequestedProtocol::UdpFecR;
        assert_eq!(decide(&[], fec_r, true), RequestDecision::Attempt);
        assert!(matches!(
            decide(&[fec_r], fec_r, true),
            RequestDecision::Reject(_)
        ));
        assert!(matches!(
            decide(&[], fec_r, false),
            RequestDecision::Reject(_)
        ));
        assert!(matches!(
            decide(&[], RequestedProtocol::UdpFecL, true),
            RequestDecision::Reject(_)
        ));
    }
}
