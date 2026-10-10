//! MouseCursor DVC（MS-RDPEMSC）：光标形状改走动态通道，Soft-Sync 后随隧道走 UDP，避开 TCP 队头阻塞。
//! 服务端确认能力后不再发 FastPath 指针。位置更新照旧忽略：本地光标本就跟手。
//! NEXSHELL_RDP_MOUSE_CURSOR_DVC=0 关闭，回到 FastPath 指针。

use std::collections::HashMap;
use std::sync::Arc;

use ironrdp_core::{impl_as_any, Decode, Encode, EncodeResult, ReadCursor, WriteCursor};
use ironrdp_dvc::{DrdynvcClient, DvcClientProcessor, DvcEncode, DvcMessage, DvcProcessor};
use ironrdp_graphics::pointer::{DecodedPointer, PointerBitmapTarget};
use ironrdp_pdu::pointer::{LargePointerAttribute, PointerAttribute};
use ironrdp_pdu::PduResult;

use super::outputs::{pointer_to_event, ptr_trace};
use super::{RdpEvent, RdpPointer};

const CHANNEL_NAME: &str = "Microsoft::Windows::RDS::MouseCursor";

const PDUTYPE_CS_CAPS_ADVERTISE: u8 = 0x01;
const PDUTYPE_SC_CAPS_CONFIRM: u8 = 0x02;
const PDUTYPE_SC_MOUSEPTR_UPDATE: u8 = 0x03;

const UPDATETYPE_SYSTEM_NULL: u8 = 0x05;
const UPDATETYPE_SYSTEM_DEFAULT: u8 = 0x06;
const UPDATETYPE_POSITION: u8 = 0x08;
const UPDATETYPE_CACHED: u8 = 0x0A;
const UPDATETYPE_POINTER: u8 = 0x0B;
const UPDATETYPE_LARGE_POINTER: u8 = 0x0C;

/// "CAPS"
const CAPS_SIGNATURE: u32 = 0x5350_4143;
const CAPS_VERSION_1: u32 = 1;
const CAPS_V1_SIZE: u32 = 12;

fn enabled() -> bool {
    std::env::var("NEXSHELL_RDP_MOUSE_CURSOR_DVC").map_or(true, |v| v != "0")
}

pub(super) fn attach(
    client: DrdynvcClient,
    event_tx: async_channel::Sender<RdpEvent>,
) -> DrdynvcClient {
    if !enabled() {
        eprintln!("[rdp-cursor] MouseCursor DVC disabled, using FastPath pointer updates");
        return client;
    }
    client.with_dynamic_channel(MouseCursorClient::new(event_tx))
}

/// RDP_MOUSE_CURSOR_CAPS_ADVERTISE_PDU，只带 CAPSET_VERSION1。
struct CapsAdvertise;

impl Encode for CapsAdvertise {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        ironrdp_core::ensure_size!(in: dst, size: self.size());
        dst.write_u8(PDUTYPE_CS_CAPS_ADVERTISE);
        dst.write_u8(0);
        dst.write_u16(0);
        dst.write_u32(CAPS_SIGNATURE);
        dst.write_u32(CAPS_VERSION_1);
        dst.write_u32(CAPS_V1_SIZE);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "RDP_MOUSE_CURSOR_CAPS_ADVERTISE_PDU"
    }

    fn size(&self) -> usize {
        16
    }
}

impl DvcEncode for CapsAdvertise {}

#[derive(Debug)]
enum Message<'a> {
    CapsConfirm { signature: u32, version: u32 },
    Update(Update<'a>),
    Unknown { pdu_type: u8, update_type: u8 },
}

#[derive(Debug)]
enum Update<'a> {
    Hidden,
    Default,
    Position,
    Cached(u16),
    Pointer(PointerAttribute<'a>),
    Large(LargePointerAttribute<'a>),
}

fn decode(payload: &[u8]) -> Result<Message<'_>, String> {
    let mut src = ReadCursor::new(payload);
    if src.len() < 4 {
        return Err(format!("short header: {} bytes", payload.len()));
    }
    let pdu_type = src.read_u8();
    let update_type = src.read_u8();
    let _reserved = src.read_u16();
    let unknown = Message::Unknown {
        pdu_type,
        update_type,
    };
    match pdu_type {
        PDUTYPE_SC_CAPS_CONFIRM => {
            if src.len() < 8 {
                return Err("short caps confirm".to_owned());
            }
            Ok(Message::CapsConfirm {
                signature: src.read_u32(),
                version: src.read_u32(),
            })
        }
        PDUTYPE_SC_MOUSEPTR_UPDATE => {
            let update = match update_type {
                UPDATETYPE_SYSTEM_NULL => Update::Hidden,
                UPDATETYPE_SYSTEM_DEFAULT => Update::Default,
                UPDATETYPE_POSITION => Update::Position,
                UPDATETYPE_CACHED => {
                    if src.len() < 2 {
                        return Err("short cached pointer update".to_owned());
                    }
                    Update::Cached(src.read_u16())
                }
                // 载荷布局同 MS-RDPBCGR 的 TS_POINTERATTRIBUTE / TS_FP_LARGEPOINTERATTRIBUTE，结尾可有 1 字节 pad。
                UPDATETYPE_POINTER => {
                    Update::Pointer(PointerAttribute::decode(&mut src).map_err(|e| e.to_string())?)
                }
                UPDATETYPE_LARGE_POINTER => Update::Large(
                    LargePointerAttribute::decode(&mut src).map_err(|e| e.to_string())?,
                ),
                _ => return Ok(unknown),
            };
            Ok(Message::Update(update))
        }
        _ => Ok(unknown),
    }
}

