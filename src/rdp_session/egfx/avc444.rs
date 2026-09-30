//! AVC444 合成（MS-RDPEGFX 3.3.8.3.2 / 3.3.8.3.3，docs/adr/0015）：主流 YUV420 与辅流
//! Chroma420 合成 surface 级 YUV444，再反滤波、按 BT.709 全范围转 RGBA。
//! 纯 CPU，只处理区域矩形内的像素；坐标一律按 surface 绝对坐标（解出的帧与 surface 对齐）。
//! 对照实现：FreeRDP `prim_YUV.c` 的 LumaToYUV444 / ChromaV1ToYUV444 / ChromaV2ToYUV444 /
//! YUV444ToRGB_DOUBLE_ROW。

use ironrdp_pdu::geometry::ExclusiveRectangle;

/// 解码器输出的一帧 NV12：Y 平面 + 交错 UV 平面，保留行距。
pub struct Nv12Frame<'a> {
    width: usize,
    height: usize,
    y: &'a [u8],
    y_stride: usize,
    uv: &'a [u8],
    uv_stride: usize,
}

impl<'a> Nv12Frame<'a> {
    /// 平面长度不够覆盖 width×height 时返回 None，之后的读取都在界内。
    pub fn new(
        width: usize,
        height: usize,
        y: &'a [u8],
        y_stride: usize,
        uv: &'a [u8],
        uv_stride: usize,
    ) -> Option<Self> {
        let half_w = width.div_ceil(2);
        let half_h = height.div_ceil(2);
        let ok = width > 0
            && height > 0
            && y_stride >= width
            && uv_stride >= half_w * 2
            && y.len() >= y_stride * (height - 1) + width
            && uv.len() >= uv_stride * (half_h - 1) + half_w * 2;
        ok.then_some(Self {
            width,
            height,
            y,
            y_stride,
            uv,
            uv_stride,
        })
    }

    fn y_row(&self, row: usize) -> &[u8] {
        &self.y[row * self.y_stride..row * self.y_stride + self.width]
    }

    /// 半分辨率色度行，交错 UVUV…
    fn uv_row(&self, half_row: usize) -> &[u8] {
        let start = half_row * self.uv_stride;
        &self.uv[start..start + self.width.div_ceil(2) * 2]
    }
}

/// 辅流排布：codecId 0x000E 为 v1，0x000F 为 v2。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromaLayout {
    V1,
    V2,
}

/// 一个 surface 的 YUV444 平面（行距 = 宽）。LC=1 只更新亮度和主色度，LC=2 只补辅色度，
/// 两者在这里累积，转 RGBA 时再合起来看。
pub struct Yuv444Planes {
    width: usize,
    height: usize,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

impl Yuv444Planes {
    pub fn new(width: u16, height: u16) -> Self {
        let (width, height) = (usize::from(width), usize::from(height));
        let n = width * height;
        Self {
            width,
            height,
            y: vec![0; n],
            u: vec![128; n],
            v: vec![128; n],
        }
    }

    pub fn size(&self) -> (u16, u16) {
        (self.width as u16, self.height as u16)
    }

    /// 矩形裁到 surface 与帧的公共范围，返回 (left, top, right, bottom)。
    fn clip(
        &self,
        rect: &ExclusiveRectangle,
        frame: &Nv12Frame<'_>,
    ) -> Option<(usize, usize, usize, usize)> {
        let right = usize::from(rect.right).min(self.width).min(frame.width);
        let bottom = usize::from(rect.bottom).min(self.height).min(frame.height);
        let (left, top) = (usize::from(rect.left), usize::from(rect.top));
        (left < right && top < bottom).then_some((left, top, right, bottom))
    }

    /// 主流（YUV420）：Y 原样拷贝，每个 2×2 块的四个色度位置都填主流的那一个值。
    pub fn apply_main(&mut self, frame: &Nv12Frame<'_>, rects: &[ExclusiveRectangle]) {
        for rect in rects {
            let Some((left, top, right, bottom)) = self.clip(rect, frame) else {
                continue;
            };
            for row in top..bottom {
                let dst = row * self.width;
                self.y[dst + left..dst + right].copy_from_slice(&frame.y_row(row)[left..right]);
                let uv = frame.uv_row(row / 2);
                for col in left..right {
                    let i = (col / 2) * 2;
                    self.u[dst + col] = uv[i];
                    self.v[dst + col] = uv[i + 1];
                }
            }
        }
    }

