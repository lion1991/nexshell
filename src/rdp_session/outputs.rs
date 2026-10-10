//! ActiveStage 输出处理：写回应答、累积脏区、指针事件、服务端结束与心跳等会话信号。

use std::sync::Arc;

use ironrdp_async::FramedWrite;
use ironrdp_pdu::geometry::InclusiveRectangle;
use ironrdp_session::image::DecodedImage;
use ironrdp_session::ActiveStageOutput;
use parking_lot::Mutex;

use super::reconnect::{io_error, SessionEnd, SessionError, SessionMemo};
use super::stats::RdpStats;
use super::{DirtyRect, RdpEvent, RdpFramebuffer, RdpPointer};

/// InclusiveRectangle（右/下含端）→ DirtyRect，clamp 到桌面尺寸。
pub(super) fn inclusive_to_dirty(rect: &InclusiveRectangle, max_w: u16, max_h: u16) -> DirtyRect {
    if max_w == 0 || max_h == 0 {
        return DirtyRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }
    let left = rect.left.min(max_w - 1);
    let top = rect.top.min(max_h - 1);
    let right = rect.right.min(max_w - 1).max(left);
    let bottom = rect.bottom.min(max_h - 1).max(top);
    DirtyRect {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    }
}

/// 两脏矩形的包围盒（累积一逻辑帧内多个 GraphicsUpdate）。
pub(super) fn union_dirty(a: DirtyRect, b: DirtyRect) -> DirtyRect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.width).max(b.x + b.width);
    let y1 = (a.y + a.height).max(b.y + b.height);
    DirtyRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

/// drain_outputs 交回主循环处理的事件（需要 ActiveStage / UDP 状态，drain 内拿不到）。
#[derive(Default)]
pub(super) struct OutputSignals {
    /// 服务端要求 Deactivation-Reactivation。
    pub reactivate: bool,
    /// 会话期到达的 Initiate Multitransport Request。
    pub multitransport: Vec<ironrdp_pdu::rdp::multitransport::MultitransportRequestPdu>,
}

/// 处理一批 outputs：ResponseFrame 立即写回；GraphicsUpdate 转脏区并入 acc（不拷贝像素）；
/// 服务端 Terminate 返回 Some（需退出）。
pub(super) async fn drain_outputs<W: FramedWrite>(
    framed: &mut W,
    outputs: Vec<ActiveStageOutput>,
    acc: &mut Option<DirtyRect>,
    desktop_w: u16,
    desktop_h: u16,
    event_tx: &async_channel::Sender<RdpEvent>,
    memo: &mut SessionMemo,
    signals: &mut OutputSignals,
) -> Result<Option<SessionEnd>, SessionError> {
    for out in outputs {
        match out {
            ActiveStageOutput::ResponseFrame(frame) => {
                framed
                    .write_all(&frame)
                    .await
                    .map_err(io_error("write response failed"))?;
            }
            ActiveStageOutput::GraphicsUpdate(region) => {
                let dirty = inclusive_to_dirty(&region, desktop_w, desktop_h);
                *acc = Some(match *acc {
                    Some(prev) => union_dirty(prev, dirty),
                    None => dirty,
                });
            }
            ActiveStageOutput::PointerDefault => {
                memo.last_pointer_key = None;
                if ptr_trace() {
                    eprintln!("[rdp-ptr] default");
                }
                let _ = event_tx.try_send(RdpEvent::PointerChanged(RdpPointer::Default));
            }
            ActiveStageOutput::PointerHidden => {
                memo.last_pointer_key = None;
                if ptr_trace() {
                    eprintln!("[rdp-ptr] hidden");
                }
                let _ = event_tx.try_send(RdpEvent::PointerChanged(RdpPointer::Hidden));
            }
            ActiveStageOutput::PointerBitmap(pointer) => {
                if let Some(p) = pointer_to_event(&pointer, &mut memo.last_pointer_key) {
                    if ptr_trace() {
                        eprintln!(
                            "[rdp-ptr] bitmap {}x{} hs=({},{})",
                            pointer.width, pointer.height, pointer.hotspot_x, pointer.hotspot_y
                        );
                    }
                    // 发送失败（通道满）回滚去重键，下次同指针可重试，否则光标永久丢失。
                    if event_tx.try_send(RdpEvent::PointerChanged(p)).is_err() {
                        memo.last_pointer_key = None;
                    }
                }
            }
            ActiveStageOutput::Terminate(reason) => {
                eprintln!("[rdp] server terminated session: {reason}");
                return Ok(Some(SessionEnd::Server(reason.to_string())));
            }
            // 动态分辨率兜底：服务端以重激活换分辨率时回传序列，交主循环 run_reactivation 走完。
            ActiveStageOutput::DeactivateAll => {
                signals.reactivate = true;
            }
            ActiveStageOutput::MultitransportRequest(request) => {
                signals.multitransport.push(request);
            }
            // 服务端实测的网络特征，留给重连时 NETCHAR_SYNC 回传。
            ActiveStageOutput::AutoDetect(request) => {
                if net_trace() {
                    eprintln!("[rdp-net] {request:?}");
                }
                memo.on_network_characteristics(&request);
            }
            // cookie 内容不打日志：拿到它就能接管会话。
            ActiveStageOutput::AutoReconnectCookie(cookie) => {
                if memo.arc_cookie.is_none() {
                    eprintln!("[rdp] auto-reconnect cookie received");
                }
                memo.arc_cookie = Some(cookie);
            }
            ActiveStageOutput::AutoReconnectFailed => {
                eprintln!(
                    "[rdp] server rejected the auto-reconnect cookie, logged on with credentials"
                );
                memo.arc_cookie = None;
            }
            ActiveStageOutput::Heartbeat {
                period,
                count1,
                count2,
            } => {
                if memo.liveness.on_heartbeat(period, count1, count2) {
                    eprintln!("[rdp] heartbeat period={period}s count1={count1} count2={count2}");
                }
            }
            // PointerPosition（本地光标本就跟手）等忽略。
            _ => {}
        }
    }
    Ok(None)
}

