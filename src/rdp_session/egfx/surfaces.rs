//! EGFX surface 合成层（docs/adr/0008 第②步）。维护 surface RGBA 像素缓冲、离屏
//! cache、Progressive 解码器状态，实现全部 surface/cache 操作并产出脏区。
//! 像素块操作为自由函数（可单测）；Progressive 解码器状态 per-surface 由库内部管理。
//! ClearCodec/AVC420/Uncompressed 由库解好经 on_bitmap_updated → write_bitmap 落盘。

use std::collections::{HashMap, HashSet};

use ironrdp_egfx::client::BitmapUpdate;
use ironrdp_egfx::pdu::{
    CacheToSurfacePdu, Color, SolidFillPdu, SurfaceToCachePdu, SurfaceToSurfacePdu,
    WireToSurface2Pdu,
};
use ironrdp_graphics::progressive::ProgressiveDecoder;
use ironrdp_pdu::codecs::rfx::progressive::{
    decode_progressive_stream, ProgressiveBlock, ProgressiveTile,
};
use ironrdp_pdu::codecs::rfx::RfxRectangle;

/// 单 surface 边长上限（8192 → 单块 ≤256 MiB）。远端 u16 宽高可直接索要近 16 GiB。
const MAX_SURFACE_DIM: u16 = 8192;
/// 会话像素总预算（surface + cache 合计）：1 GiB。
const MAX_PIXEL_BUDGET: usize = 1 << 30;
/// 传给库的固定 Progressive codec context id：**忽略** wire 上的 codecContextId，
/// 让 tile 状态按 surface 单份存（对齐 FreeRDP progressive.c 的 per-surface 模型）。
const PROGRESSIVE_CTX: u32 = 0;

/// 远端请求的像素分配超预算：拒绝分配，调用方据此断开会话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelBudgetExceeded {
    pub what: &'static str,
    pub requested: usize,
    pub limit: usize,
}

impl std::fmt::Display for PixelBudgetExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "EGFX {} 超预算：请求 {} 字节 > 上限 {} 字节",
            self.what, self.requested, self.limit
        )
    }
}

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

#[derive(Default)]
struct ProgressiveFramePaintState {
    frame_id: Option<u32>,
    updated_tiles: Vec<(u16, u16)>,
    /// 帧内各 tile 最新重建像素（64x64 RGBA）。上游解码器不再暴露 tile 状态，
    /// 后续 PDU 的 REGION 命中先前 tile 时从这里重绘。
    tile_pixels: HashMap<(u16, u16), Vec<u8>>,
    /// updated_tiles 的成员集，避免同 frame_id 跨多 PDU 累积时 O(n²) 线性去重。
    updated_set: HashSet<(u16, u16)>,
}

#[derive(Default)]
struct ProgressivePaintPlan {
    clip_rects: Vec<RfxRectangle>,
    updated_tiles: Vec<(u16, u16)>,
}

/// surface 合成器：surface 表 + cache 表 + 单一 Progressive 解码器（内部按
/// surfaceId 管理 tile 状态）。ClearCodec 解码器现由库连接级单例持有（见 ADR 0008 / 上游 #1175）。
pub struct Compositor {
    surfaces: HashMap<u16, Surface>,
    cache: HashMap<u16, CachedBitmap>,
    /// surface + cache 已占像素字节，用于会话总预算。
    allocated_bytes: usize,
    progressive: ProgressiveDecoder,
    /// 本地统计：收到的 DeleteEncodingContext 数（仅观测，DEC 本身是 no-op）。
    dec_count: u64,
    progressive_frames: HashMap<u16, ProgressiveFramePaintState>,
    fallback_progressive_frame_id: u32,
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
            allocated_bytes: 0,
            progressive: ProgressiveDecoder::new(),
            dec_count: 0,
            progressive_frames: HashMap::new(),
            fallback_progressive_frame_id: 0,
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

