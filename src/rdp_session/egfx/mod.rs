//! EGFX 图形管线（MS-RDPEGFX，docs/adr/0008 第②步）：合成层 + VideoToolbox H.264 硬解。
//! 挂 EGFX DVC 通道，广告 V10.7+V8.1+V8（接上 decoder 后服务端自动选 AVC420）；
//! AVC420/Uncompressed/ClearCodec 均由库解好经 on_bitmap_updated（RGBA）落 surface（ClearCodec
//! 连接级单例解码器，上游 #1175）；Progressive(WireToSurface2) 由本层调库解码器写 tile。
//! EndFrame 时把已映射 surface 的脏区合成进共享 RdpFramebuffer，对齐现有 publish 语义。
//!
//! 与 legacy fastpath 路径写同一个 framebuffer：同一会话只走其中一条管线（EGFX 激活后
//! legacy 图形流停），靠此保证不打架。

mod decoder_vt;
mod diag;
mod surfaces;

use std::sync::Arc;

use ironrdp_dvc::DrdynvcClient;
use ironrdp_egfx::client::{
    BitmapUpdate, GraphicsPipelineClient, GraphicsPipelineHandler, Surface as EgfxSurface,
};
use ironrdp_egfx::pdu::{
    CacheToSurfacePdu, CapabilitiesV107Flags, CapabilitiesV81Flags, CapabilitiesV8Flags,
    CapabilitySet, DeleteEncodingContextPdu, EvictCacheEntryPdu, GfxPdu,
    MapSurfaceToScaledOutputPdu, SolidFillPdu, SurfaceToCachePdu, SurfaceToSurfacePdu,
    WireToSurface2Pdu,
};
use parking_lot::Mutex;

use self::decoder_vt::VtH264Decoder;

/// 离线回放入口（examples/vt_replay 用）：读 dump 目录逐帧喂 VT 解码器。
#[doc(hidden)]
pub use self::decoder_vt::vt_replay_dir;
use self::diag::EgfxDiag;
use self::surfaces::{Compositor, SurfaceRect};
use super::{DirtyRect, RdpEvent, RdpFramebuffer, RdpStats};

/// 挂 EGFX 合成 handler + VideoToolbox 解码器的 DVC 静态通道。
///
/// decoder 接上后，库 `start()` 不再过滤 AVC 能力集，现有 capabilities()（V10.7+V8.1+V8）
/// 生效，服务端将选 AVC420（仍会混发 ClearCodec 小块与 Progressive）。
pub fn build_dvc_client(
    framebuffer: Arc<Mutex<RdpFramebuffer>>,
    event_tx: async_channel::Sender<RdpEvent>,
    stats: Arc<RdpStats>,
    desktop_width: u16,
    desktop_height: u16,
) -> DrdynvcClient {
    let handler = EgfxHandler::new(framebuffer, event_tx, stats, desktop_width, desktop_height);
    DrdynvcClient::new().with_dynamic_channel(GraphicsPipelineClient::new(
        Box::new(handler),
        Some(Box::new(VtH264Decoder::new())),
    ))
}

/// EGFX 合成 handler：拥有 surface 合成器 + 共享 framebuffer/事件；帧内累积输出脏区，
/// EndFrame 合成并发布。
struct EgfxHandler {
    compositor: Compositor,
    framebuffer: Arc<Mutex<RdpFramebuffer>>,
    event_tx: async_channel::Sender<RdpEvent>,
    stats: Arc<RdpStats>,
    /// 权威桌面尺寸（reset_graphics 更新）。
    desktop: (u16, u16),
    /// 本帧累积的输出坐标脏区包围盒 (x0,y0,x1,y1)；None=本帧无变更。
    acc: Option<(i32, i32, i32, i32)>,
    frames: u64,
    /// 库未内部解码的编码（AVC444 等）累计，供频控日志。
    unsupported_count: u64,
    prog_count: u64,
    /// 被库降级跳过的 PDU/解码错误累计（非致命），供频控诊断日志。
    error_count: u64,
    /// 运行时逐操作诊断（NEXSHELL_RDP_EGFX_DIAG）。
    diag: EgfxDiag,
    /// 逐 PDU 坐标级 trace（NEXSHELL_RDP_EGFX_TRACE），黑块溯源用。
    trace: bool,
    /// 像素级写入覆盖掩码（NEXSHELL_RDP_EGFX_COVERAGE=<file>）：desktop 尺寸，每字节
    /// 按操作类型置位（1=bitmap 2=prog 4=fill 8=s2s 16=c2s），drop 时落盘。黑块溯源用。
    coverage: Option<(std::path::PathBuf, Vec<u8>)>,
}

