//! CF_DIB 与像素互转。
//! 生成：BITMAPINFOHEADER（40 字节）+ 32 位 BI_RGB 像素，行自下而上。
//! 解析：CF_DIB / CF_DIBV5 的 24、32 位 BI_RGB 与 BI_BITFIELDS；调色板和 16 位格式不支持。
// 解析只有 macOS 的辅助进程在用。
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use super::Bitmap;

const HEADER_LEN: u32 = 40;
const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;

/// 自上而下的 BGRA 位图 → CF_DIB。分辨率字段填 0，由接收方按屏幕 DPI 处理。
pub(super) fn bitmap_to_dib(bitmap: &Bitmap) -> Option<Vec<u8>> {
    let row = bitmap.width.checked_mul(4)?;
    let size = row.checked_mul(bitmap.height)?;
    if size == 0 || bitmap.bgra.len() < size {
        return None;
    }
    let mut out = Vec::with_capacity(HEADER_LEN as usize + size);
    out.extend_from_slice(&HEADER_LEN.to_le_bytes());
    out.extend_from_slice(&i32::try_from(bitmap.width).ok()?.to_le_bytes());
    // 正高度表示自下而上。
    out.extend_from_slice(&i32::try_from(bitmap.height).ok()?.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&u32::try_from(size).ok()?.to_le_bytes());
    out.extend_from_slice(&[0; 16]); // X/Y 分辨率、clrUsed、clrImportant
    for line in bitmap.bgra[..size].chunks_exact(row).rev() {
        out.extend_from_slice(line);
    }
    Some(out)
}

/// CF_DIB / CF_DIBV5 → 自上而下的 BGRA，alpha 不预乘。
/// 32 位 BI_RGB 的第 4 字节按 alpha 读；全为 0 时（多数程序不填）当作不透明。
pub(super) fn dib_to_bitmap(dib: &[u8]) -> Option<Bitmap> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(dib.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(dib.get(at..at + 4)?.try_into().ok()?));
    let header = u32_at(0)? as usize;
    if header < HEADER_LEN as usize {
        return None;
    }
    let width = usize::try_from(u32_at(4)? as i32).ok().filter(|&w| w > 0)?;
    let raw_height = u32_at(8)? as i32;
    let height = raw_height.unsigned_abs() as usize;
    let bits = u16_at(14)?;
    let compression = u32_at(16)?;
    let clr_used = u32_at(32)? as usize;
    // 通道掩码：V4/V5 头里自带；40 字节头的 BI_BITFIELDS 紧跟在头后面。
    let (masks, masks_len) = match (bits, compression) {
        (24, BI_RGB) => ([0xFF_0000, 0xFF00, 0xFF, 0], 0),
        (32, BI_RGB) => ([0xFF_0000, 0xFF00, 0xFF, 0xFF00_0000], 0),
        (32, BI_BITFIELDS) if header >= 52 => {
            let alpha = if header >= 56 { u32_at(52)? } else { 0 };
            ([u32_at(40)?, u32_at(44)?, u32_at(48)?, alpha], 0)
        }
        (32, BI_BITFIELDS) => (
            [u32_at(header)?, u32_at(header + 4)?, u32_at(header + 8)?, 0],
            12,
        ),
        _ => return None,
    };
    let [r, g, b] = [masks[0], masks[1], masks[2]].map(shift_of);
    let (r, g, b) = (r?, g?, b?);
    let a = shift_of(masks[3]);
    let stride = (width * usize::from(bits)).div_ceil(32) * 4;
    let offset = header + masks_len + clr_used * 4;
    let pixels = dib.get(offset..offset.checked_add(stride.checked_mul(height)?)?)?;
    let mut bgra = vec![0; width.checked_mul(height)?.checked_mul(4)?];
    if bgra.is_empty() {
        return None;
    }
    let mut any_alpha = false;
    for (y, out) in bgra.chunks_exact_mut(width * 4).enumerate() {
        let src_row = if raw_height > 0 { height - 1 - y } else { y };
        let row = &pixels[src_row * stride..];
        for (x, px) in out.chunks_exact_mut(4).enumerate() {
            let v = if bits == 24 {
                let p = &row[x * 3..x * 3 + 3];
                u32::from_le_bytes([p[0], p[1], p[2], 0])
            } else {
                u32::from_le_bytes(row[x * 4..x * 4 + 4].try_into().ok()?)
            };
            let alpha = a.map_or(0, |s| (v >> s) as u8);
            any_alpha |= alpha != 0;
            px.copy_from_slice(&[(v >> b) as u8, (v >> g) as u8, (v >> r) as u8, alpha]);
        }
    }
    if !any_alpha {
        bgra.chunks_exact_mut(4).for_each(|px| px[3] = 255);
    }
    Some(Bitmap {
        width,
        height,
        bgra,
    })
}