struct MouseCursorClient {
    event_tx: async_channel::Sender<RdpEvent>,
    /// 本通道自己的指针缓存：FastPath 的缓存在 ironrdp-session 内部，没法共用。
    cache: HashMap<u16, Arc<DecodedPointer>>,
    last_pointer_key: Option<u64>,
}

impl_as_any!(MouseCursorClient);

impl MouseCursorClient {
    fn new(event_tx: async_channel::Sender<RdpEvent>) -> Self {
        Self {
            event_tx,
            cache: HashMap::new(),
            last_pointer_key: None,
        }
    }

    /// 更新缓存并给出要发给 UI 的光标；None 表示不用变。
    fn apply(&mut self, update: Update<'_>) -> Option<RdpPointer> {
        let (cache_index, decoded) = match update {
            Update::Hidden => {
                self.last_pointer_key = None;
                return Some(RdpPointer::Hidden);
            }
            Update::Default => {
                self.last_pointer_key = None;
                return Some(RdpPointer::Default);
            }
            Update::Position => return None,
            Update::Cached(index) => {
                let Some(pointer) = self.cache.get(&index) else {
                    eprintln!("[rdp-cursor] cached pointer {index} not found");
                    return None;
                };
                return pointer_to_event(pointer, &mut self.last_pointer_key);
            }
            Update::Pointer(attr) => (
                attr.color_pointer.cache_index,
                DecodedPointer::decode_pointer_attribute(&attr, PointerBitmapTarget::Accelerated),
            ),
            Update::Large(attr) => (
                attr.cache_index,
                DecodedPointer::decode_large_pointer_attribute(
                    &attr,
                    PointerBitmapTarget::Accelerated,
                ),
            ),
        };
        match decoded {
            Ok(pointer) => {
                let pointer = Arc::new(pointer);
                self.cache.insert(cache_index, Arc::clone(&pointer));
                pointer_to_event(&pointer, &mut self.last_pointer_key)
            }
            // 同 FastPath：解码失败丢掉该缓存槽，退回系统箭头。
            Err(e) => {
                eprintln!("[rdp-cursor] ignoring undecodable pointer {cache_index}: {e:?}");
                self.cache.remove(&cache_index);
                self.last_pointer_key = None;
                Some(RdpPointer::Default)
            }
        }
    }
}

impl DvcProcessor for MouseCursorClient {
    fn channel_name(&self) -> &str {
        CHANNEL_NAME
    }

    fn start(&mut self, channel_id: u32) -> PduResult<Vec<DvcMessage>> {
        eprintln!(
            "[rdp-cursor] {CHANNEL_NAME} opened (channel_id={channel_id}), advertising caps v1"
        );
        Ok(vec![Box::new(CapsAdvertise)])
    }

    // 错误一律不上抛：DVC 层会把它当成致命错误。
    fn process(&mut self, _channel_id: u32, payload: &[u8]) -> PduResult<Vec<DvcMessage>> {
        match decode(payload) {
            Ok(Message::CapsConfirm { signature, version }) => {
                // 不合法也不关通道：服务端确认后就改走这里发光标，关掉反而丢光标。
                if signature == CAPS_SIGNATURE && version == CAPS_VERSION_1 {
                    eprintln!("[rdp-cursor] caps confirmed, pointer updates now on the DVC");
                } else {
                    eprintln!(
                        "[rdp-cursor] unexpected caps confirm signature={signature:#x} version={version}"
                    );
                }
            }
            Ok(Message::Update(update)) => {
                if let Some(pointer) = self.apply(update) {
                    if ptr_trace() {
                        match &pointer {
                            RdpPointer::Bitmap {
                                width,
                                height,
                                hotspot_x,
                                hotspot_y,
                                ..
                            } => eprintln!("[rdp-ptr] dvc bitmap {width}x{height} hs=({hotspot_x},{hotspot_y})"),
                            other => eprintln!("[rdp-ptr] dvc {other:?}"),
                        }
                    }
                    // 发送失败（通道满）回滚去重键，下次同指针可重试。
                    if self
                        .event_tx
                        .try_send(RdpEvent::PointerChanged(pointer))
                        .is_err()
                    {
                        self.last_pointer_key = None;
                    }
                }
            }
            Ok(Message::Unknown {
                pdu_type,
                update_type,
            }) => {
                eprintln!(
                    "[rdp-cursor] ignoring pduType={pdu_type:#04x} updateType={update_type:#04x}"
                );
            }
            Err(e) => eprintln!("[rdp-cursor] bad PDU ({} bytes): {e}", payload.len()),
        }
        Ok(Vec::new())
    }

