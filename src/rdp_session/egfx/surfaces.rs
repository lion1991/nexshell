//! EGFX surface 合成层（docs/adr/0008 第②步）。维护 surface RGBA 像素缓冲、离屏
//! cache、Progressive 解码器状态，实现全部 surface/cache 操作并产出脏区。
//! 像素块操作为自由函数（可单测）；Progressive 解码器状态 per-surface 由库内部管理。
//! ClearCodec/AVC420/Uncompressed 由库解好经 on_bitmap_updated → write_bitmap 落盘。

use std::collections::HashMap;

use ironrdp_egfx::client::{BitmapUpdate, Surface as EgfxSurface};
use ironrdp_egfx::pdu::{
    CacheToSurfacePdu, Color, SolidFillPdu, SurfaceToCachePdu, SurfaceToSurfacePdu,
    WireToSurface2Pdu,
};
use ironrdp_graphics::progressive::ProgressiveDecoder;

/// surface 上一块脏区（surface-local 坐标，像素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceRect {
    pub surface_id: u16,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// 客户端 surface：RGBA 像素 + 是否映射到 output（原点）。
pub struct Surface {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
    /// 映射到 output 的原点（像素）；None=离屏。
    pub mapped: Option<(i32, i32)>,
}

impl Surface {
    fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; usize::from(width) * usize::from(height) * 4],
            mapped: None,
        }
    }
}

/// 离屏 cache 项（RGBA）。
struct CachedBitmap {
    width: u16,
    height: u16,
    pixels: Vec<u8>,
}

/// surface 合成器：surface 表 + cache 表 + 单一 Progressive 解码器（内部按
/// surfaceId 管理 tile 状态）。ClearCodec 解码器现由库连接级单例持有（见 ADR 0008 / 上游 #1175）。
pub struct Compositor {
    surfaces: HashMap<u16, Surface>,
    cache: HashMap<u16, CachedBitmap>,
    progressive: ProgressiveDecoder,
    /// Progressive 解码失败累计（供频控日志分类计数）。
    prog_fail_count: u64,
    /// CacheToSurface 引用了不存在的 cache 槽位（静默黑块高嫌疑，频控日志）。
    c2s_miss_count: u64,
}

impl Compositor {
    pub fn new() -> Self {
        Self {
            surfaces: HashMap::new(),
            cache: HashMap::new(),
            progressive: ProgressiveDecoder::new(),
            prog_fail_count: 0,
            c2s_miss_count: 0,
        }
    }

    /// 遍历所有已映射 surface（用于 EndFrame 合成到 framebuffer）。
    pub fn mapped_surfaces(&self) -> impl Iterator<Item = (i32, i32, &Surface)> {
        self.surfaces.values().filter_map(|s| {
            let (ox, oy) = s.mapped?;
            Some((ox, oy, s))
        })
    }

    /// 取 surface 上一个像素 RGBA（trace 黑块溯源用）。
    pub fn sample_pixel(&self, surface_id: u16, x: u16, y: u16) -> Option<[u8; 4]> {
        let s = self.surfaces.get(&surface_id)?;
        if x >= s.width || y >= s.height {
            return None;
        }
        let o = (usize::from(y) * usize::from(s.width) + usize::from(x)) * 4;
        s.pixels.get(o..o + 4).map(|p| [p[0], p[1], p[2], p[3]])
    }

    pub fn surface_origin(&self, surface_id: u16) -> Option<(i32, i32)> {
        self.surfaces.get(&surface_id).and_then(|s| s.mapped)
    }

    // ---- 生命周期 ----

    pub fn create_surface(&mut self, surface: &EgfxSurface) {
        self.surfaces
            .insert(surface.id, Surface::new(surface.width, surface.height));
    }

    pub fn delete_surface(&mut self, surface_id: u16) {
        self.surfaces.remove(&surface_id);
        // Progressive tile 系数状态随 surface 存亡（对齐 FreeRDP）。
        self.progressive.delete_surface(surface_id);
    }

    pub fn map_surface(&mut self, surface_id: u16, origin_x: i32, origin_y: i32) {
        if let Some(s) = self.surfaces.get_mut(&surface_id) {
            s.mapped = Some((origin_x, origin_y));
        }
    }