impl EgfxHandler {
    fn new(
        framebuffer: Arc<Mutex<RdpFramebuffer>>,
        event_tx: async_channel::Sender<RdpEvent>,
        stats: Arc<RdpStats>,
        desktop_width: u16,
        desktop_height: u16,
    ) -> Self {
        Self {
            compositor: Compositor::new(),
            framebuffer,
            event_tx,
            stats,
            desktop: (desktop_width, desktop_height),
            acc: None,
            frames: 0,
            unsupported_count: 0,
            prog_count: 0,
            error_count: 0,
            diag: EgfxDiag::new(),
            trace: std::env::var_os("NEXSHELL_RDP_EGFX_TRACE").is_some(),
            coverage: std::env::var_os("NEXSHELL_RDP_EGFX_COVERAGE").map(|p| {
                (
                    std::path::PathBuf::from(p),
                    vec![0u8; usize::from(desktop_width) * usize::from(desktop_height)],
                )
            }),
        }
    }

    /// 覆盖掩码：把 surface-local 脏区（平移到 output 坐标）按操作类型置位。
    fn mark_coverage(&mut self, rects: &[SurfaceRect], class: u8) {
        let Some((_, mask)) = &mut self.coverage else {
            return;
        };
        let (dw, dh) = (i32::from(self.desktop.0), i32::from(self.desktop.1));
        for r in rects {
            let Some((ox, oy)) = self.compositor.surface_origin(r.surface_id) else {
                continue;
            };
            let x0 = (ox + i32::from(r.x)).clamp(0, dw);
            let y0 = (oy + i32::from(r.y)).clamp(0, dh);
            let x1 = (ox + i32::from(r.x) + i32::from(r.w)).clamp(0, dw);
            let y1 = (oy + i32::from(r.y) + i32::from(r.h)).clamp(0, dh);
            // 记「最后写入者」而非 union：黑块溯源需要知道谁最后动过该像素。
            for y in y0..y1 {
                let row = y as usize * dw as usize;
                for x in x0..x1 {
                    mask[row + x as usize] = class;
                }
            }
        }
    }

    /// 把一批 surface-local 脏区平移到 output 坐标并入本帧包围盒（未映射 surface 忽略）。
    fn accumulate(&mut self, rects: &[SurfaceRect]) {
        for r in rects {
            let Some((ox, oy)) = self.compositor.surface_origin(r.surface_id) else {
                continue;
            };
            let x0 = ox + i32::from(r.x);
            let y0 = oy + i32::from(r.y);
            let x1 = x0 + i32::from(r.w);
            let y1 = y0 + i32::from(r.h);
            self.acc = Some(match self.acc {
                Some((ax0, ay0, ax1, ay1)) => (ax0.min(x0), ay0.min(y0), ax1.max(x1), ay1.max(y1)),
                None => (x0, y0, x1, y1),
            });
        }
    }

    fn accumulate_one(&mut self, rect: Option<SurfaceRect>) {
        if let Some(r) = rect {
            self.accumulate(&[r]);
        }
    }

    /// EndFrame：把本帧脏区包围盒内的已映射 surface 合成进 framebuffer，+generation，发一条
    /// FrameUpdated。帧内中间态不发布。
    fn publish(&mut self) {
        let Some((x0, y0, x1, y1)) = self.acc.take() else {
            return;
        };
        let (dw, dh) = (i32::from(self.desktop.0), i32::from(self.desktop.1));
        let cx0 = x0.clamp(0, dw);
        let cy0 = y0.clamp(0, dh);
        let cx1 = x1.clamp(0, dw);
        let cy1 = y1.clamp(0, dh);
        if cx1 <= cx0 || cy1 <= cy0 {
            return;
        }
        let dirty = DirtyRect {
            x: cx0 as u16,
            y: cy0 as u16,
            width: (cx1 - cx0) as u16,
            height: (cy1 - cy0) as u16,
        };
        {
            let mut fb = self.framebuffer.lock();
            for (ox, oy, surf) in self.compositor.mapped_surfaces() {
                fb.compose_surface(ox, oy, &surf.pixels, surf.width, surf.height, dirty);
            }
            fb.bump_generation();
        }
        self.stats.inc_frame();
        let _ = self.event_tx.try_send(RdpEvent::FrameUpdated { dirty });
    }
}

