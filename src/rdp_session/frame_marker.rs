//! FastPath 真帧边界只读 peek：不改协议状态，扫 SurfaceCommand::FrameMarker。
//! 解码方式照抄 ironrdp-session fast_path.rs 的 process（先解 header 再循环解 update）。
//! 任何错误都当"没看见"返回，绝不 panic/Err——peek 失败退化到旧启发式。

use ironrdp_core::{decode_cursor, ReadCursor};
use ironrdp_pdu::fast_path::{
    FastPathHeader, FastPathUpdate, FastPathUpdatePdu, Fragmentation, UpdateCode,
};
use ironrdp_pdu::surface_commands::{FrameAction, SurfaceCommand};

/// 一个 FastPath PDU 的帧边界观察结果。
pub(super) struct MarkerPeek {
    /// 见到任意 FrameMarker（Begin 或 End）——判定服务端走 surface-command 管线。
    pub saw_marker: bool,
    /// 见到 FrameMarker(End)——真帧结束，可发布。
    pub saw_end: bool,
    /// 见到 Bitmap update code——位图回退管线（仅用于一次性诊断日志）。
    pub saw_bitmap: bool,
}

/// 只读扫描 FastPath payload（含 FastPathHeader），报告帧边界信号。
pub(super) fn peek_frame_markers(payload: &[u8]) -> MarkerPeek {
    let mut peek = MarkerPeek {
        saw_marker: false,
        saw_end: false,
        saw_bitmap: false,
    };
    let mut cursor = ReadCursor::new(payload);
    if decode_cursor::<FastPathHeader>(&mut cursor).is_err() {
        return peek;
    }
    while !cursor.is_empty() {
        // 解不动即停，返回已累积（cursor 已损坏，续解不可靠）。
        let Ok(update) = decode_cursor::<FastPathUpdatePdu<'_>>(&mut cursor) else {
            break;
        };
        match update.update_code {
            UpdateCode::Bitmap => peek.saw_bitmap = true,
            UpdateCode::SurfaceCommands => {
                // 分片/带压缩标志的 update 内容不完整或需解压，跳过 marker 扫描。
                if update.fragmentation != Fragmentation::Single
                    || update.compression_flags.is_some()
                {
                    continue;
                }
                if let Ok(FastPathUpdate::SurfaceCommands(cmds)) =
                    FastPathUpdate::decode_with_code(update.data, UpdateCode::SurfaceCommands)
                {
                    for cmd in cmds {
                        if let SurfaceCommand::FrameMarker(marker) = cmd {
                            peek.saw_marker = true;
                            if marker.frame_action == FrameAction::End {
                                peek.saw_end = true;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    peek
}

#[cfg(test)]
mod tests {
    use super::*;
    use ironrdp_core::{encode_vec, Encode, WriteCursor};
    use ironrdp_pdu::fast_path::{EncryptionFlags, FastPathHeader};
    use ironrdp_pdu::surface_commands::FrameMarkerPdu;

    /// 把 update payload 包成完整 FastPath PDU 字节（header + 单个 Single update）。
    fn wrap_fastpath(update: &FastPathUpdatePdu<'_>) -> Vec<u8> {
        let update_bytes = encode_vec(update).unwrap();
        let header = FastPathHeader::new(EncryptionFlags::empty(), update_bytes.len());
        let mut out = vec![0u8; header.size() + update_bytes.len()];
        let mut cursor = WriteCursor::new(&mut out);
        header.encode(&mut cursor).unwrap();
        cursor.write_slice(&update_bytes);
        out
    }

    fn surface_update(cmds: Vec<SurfaceCommand<'static>>) -> Vec<u8> {
        let inner = encode_vec(&FastPathUpdate::SurfaceCommands(cmds)).unwrap();
        // inner 需在 update 生命周期内存活：这里直接构造并 wrap。
        let update = FastPathUpdatePdu {
            fragmentation: Fragmentation::Single,
            update_code: UpdateCode::SurfaceCommands,
            compression_flags: None,
            compression_type: None,
            data: &inner,
        };
        wrap_fastpath(&update)
    }

    #[test]
    fn detects_frame_marker_end() {
        let bytes = surface_update(vec![SurfaceCommand::FrameMarker(FrameMarkerPdu {
            frame_action: FrameAction::End,
            frame_id: Some(7),
        })]);
        let peek = peek_frame_markers(&bytes);
        assert!(peek.saw_marker);
        assert!(peek.saw_end);
    }

    #[test]
    fn frame_marker_begin_is_marker_but_not_end() {
        let bytes = surface_update(vec![SurfaceCommand::FrameMarker(FrameMarkerPdu {
            frame_action: FrameAction::Begin,
            frame_id: Some(1),
        })]);
        let peek = peek_frame_markers(&bytes);
        assert!(peek.saw_marker);
        assert!(!peek.saw_end);
    }

    #[test]
    fn bitmap_update_has_no_marker() {
        // Bitmap update code + 空 update 数据（不必是合法位图，peek 只看 update_code）。
        let update = FastPathUpdatePdu {
            fragmentation: Fragmentation::Single,
            update_code: UpdateCode::Bitmap,
            compression_flags: None,
            compression_type: None,
            data: &[],
        };
        let bytes = wrap_fastpath(&update);
        let peek = peek_frame_markers(&bytes);
        assert!(!peek.saw_marker);
        assert!(!peek.saw_end);
        assert!(peek.saw_bitmap);
    }

    #[test]
    fn garbage_bytes_never_panic() {
        for junk in [
            vec![],
            vec![0u8],
            vec![0xFF; 3],
            vec![0x00, 0x80, 0xFF, 0x04, 0x13, 0x37],
            (0..64u8).collect::<Vec<_>>(),
        ] {
            let peek = peek_frame_markers(&junk);
            assert!(!peek.saw_marker);
            assert!(!peek.saw_end);
        }
    }
}