    /// ResetGraphics（桌面/监视器布局变更）：销毁并等服务端重建 surface、重置 Progressive
    /// 上下文；**离屏 cache 保留**——MS-RDPEGFX 未规定 ResetGraphics 清 cache，FreeRDP
    /// gdi/gfx.c 亦只清 SurfaceTable 而留 cacheSlots，清了会致跨 reset 的 CacheToSurface 取空出黑块。
    pub fn reset(&mut self) {
        self.surfaces.clear();
        self.progressive.reset();
    }

    /// DeleteEncodingContext：**不清** Progressive tile 状态。对齐 FreeRDP（gfx.c 里是
    /// no-op）：服务端频繁按 codec context 发 DEC，若据此清 per-surface 系数状态，随后的
    /// UPGRADE tile 会在全零系数上升级 → 输出中性灰 (128,128,128) 块并被 tile cache 固化。
    /// tile 状态只随 surface 删除/reset 消亡（见 delete_surface / reset）。
    pub fn delete_encoding_context(&mut self, _surface_id: u16) {}

    // ---- 写入 ----

    /// lib 已解好的 Uncompressed/AVC420 输出（RGBA）→ 写入 surface。
    pub fn write_bitmap(&mut self, update: &BitmapUpdate) -> Option<SurfaceRect> {
        let surface = self.surfaces.get_mut(&update.surface_id)?;
        let rect = &update.destination_rectangle;
        blit(
            &mut surface.pixels,
            surface.width,
            surface.height,
            i32::from(rect.left),
            i32::from(rect.top),
            &update.data,
            update.width,
            update.height,
        )
        .map(|(x, y, w, h)| SurfaceRect {
            surface_id: update.surface_id,
            x,
            y,
            w,
            h,
        })
    }

    /// WireToSurface2 + RemoteFX Progressive：解码 tile（RGBA 64x64）→ 写入 surface。
    /// 失败频控日志：每类前 5 次 + 每 300 次汇总（真机上失败会刷屏，见 ADR 0008）。
    /// 返回 `(脏矩形, 解码是否真失败)`；后者用于诊断区分真 Err 与正常 0-tile PDU。
    pub fn write_progressive(&mut self, pdu: &WireToSurface2Pdu) -> (Vec<SurfaceRect>, bool) {
        let Some(surface) = self.surfaces.get_mut(&pdu.surface_id) else {
            return (Vec::new(), false);
        };
        let tiles = match self.progressive.decode_bitmap(
            pdu.surface_id,
            surface.width,
            surface.height,
            &pdu.bitmap_data,
        ) {
            Ok(t) => t,
            Err(e) => {
                self.prog_fail_count += 1;
                let n = self.prog_fail_count;
                if n <= 5 || n.is_multiple_of(300) {
                    eprintln!(
                        "[egfx] progressive decode failed (#{n}) surface={}: {e}",
                        pdu.surface_id
                    );
                }
                return (Vec::new(), true);
            }
        };
        let mut dirties = Vec::with_capacity(tiles.len());
        for tile in tiles {
            let tx = i32::from(tile.x_idx) * 64;
            let ty = i32::from(tile.y_idx) * 64;
            if let Some((x, y, w, h)) = blit(
                &mut surface.pixels,
                surface.width,
                surface.height,
                tx,
                ty,
                &tile.pixels,
                64,
                64,
            ) {
                dirties.push(SurfaceRect {
                    surface_id: pdu.surface_id,
                    x,
                    y,
                    w,
                    h,
                });
            }
        }
        (dirties, false)
    }

    // ---- surface / cache 操作 ----

    pub fn solid_fill(&mut self, pdu: &SolidFillPdu) -> Vec<SurfaceRect> {
        let Some(surface) = self.surfaces.get_mut(&pdu.surface_id) else {
            return Vec::new();
        };
        let rgba = color_to_rgba(&pdu.fill_pixel);
        let mut dirties = Vec::new();
        for rect in &pdu.rectangles {
            let w = rect.right.saturating_sub(rect.left);
            let h = rect.bottom.saturating_sub(rect.top);
            if let Some(d) = fill_rect(
                &mut surface.pixels,
                surface.width,
                surface.height,
                i32::from(rect.left),
                i32::from(rect.top),
                w,
                h,
                rgba,
            ) {
                dirties.push(SurfaceRect {
                    surface_id: pdu.surface_id,
                    x: d.0,
                    y: d.1,
                    w: d.2,
                    h: d.3,
                });
            }
        }
        dirties
    }

