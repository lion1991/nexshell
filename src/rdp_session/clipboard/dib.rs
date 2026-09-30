//! 像素转 CF_DIB：BITMAPINFOHEADER（40 字节）+ 32 位 BI_RGB 像素，行自下而上。

use super::Bitmap;

const HEADER_LEN: u32 = 40;

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
}