/// 网络探测结果追踪开关（NEXSHELL_RDP_NET_TRACE=1）。
fn net_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NEXSHELL_RDP_NET_TRACE").is_ok_and(|v| v == "1"))
}

/// 指针链路追踪开关（NEXSHELL_RDP_PTR_TRACE=1）。
fn ptr_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NEXSHELL_RDP_PTR_TRACE").is_ok_and(|v| v == "1"))
}

/// DecodedPointer(Arc) → RdpPointer::Bitmap；与上次同一指针（同 cache_key）则返回 None 去重。
/// cache_key 取内容 hash 而非 Arc 地址：地址在 Arc 释放后可被新指针复用，
/// 会让 UI 光标缓存/去重误命中旧指针。位图仅数 KB 且只在指针变化时触发，开销可忽略。
pub(super) fn pointer_to_event(
    pointer: &std::sync::Arc<ironrdp_graphics::pointer::DecodedPointer>,
    last_pointer_key: &mut Option<u64>,
) -> Option<RdpPointer> {
    let cache_key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (
            pointer.width,
            pointer.height,
            pointer.hotspot_x,
            pointer.hotspot_y,
        )
            .hash(&mut hasher);
        pointer.bitmap_data.hash(&mut hasher);
        hasher.finish()
    };
    if *last_pointer_key == Some(cache_key) {
        return None;
    }
    *last_pointer_key = Some(cache_key);
    Some(RdpPointer::Bitmap {
        rgba: pointer.bitmap_data.clone(),
        width: pointer.width as u32,
        height: pointer.height as u32,
        hotspot_x: pointer.hotspot_x as f32,
        hotspot_y: pointer.hotspot_y as f32,
        cache_key,
    })
}

/// 发布累积脏区：apply_region 拷一次 + 发一条 FrameUpdated。acc 为 None 则空操作。
pub(super) fn publish_frame(
    framebuffer: &Arc<Mutex<RdpFramebuffer>>,
    image: &DecodedImage,
    acc: &mut Option<DirtyRect>,
    stats: &Arc<RdpStats>,
    event_tx: &async_channel::Sender<RdpEvent>,
) {
    if let Some(dirty) = acc.take() {
        if dirty.width == 0 || dirty.height == 0 {
            return;
        }
        framebuffer.lock().apply_region(image.data(), dirty);
        stats.inc_frame();
        let _ = event_tx.try_send(RdpEvent::FrameUpdated { dirty });
    }
}