    /// 按远端宽高分配 surface；超单块尺寸或会话总预算则拒绝并报错（调用方断开会话）。
    pub fn create_surface(
        &mut self,
        id: u16,
        width: u16,
        height: u16,
    ) -> Result<(), PixelBudgetExceeded> {
        let bytes = usize::from(width) * usize::from(height) * 4;
        if width > MAX_SURFACE_DIM || height > MAX_SURFACE_DIM {
            return Err(PixelBudgetExceeded {
                what: "surface 尺寸",
                requested: bytes,
                limit: usize::from(MAX_SURFACE_DIM) * usize::from(MAX_SURFACE_DIM) * 4,
            });
        }
        // 同 id 重建先归还旧账。
        let freed = self.surfaces.get(&id).map_or(0, |s| s.pixels.len());
        let after = self.allocated_bytes.saturating_sub(freed) + bytes;
        if after > MAX_PIXEL_BUDGET {
            return Err(PixelBudgetExceeded {
                what: "会话像素总量",
                requested: after,
                limit: MAX_PIXEL_BUDGET,
            });
        }
        self.allocated_bytes = after;
        self.surfaces.insert(id, Surface::new(width, height));
        Ok(())
    }

    pub fn delete_surface(&mut self, surface_id: u16) {
        if let Some(s) = self.surfaces.remove(&surface_id) {
            self.allocated_bytes = self.allocated_bytes.saturating_sub(s.pixels.len());
        }
        // Progressive tile 系数状态随 surface 存亡（对齐 FreeRDP）。
        self.progressive.delete_surface(surface_id);
        self.progressive_frames.remove(&surface_id);
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
        for s in self.surfaces.values() {
            self.allocated_bytes = self.allocated_bytes.saturating_sub(s.pixels.len());
        }
        self.surfaces.clear();
        // 只 reset contexts：不能在这里逐 surface delete_surface——那会连带清
        // surface_context_flags，若服务端 ResetGraphics 后不重发 CONTEXT 块即
        // MissingBlock("CONTEXT") 画面冻结。references 有界（每 surface ≈12MB），非泄漏源。
        self.progressive.reset();
        self.progressive_frames.clear();
    }

    /// DeleteEncodingContext：no-op（对齐 FreeRDP）。tile 状态按 surface 存，
    /// codecContextId 不参与键（见 `PROGRESSIVE_CTX`），故 DEC 无需释放任何东西；
    /// 状态只随 surface delete/reset 消亡。这里只计数供诊断观测。
    pub fn delete_encoding_context(&mut self, _surface_id: u16, _codec_context_id: u32) {
        self.dec_count += 1;
    }