/// 只认占满一个字节的通道掩码，返回右移位数。
fn shift_of(mask: u32) -> Option<u32> {
    let shift = mask.trailing_zeros();
    (mask != 0 && mask >> shift == 0xFF).then_some(shift)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u32(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn header_fields_and_bottom_up_rows() {
        // 2×2：上行 A B，下行 C D，每像素 4 字节用同值标记。
        let bgra = [[1u8; 4], [2; 4], [3; 4], [4; 4]].concat();
        let dib = bitmap_to_dib(&Bitmap {
            width: 2,
            height: 2,
            bgra,
        })
        .unwrap();
        assert_eq!(dib.len(), 40 + 16);
        assert_eq!(read_u32(&dib, 0), 40);
        assert_eq!(read_u32(&dib, 4), 2);
        assert_eq!(read_u32(&dib, 8), 2);
        assert_eq!(u16::from_le_bytes([dib[12], dib[13]]), 1);
        assert_eq!(u16::from_le_bytes([dib[14], dib[15]]), 32);
        assert_eq!(read_u32(&dib, 16), 0);
        assert_eq!(read_u32(&dib, 20), 16);
        // 像素区先放下行。
        assert_eq!(&dib[40..48], &[[3u8; 4], [4; 4]].concat()[..]);
        assert_eq!(&dib[48..56], &[[1u8; 4], [2; 4]].concat()[..]);
    }

    #[test]
    fn rejects_empty_or_short_buffers() {
        let empty = Bitmap {
            width: 0,
            height: 3,
            bgra: Vec::new(),
        };
        assert!(bitmap_to_dib(&empty).is_none());
        let short = Bitmap {
            width: 2,
            height: 2,
            bgra: vec![0; 15],
        };
        assert!(bitmap_to_dib(&short).is_none());
    }

    #[test]
    fn dib_roundtrip_keeps_rows_and_makes_opaque() {
        let bgra = [[1u8, 2, 3, 0], [4, 5, 6, 0], [7, 8, 9, 0], [10, 11, 12, 0]].concat();
        let dib = bitmap_to_dib(&Bitmap {
            width: 2,
            height: 2,
            bgra,
        })
        .unwrap();
        let back = dib_to_bitmap(&dib).unwrap();
        assert_eq!((back.width, back.height), (2, 2));
        assert_eq!(
            back.bgra,
            [
                [1u8, 2, 3, 255],
                [4, 5, 6, 255],
                [7, 8, 9, 255],
                [10, 11, 12, 255]
            ]
            .concat()
        );
    }

    #[test]
    fn dib_24_bit_rows_are_padded_to_four_bytes() {
        // 1×2、24 位、自上而下（负高度）：每行 3 字节像素 + 1 字节填充。
        let mut dib = vec![0u8; 40];
        dib[0] = 40;
        dib[4] = 1;
        dib[8..12].copy_from_slice(&(-2i32).to_le_bytes());
        dib[14] = 24;
        dib.extend_from_slice(&[10, 20, 30, 0, 40, 50, 60, 0]);
        let bitmap = dib_to_bitmap(&dib).unwrap();
        assert_eq!(bitmap.bgra, [10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn dibv5_bitfields_keep_alpha() {
        // 1×1、32 位 BI_BITFIELDS、V5 头（124 字节），RGBA 字节序掩码。
        let mut dib = vec![0u8; 124];
        dib[0] = 124;
        dib[4] = 1;
        dib[8] = 1;
        dib[14] = 32;
        dib[16] = 3;
        for (at, mask) in [
            (40, 0xFFu32),
            (44, 0xFF00),
            (48, 0xFF_0000),
            (52, 0xFF00_0000),
        ] {
            dib[at..at + 4].copy_from_slice(&mask.to_le_bytes());
        }
        dib.extend_from_slice(&[0x11, 0x22, 0x33, 0x80]);
        let bitmap = dib_to_bitmap(&dib).unwrap();
        assert_eq!(bitmap.bgra, [0x33, 0x22, 0x11, 0x80]);
    }

    #[test]
    fn dib_rejects_palettes_and_short_data() {
        let mut dib = vec![0u8; 40];
        dib[0] = 40;
        dib[4] = 1;
        dib[8] = 1;
        dib[14] = 8;
        dib.extend_from_slice(&[0; 8]);
        assert!(dib_to_bitmap(&dib).is_none());
        dib[14] = 32;
        dib.truncate(42);
        assert!(dib_to_bitmap(&dib).is_none());
    }
}
