//! MouseCursor DVC（MS-RDPEMSC）探测，NEXSHELL_RDP_MOUSE_CURSOR_PROBE=1 开启。
//! 接受服务端创建的通道但不发 CapsAdvertise，服务端照旧走 FastPath 指针，光标不受影响。
//! 只用来确认服务端是否创建该通道、是否经 Soft-Sync 迁到 UDP 隧道（看 IronRDP 的 Soft-Sync debug 日志）。

use ironrdp_core::impl_as_any;
use ironrdp_dvc::{DrdynvcClient, DvcClientProcessor, DvcMessage, DvcProcessor};
use ironrdp_pdu::PduResult;

const CHANNEL_NAME: &str = "Microsoft::Windows::RDS::MouseCursor";

fn enabled() -> bool {
    std::env::var("NEXSHELL_RDP_MOUSE_CURSOR_PROBE").is_ok_and(|v| v == "1")
}

pub(super) fn attach_probe(client: DrdynvcClient) -> DrdynvcClient {
    if !enabled() {
        return client;
    }
    eprintln!("[rdp-cursor] probing {CHANNEL_NAME}");
    client.with_dynamic_channel(MouseCursorProbe)
}

struct MouseCursorProbe;

impl_as_any!(MouseCursorProbe);

impl DvcProcessor for MouseCursorProbe {
    fn channel_name(&self) -> &str {
        CHANNEL_NAME
    }

    fn start(&mut self, channel_id: u32) -> PduResult<Vec<DvcMessage>> {
        eprintln!("[rdp-cursor] server opened {CHANNEL_NAME} channel_id={channel_id} (probe: no CapsAdvertise)");
        Ok(Vec::new())
    }

    fn process(&mut self, channel_id: u32, payload: &[u8]) -> PduResult<Vec<DvcMessage>> {
        eprintln!(
            "[rdp-cursor] channel_id={channel_id} {} bytes: {:02x?}",
            payload.len(),
            &payload[..payload.len().min(16)]
        );
        Ok(Vec::new())
    }

    fn close(&mut self, channel_id: u32) {
        eprintln!("[rdp-cursor] channel_id={channel_id} closed");
    }
}

impl DvcClientProcessor for MouseCursorProbe {}