    /// 辅流（Chroma420）：写入主流缺的 3/4 色度采样。
    pub fn apply_aux(
        &mut self,
        frame: &Nv12Frame<'_>,
        layout: ChromaLayout,
        rects: &[ExclusiveRectangle],
    ) {
        for rect in rects {
            let Some((left, top, right, bottom)) = self.clip(rect, frame) else {
                continue;
            };
            match layout {
                ChromaLayout::V1 => self.aux_v1(frame, left, top, right, bottom),
                ChromaLayout::V2 => self.aux_v2(frame, left, top, right, bottom),
            }
        }
    }

    /// v1：辅流 Y 每 16 行一组，前 8 行装 U 的奇数行、后 8 行装 V 的奇数行；
    /// 辅流 U/V 装偶数行的奇数列。
    fn aux_v1(
        &mut self,
        frame: &Nv12Frame<'_>,
        left: usize,
        top: usize,
        right: usize,
        bottom: usize,
    ) {
        for row in top..bottom {
            let dst = row * self.width;
            if row % 2 == 1 {
                let src_u = (row & !15) + ((row & 15) >> 1);
                let src_v = src_u + 8;
                if src_v >= frame.height {
                    continue;
                }
                self.u[dst + left..dst + right].copy_from_slice(&frame.y_row(src_u)[left..right]);
                self.v[dst + left..dst + right].copy_from_slice(&frame.y_row(src_v)[left..right]);
            } else {
                let uv = frame.uv_row(row / 2);
                for col in ((left | 1)..right).step_by(2) {
                    let i = (col / 2) * 2;
                    self.u[dst + col] = uv[i];
                    self.v[dst + col] = uv[i + 1];
                }
            }
        }
    }

    /// v2：辅流 Y 左半装 U 的奇数列、右半装 V 的奇数列（所有行）；辅流 U/V 按四分之一宽
    /// 分块，装奇数行上列号 4k（辅 U）和 4k+2（辅 V）的采样，左块是 U、右块是 V。
    /// 半区按解出的帧宽（16 对齐的编码宽）划分，不是 surface 宽：1366 宽实测按 1376 切。
    fn aux_v2(
        &mut self,
        frame: &Nv12Frame<'_>,
        left: usize,
        top: usize,
        right: usize,
        bottom: usize,
    ) {
        let half = frame.width / 2;
        let quarter = frame.width / 4;
        for row in top..bottom {
            let dst = row * self.width;
            let y = frame.y_row(row);
            for col in ((left | 1)..right).step_by(2) {
                self.u[dst + col] = y[col / 2];
                self.v[dst + col] = y[half + col / 2];
            }
            if row % 2 == 1 {
                let uv = frame.uv_row(row / 2);
                for col in (((left + 1) & !1)..right).step_by(2) {
                    // 辅 U 平面 = 交错 UV 的偶数字节，辅 V 平面 = 奇数字节。
                    let plane = usize::from(col % 4 == 2);
                    let k = col / 4;
                    self.u[dst + col] = uv[k * 2 + plane];
                    self.v[dst + col] = uv[(quarter + k) * 2 + plane];
                }
            }
        }
    }

