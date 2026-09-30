//! Accelerate vImage 转色（NEON 向量化 + 内部多线程），替换逐像素标量循环。
//! 结构同 Windows App：VT 出 NV12，CPU 转 RGBA；系数仍是 BT.709（ADR 0015 决策 4）。
//! 与标量参考实现（`avc444::yuv_to_rgb`，截断取整）相比，vImage 四舍五入，差值不超过 1。

use std::ffi::c_void;
use std::sync::OnceLock;

use super::avc444::Nv12Frame;

#[repr(C)]
struct Buffer {
    data: *mut c_void,
    height: usize,
    width: usize,
    row_bytes: usize,
}

/// G 行的两个系数取负值（vImage 约定：G = Y + Cb_G·Cb + Cr_G·Cr）。
#[repr(C)]
struct Matrix {
    yp: f32,
    cr_r: f32,
    cr_g: f32,
    cb_g: f32,
    cb_b: f32,
}

#[repr(C)]
struct PixelRange {
    yp_bias: i32,
    cbcr_bias: i32,
    yp_range_max: i32,
    cbcr_range_max: i32,
    yp_max: i32,
    yp_min: i32,
    cbcr_max: i32,
    cbcr_min: i32,
}

#[repr(C, align(16))]
struct Conversion([u8; 128]);

const YPCBCR_420_YP8_CBCR8: u32 = 4;
const YPCBCR_444_AYPCBCR8: u32 = 5;
const ARGB8888: u32 = 0;
const NO_FLAGS: u32 = 0;
/// vImage 按 ARGB 算出四个通道，permute 取成 RGBA。
const TO_RGBA: [u8; 4] = [1, 2, 3, 0];

/// vImage 把全范围色度按 (C−128)/254 归一到 ±0.5，系数乘 254/255 抵消，
/// 每个码值的权重就正好是 FreeRDP 的整数系数 403 / −120 / −48 / 475 除以 256。
const FULL_SCALE: f32 = 254.0 / 255.0 / 256.0;
const FULL_MATRIX: Matrix = Matrix {
    yp: 1.0,
    cr_r: 403.0 * FULL_SCALE,
    cr_g: -120.0 * FULL_SCALE,
    cb_g: -48.0 * FULL_SCALE,
    cb_b: 475.0 * FULL_SCALE,
};
const FULL_RANGE: PixelRange = PixelRange {
    yp_bias: 0,
    cbcr_bias: 128,
    yp_range_max: 255,
    cbcr_range_max: 255,
    yp_max: 255,
    yp_min: 0,
    cbcr_max: 255,
    cbcr_min: 0,
};
const VIDEO_MATRIX: Matrix = Matrix {
    yp: 1.0,
    cr_r: 1.5748,
    cr_g: -0.4681,
    cb_g: -0.1873,
    cb_b: 1.8556,
};
const VIDEO_RANGE: PixelRange = PixelRange {
    yp_bias: 16,
    cbcr_bias: 128,
    yp_range_max: 235,
    cbcr_range_max: 240,
    yp_max: 255,
    yp_min: 0,
    cbcr_max: 255,
    cbcr_min: 0,
};

#[link(name = "Accelerate", kind = "framework")]
unsafe extern "C" {
    fn vImageConvert_YpCbCrToARGB_GenerateConversion(
        matrix: *const Matrix,
        pixel_range: *const PixelRange,
        out_info: *mut Conversion,
        in_type: u32,
        out_type: u32,
        flags: u32,
    ) -> isize;
    fn vImageConvert_420Yp8_CbCr8ToARGB8888(
        src_yp: *const Buffer,
        src_cbcr: *const Buffer,
        dest: *const Buffer,
        info: *const Conversion,
        permute_map: *const u8,
        alpha: u8,
        flags: u32,
    ) -> isize;
    fn vImageConvert_444AYpCbCr8ToARGB8888(
        src: *const Buffer,
        dest: *const Buffer,
        info: *const Conversion,
        permute_map: *const u8,
        flags: u32,
    ) -> isize;
}