impl GraphicsPipelineHandler for EgfxHandler {
    /// 广告 V10.7(AVC420+444) + V8.1(AVC420) + V8 兜底；接上 decoder 后不再被库过滤。
    /// 不设 SMALL_CACHE：对齐微软 Windows App，让服务端用标准（大）离屏 cache 策略——
    /// 合成层 cache 是无上限 HashMap，任意 slot 数都撑得住；SMALL_CACHE 会逼服务端退化缓存
    /// 策略、更早 evict，浏览器滚动/缩略图场景更易出黑块。codec 中立（只影响 cache 尺寸）。
    fn capabilities(&self) -> Vec<CapabilitySet> {
        vec![
            CapabilitySet::V10_7 {
                flags: CapabilitiesV107Flags::empty(),
            },
            CapabilitySet::V8_1 {
                flags: CapabilitiesV81Flags::AVC420_ENABLED,
            },
            CapabilitySet::V8 {
                flags: CapabilitiesV8Flags::empty(),
            },
        ]
    }

    fn on_capabilities_confirmed(&mut self, caps: &CapabilitySet) {
        eprintln!("[egfx] caps_confirmed {caps:?}");
        self.stats.set_pipeline_egfx();
    }

    fn on_reset_graphics(&mut self, width: u32, height: u32) {
        eprintln!("[egfx] reset_graphics {width}x{height}");
        self.compositor.reset();
        let (w, h) = (
            width.min(u16::MAX as u32) as u16,
            height.min(u16::MAX as u32) as u16,
        );
        self.desktop = (w, h);
        self.acc = None;
        // 桌面尺寸不匹配则按权威尺寸重建 framebuffer（UI 侧 letterbox 自适应）。
        let mut fb = self.framebuffer.lock();
        if fb.width != w || fb.height != h {
            *fb = RdpFramebuffer::new(w, h);
        }
    }

    fn on_surface_created(&mut self, surface: &EgfxSurface) {
        self.diag.on_surf_create();
        eprintln!(
            "[egfx] create_surface id={} {}x{}",
            surface.id, surface.width, surface.height
        );
        self.compositor.create_surface(surface);
    }

    fn on_surface_deleted(&mut self, surface_id: u16) {
        self.diag.on_surf_delete();
        self.compositor.delete_surface(surface_id);
    }

    fn on_surface_mapped(&mut self, surface_id: u16, origin_x: u32, origin_y: u32) {
        self.diag.on_map(false);
        self.compositor
            .map_surface(surface_id, origin_x as i32, origin_y as i32);
    }

    fn on_map_surface_to_scaled_output(&mut self, pdu: &MapSurfaceToScaledOutputPdu) {
        // 缩放输出：v1 只取原点，忽略 target 缩放（少见；缩放渲染后续再做）。
        self.diag.on_map(true);
        eprintln!(
            "[egfx] map_scaled surface={} origin=({},{}) target={}x{}",
            pdu.surface_id,
            pdu.output_origin_x,
            pdu.output_origin_y,
            pdu.target_width,
            pdu.target_height
        );
        self.compositor.map_surface(
            pdu.surface_id,
            pdu.output_origin_x as i32,
            pdu.output_origin_y as i32,
        );
    }

    /// lib 解好的 Uncompressed / AVC420 / ClearCodec RGBA → 写 surface。
    fn on_bitmap_updated(&mut self, update: &BitmapUpdate) {
        self.diag.on_bitmap(update.codec_id);
        if self.trace {
            let r = &update.destination_rectangle;
            let all_black = usize::from(update.width) * usize::from(update.height) >= 32 * 32
                && update
                    .data
                    .chunks_exact(4)
                    .all(|p| p[0] < 8 && p[1] < 8 && p[2] < 8);
            eprintln!(
                "[egfx-trace] bitmap {:?} surf={} rect=({},{})-({},{}){}",
                update.codec_id,
                update.surface_id,
                r.left,
                r.top,
                r.right,
                r.bottom,
                if all_black { " DECODED-ALL-BLACK" } else { "" }
            );
        }
        let rect = self.compositor.write_bitmap(update);
        if let Some(r) = rect {
            self.mark_coverage(&[r], 1);
        }
        self.accumulate_one(rect);
    }

    fn on_wire_to_surface2(&mut self, pdu: &WireToSurface2Pdu) {
        self.prog_count += 1;
        if self.prog_count % 300 == 1 {
            eprintln!("[egfx] progressive frame #{}", self.prog_count);
        }
        let (rects, failed) = self.compositor.write_progressive(pdu);
        self.diag.on_progressive(rects.len(), failed);
        if self.trace {
            eprintln!(
                "[egfx-trace] prog surf={} bytes={} tiles={} failed={}",
                pdu.surface_id,
                pdu.bitmap_data.len(),
                rects.len(),
                failed
            );
        }
        self.mark_coverage(&rects, 2);
        self.accumulate(&rects);
    }

    fn on_delete_encoding_context(&mut self, pdu: &DeleteEncodingContextPdu) {
        self.compositor
            .delete_encoding_context(pdu.surface_id);
    }