    /// 区域内 YUV444 → RGBA 写进 surface 像素（行距 = surface 宽 × 4）。
    /// (2x, 2y) 位置存的是 2×2 平均值，按 `4·均值 − 另外三个` 还原，差值 < 30 时保留均值。
    pub fn write_rgba(&self, rects: &[ExclusiveRectangle], dst: &mut [u8]) {
        if dst.len() < self.width * self.height * 4 {
            return;
        }
        for rect in rects {
            let right = usize::from(rect.right).min(self.width);
            let bottom = usize::from(rect.bottom).min(self.height);
            let (left, top) = (usize::from(rect.left), usize::from(rect.top));
            for row in top..bottom {
                let base = row * self.width;
                let filter_row = row % 2 == 0 && row + 1 < self.height;
                for col in left..right {
                    let i = base + col;
                    let (mut u, mut v) = (self.u[i], self.v[i]);
                    if filter_row && col % 2 == 0 && col + 1 < self.width {
                        let below = i + self.width;
                        u = unfilter(u, self.u[i + 1], self.u[below], self.u[below + 1]);
                        v = unfilter(v, self.v[i + 1], self.v[below], self.v[below + 1]);
                    }
                    let [r, g, b] = yuv_to_rgb(self.y[i], u, v);
                    dst[i * 4..i * 4 + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
        }
    }
}

/// FreeRDP `CONDITIONAL_CLIP`：还原值与均值相差不到 30 视为编码噪声，保留均值。
fn unfilter(avg: u8, a: u8, b: u8, c: u8) -> u8 {
    let restored = (4 * i32::from(avg) - i32::from(a) - i32::from(b) - i32::from(c)).clamp(0, 255);
    if restored.abs_diff(i32::from(avg)) < 30 {
        avg
    } else {
        restored as u8
    }
}

/// YUV → RGB，BT.709 全范围，系数同 FreeRDP（403 / −48 / −120 / 475，除以 256）。
pub fn yuv_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let c = 256 * i32::from(y);
    let d = i32::from(u) - 128;
    let e = i32::from(v) - 128;
    let clip = |x: i32| (x >> 8).clamp(0, 255) as u8;
    [
        clip(c + 403 * e),
        clip(c - 48 * d - 120 * e),
        clip(c + 475 * d),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用 YUV444 平面（行距 = 宽）。
    struct Yuv444 {
        w: usize,
        h: usize,
        y: Vec<u8>,
        u: Vec<u8>,
        v: Vec<u8>,
    }

    /// 测试用 NV12 帧缓冲（宽高按 16 对齐，与 H.264 编码尺寸一致）。
    struct Nv12Buf {
        w: usize,
        h: usize,
        y: Vec<u8>,
        uv: Vec<u8>,
    }

    impl Nv12Buf {
        fn new(w: usize, h: usize) -> Self {
            let (w, h) = (w.next_multiple_of(16), h.next_multiple_of(16));
            Self {
                w,
                h,
                y: vec![0; w * h],
                uv: vec![128; w * h / 2],
            }
        }

        fn frame(&self) -> Nv12Frame<'_> {
            Nv12Frame::new(self.w, self.h, &self.y, self.w, &self.uv, self.w).unwrap()
        }

        fn set_u(&mut self, x: usize, y: usize, val: u8) {
            self.uv[y * self.w + x * 2] = val;
        }

        fn set_v(&mut self, x: usize, y: usize, val: u8) {
            self.uv[y * self.w + x * 2 + 1] = val;
        }
    }

    fn pattern(w: usize, h: usize, seed: u32) -> Yuv444 {
        let mut s = seed;
        let mut next = move || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (s >> 24) as u8
        };
        let n = w * h;
        Yuv444 {
            w,
            h,
            y: (0..n).map(|_| next()).collect(),
            u: (0..n).map(|_| next()).collect(),
            v: (0..n).map(|_| next()).collect(),
        }
    }

    /// 按 FreeRDP 编码端（general_RGBToAVC444YUV / v2）把 YUV444 拆成主辅两路。
    /// 主流色度存 2×2 均值；宽高取偶数，省掉边缘分支。v2 半区按编码宽（16 对齐）划分。
    fn split(src: &Yuv444, layout: ChromaLayout) -> (Nv12Buf, Nv12Buf) {
        let (w, h) = (src.w, src.h);
        assert!(w % 4 == 0 && h % 2 == 0);
        let at = |p: &[u8], x: usize, y: usize| p[y * w + x];
        let mut main = Nv12Buf::new(w, h);
        let mut aux = Nv12Buf::new(w, h);
        for y in 0..h {
            main.y[y * main.w..y * main.w + w].copy_from_slice(&src.y[y * w..y * w + w]);
        }
        for y in 0..h / 2 {
            for x in 0..w / 2 {
                let avg = |p: &[u8]| {
                    ((u16::from(at(p, 2 * x, 2 * y))
                        + u16::from(at(p, 2 * x + 1, 2 * y))
                        + u16::from(at(p, 2 * x, 2 * y + 1))
                        + u16::from(at(p, 2 * x + 1, 2 * y + 1)))
                        / 4) as u8
                };
                main.set_u(x, y, avg(&src.u));
                main.set_v(x, y, avg(&src.v));
            }
        }
        match layout {
            ChromaLayout::V1 => {
                for i in 0..h / 2 {
                    let n = (i & !7) + i;
                    for x in 0..w {
                        aux.y[n * aux.w + x] = at(&src.u, x, 2 * i + 1);
                        aux.y[(n + 8) * aux.w + x] = at(&src.v, x, 2 * i + 1);
                    }
                }
                for y in 0..h / 2 {
                    for x in 0..w / 2 {
                        aux.set_u(x, y, at(&src.u, 2 * x + 1, 2 * y));
                        aux.set_v(x, y, at(&src.v, 2 * x + 1, 2 * y));
                    }
                }
            }
            ChromaLayout::V2 => {
                for y in 0..h {
                    for x in 0..w / 2 {
                        aux.y[y * aux.w + x] = at(&src.u, 2 * x + 1, y);
                        aux.y[y * aux.w + aux.w / 2 + x] = at(&src.v, 2 * x + 1, y);
                    }
                }
                for y in 0..h / 2 {
                    for x in 0..w / 4 {
                        aux.set_u(x, y, at(&src.u, 4 * x, 2 * y + 1));
                        aux.set_u(aux.w / 4 + x, y, at(&src.v, 4 * x, 2 * y + 1));
                        aux.set_v(x, y, at(&src.u, 4 * x + 2, 2 * y + 1));
                        aux.set_v(aux.w / 4 + x, y, at(&src.v, 4 * x + 2, 2 * y + 1));
                    }
                }
            }
        }
        (main, aux)
    }

    fn full(w: usize, h: usize) -> ExclusiveRectangle {
        ExclusiveRectangle {
            left: 0,
            top: 0,
            right: w as u16,
            bottom: h as u16,
        }
    }

    /// 期望的 RGBA：(2x, 2y) 位置按规则由均值还原，其余位置用原值。
    fn expected_rgba(src: &Yuv444) -> Vec<u8> {
        let w = src.w;
        let mut out = vec![0; w * src.h * 4];
        for y in 0..src.h {
            for x in 0..w {
                let i = y * w + x;
                let (mut u, mut v) = (src.u[i], src.v[i]);
                if x % 2 == 0 && y % 2 == 0 {
                    let restore = |p: &[u8]| {
                        let [a, b, c] = [p[i + 1], p[i + w], p[i + w + 1]];
                        let sum = [p[i], a, b, c].map(u16::from).iter().sum::<u16>();
                        unfilter((sum / 4) as u8, a, b, c)
                    };
                    u = restore(&src.u);
                    v = restore(&src.v);
                }
                let [r, g, b] = yuv_to_rgb(src.y[i], u, v);
                out[i * 4..i * 4 + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
        out
    }

    fn round_trip(layout: ChromaLayout, w: usize, h: usize) {
        let src = pattern(w, h, 7);
        let (main, aux) = split(&src, layout);
        let mut planes = Yuv444Planes::new(w as u16, h as u16);
        let rect = [full(w, h)];
        planes.apply_main(&main.frame(), &rect);
        planes.apply_aux(&aux.frame(), layout, &rect);

        assert_eq!(planes.y, src.y);
        for y in 0..h {
            for x in 0..w {
                if x % 2 == 0 && y % 2 == 0 {
                    continue;
                }
                let i = y * w + x;
                assert_eq!(planes.u[i], src.u[i], "{layout:?} U at ({x}, {y})");
                assert_eq!(planes.v[i], src.v[i], "{layout:?} V at ({x}, {y})");
            }
        }
        let mut rgba = vec![0; w * h * 4];
        planes.write_rgba(&rect, &mut rgba);
        assert!(rgba == expected_rgba(&src), "{layout:?} RGBA mismatch");
    }

    #[test]
    fn v1_round_trip() {
        round_trip(ChromaLayout::V1, 64, 48);
        // 高度不是 16 的倍数：最后一组 8 行只用到一部分。
        round_trip(ChromaLayout::V1, 40, 26);
    }

    #[test]
    fn v2_round_trip() {
        round_trip(ChromaLayout::V2, 64, 48);
        // 宽度不是 16 的倍数：编码宽 48，半区从 24 开始而不是 20。
        round_trip(ChromaLayout::V2, 40, 26);
    }

    #[test]
    fn unfilter_recovers_sharp_chroma_and_keeps_noise() {
        // 一个 200 三个 50：均值 87，还原 198，差 111 ≥ 30，采用还原值。
        assert_eq!(unfilter(87, 50, 50, 50), 198);
        // 四个接近的值：还原值与均值差 < 30，保留均值。
        assert_eq!(unfilter(100, 98, 101, 99), 100);
    }

    #[test]
    fn luma_update_resets_chroma_only_inside_rects() {
        let (w, h) = (32, 32);
        let src = pattern(w, h, 3);
        let (main, aux) = split(&src, ChromaLayout::V1);
        let mut planes = Yuv444Planes::new(w as u16, h as u16);
        planes.apply_main(&main.frame(), &[full(w, h)]);
        planes.apply_aux(&aux.frame(), ChromaLayout::V1, &[full(w, h)]);

        let mut flat = Nv12Buf::new(w, h);
        flat.y.fill(10);
        flat.uv.fill(77);
        let sub = ExclusiveRectangle {
            left: 8,
            top: 8,
            right: 16,
            bottom: 16,
        };
        planes.apply_main(&flat.frame(), &[sub]);
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let inside = (8..16).contains(&x) && (8..16).contains(&y);
                if inside {
                    assert_eq!((planes.y[i], planes.u[i], planes.v[i]), (10, 77, 77));
                } else if !(x % 2 == 0 && y % 2 == 0) {
                    assert_eq!((planes.y[i], planes.u[i]), (src.y[i], src.u[i]));
                }
            }
        }
    }

    #[test]
    fn aux_only_touches_its_rects() {
        let (w, h) = (32, 32);
        let src = pattern(w, h, 5);
        let (_, aux) = split(&src, ChromaLayout::V2);
        let mut planes = Yuv444Planes::new(w as u16, h as u16);
        let sub = ExclusiveRectangle {
            left: 16,
            top: 0,
            right: 32,
            bottom: 16,
        };
        planes.apply_aux(&aux.frame(), ChromaLayout::V2, &[sub]);
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let inside = x >= 16 && y < 16;
                if inside && !(x % 2 == 0 && y % 2 == 0) {
                    assert_eq!(planes.u[i], src.u[i]);
                } else if !inside {
                    assert_eq!((planes.u[i], planes.v[i]), (128, 128));
                }
            }
        }
    }

