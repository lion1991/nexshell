//! AVC444 接入（docs/adr/0015）：解析 WireToSurface1 的 RFX_AVC444_BITMAP_STREAM，按 LC 把
//! 主流/辅流依线上顺序送进**同一个** VT 解码器（两路是同一条码流：frame_num 连续编号，
//! 主帧引用长期参考 0、辅帧引用长期参考 1，辅流也会带 IDR），解出的 NV12 合成进
//! surface 级 YUV444，再把区域矩形转 RGBA 写回 surface。区域矩形是 surface 坐标。

use std::collections::HashMap;

use ironrdp_core::decode;
use ironrdp_egfx::decode::H264Decoder as _;
use ironrdp_egfx::pdu::{Avc444BitmapStream, Codec1Type, Encoding, WireToSurface1Pdu};

use super::avc444::{ChromaLayout, Yuv444Planes};
use super::decoder_vt::VtH264Decoder;
use super::surfaces::{Compositor, SurfaceRect};

pub struct Avc444Stream {
    decoder: VtH264Decoder,
    planes: HashMap<u16, Yuv444Planes>,
}

impl Avc444Stream {
    pub fn new() -> Self {
        Self {
            decoder: VtH264Decoder::with_label("AVC444"),
            planes: HashMap::new(),
        }
    }

    /// ResetGraphics：surface 全部重建，码流也会从 IDR 重新开始。
    pub fn reset(&mut self) {
        self.decoder.reset();
        self.planes.clear();
    }

    pub fn remove_surface(&mut self, surface_id: u16) {
        self.planes.remove(&surface_id);
    }

    /// 解码并写回 surface，返回脏区（surface 坐标）。失败时 surface 像素不动。
    pub fn apply(
        &mut self,
        compositor: &mut Compositor,
        pdu: &WireToSurface1Pdu,
    ) -> Result<Vec<SurfaceRect>, String> {
        let layout = match pdu.codec_id {
            Codec1Type::Avc444 => ChromaLayout::V1,
            Codec1Type::Avc444v2 => ChromaLayout::V2,
            other => return Err(format!("not an AVC444 codec: {other:?}")),
        };
        let stream = decode::<Avc444BitmapStream<'_>>(&pdu.bitmap_data)
            .map_err(|e| format!("AVC444 parse failed: {e}"))?;
        let surface = compositor
            .surface_mut(pdu.surface_id)
            .ok_or("unknown surface")?;
        let (width, height) = (surface.width, surface.height);

        let Self { decoder, planes } = self;
        let planes = planes
            .entry(pdu.surface_id)
            .or_insert_with(|| Yuv444Planes::new(width, height));
        if planes.size() != (width, height) {
            *planes = Yuv444Planes::new(width, height);
        }

        // LC=2 时辅流放在第一路的位置。
        let (main, aux) = if stream.encoding == Encoding::CHROMA {
            (None, Some(&stream.stream1))
        } else {
            (Some(&stream.stream1), stream.stream2.as_ref())
        };
        let mut rects = Vec::new();
        if let Some(main) = main {
            decoder
                .decode_nv12(main.data, |frame| {
                    planes.apply_main(frame, &main.rectangles)
                })
                .map_err(|e| format!("AVC444 main: {e}"))?;
            rects.extend_from_slice(&main.rectangles);
        }
        if let Some(aux) = aux {
            decoder
                .decode_nv12(aux.data, |frame| {
                    planes.apply_aux(frame, layout, &aux.rectangles)
                })
                .map_err(|e| format!("AVC444 aux: {e}"))?;
            rects.extend_from_slice(&aux.rectangles);
        }
        planes.write_rgba(&rects, &mut surface.pixels);

        Ok(rects
            .iter()
            .filter_map(|r| {
                let right = r.right.min(width);
                let bottom = r.bottom.min(height);
                (r.left < right && r.top < bottom).then_some(SurfaceRect {
                    surface_id: pdu.surface_id,
                    x: r.left,
                    y: r.top,
                    w: right - r.left,
                    h: bottom - r.top,
                })
            })
            .collect())
    }
}