    fn on_solid_fill(&mut self, pdu: &SolidFillPdu) {
        self.diag.on_solid_fill();
        if self.trace {
            let rects: Vec<String> = pdu
                .rectangles
                .iter()
                .take(4)
                .map(|r| format!("({},{})-({},{})", r.left, r.top, r.right, r.bottom))
                .collect();
            eprintln!(
                "[egfx-trace] fill surf={} color=({},{},{}) n={} rects={}",
                pdu.surface_id,
                pdu.fill_pixel.r,
                pdu.fill_pixel.g,
                pdu.fill_pixel.b,
                pdu.rectangles.len(),
                rects.join(",")
            );
        }
        let rects = self.compositor.solid_fill(pdu);
        self.mark_coverage(&rects, 4);
        self.accumulate(&rects);
    }

    fn on_surface_to_surface(&mut self, pdu: &SurfaceToSurfacePdu) {
        self.diag.on_s2s();
        let rects = self.compositor.surface_to_surface(pdu);
        self.mark_coverage(&rects, 8);
        self.accumulate(&rects);
    }

    fn on_surface_to_cache(&mut self, pdu: &SurfaceToCachePdu) {
        self.diag.on_s2c();
        if self.trace {
            let r = &pdu.source_rectangle;
            let px = self
                .compositor
                .sample_pixel(pdu.surface_id, r.left, r.top)
                .unwrap_or([0; 4]);
            eprintln!(
                "[egfx-trace] s2c slot={} surf={} rect=({},{})-({},{}) px={:?}",
                pdu.cache_slot, pdu.surface_id, r.left, r.top, r.right, r.bottom, px
            );
        }
        self.compositor.surface_to_cache(pdu);
    }

    fn on_cache_to_surface(&mut self, pdu: &CacheToSurfacePdu) {
        self.diag.on_c2s();
        if self.trace {
            let pts: Vec<String> = pdu
                .destination_points
                .iter()
                .take(4)
                .map(|p| format!("({},{})", p.x, p.y))
                .collect();
            eprintln!(
                "[egfx-trace] c2s slot={} surf={} n={} dst={}",
                pdu.cache_slot,
                pdu.surface_id,
                pdu.destination_points.len(),
                pts.join(",")
            );
        }
        let rects = self.compositor.cache_to_surface(pdu);
        self.mark_coverage(&rects, 16);
        self.accumulate(&rects);
    }

    fn on_evict_cache_entry(&mut self, pdu: &EvictCacheEntryPdu) {
        self.diag.on_evict();
        self.compositor.evict_cache(pdu.cache_slot);
    }

    fn on_frame_complete(&mut self, _frame_id: u32) {
        self.frames += 1;
        self.diag.on_end_frame();
        self.publish();
        // 覆盖掩码定期覆写（probe 到点直接退出时 on_close 不一定触发）。
        if self.frames.is_multiple_of(100) {
            if let Some((path, mask)) = &self.coverage {
                let _ = std::fs::write(path, mask);
            }
        }
    }

    fn on_close(&mut self) {
        eprintln!("[egfx] channel_closed");
        if let Some((path, mask)) = &self.coverage {
            match std::fs::write(path, mask) {
                Ok(()) => eprintln!(
                    "[egfx] coverage mask written {} ({}x{})",
                    path.display(),
                    self.desktop.0,
                    self.desktop.1
                ),
                Err(e) => eprintln!("[egfx] coverage write failed: {e}"),
            }
        }
    }

    /// 库把某个 PDU/解码错误降级为跳过（非致命）时回调。EGFX 是图形增强层，
    /// 单个失败绝不拆会话；这里频控打印错误摘要（含 PDU 类型/codec/surface/长度/错误链），
    /// 供真机复测一击定位。
    fn on_pipeline_error(&mut self, detail: &str) {
        self.diag.on_pipeline_error();
        self.error_count += 1;
        let n = self.error_count;
        if n <= 5 || n.is_multiple_of(300) {
            eprintln!("[egfx] pipeline_error #{n}: {detail}");
        }
    }

    /// 库未内部解码的 WireToSurface1 编码（AVC444 等）走这里：v1 暂不支持，频控记日志。
    /// ClearCodec/AVC420/Uncompressed 已由库解好经 on_bitmap_updated 落盘，不到这里。
    fn on_unhandled_pdu(&mut self, pdu: &GfxPdu) {
        if let GfxPdu::WireToSurface1(w) = pdu {
            self.diag.on_unhandled_wire1(w.codec_id);
            self.unsupported_count += 1;
            let n = self.unsupported_count;
            if n <= 5 || n.is_multiple_of(300) {
                eprintln!(
                    "[egfx] unsupported WireToSurface1 codec {:?} (#{n})",
                    w.codec_id
                );
            }
        }
    }
}