struct Conversions {
    nv12_full: Conversion,
    nv12_video: Conversion,
    ayuv_full: Conversion,
}

fn conversions() -> Result<&'static Conversions, String> {
    static CONVERSIONS: OnceLock<Result<Conversions, isize>> = OnceLock::new();
    CONVERSIONS
        .get_or_init(|| {
            Ok(Conversions {
                nv12_full: generate(&FULL_MATRIX, &FULL_RANGE, YPCBCR_420_YP8_CBCR8)?,
                nv12_video: generate(&VIDEO_MATRIX, &VIDEO_RANGE, YPCBCR_420_YP8_CBCR8)?,
                ayuv_full: generate(&FULL_MATRIX, &FULL_RANGE, YPCBCR_444_AYPCBCR8)?,
            })
        })
        .as_ref()
        .map_err(|e| format!("vImage conversion setup failed: {e}"))
}

fn generate(matrix: &Matrix, range: &PixelRange, in_type: u32) -> Result<Conversion, isize> {
    let mut out = Conversion([0; 128]);
    // SAFETY: 三个指针都指向有效结构，vImage 只写 out。
    let err = unsafe {
        vImageConvert_YpCbCrToARGB_GenerateConversion(
            matrix, range, &mut out, in_type, ARGB8888, NO_FLAGS,
        )
    };
    if err == 0 {
        Ok(out)
    } else {
        Err(err)
    }
}

/// NV12 → RGBA，`dst` 行距 = 宽 × 4。宽高须为偶数（VT 输出按宏块对齐）。
pub fn nv12_to_rgba(frame: &Nv12Frame<'_>, full_range: bool, dst: &mut [u8]) -> Result<(), String> {
    let (w, h) = (frame.width, frame.height);
    if w % 2 != 0 || h % 2 != 0 {
        return Err(format!("NV12 {w}x{h} is not even-sized"));
    }
    if dst.len() < w * h * 4 {
        return Err("RGBA buffer shorter than frame".to_owned());
    }
    let c = conversions()?;
    let info = if full_range {
        &c.nv12_full
    } else {
        &c.nv12_video
    };
    // vImage 只读源平面；Nv12Frame::new 已校验平面覆盖 w×h。
    let yp = Buffer {
        data: frame.y.as_ptr().cast_mut().cast(),
        height: h,
        width: w,
        row_bytes: frame.y_stride,
    };
    let cbcr = Buffer {
        data: frame.uv.as_ptr().cast_mut().cast(),
        height: h / 2,
        width: w / 2,
        row_bytes: frame.uv_stride,
    };
    let dest = Buffer {
        data: dst.as_mut_ptr().cast(),
        height: h,
        width: w,
        row_bytes: w * 4,
    };
    // SAFETY: 三个缓冲的尺寸与行距都在对应切片界内，info 由 GenerateConversion 生成。
    let err = unsafe {
        vImageConvert_420Yp8_CbCr8ToARGB8888(
            &yp,
            &cbcr,
            &dest,
            info,
            TO_RGBA.as_ptr(),
            255,
            NO_FLAGS,
        )
    };
    check(err)
}

/// AYpCbCr8 交错块（全范围，行距 = 宽 × 4）→ RGBA，写到 `dst` 开头、行距 `dst_stride`。
pub fn ayuv_to_rgba(
    src: &[u8],
    width: usize,
    height: usize,
    dst: &mut [u8],
    dst_stride: usize,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Ok(());
    }
    if src.len() < width * height * 4
        || dst_stride < width * 4
        || dst.len() < dst_stride * (height - 1) + width * 4
    {
        return Err("AYUV / RGBA buffer shorter than block".to_owned());
    }
    let info = &conversions()?.ayuv_full;
    let src = Buffer {
        data: src.as_ptr().cast_mut().cast(),
        height,
        width,
        row_bytes: width * 4,
    };
    let dest = Buffer {
        data: dst.as_mut_ptr().cast(),
        height,
        width,
        row_bytes: dst_stride,
    };
    // SAFETY: 两个缓冲的尺寸与行距都已按切片长度校验，info 由 GenerateConversion 生成。
    let err = unsafe {
        vImageConvert_444AYpCbCr8ToARGB8888(&src, &dest, info, TO_RGBA.as_ptr(), NO_FLAGS)
    };
    check(err)
}