    pub fn surface_to_surface(&mut self, pdu: &SurfaceToSurfacePdu) -> Vec<SurfaceRect> {
        let rect = &pdu.source_rectangle;
        let w = rect.right.saturating_sub(rect.left);
        let h = rect.bottom.saturating_sub(rect.top);
        // 先从源 surface 抽出区域（不可变借用），再写入目标（可变借用）。
        let region = {
            let Some(src) = self.surfaces.get(&pdu.source_surface_id) else {
                return Vec::new();
            };
            extract_region(
                &src.pixels,
                src.width,
                src.height,
                rect.left,
                rect.top,
                w,
                h,
            )
        };
        let Some(dst) = self.surfaces.get_mut(&pdu.destination_surface_id) else {
            return Vec::new();
        };
        let mut dirties = Vec::new();
        for point in &pdu.destination_points {
            if let Some(d) = blit(
                &mut dst.pixels,
                dst.width,
                dst.height,
                i32::from(point.x),
                i32::from(point.y),
                &region,
                w,
                h,
            ) {
                dirties.push(SurfaceRect {
                    surface_id: pdu.destination_surface_id,
                    x: d.0,
                    y: d.1,
                    w: d.2,
                    h: d.3,
                });
            }
        }
        dirties
    }

    pub fn surface_to_cache(&mut self, pdu: &SurfaceToCachePdu) {
        let rect = &pdu.source_rectangle;
        let w = rect.right.saturating_sub(rect.left);
        let h = rect.bottom.saturating_sub(rect.top);
        let Some(src) = self.surfaces.get(&pdu.surface_id) else {
            return;
        };
        let pixels = extract_region(
            &src.pixels,
            src.width,
            src.height,
            rect.left,
            rect.top,
            w,
            h,
        );
        if std::env::var_os("NEXSHELL_RDP_EGFX_TRACE").is_some()
            && pixels
                .chunks_exact(4)
                .all(|p| p[0] < 8 && p[1] < 8 && p[2] < 8)
        {
            eprintln!(
                "[egfx] s2c captured ALL-BLACK slot={} rect=({},{})-({},{})",
                pdu.cache_slot, rect.left, rect.top, rect.right, rect.bottom
            );
        }
        self.cache.insert(
            pdu.cache_slot,
            CachedBitmap {
                width: w,
                height: h,
                pixels,
            },
        );
    }

    pub fn cache_to_surface(&mut self, pdu: &CacheToSurfacePdu) -> Vec<SurfaceRect> {
        let Some(entry) = self.cache.get(&pdu.cache_slot) else {
            self.c2s_miss_count += 1;
            let n = self.c2s_miss_count;
            if n <= 10 || n.is_multiple_of(300) {
                eprintln!(
                    "[egfx] cache_to_surface MISS (#{n}) slot={} surface={} points={}",
                    pdu.cache_slot,
                    pdu.surface_id,
                    pdu.destination_points.len()
                );
            }
            return Vec::new();
        };
        let (cw, ch, pixels) = (entry.width, entry.height, entry.pixels.clone());
        if std::env::var_os("NEXSHELL_RDP_EGFX_TRACE").is_some()
            && pixels
                .chunks_exact(4)
                .all(|p| p[0] < 8 && p[1] < 8 && p[2] < 8)
        {
            eprintln!(
                "[egfx] c2s stamping ALL-BLACK slot={} surf={} n={}",
                pdu.cache_slot,
                pdu.surface_id,
                pdu.destination_points.len()
            );
        }
        let Some(dst) = self.surfaces.get_mut(&pdu.surface_id) else {
            return Vec::new();
        };
        let mut dirties = Vec::new();
        for point in &pdu.destination_points {
            if let Some(d) = blit(
                &mut dst.pixels,
                dst.width,
                dst.height,
                i32::from(point.x),
                i32::from(point.y),
                &pixels,
                cw,
                ch,
            ) {
                dirties.push(SurfaceRect {
                    surface_id: pdu.surface_id,
                    x: d.0,
                    y: d.1,
                    w: d.2,
                    h: d.3,
                });
            }
        }
        dirties
    }

    pub fn evict_cache(&mut self, cache_slot: u16) {
        self.cache.remove(&cache_slot);
    }
}

// ============================================================================
// 像素块自由函数（可单测）
// ============================================================================