    /// 收到的 DeleteEncodingContext 累计数（诊断）。
    pub fn dec_count(&self) -> u64 {
        self.dec_count
    }

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
    pub fn write_progressive(
        &mut self,
        pdu: &WireToSurface2Pdu,
        egfx_frame_id: Option<u32>,
    ) -> (Vec<SurfaceRect>, bool) {
        let Some(surface) = self.surfaces.get_mut(&pdu.surface_id) else {
            return (Vec::new(), false);
        };
        let paint_plan = progressive_paint_plan(&pdu.bitmap_data);
        // 固定 ctx：pdu.codec_context_id 被有意忽略（Windows 每帧换 id，按 id 分存状态
        // 会让 DIFFERENCE/UPGRADE 找不到前序系数 → 绿块，见 ADR 0008 第⑥步）。
        let tiles = match self.progressive.decode_bitmap(
            pdu.surface_id,
            PROGRESSIVE_CTX,
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
        let frame_id = egfx_frame_id.unwrap_or_else(|| {
            self.fallback_progressive_frame_id = self.fallback_progressive_frame_id.wrapping_add(1);
            self.fallback_progressive_frame_id
        });
        let frame = self.progressive_frames.entry(pdu.surface_id).or_default();
        if frame.frame_id != Some(frame_id) {
            frame.frame_id = Some(frame_id);
            frame.updated_tiles.clear();
            frame.updated_set.clear();
            frame.tile_pixels.clear();
        }
        for tile in tiles {
            frame
                .tile_pixels
                .insert((tile.x_idx, tile.y_idx), tile.pixels);
        }
        for tile in paint_plan.updated_tiles {
            if frame.updated_set.insert(tile) {
                frame.updated_tiles.push(tile);
            }
        }

        let trace = std::env::var_os("NEXSHELL_RDP_EGFX_TRACE").is_some();
        // 诊断 A/B 开关：设 NEXSHELL_RDP_PROG_NOCLIP=1 回退整块 64x64 blit（不按 REGION rects
        // 裁剪）。用于判定拖窗轨迹残留是否由 tile 裁剪漏掉擦除更新所致（对照旧行为）。
        let noclip = std::env::var_os("NEXSHELL_RDP_PROG_NOCLIP").is_some();
        let mut dirties = Vec::with_capacity(frame.updated_tiles.len());
        for &(tile_x, tile_y) in &frame.updated_tiles {
            // 先算裁剪子矩形：与本 PDU region 不相交的累积 tile，其重建结果本会被下方 clip 丢弃，
            // 故跳过重建（reconstruct_to_rgba 是 IDWT，最重的活）。上屏像素逐字节不变。
            let sub_rects = if noclip {
                Vec::new()
            } else {
                clip_tile_to_rects(tile_x, tile_y, &paint_plan.clip_rects)
            };
            if !noclip && sub_rects.is_empty() {
                continue;
            }
            let Some(pixels) = frame.tile_pixels.get(&(tile_x, tile_y)) else {
                continue;
            };
            let tx = i32::from(tile_x) * 64;
            let ty = i32::from(tile_y) * 64;
            if trace && has_dark_vline(pixels, 64, 64, 16) {
                eprintln!("[egfx] prog HASLINE tile=({tx},{ty})");
            }
            if noclip {
                if let Some((x, y, w, h)) = blit(
                    &mut surface.pixels,
                    surface.width,
                    surface.height,
                    tx,
                    ty,
                    pixels,
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
                continue;
            }
            // 只把 REGION rects 裁出的子矩形上屏（对齐 FreeRDP progressive_decompress）：
            // 整块 blit 会把 RFX 影子状态里已被其他 codec 重绘区域的陈旧内容复活
            // （拖窗描边残留/闪烁根因，ADR 0008 第⑤步）。sub_rects 已在重建前算好。
            for sr in sub_rects {
                if let Some((x, y, w, h)) = blit_sub(
                    &mut surface.pixels,
                    surface.width,
                    surface.height,
                    tx + i32::from(sr.x),
                    ty + i32::from(sr.y),
                    pixels,
                    64,
                    (sr.x, sr.y, sr.w, sr.h),
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
            // 源矩形越界诊断：extract_region 零填充会造成"擦除缺失"型残留
            if rect.right > src.width || rect.bottom > src.height {
                eprintln!(
                    "[egfx] s2s SRC-OOB src_surf={} dst_surf={} src=({},{})-({},{}) src_size={}x{} n={}",
                    pdu.source_surface_id,
                    pdu.destination_surface_id,
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.bottom,
                    src.width,
                    src.height,
                    pdu.destination_points.len()
                );
            }
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

    /// 缓存 surface 区域；计入会话总预算，超限拒绝并报错（调用方断开会话）。
    pub fn surface_to_cache(&mut self, pdu: &SurfaceToCachePdu) -> Result<(), PixelBudgetExceeded> {
        let rect = &pdu.source_rectangle;
        let w = rect.right.saturating_sub(rect.left);
        let h = rect.bottom.saturating_sub(rect.top);
        let Some(src) = self.surfaces.get(&pdu.surface_id) else {
            return Ok(());
        };
        let freed = self
            .cache
            .get(&pdu.cache_slot)
            .map_or(0, |c| c.pixels.len());
        let after =
            self.allocated_bytes.saturating_sub(freed) + usize::from(w) * usize::from(h) * 4;
        if after > MAX_PIXEL_BUDGET {
            return Err(PixelBudgetExceeded {
                what: "会话像素总量",
                requested: after,
                limit: MAX_PIXEL_BUDGET,
            });
        }
        let pixels = extract_region(
            &src.pixels,
            src.width,
            src.height,
            rect.left,
            rect.top,
            w,
            h,
        );
        if std::env::var_os("NEXSHELL_RDP_EGFX_TRACE").is_some() {
            if pixels
                .chunks_exact(4)
                .all(|p| p[0] < 8 && p[1] < 8 && p[2] < 8)
            {
                eprintln!(
                    "[egfx] s2c captured ALL-BLACK slot={} rect=({},{})-({},{})",
                    pdu.cache_slot, rect.left, rect.top, rect.right, rect.bottom
                );
            } else if has_dark_vline(&pixels, w, h, 16) {
                eprintln!(
                    "[egfx] s2c HASLINE slot={} rect=({},{})-({},{})",
                    pdu.cache_slot, rect.left, rect.top, rect.right, rect.bottom
                );
            }
            eprintln!(
                "[egfx-trace] s2c-hash slot={} h={:016x}",
                pdu.cache_slot,
                fnv_hash(&pixels)
            );
        }
        self.allocated_bytes = after;
        self.cache.insert(
            pdu.cache_slot,
            CachedBitmap {
                width: w,
                height: h,
                pixels,
            },
        );
        Ok(())
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
        if std::env::var_os("NEXSHELL_RDP_EGFX_TRACE").is_some() {
            if pixels
                .chunks_exact(4)
                .all(|p| p[0] < 8 && p[1] < 8 && p[2] < 8)
            {
                eprintln!(
                    "[egfx] c2s stamping ALL-BLACK slot={} surf={} n={}",
                    pdu.cache_slot,
                    pdu.surface_id,
                    pdu.destination_points.len()
                );
            } else if has_dark_vline(&pixels, cw, ch, 16) {
                eprintln!(
                    "[egfx] c2s HASLINE slot={} surf={} n={}",
                    pdu.cache_slot,
                    pdu.surface_id,
                    pdu.destination_points.len()
                );
            }
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
        if let Some(c) = self.cache.remove(&cache_slot) {
            self.allocated_bytes = self.allocated_bytes.saturating_sub(c.pixels.len());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TileSubRect {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
}

// 这里再解一遍 progressive stream，是为取本 PDU 的 REGION rects 与全量 tile 坐标：
// 库返回的 DecodedTile.update_rectangles 只覆盖本 PDU 解出的 tile，裁不了 ADR 0008 第⑤步
// 要求的"用本 PDU rects 裁剪先前 PDU 累积 tile（frame.tile_pixels）"，删掉会回归拖窗描边残留。
// 改进方向：让 fork 的 decode_bitmap 顺带把 REGION rects 一起返回。
fn progressive_paint_plan(bitmap_data: &[u8]) -> ProgressivePaintPlan {
    let Ok(blocks) = decode_progressive_stream(bitmap_data) else {
        return ProgressivePaintPlan::default();
    };
    let mut plan = ProgressivePaintPlan::default();
    for block in &blocks {
        let ProgressiveBlock::Region(region) = block else {
            continue;
        };
        plan.clip_rects = region.rects.clone();
        for tile in &region.tiles {
            plan.updated_tiles.push(progressive_tile_coord(tile));
        }
    }
    plan
}

fn progressive_tile_coord(tile: &ProgressiveTile<'_>) -> (u16, u16) {
    match tile {
        ProgressiveTile::Simple(tile) => (tile.x_idx, tile.y_idx),
        ProgressiveTile::First(tile) => (tile.x_idx, tile.y_idx),
        ProgressiveTile::Upgrade(tile) => (tile.x_idx, tile.y_idx),
    }
}

fn clip_tile_to_rects(x_idx: u16, y_idx: u16, rects: &[RfxRectangle]) -> Vec<TileSubRect> {
    let tx = u32::from(x_idx) * 64;
    let ty = u32::from(y_idx) * 64;
    let mut out = Vec::new();
    for rect in rects {
        let x0 = u32::from(rect.x).max(tx);
        let y0 = u32::from(rect.y).max(ty);
        let x1 = (u32::from(rect.x) + u32::from(rect.width)).min(tx + 64);
        let y1 = (u32::from(rect.y) + u32::from(rect.height)).min(ty + 64);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        out.push(TileSubRect {
            x: (x0 - tx) as u16,
            y: (y0 - ty) as u16,
            w: (x1 - x0) as u16,
            h: (y1 - y0) as u16,
        });
    }
    out
}

/// 诊断（TRACE 用）：FNV-1a 内容哈希，用于跨时点比对 cache/tile 内容一致性。
pub fn fnv_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 诊断（TRACE 用）：RGBA 块内是否存在近黑竖线段（某列连续 >=min_run 个暗像素）。
pub fn has_dark_vline(pixels: &[u8], w: u16, h: u16, min_run: usize) -> bool {
    let (w, h) = (usize::from(w), usize::from(h));
    for x in 0..w {
        let mut run = 0;
        for y in 0..h {
            let o = (y * w + x) * 4;
            let dark = pixels
                .get(o..o + 3)
                .is_some_and(|p| p[0] < 40 && p[1] < 40 && p[2] < 40);
            if dark {
                run += 1;
                if run >= min_run {
                    return true;
                }
            } else {
                run = 0;
            }
        }
    }
    false
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

/// 把 `src`（src_w 行宽 RGBA）内的子矩形 (sx,sy,sw,sh) 拷进 `dst` (dx,dy) 处，
/// 裁剪越界。返回实际写入的 dst 脏区，全裁剪则 None。（Progressive tile 按
/// REGION rects 局部上屏用。）
fn blit_sub(
    dst: &mut [u8],
    dst_w: u16,
    dst_h: u16,
    dx: i32,
    dy: i32,
    src: &[u8],
    src_w: u16,
    sub: (u16, u16, u16, u16),
) -> Option<(u16, u16, u16, u16)> {
    let (sx, sy, sw, sh) = sub;
    let (dst_w, dst_h) = (i32::from(dst_w), i32::from(dst_h));
    let x0 = dx.max(0);
    let y0 = dy.max(0);
    let x1 = (dx + i32::from(sw)).min(dst_w);
    let y1 = (dy + i32::from(sh)).min(dst_h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let copy_w = (x1 - x0) as usize;
    let dst_stride = dst_w as usize * 4;
    let src_stride = usize::from(src_w) * 4;
    for row in y0..y1 {
        let src_row = usize::from(sy) + (row - dy) as usize;
        let src_col = usize::from(sx) + (x0 - dx) as usize;
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
    fn blit_sub_copies_only_subrect() {
        // src 4x4：全 9；dst 8x8 全 0。只拷 src 子矩形 (1,1,2,2) 到 dst (3,3)。
        let mut dst = vec![0u8; 8 * 8 * 4];
        let mut src = vec![0u8; 4 * 4 * 4];
        for y in 1..3usize {
            for x in 1..3usize {
                src[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4].copy_from_slice(&[9, 9, 9, 9]);
            }
        }
        let d = blit_sub(&mut dst, 8, 8, 3, 3, &src, 4, (1, 1, 2, 2)).unwrap();
        assert_eq!(d, (3, 3, 2, 2));
        let at = |x: usize, y: usize| (y * 8 + x) * 4;
        // 子矩形内容落在 (3,3)-(4,4)。
        assert!(dst[at(3, 3)..at(3, 3) + 4].iter().all(|&b| b == 9));
        assert!(dst[at(4, 4)..at(4, 4) + 4].iter().all(|&b| b == 9));
        // 子矩形之外（如 (2,3)、(5,5)）未被写——整块 blit 会污染这里。
        assert!(dst[at(2, 3)..at(2, 3) + 4].iter().all(|&b| b == 0));
        assert!(dst[at(5, 5)..at(5, 5) + 4].iter().all(|&b| b == 0));
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
        })
        .unwrap();
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
        })
        .unwrap();
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

    /// 合成一条单 tile 的 progressive 流（SYNC[+CONTEXT]+FRAME+REGION+tile）。
    fn progressive_stream(
        include_context: bool,
        quant_prog_vals: Vec<ironrdp_pdu::codecs::rfx::progressive::ProgressiveCodecQuant>,
        tile: ironrdp_pdu::codecs::rfx::progressive::ProgressiveTile<'_>,
    ) -> Vec<u8> {
        use ironrdp_pdu::codecs::rfx::progressive::*;

        let mut blocks = vec![ProgressiveBlock::Sync(ProgressiveSyncPdu)];
        if include_context {
            blocks.push(ProgressiveBlock::Context(ProgressiveContextPdu {
                context_id: 0,
                tile_size: 0x0040,
                flags: 0,
            }));
        }
        blocks.extend([
            ProgressiveBlock::FrameBegin(ProgressiveFrameBeginPdu {
                frame_index: 0,
                region_count: 1,
            }),
            ProgressiveBlock::Region(ProgressiveRegion {
                tile_size: 0x40,
                rects: vec![RfxRectangle {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 64,
                }],
                quant_vals: vec![ComponentCodecQuant::LOSSLESS],
                quant_prog_vals,
                flags: 0,
                tiles: vec![tile],
            }),
            ProgressiveBlock::FrameEnd(ProgressiveFrameEndPdu),
        ]);
        encode_progressive_stream(&blocks).expect("合成流应可编码")
    }

    #[test]
    fn progressive_state_is_per_surface_not_per_codec_context() {
        // 对齐 FreeRDP：codecContextId 不参与 tile 状态的键。先用 ctx=7 发 FIRST 建立
        // tile 状态，再用 ctx=8 发 UPGRADE——若按 (surface, ctx) 分存，UPGRADE 会命中
        // fork 的 pass == 0 分支被静默丢弃（无脏区）；固定 ctx 下应正常升级并出脏区。
        use ironrdp_pdu::codecs::rfx::progressive::{
            ComponentCodecQuant, ProgressiveCodecQuant, ProgressiveTile, TileFirst, TileUpgrade,
        };

        let mut c = Compositor::new();
        c.surfaces.insert(1, Surface::new(64, 64));
        let pdu = |ctx: u32, data: Vec<u8>| WireToSurface2Pdu {
            surface_id: 1,
            codec_id: ironrdp_egfx::pdu::Codec2Type::RemoteFxProgressive,
            codec_context_id: ctx,
            pixel_format: ironrdp_egfx::pdu::PixelFormat::XRgb,
            bitmap_data: data,
        };
        let prog_quant = |quality: u8| ProgressiveCodecQuant {
            quality,
            y_quant: ComponentCodecQuant::LOSSLESS,
            cb_quant: ComponentCodecQuant::LOSSLESS,
            cr_quant: ComponentCodecQuant::LOSSLESS,
        };

        // RLGR 流内容无关紧要（只要非空可解），本测试只关心 tile 状态是否跨 ctx 存活。
        let comp = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let first = progressive_stream(
            true,
            vec![prog_quant(0)],
            ProgressiveTile::First(TileFirst {
                quant_idx_y: 0,
                quant_idx_cb: 0,
                quant_idx_cr: 0,
                x_idx: 0,
                y_idx: 0,
                flags: 0,
                quality: 0,
                y_data: &comp,
                cb_data: &comp,
                cr_data: &comp,
                tail_data: &[],
            }),
        );
        let (rects, failed) = c.write_progressive(&pdu(7, first), Some(1));
        assert!(!failed, "FIRST tile 应解码成功");
        assert!(!rects.is_empty(), "FIRST tile 应产出脏区");

        // 换 ctx（真机每帧都换）后发 UPGRADE：状态必须还在。
        let raw = vec![0u8; 64 * 64 / 8];
        let upgrade = progressive_stream(
            false,
            vec![prog_quant(0), prog_quant(1)],
            ProgressiveTile::Upgrade(TileUpgrade {
                quant_idx_y: 0,
                quant_idx_cb: 0,
                quant_idx_cr: 0,
                x_idx: 0,
                y_idx: 0,
                quality: 1,
                y_srl_data: &[],
                y_raw_data: &raw,
                cb_srl_data: &[],
                cb_raw_data: &raw,
                cr_srl_data: &[],
                cr_raw_data: &raw,
            }),
        );
        let (rects, failed) = c.write_progressive(&pdu(8, upgrade), Some(2));
        assert!(!failed, "换 ctx 后 UPGRADE 不应解码失败");
        assert!(
            !rects.is_empty(),
            "换 ctx 后 UPGRADE 应仍能升级出脏区（状态按 surface 存）"
        );

        // DEC 是 no-op，只计数。
        c.delete_encoding_context(1, 7);
        assert_eq!(c.dec_count(), 1);

        // delete_surface 后 surface 与帧状态干净；写入不存在的 surface 直接空返回。
        c.delete_surface(1);
        assert!(c.surfaces.is_empty());
        assert!(c.progressive_frames.is_empty());
        let (rects, failed) = c.write_progressive(&pdu(9, Vec::new()), Some(3));
        assert!(rects.is_empty() && !failed);
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

    #[test]
    fn create_surface_rejects_oversized_dimensions() {
        // 远端可给到 u16 上限：单块 65535x65535 ≈16 GiB，必须拒绝且不分配。
        let mut c = Compositor::new();
        let err = c.create_surface(1, u16::MAX, u16::MAX).unwrap_err();
        assert_eq!(err.what, "surface 尺寸");
        assert!(c.surfaces.is_empty());
        assert_eq!(c.allocated_bytes, 0);
    }

    #[test]
    fn create_surface_within_limits_succeeds() {
        let mut c = Compositor::new();
        c.create_surface(1, 1920, 1080).unwrap();
        assert_eq!(c.allocated_bytes, 1920 * 1080 * 4);
        // 同 id 重建只记一份；删除后归零。
        c.create_surface(1, 800, 600).unwrap();
        assert_eq!(c.allocated_bytes, 800 * 600 * 4);
        c.delete_surface(1);
        assert_eq!(c.allocated_bytes, 0);
    }

    #[test]
    fn create_surface_rejects_over_session_budget() {
        // 每块 8192x8192=256 MiB，第 5 块越过 1 GiB 会话预算。
        let mut c = Compositor::new();
        for id in 0..4u16 {
            c.create_surface(id, MAX_SURFACE_DIM, MAX_SURFACE_DIM)
                .unwrap();
        }
        let err = c
            .create_surface(4, MAX_SURFACE_DIM, MAX_SURFACE_DIM)
            .unwrap_err();
        assert_eq!(err.what, "会话像素总量");
        assert_eq!(c.surfaces.len(), 4);
    }

    #[test]
    fn surface_to_cache_rejects_over_session_budget() {
        // surface 已占满预算时，再缓存一块必须被拒（cache 也计入总预算）。
        let mut c = Compositor::new();
        for id in 0..4u16 {
            c.create_surface(id, MAX_SURFACE_DIM, MAX_SURFACE_DIM)
                .unwrap();
        }
        let err = c
            .surface_to_cache(&SurfaceToCachePdu {
                surface_id: 0,
                cache_key: 1,
                cache_slot: 1,
                source_rectangle: excl(0, 0, 64, 64),
            })
            .unwrap_err();
        assert_eq!(err.what, "会话像素总量");
        assert!(c.cache.is_empty());
    }
}