fn check(err: isize) -> Result<(), String> {
    if err == 0 {
        Ok(())
    } else {
        Err(format!("vImage conversion failed: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::avc444::yuv_to_rgb;
    use super::*;

    fn samples(n: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 24) as u8
            })
            .collect()
    }

    fn assert_close(got: &[u8], want: [u8; 3], what: &str) {
        for c in 0..3 {
            assert!(
                got[c].abs_diff(want[c]) <= 1,
                "{what}: got {:?}, want {want:?}",
                &got[..3]
            );
        }
        assert_eq!(got[3], 255, "{what}: alpha");
    }

    #[test]
    fn nv12_full_range_matches_scalar_reference() {
        let (w, h) = (32, 16);
        let y = samples(w * h, 1);
        let uv = samples(w * h / 2, 2);
        let frame = Nv12Frame::new(w, h, &y, w, &uv, w).unwrap();
        let mut rgba = vec![0; w * h * 4];
        nv12_to_rgba(&frame, true, &mut rgba).unwrap();
        for row in 0..h {
            for col in 0..w {
                let c = (row / 2) * w + (col & !1);
                let want = yuv_to_rgb(y[row * w + col], uv[c], uv[c + 1]);
                let i = (row * w + col) * 4;
                assert_close(&rgba[i..i + 4], want, &format!("({col}, {row})"));
            }
        }
    }

    #[test]
    fn nv12_skips_stride_padding() {
        let (w, h, y_stride, uv_stride) = (2, 2, 5, 6);
        let mut y = vec![0xFF; y_stride * h];
        for row in 0..h {
            y[row * y_stride..row * y_stride + w].fill(128);
        }
        let mut uv = vec![0xFF; uv_stride];
        uv[..2].fill(128);
        let frame = Nv12Frame::new(w, h, &y, y_stride, &uv, uv_stride).unwrap();
        let mut rgba = vec![0; w * h * 4];
        nv12_to_rgba(&frame, true, &mut rgba).unwrap();
        for px in rgba.chunks_exact(4) {
            assert_eq!(px, [128, 128, 128, 255]);
        }
    }

    #[test]
    fn nv12_video_range_red() {
        // BT.709 video range 纯红：Y=63, Cb=102, Cr=240。
        let y = [63; 4];
        let uv = [102, 240];
        let frame = Nv12Frame::new(2, 2, &y, 2, &uv, 2).unwrap();
        let mut rgba = vec![0; 16];
        nv12_to_rgba(&frame, false, &mut rgba).unwrap();
        let px = &rgba[..4];
        assert!(px[0] >= 250 && px[1] <= 5 && px[2] <= 5, "got {px:?}");
    }

    #[test]
    fn ayuv_matches_scalar_reference() {
        let n = 4096;
        let yuv = samples(n * 3, 3);
        let src: Vec<u8> = yuv
            .chunks_exact(3)
            .flat_map(|p| [255, p[0], p[1], p[2]])
            .collect();
        let mut rgba = vec![0; n * 4];
        ayuv_to_rgba(&src, 64, 64, &mut rgba, 64 * 4).unwrap();
        for (i, p) in yuv.chunks_exact(3).enumerate() {
            let want = yuv_to_rgb(p[0], p[1], p[2]);
            assert_close(&rgba[i * 4..i * 4 + 4], want, &format!("{p:?}"));
        }
    }

    #[test]
    fn ayuv_writes_only_block_inside_stride() {
        let src = [255, 100, 128, 128].repeat(4);
        let mut dst = vec![7; 4 * 4 * 2];
        ayuv_to_rgba(&src, 2, 2, &mut dst, 16).unwrap();
        for row in 0..2 {
            let line = &dst[row * 16..row * 16 + 16];
            assert_eq!(&line[..8], [100, 100, 100, 255, 100, 100, 100, 255]);
            assert_eq!(&line[8..], [7; 8]);
        }
    }
}