    fn close(&mut self, channel_id: u32) {
        eprintln!("[rdp-cursor] {CHANNEL_NAME} closed (channel_id={channel_id})");
        self.cache.clear();
    }
}

impl DvcClientProcessor for MouseCursorClient {}

#[cfg(test)]
mod tests {
    use ironrdp_pdu::pointer::{ColorPointerAttribute, Point16};

    use super::*;

    fn client() -> (MouseCursorClient, async_channel::Receiver<RdpEvent>) {
        let (tx, rx) = async_channel::bounded(8);
        (MouseCursorClient::new(tx), rx)
    }

    fn update_pdu(update_type: u8, body: &[u8]) -> Vec<u8> {
        let mut pdu = vec![PDUTYPE_SC_MOUSEPTR_UPDATE, update_type, 0, 0];
        pdu.extend_from_slice(body);
        pdu
    }

    /// 2×2、24 bpp：XOR 每行 6 字节（已是偶数），AND 每行 1 bit 补到 2 字节。
    fn pointer_body(cache_index: u16, red: u8) -> Vec<u8> {
        let xor_mask = [0, 0, red, 0, 0, red, 0, 0, red, 0, 0, red];
        let and_mask = [0u8; 4];
        let attr = PointerAttribute {
            xor_bpp: 24,
            color_pointer: ColorPointerAttribute {
                cache_index,
                hot_spot: Point16 { x: 1, y: 0 },
                width: 2,
                height: 2,
                xor_mask: &xor_mask,
                and_mask: &and_mask,
            },
        };
        let mut body = ironrdp_core::encode_vec(&attr).unwrap();
        body.push(0); // pad
        body
    }

    fn take(rx: &async_channel::Receiver<RdpEvent>) -> Option<RdpPointer> {
        match rx.try_recv().ok()? {
            RdpEvent::PointerChanged(p) => Some(p),
            _ => None,
        }
    }

    #[test]
    fn caps_advertise_matches_windows_app_bytes() {
        let bytes = ironrdp_core::encode_vec(&CapsAdvertise).unwrap();
        assert_eq!(
            bytes,
            [0x01, 0, 0, 0, 0x43, 0x41, 0x50, 0x53, 0x01, 0, 0, 0, 0x0C, 0, 0, 0]
        );
    }

    #[test]
    fn caps_confirm_is_decoded() {
        let pdu = [
            0x02, 0, 0, 0, 0x43, 0x41, 0x50, 0x53, 0x01, 0, 0, 0, 0x0C, 0, 0, 0,
        ];
        assert!(matches!(
            decode(&pdu),
            Ok(Message::CapsConfirm {
                signature: CAPS_SIGNATURE,
                version: CAPS_VERSION_1
            })
        ));
    }

    #[test]
    fn pointer_is_cached_and_replayed_by_index() {
        let (mut c, rx) = client();
        c.process(1, &update_pdu(UPDATETYPE_POINTER, &pointer_body(3, 0xFF)))
            .unwrap();
        let Some(RdpPointer::Bitmap {
            width,
            height,
            hotspot_x,
            cache_key,
            ..
        }) = take(&rx)
        else {
            panic!("expected a bitmap pointer");
        };
        assert_eq!((width, height, hotspot_x), (2, 2, 1.0));

        c.process(1, &update_pdu(UPDATETYPE_SYSTEM_DEFAULT, &[]))
            .unwrap();
        assert!(matches!(take(&rx), Some(RdpPointer::Default)));

        c.process(1, &update_pdu(UPDATETYPE_CACHED, &3u16.to_le_bytes()))
            .unwrap();
        assert!(
            matches!(take(&rx), Some(RdpPointer::Bitmap { cache_key: k, .. }) if k == cache_key)
        );
        // 同一指针重复下发不再打扰 UI。
        c.process(1, &update_pdu(UPDATETYPE_CACHED, &3u16.to_le_bytes()))
            .unwrap();
        assert!(take(&rx).is_none());
    }

    #[test]
    fn hidden_position_and_cache_miss() {
        let (mut c, rx) = client();
        c.process(1, &update_pdu(UPDATETYPE_SYSTEM_NULL, &[]))
            .unwrap();
        assert!(matches!(take(&rx), Some(RdpPointer::Hidden)));
        c.process(1, &update_pdu(UPDATETYPE_POSITION, &[10, 0, 20, 0]))
            .unwrap();
        c.process(1, &update_pdu(UPDATETYPE_CACHED, &7u16.to_le_bytes()))
            .unwrap();
        assert!(take(&rx).is_none());
    }

    #[test]
    fn malformed_pdus_do_not_fail_the_channel() {
        let (mut c, rx) = client();
        for pdu in [
            vec![0x03],
            update_pdu(UPDATETYPE_POINTER, &[24, 0, 1]),
            update_pdu(UPDATETYPE_CACHED, &[]),
            vec![0x09, 0, 0, 0],
        ] {
            assert!(c.process(1, &pdu).unwrap().is_empty());
        }
        assert!(take(&rx).is_none());
    }
}