    #[test]
    fn write_rgba_only_touches_rects() {
        let planes = Yuv444Planes::new(8, 8);
        let mut rgba = vec![9; 8 * 8 * 4];
        let rect = ExclusiveRectangle {
            left: 2,
            top: 2,
            right: 4,
            bottom: 4,
        };
        planes.write_rgba(&[rect], &mut rgba);
        assert_eq!(&rgba[(2 * 8 + 2) * 4..(2 * 8 + 2) * 4 + 4], &[0, 0, 0, 255]);
        assert_eq!(&rgba[0..4], &[9, 9, 9, 9]);
        assert_eq!(&rgba[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4], &[9, 9, 9, 9]);
    }

    #[test]
    fn bt709_full_range_known_colors() {
        assert_eq!(yuv_to_rgb(0, 128, 128), [0, 0, 0]);
        assert_eq!(yuv_to_rgb(255, 128, 128), [255, 255, 255]);
        assert_eq!(yuv_to_rgb(100, 128, 128), [100, 100, 100]);
        // FreeRDP 正向系数（RGB2Y/U/V）下纯红 = (53, 99, 255)、纯蓝 = (17, 255, 116)。
        assert_eq!(yuv_to_rgb(53, 99, 255), [252, 0, 0]);
        assert_eq!(yuv_to_rgb(17, 255, 116), [0, 0, 252]);
    }

    #[test]
    fn nv12_frame_rejects_short_planes() {
        let y = vec![0; 16 * 16];
        let uv = vec![0; 16 * 8];
        assert!(Nv12Frame::new(16, 16, &y, 16, &uv, 16).is_some());
        assert!(Nv12Frame::new(16, 16, &y[..200], 16, &uv, 16).is_none());
        assert!(Nv12Frame::new(16, 16, &y, 16, &uv[..100], 16).is_none());
    }
}