/// RDPGFX_COLOR32（b,g,r,xa）→ RGBA（不透明）。
fn color_to_rgba(c: &Color) -> [u8; 4] {
    [c.r, c.g, c.b, 0xFF]
}

/// 把 `src`（src_w×src_h RGBA）拷进 `dst`（dst_w×dst_h RGBA）(dx,dy) 处，裁剪越界/负偏移。
/// 返回实际写入的 dst 脏区 (x,y,w,h)，全裁剪则 None。
fn blit(
    dst: &mut [u8],
    dst_w: u16,
    dst_h: u16,
    dx: i32,
    dy: i32,
    src: &[u8],
    src_w: u16,
    src_h: u16,
) -> Option<(u16, u16, u16, u16)> {
    let (dst_w, dst_h) = (i32::from(dst_w), i32::from(dst_h));
    let (src_w, src_h) = (i32::from(src_w), i32::from(src_h));
    // 目标可见范围。
    let x0 = dx.max(0);
    let y0 = dy.max(0);
    let x1 = (dx + src_w).min(dst_w);
    let y1 = (dy + src_h).min(dst_h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let copy_w = (x1 - x0) as usize;
    let dst_stride = dst_w as usize * 4;
    let src_stride = src_w as usize * 4;
    for row in y0..y1 {
        let src_row = (row - dy) as usize;
        let src_col = (x0 - dx) as usize;
        let so = src_row * src_stride + src_col * 4;
        let dofs = row as usize * dst_stride + x0 as usize * 4;
        let n = copy_w * 4;
        if so + n > src.len() || dofs + n > dst.len() {
            continue;
        }
        dst[dofs..dofs + n].copy_from_slice(&src[so..so + n]);
    }
    Some((x0 as u16, y0 as u16, (x1 - x0) as u16, (y1 - y0) as u16))
}

/// 用单色 `rgba` 填 `dst` (x,y,w,h) 矩形，裁剪越界。返回实际脏区，全裁剪则 None。
fn fill_rect(
    dst: &mut [u8],
    dst_w: u16,
    dst_h: u16,
    x: i32,
    y: i32,
    w: u16,
    h: u16,
    rgba: [u8; 4],
) -> Option<(u16, u16, u16, u16)> {
    let (dst_w, dst_h) = (i32::from(dst_w), i32::from(dst_h));
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + i32::from(w)).min(dst_w);
    let y1 = (y + i32::from(h)).min(dst_h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let dst_stride = dst_w as usize * 4;
    for row in y0..y1 {
        for col in x0..x1 {
            let o = row as usize * dst_stride + col as usize * 4;
            if o + 4 <= dst.len() {
                dst[o..o + 4].copy_from_slice(&rgba);
            }
        }
    }
    Some((x0 as u16, y0 as u16, (x1 - x0) as u16, (y1 - y0) as u16))
}

/// 从 `src`（src_w×src_h RGBA）抽出 (x,y,w,h) 区域为独立 RGBA（越界部分留 0）。
fn extract_region(src: &[u8], src_w: u16, src_h: u16, x: u16, y: u16, w: u16, h: u16) -> Vec<u8> {
    let mut out = vec![0u8; usize::from(w) * usize::from(h) * 4];
    let src_stride = usize::from(src_w) * 4;
    let out_stride = usize::from(w) * 4;
    for row in 0..usize::from(h) {
        let sy = usize::from(y) + row;
        if sy >= usize::from(src_h) {
            break;
        }
        for col in 0..usize::from(w) {
            let sx = usize::from(x) + col;
            if sx >= usize::from(src_w) {
                break;
            }
            let so = sy * src_stride + sx * 4;
            let oo = row * out_stride + col * 4;
            if so + 4 <= src.len() && oo + 4 <= out.len() {
                out[oo..oo + 4].copy_from_slice(&src[so..so + 4]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use ironrdp_pdu::geometry::ExclusiveRectangle;

    use super::*;

    /// EGFX 矩形 exclusive（right/bottom 是 one-past-end，宽=right-left）。
    fn excl(left: u16, top: u16, right: u16, bottom: u16) -> ExclusiveRectangle {
        ExclusiveRectangle {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn blit_clips_to_target() {
        // 2x2 dst 全 0，src 4x4 全 7 从 (1,1) 起 → 只右下角 1x1 被写。
        let mut dst = vec![0u8; 2 * 2 * 4];
        let src = vec![7u8; 4 * 4 * 4];
        let d = blit(&mut dst, 2, 2, 1, 1, &src, 4, 4).unwrap();
        assert_eq!(d, (1, 1, 1, 1));
        // 像素 (1,1) 被写 7，其余仍 0。
        assert!(dst[(1 * 2 + 1) * 4..(1 * 2 + 1) * 4 + 4]
            .iter()
            .all(|&b| b == 7));
        assert!(dst[0..4].iter().all(|&b| b == 0));
    }

    #[test]
    fn blit_negative_offset_clips_left_top() {
        let mut dst = vec![0u8; 4 * 4 * 4];
        let src = vec![9u8; 2 * 2 * 4];
        // 从 (-1,-1) 起 → 只 src 的右下 1x1 落在 dst (0,0)。
        let d = blit(&mut dst, 4, 4, -1, -1, &src, 2, 2).unwrap();
        assert_eq!(d, (0, 0, 1, 1));
        assert!(dst[0..4].iter().all(|&b| b == 9));
    }

    #[test]
    fn solid_fill_writes_rect() {
        let mut c = Compositor::new();
        c.surfaces.insert(1, Surface::new(4, 4));
        let pdu = SolidFillPdu {
            surface_id: 1,
            fill_pixel: Color {
                b: 10,
                g: 20,
                r: 30,
                xa: 0,
            },
            rectangles: vec![excl(1, 1, 3, 3)],
        };
        let d = c.solid_fill(&pdu);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].x, d[0].y, d[0].w, d[0].h), (1, 1, 2, 2));
        let s = &c.surfaces[&1];
        // (1,1) = RGBA(30,20,10,255)
        let o = (1 * 4 + 1) * 4;
        assert_eq!(&s.pixels[o..o + 4], &[30, 20, 10, 255]);
        // (0,0) 未动。
        assert_eq!(&s.pixels[0..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn surface_to_surface_copies_region() {
        let mut c = Compositor::new();
        let mut src = Surface::new(4, 4);
        // 源 (0,0) 填 5。
        src.pixels[0..4].copy_from_slice(&[5, 5, 5, 5]);
        c.surfaces.insert(1, src);
        c.surfaces.insert(2, Surface::new(4, 4));
        let pdu = SurfaceToSurfacePdu {
            source_surface_id: 1,
            destination_surface_id: 2,
            source_rectangle: excl(0, 0, 1, 1),
            destination_points: vec![ironrdp_egfx::pdu::Point { x: 2, y: 2 }],
        };
        let d = c.surface_to_surface(&pdu);
        assert_eq!(d.len(), 1);
        let dst = &c.surfaces[&2];
        let o = (2 * 4 + 2) * 4;
        assert_eq!(&dst.pixels[o..o + 4], &[5, 5, 5, 5]);
    }

    #[test]
    fn cache_roundtrip() {
        let mut c = Compositor::new();
        let mut s = Surface::new(4, 4);
        s.pixels[0..4].copy_from_slice(&[1, 2, 3, 4]);
        c.surfaces.insert(1, s);
        c.surface_to_cache(&SurfaceToCachePdu {
            surface_id: 1,
            cache_key: 0,
            cache_slot: 7,
            source_rectangle: excl(0, 0, 1, 1),
        });
        c.surfaces.insert(2, Surface::new(4, 4));
        let d = c.cache_to_surface(&CacheToSurfacePdu {
            cache_slot: 7,
            surface_id: 2,
            destination_points: vec![ironrdp_egfx::pdu::Point { x: 1, y: 1 }],
        });
        assert_eq!(d.len(), 1);
        let dst = &c.surfaces[&2];
        let o = (1 * 4 + 1) * 4;
        assert_eq!(&dst.pixels[o..o + 4], &[1, 2, 3, 4]);
        // evict 后再取空。
        c.evict_cache(7);
        let d2 = c.cache_to_surface(&CacheToSurfacePdu {
            cache_slot: 7,
            surface_id: 2,
            destination_points: vec![ironrdp_egfx::pdu::Point { x: 0, y: 0 }],
        });
        assert!(d2.is_empty());
    }

    #[test]
    fn reset_retains_cache() {
        // ResetGraphics 清 surface 但留 cache：缓存内容 reset 后仍可 CacheToSurface。
        let mut c = Compositor::new();
        let mut s = Surface::new(4, 4);
        s.pixels[0..4].copy_from_slice(&[8, 8, 8, 8]);
        c.surfaces.insert(1, s);
        c.surface_to_cache(&SurfaceToCachePdu {
            surface_id: 1,
            cache_key: 0,
            cache_slot: 3,
            source_rectangle: excl(0, 0, 1, 1),
        });
        c.reset();
        assert!(c.surfaces.is_empty(), "surfaces 应被清");
        assert!(c.cache.contains_key(&3), "cache 应保留");
        // reset 后新建 surface 仍能取回缓存像素。
        c.surfaces.insert(2, Surface::new(4, 4));
        let d = c.cache_to_surface(&CacheToSurfacePdu {
            cache_slot: 3,
            surface_id: 2,
            destination_points: vec![ironrdp_egfx::pdu::Point { x: 0, y: 0 }],
        });
        assert_eq!(d.len(), 1);
        assert_eq!(&c.surfaces[&2].pixels[0..4], &[8, 8, 8, 8]);
    }

    #[test]
    fn s2s_same_surface_overlap() {
        // 同 surface 重叠拷贝（滚动典型）：源 (0,0) 拷到 (1,0)，先抽独立缓冲再写，
        // 不因原地重叠而污染。src 行 [A,B,_,_] → 拷 (0,0)2x1 到 (1,0) → [A,A,B,_]。
        let mut c = Compositor::new();
        let mut s = Surface::new(4, 1);
        s.pixels[0..4].copy_from_slice(&[1, 1, 1, 1]); // px0 = A
        s.pixels[4..8].copy_from_slice(&[2, 2, 2, 2]); // px1 = B
        c.surfaces.insert(1, s);
        let d = c.surface_to_surface(&SurfaceToSurfacePdu {
            source_surface_id: 1,
            destination_surface_id: 1,
            source_rectangle: excl(0, 0, 2, 1), // 抽 px0..2 = [A,B]
            destination_points: vec![ironrdp_egfx::pdu::Point { x: 1, y: 0 }],
        });
        assert_eq!(d.len(), 1);
        let p = &c.surfaces[&1].pixels;
        assert_eq!(&p[0..4], &[1, 1, 1, 1]); // px0 未动 = A
        assert_eq!(&p[4..8], &[1, 1, 1, 1]); // px1 = 源 px0 = A
        assert_eq!(&p[8..12], &[2, 2, 2, 2]); // px2 = 源 px1 = B（未被中间态污染）
    }

    /// EGFX exclusive 矩形 → surface 写回区域（假设 1 回归锁定）。
    /// WireToSurface1/2 的 destination_rectangle 是 ExclusiveRectangle：
    /// 宽=right-left、高=bottom-top，写回覆盖 [left,right)×[top,bottom)，不含端点行/列。
    /// write_bitmap 以 rect.left/top 为原点、update.width/height（=库按 right-left 算好）为源尺寸
    /// blit —— 此测经 blit 直接验证该映射，防再退回 inclusive(+1)。
    #[test]
    fn exclusive_rect_maps_to_writeback_region() {
        // exclusive {left:1,top:1,right:3,bottom:3} → 宽2 高2，覆盖 x∈[1,3) y∈[1,3)。
        let r = excl(1, 1, 3, 3);
        let w = r.right - r.left; // 2
        let h = r.bottom - r.top; // 2
        assert_eq!((w, h), (2, 2));

        let mut dst = vec![0u8; 4 * 4 * 4];
        let src = vec![7u8; usize::from(w) * usize::from(h) * 4];
        let d = blit(
            &mut dst,
            4,
            4,
            i32::from(r.left),
            i32::from(r.top),
            &src,
            w,
            h,
        )
        .unwrap();
        // 写回区域正是 (1,1,2,2)，右/下端点行列（x=3,y=3）不写。
        assert_eq!(d, (1, 1, 2, 2));
        // (1,1) 与 (2,2) 被写，(3,3)/(0,0) 未动。
        let at = |x: usize, y: usize| (y * 4 + x) * 4;
        assert!(dst[at(1, 1)..at(1, 1) + 4].iter().all(|&b| b == 7));
        assert!(dst[at(2, 2)..at(2, 2) + 4].iter().all(|&b| b == 7));
        assert!(dst[at(3, 3)..at(3, 3) + 4].iter().all(|&b| b == 0));
        assert!(dst[at(0, 0)..at(0, 0) + 4].iter().all(|&b| b == 0));
    }
}
