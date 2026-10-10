//! Suppress Output（MS-RDPBCGR 2.2.11.3.1）：画面不可见时让服务端停发图形更新，
//! 恢复时带桌面矩形重新放行，这条 PDU 本身会触发整屏刷新。
//! 只在服务端 General 能力集声明 suppressOutputSupport 时发。

use ironrdp_async::FramedWrite;
use ironrdp_core::WriteBuf;
use ironrdp_pdu::geometry::InclusiveRectangle;
use ironrdp_pdu::rdp::headers::ShareDataPdu;
use ironrdp_pdu::rdp::suppress_output::SuppressOutputPdu;
use ironrdp_session::ActiveStage;

use super::reconnect::{io_error, SessionError};

/// allow=true 时 desktopRect 按规范填含端点矩形（宽 − 1、高 − 1）；
/// Windows App 填宽、高，Windows 服务端两种都收。
fn pdu(visible: bool, width: u16, height: u16) -> SuppressOutputPdu {
    SuppressOutputPdu {
        desktop_rect: visible.then(|| InclusiveRectangle {
            left: 0,
            top: 0,
            right: width.saturating_sub(1),
            bottom: height.saturating_sub(1),
        }),
    }
}

pub(super) async fn send<W: FramedWrite>(
    active_stage: &ActiveStage,
    framed: &mut W,
    visible: bool,
    width: u16,
    height: u16,
) -> Result<(), SessionError> {
    eprintln!(
        "[rdp] suppress output: {}",
        if visible { "allow" } else { "suppress" }
    );
    let mut buf = WriteBuf::new();
    active_stage
        .encode_static(
            &mut buf,
            ShareDataPdu::SuppressOutput(pdu(visible, width, height)),
        )
        .map_err(|e| format!("encode suppress output failed: {e}"))?;
    framed
        .write_all(buf.filled())
        .await
        .map_err(io_error("write suppress output failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suppress_has_no_rect_and_allow_uses_an_inclusive_rect() {
        assert_eq!(pdu(false, 1920, 1080).desktop_rect, None);
        assert_eq!(
            pdu(true, 1920, 1080).desktop_rect,
            Some(InclusiveRectangle {
                left: 0,
                top: 0,
                right: 1919,
                bottom: 1079,
            })
        );
    }
}
