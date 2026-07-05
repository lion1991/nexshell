use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ironrdp_core::{AsAny, Encode as _, ReadCursor};
use ironrdp_dvc::{DvcClientProcessor, DvcMessage, DvcProcessor};
use ironrdp_egfx::client::GraphicsPipelineClient;
use ironrdp_egfx::pdu::GfxPdu;
use ironrdp_graphics::zgfx::Decompressor;
use ironrdp_pdu::codecs::rfx::progressive::{
    decode_progressive_stream, ProgressiveBlock, ProgressiveTile,
};
use ironrdp_pdu::codecs::rfx::RfxRectangle;
use ironrdp_pdu::decode_cursor;
use ironrdp_pdu::{geometry::ExclusiveRectangle, PduResult};
use parking_lot::Mutex;

use super::decoder_vt::VtH264Decoder;
use super::surfaces::fnv_hash;
use super::surfaces::{Compositor, SurfaceRect};
use super::EgfxHandler;
use crate::rdp_session::{DirtyRect, RdpEvent, RdpFramebuffer, RdpStats};

const MAGIC: &[u8; 8] = b"NXEGFXD1";
const DIRECTION_S2C: u8 = 1;
const ENV_WIRE_DUMP: &str = "NEXSHELL_RDP_EGFX_WIRE_DUMP";

pub fn wrap_if_enabled<T>(inner: T) -> WireDumpingProcessor<T>
where
    T: DvcClientProcessor + 'static,
{
    WireDumpingProcessor::new(inner)
}

pub struct WireDumpingProcessor<T> {
    inner: T,
    writer: Option<WireDumpWriter>,
    frame_probe: WireToSurface2FrameProbe,
}

impl<T> WireDumpingProcessor<T>
where
    T: DvcClientProcessor + 'static,
{
    fn new(inner: T) -> Self {
        let writer = std::env::var_os(ENV_WIRE_DUMP)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .and_then(|path| match WireDumpWriter::create(&path) {
                Ok(writer) => {
                    eprintln!("[egfx] wire dump enabled -> {}", path.display());
                    Some(writer)
                }
                Err(e) => {
                    eprintln!(
                        "[egfx] wire dump disabled, cannot create {}: {e}",
                        path.display()
                    );
                    None
                }
            });
        Self {
            inner,
            writer,
            frame_probe: WireToSurface2FrameProbe::new(),
        }
    }
}

impl<T: 'static> AsAny for WireDumpingProcessor<T> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl<T> DvcProcessor for WireDumpingProcessor<T>
where
    T: DvcClientProcessor + 'static,
{
    fn channel_name(&self) -> &str {
        self.inner.channel_name()
    }

    fn start(&mut self, channel_id: u32) -> PduResult<Vec<DvcMessage>> {
        self.inner.start(channel_id)
    }

    fn process(&mut self, channel_id: u32, payload: &[u8]) -> PduResult<Vec<DvcMessage>> {
        self.frame_probe.prepare(payload);
        if let Some(writer) = &mut self.writer {
            if let Err(e) = writer.write_s2c(channel_id, payload) {
                eprintln!("[egfx] wire dump write failed; disabling dump: {e}");
                self.writer = None;
            }
        }
        self.inner.process(channel_id, payload)
    }

    fn close(&mut self, channel_id: u32) {
        self.inner.close(channel_id);
    }
}

impl<T> DvcClientProcessor for WireDumpingProcessor<T> where T: DvcClientProcessor + 'static {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireDumpRecord {
    pub seq: u64,
    pub timestamp_us: u64,
    pub channel_id: u32,
    pub direction: u8,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChecksumRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug)]
pub struct WireReplayOptions<'a> {
    pub dump_path: &'a Path,
    pub until_seq: Option<u64>,
    pub frame_every: u64,
    pub checksum_rect: Option<ChecksumRect>,
    pub watch_points: Vec<WatchPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WatchPoint {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchEvent {
    pub record_seq: u64,
    pub point: WatchPoint,
    pub op: String,
    pub detail: String,
    pub value: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WirePipelineError {
    pub record_seq: u64,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct WireReplayFrame {
    pub frame_index: u64,
    pub record_seq: u64,
    pub dirty: DirtyRect,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    pub full_hash: u64,
    pub tile_hash: Option<u64>,
    pub checksum_rect: Option<ChecksumRect>,
}

#[derive(Clone, Debug, Default)]
pub struct WireReplaySummary {
    pub records: u64,
    pub frames: u64,
    pub final_width: u16,
    pub final_height: u16,
    pub final_hash: Option<u64>,
    pub final_tile_hash: Option<u64>,
    pub watch_events: Vec<WatchEvent>,
    pub pipeline_errors: Vec<WirePipelineError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WirePduRecord {
    pub seq: u64,
    pub timestamp_us: u64,
    pub channel_id: u32,
    pub payload_len: usize,
    pub decompressed_len: usize,
    pub pdus: Vec<WirePduInfo>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WirePduInfo {
    pub index: usize,
    pub kind: String,
    pub encoded_len: usize,
    pub detail: String,
}

pub struct WireDumpWriter {
    file: File,
    seq: u64,
}

impl WireDumpWriter {
    pub fn create(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(path)?;
        file.write_all(MAGIC)?;
        Ok(Self { file, seq: 0 })
    }

    pub fn write_s2c(&mut self, channel_id: u32, payload: &[u8]) -> io::Result<()> {
        let timestamp_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros()
            .min(u128::from(u64::MAX)) as u64;

        self.file.write_all(&self.seq.to_le_bytes())?;
        self.file.write_all(&timestamp_us.to_le_bytes())?;
        self.file.write_all(&channel_id.to_le_bytes())?;
        self.file.write_all(&[DIRECTION_S2C])?;
        self.file.write_all(&(payload.len() as u32).to_le_bytes())?;
        self.file.write_all(payload)?;
        self.file.flush()?;
        self.seq = self.seq.wrapping_add(1);
        Ok(())
    }
}

#[derive(Debug)]
pub struct WireDumpReader {
    file: File,
}

impl WireDumpReader {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut magic = [0u8; MAGIC.len()];
        file.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not an EGFX wire dump (bad magic)",
            ));
        }
        Ok(Self { file })
    }
}

impl Iterator for WireDumpReader {
    type Item = io::Result<WireDumpRecord>;

    fn next(&mut self) -> Option<Self::Item> {
        let seq = match read_u64_or_eof(&mut self.file) {
            Ok(Some(v)) => v,
            Ok(None) => return None,
            Err(e) => return Some(Err(e)),
        };
        let timestamp_us = match read_u64(&mut self.file) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        let channel_id = match read_u32(&mut self.file) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        let mut direction = [0u8; 1];
        if let Err(e) = self.file.read_exact(&mut direction) {
            return Some(Err(e));
        }
        let len = match read_u32(&mut self.file) {
            Ok(v) => v as usize,
            Err(e) => return Some(Err(e)),
        };
        let mut payload = vec![0u8; len];
        if let Err(e) = self.file.read_exact(&mut payload) {
            return Some(Err(e));
        }
        Some(Ok(WireDumpRecord {
            seq,
            timestamp_us,
            channel_id,
            direction: direction[0],
            payload,
        }))
    }
}

pub fn replay_wire_dump<F>(
    options: WireReplayOptions<'_>,
    mut on_frame: F,
) -> io::Result<WireReplaySummary>
where
    F: FnMut(&WireReplayFrame) -> io::Result<()>,
{
    let framebuffer = Arc::new(Mutex::new(RdpFramebuffer::new(1, 1)));
    let (event_tx, event_rx) = async_channel::unbounded();
    let stats = Arc::new(RdpStats::new());
    let handler = EgfxHandler::new(framebuffer.clone(), event_tx, stats, 1, 1);
    let mut client =
        GraphicsPipelineClient::new(Box::new(handler), Some(Box::new(VtH264Decoder::new())));
    let mut frame_probe = WireToSurface2FrameProbe::new();
    let mut summary = WireReplaySummary::default();
    let frame_every = options.frame_every.max(1);
    let watch_points = options.watch_points.clone();
    probe_begin(watch_points);

    for rec in WireDumpReader::open(options.dump_path)? {
        let rec = rec?;
        if let Some(until) = options.until_seq {
            if rec.seq > until {
                break;
            }
        }
        if rec.direction != DIRECTION_S2C {
            continue;
        }
        summary.records += 1;
        probe_set_record(rec.seq);
        frame_probe.prepare(&rec.payload);
        client.process(rec.channel_id, &rec.payload).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("EGFX replay failed at record {}: {e}", rec.seq),
            )
        })?;

        while let Ok(event) = event_rx.try_recv() {
            let RdpEvent::FrameUpdated { dirty } = event else {
                continue;
            };
            summary.frames += 1;
            let fb = framebuffer.lock();
            let full_hash = fnv_hash(&fb.rgba);
            let tile_hash = options
                .checksum_rect
                .map(|rect| hash_rect_rgba(&fb.rgba, fb.width, fb.height, rect));
            summary.final_width = fb.width;
            summary.final_height = fb.height;
            summary.final_hash = Some(full_hash);
            summary.final_tile_hash = tile_hash;

            if summary.frames.is_multiple_of(frame_every) {
                let frame = WireReplayFrame {
                    frame_index: summary.frames,
                    record_seq: rec.seq,
                    dirty,
                    width: fb.width,
                    height: fb.height,
                    rgba: fb.rgba.clone(),
                    full_hash,
                    tile_hash,
                    checksum_rect: options.checksum_rect,
                };
                drop(fb);
                on_frame(&frame)?;
            }
        }
    }

    summary.pipeline_errors = probe_take_pipeline_errors();
    summary.watch_events = probe_take_events();
    Ok(summary)
}

pub fn inspect_wire_dump_pdus(
    dump_path: &Path,
    record_seqs: &[u64],
) -> io::Result<Vec<WirePduRecord>> {
    inspect_wire_dump_pdus_with_points(dump_path, record_seqs, &[])
}

pub fn inspect_wire_dump_pdus_with_points(
    dump_path: &Path,
    record_seqs: &[u64],
    watch_points: &[WatchPoint],
) -> io::Result<Vec<WirePduRecord>> {
    let max_seq = record_seqs.iter().copied().max();
    let mut decompressor = Decompressor::new();
    let mut decompressed = Vec::new();
    let mut out = Vec::new();

    for rec in WireDumpReader::open(dump_path)? {
        let rec = rec?;
        if let Some(max_seq) = max_seq {
            if rec.seq > max_seq {
                break;
            }
        }
        if rec.direction != DIRECTION_S2C {
            continue;
        }

        decompressed.clear();
        let decompress_result = decompressor.decompress(&rec.payload, &mut decompressed);
        let wanted = record_seqs.is_empty() || record_seqs.contains(&rec.seq);
        if !wanted {
            continue;
        }

        let mut record = WirePduRecord {
            seq: rec.seq,
            timestamp_us: rec.timestamp_us,
            channel_id: rec.channel_id,
            payload_len: rec.payload.len(),
            decompressed_len: decompressed.len(),
            pdus: Vec::new(),
            error: None,
        };
        if let Err(e) = decompress_result {
            record.error = Some(format!("ZGFX decompress failed: {e}"));
            out.push(record);
            continue;
        }

        let mut cursor = ReadCursor::new(decompressed.as_slice());
        while !cursor.is_empty() {
            let index = record.pdus.len();
            match decode_cursor::<GfxPdu>(&mut cursor) {
                Ok(pdu) => record
                    .pdus
                    .push(summarize_gfx_pdu(index, &pdu, watch_points)),
                Err(e) => {
                    record.error = Some(format!("GfxPdu decode failed after {index} PDUs: {e}"));
                    break;
                }
            }
        }
        out.push(record);
    }

    Ok(out)
}

struct WireToSurface2FrameProbe {
    decompressor: Decompressor,
    decompressed: Vec<u8>,
    current_frame_id: Option<u32>,
}

impl WireToSurface2FrameProbe {
    fn new() -> Self {
        Self {
            decompressor: Decompressor::new(),
            decompressed: Vec::new(),
            current_frame_id: None,
        }
    }

    fn prepare(&mut self, payload: &[u8]) {
        clear_wire_to_surface2_frame_queue();
        self.decompressed.clear();
        if self
            .decompressor
            .decompress(payload, &mut self.decompressed)
            .is_err()
        {
            return;
        }
        let mut cursor = ReadCursor::new(self.decompressed.as_slice());
        while !cursor.is_empty() {
            let Ok(pdu) = decode_cursor::<GfxPdu>(&mut cursor) else {
                return;
            };
            match pdu {
                GfxPdu::StartFrame(pdu) => self.current_frame_id = Some(pdu.frame_id),
                GfxPdu::WireToSurface2(_) => {
                    push_wire_to_surface2_frame_id(self.current_frame_id);
                }
                GfxPdu::EndFrame(pdu) => {
                    if self.current_frame_id == Some(pdu.frame_id) {
                        self.current_frame_id = None;
                    }
                }
                _ => {}
            }
        }
    }
}

fn summarize_gfx_pdu(index: usize, pdu: &GfxPdu, watch_points: &[WatchPoint]) -> WirePduInfo {
    let (kind, detail) = match pdu {
        GfxPdu::WireToSurface1(pdu) => (
            "WireToSurface1",
            format!(
                "surf={} codec={:?} pixel={:?} dest={} bytes={} head={}",
                pdu.surface_id,
                pdu.codec_id,
                pdu.pixel_format,
                rect_desc(&pdu.destination_rectangle),
                pdu.bitmap_data.len(),
                bytes_head_hex(&pdu.bitmap_data)
            ),
        ),
        GfxPdu::WireToSurface2(pdu) => (
            "WireToSurface2",
            format!(
                "surf={} codec={:?} ctx={} pixel={:?} bytes={} head={}{}",
                pdu.surface_id,
                pdu.codec_id,
                pdu.codec_context_id,
                pdu.pixel_format,
                pdu.bitmap_data.len(),
                bytes_head_hex(&pdu.bitmap_data),
                progressive_stream_desc(&pdu.bitmap_data, watch_points)
            ),
        ),
        GfxPdu::DeleteEncodingContext(pdu) => (
            "DeleteEncodingContext",
            format!("surf={} ctx={}", pdu.surface_id, pdu.codec_context_id),
        ),
        GfxPdu::SolidFill(pdu) => (
            "SolidFill",
            format!(
                "surf={} color=({},{},{}) rects={}",
                pdu.surface_id,
                pdu.fill_pixel.r,
                pdu.fill_pixel.g,
                pdu.fill_pixel.b,
                rects_desc(&pdu.rectangles)
            ),
        ),
        GfxPdu::SurfaceToSurface(pdu) => (
            "SurfaceToSurface",
            format!(
                "src_surf={} dst_surf={} src={} points={}",
                pdu.source_surface_id,
                pdu.destination_surface_id,
                rect_desc(&pdu.source_rectangle),
                points_desc(&pdu.destination_points)
            ),
        ),
        GfxPdu::SurfaceToCache(pdu) => (
            "SurfaceToCache",
            format!(
                "slot={} key={} surf={} src={}",
                pdu.cache_slot,
                pdu.cache_key,
                pdu.surface_id,
                rect_desc(&pdu.source_rectangle)
            ),
        ),
        GfxPdu::CacheToSurface(pdu) => (
            "CacheToSurface",
            format!(
                "slot={} surf={} points={}",
                pdu.cache_slot,
                pdu.surface_id,
                points_desc(&pdu.destination_points)
            ),
        ),
        GfxPdu::EvictCacheEntry(pdu) => ("EvictCacheEntry", format!("slot={}", pdu.cache_slot)),
        GfxPdu::CreateSurface(pdu) => (
            "CreateSurface",
            format!(
                "surf={} size={}x{} pixel={:?}",
                pdu.surface_id, pdu.width, pdu.height, pdu.pixel_format
            ),
        ),
        GfxPdu::DeleteSurface(pdu) => ("DeleteSurface", format!("surf={}", pdu.surface_id)),
        GfxPdu::StartFrame(pdu) => (
            "StartFrame",
            format!("frame_id={} timestamp={:?}", pdu.frame_id, pdu.timestamp),
        ),
        GfxPdu::EndFrame(pdu) => ("EndFrame", format!("frame_id={}", pdu.frame_id)),
        GfxPdu::FrameAcknowledge(pdu) => (
            "FrameAcknowledge",
            format!(
                "queue_depth={:?} frame_id={} total={}",
                pdu.queue_depth, pdu.frame_id, pdu.total_frames_decoded
            ),
        ),
        GfxPdu::ResetGraphics(pdu) => (
            "ResetGraphics",
            format!(
                "size={}x{} monitors={}",
                pdu.width,
                pdu.height,
                pdu.monitors.len()
            ),
        ),
        GfxPdu::MapSurfaceToOutput(pdu) => (
            "MapSurfaceToOutput",
            format!(
                "surf={} origin=({}, {})",
                pdu.surface_id, pdu.output_origin_x, pdu.output_origin_y
            ),
        ),
        GfxPdu::CacheImportOffer(pdu) => (
            "CacheImportOffer",
            format!("entries={}", pdu.cache_entries.len()),
        ),
        GfxPdu::CacheImportReply(pdu) => {
            ("CacheImportReply", format!("slots={:?}", pdu.cache_slots))
        }
        GfxPdu::CapabilitiesAdvertise(pdu) => {
            ("CapabilitiesAdvertise", format!("caps={}", pdu.0.len()))
        }
        GfxPdu::CapabilitiesConfirm(pdu) => ("CapabilitiesConfirm", format!("cap={:?}", pdu.0)),
        GfxPdu::MapSurfaceToWindow(pdu) => (
            "MapSurfaceToWindow",
            format!("surf={} window={}", pdu.surface_id, pdu.window_id),
        ),
        GfxPdu::QoeFrameAcknowledge(pdu) => (
            "QoeFrameAcknowledge",
            format!(
                "frame_id={} timestamp={} time_diff_se={}",
                pdu.frame_id, pdu.timestamp, pdu.time_diff_se
            ),
        ),
        GfxPdu::MapSurfaceToScaledOutput(pdu) => (
            "MapSurfaceToScaledOutput",
            format!(
                "surf={} origin=({}, {}) target={}x{}",
                pdu.surface_id,
                pdu.output_origin_x,
                pdu.output_origin_y,
                pdu.target_width,
                pdu.target_height
            ),
        ),
        GfxPdu::MapSurfaceToScaledWindow(pdu) => (
            "MapSurfaceToScaledWindow",
            format!(
                "surf={} window={} target={}x{}",
                pdu.surface_id, pdu.window_id, pdu.target_width, pdu.target_height
            ),
        ),
        _ => ("Unknown", format!("{pdu:?}")),
    };

    WirePduInfo {
        index,
        kind: kind.to_owned(),
        encoded_len: pdu.size(),
        detail,
    }
}

fn progressive_stream_desc(bytes: &[u8], watch_points: &[WatchPoint]) -> String {
    let blocks = match decode_progressive_stream(bytes) {
        Ok(blocks) => blocks,
        Err(e) => return format!(" prog=parse_err:{e}"),
    };

    let mut sync = 0usize;
    let mut context = Vec::new();
    let mut frame_begin = Vec::new();
    let mut frame_end = 0usize;
    let mut regions = Vec::new();
    let mut watch_hits = Vec::new();

    for block in &blocks {
        match block {
            ProgressiveBlock::Sync(_) => sync += 1,
            ProgressiveBlock::Context(ctx) => context.push(format!(
                "ctx{} tile={} flags=0x{:02x} rex={}",
                ctx.context_id,
                ctx.tile_size,
                ctx.flags,
                ctx.uses_reduce_extrapolate()
            )),
            ProgressiveBlock::FrameBegin(frame) => frame_begin.push(format!(
                "frame={} region_count={}",
                frame.frame_index, frame.region_count
            )),
            ProgressiveBlock::FrameEnd(_) => frame_end += 1,
            ProgressiveBlock::Region(region) => {
                let region_index = regions.len();
                regions.push(format!(
                    "r{} flags=0x{:02x} rex={} rects={} [{}] tiles={} [{}]",
                    region_index,
                    region.flags,
                    region.uses_reduce_extrapolate(),
                    region.rects.len(),
                    rfx_rects_desc(&region.rects),
                    region.tiles.len(),
                    progressive_tiles_desc(&region.tiles)
                ));
                for point in watch_points {
                    let point_hits = progressive_point_hits(region_index, region, *point);
                    if !point_hits.is_empty() {
                        watch_hits.extend(point_hits);
                    }
                }
            }
        }
    }

    let mut out = format!(
        " prog=blocks={} sync={} ctx=[{}] begin=[{}] regions={} [{}] end={}",
        blocks.len(),
        sync,
        context.join(";"),
        frame_begin.join(";"),
        regions.len(),
        regions.join("; "),
        frame_end
    );
    if !watch_hits.is_empty() {
        out.push_str(&format!(" watch=[{}]", watch_hits.join("; ")));
    }
    out
}

fn progressive_point_hits(
    region_index: usize,
    region: &ironrdp_pdu::codecs::rfx::progressive::ProgressiveRegion<'_>,
    point: WatchPoint,
) -> Vec<String> {
    let rect_hits = region
        .rects
        .iter()
        .enumerate()
        .filter(|(_, rect)| rfx_rect_contains(rect, point))
        .map(|(i, rect)| format!("#{i}{}", rfx_rect_desc(rect)))
        .collect::<Vec<_>>();
    let tile_hits = region
        .tiles
        .iter()
        .filter_map(|tile| {
            let (kind, x, y, quality, difference) = progressive_tile_brief(tile);
            let tx = x.saturating_mul(64);
            let ty = y.saturating_mul(64);
            let inside = point.x >= tx
                && point.x < tx.saturating_add(64)
                && point.y >= ty
                && point.y < ty.saturating_add(64);
            inside.then(|| {
                format!(
                    "{kind}{x},{y} q={quality} diff={} local=({}, {})",
                    difference,
                    point.x - tx,
                    point.y - ty
                )
            })
        })
        .collect::<Vec<_>>();

    if rect_hits.is_empty() && tile_hits.is_empty() {
        return Vec::new();
    }
    vec![format!(
        "p=({}, {}) r{} rects=[{}] tiles=[{}] blits={}",
        point.x,
        point.y,
        region_index,
        rect_hits.join(","),
        tile_hits.join(","),
        !rect_hits.is_empty() && !tile_hits.is_empty()
    )]
}

fn progressive_tiles_desc(tiles: &[ProgressiveTile<'_>]) -> String {
    let mut out = tiles
        .iter()
        .take(32)
        .map(|tile| {
            let (kind, x, y, quality, difference) = progressive_tile_brief(tile);
            if difference {
                format!("{kind}{x},{y}:q{quality}:diff")
            } else {
                format!("{kind}{x},{y}:q{quality}")
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    if tiles.len() > 32 {
        out.push_str(&format!(",...(+{})", tiles.len() - 32));
    }
    out
}

fn progressive_tile_brief(tile: &ProgressiveTile<'_>) -> (&'static str, u16, u16, u8, bool) {
    match tile {
        ProgressiveTile::Simple(tile) => ("S", tile.x_idx, tile.y_idx, 0xFF, tile.is_difference()),
        ProgressiveTile::First(tile) => (
            "F",
            tile.x_idx,
            tile.y_idx,
            tile.quality,
            tile.is_difference(),
        ),
        ProgressiveTile::Upgrade(tile) => ("U", tile.x_idx, tile.y_idx, tile.quality, false),
    }
}

fn rect_desc(rect: &ExclusiveRectangle) -> String {
    format!(
        "({},{})-({},{})",
        rect.left, rect.top, rect.right, rect.bottom
    )
}

fn rects_desc(rects: &[ExclusiveRectangle]) -> String {
    let mut out = rects
        .iter()
        .take(8)
        .map(rect_desc)
        .collect::<Vec<_>>()
        .join(",");
    if rects.len() > 8 {
        out.push_str(&format!(",...(+{})", rects.len() - 8));
    }
    out
}

fn rfx_rect_desc(rect: &RfxRectangle) -> String {
    format!("({},{})+{}x{}", rect.x, rect.y, rect.width, rect.height)
}

fn rfx_rects_desc(rects: &[RfxRectangle]) -> String {
    let mut out = rects
        .iter()
        .take(12)
        .map(rfx_rect_desc)
        .collect::<Vec<_>>()
        .join(",");
    if rects.len() > 12 {
        out.push_str(&format!(",...(+{})", rects.len() - 12));
    }
    out
}

fn rfx_rect_contains(rect: &RfxRectangle, point: WatchPoint) -> bool {
    point.x >= rect.x
        && point.x < rect.x.saturating_add(rect.width)
        && point.y >= rect.y
        && point.y < rect.y.saturating_add(rect.height)
}

fn points_desc(points: &[ironrdp_egfx::pdu::Point]) -> String {
    let mut out = points
        .iter()
        .take(12)
        .map(|p| format!("({},{})", p.x, p.y))
        .collect::<Vec<_>>()
        .join(",");
    if points.len() > 12 {
        out.push_str(&format!(",...(+{})", points.len() - 12));
    }
    out
}

fn bytes_head_hex(bytes: &[u8]) -> String {
    let mut out = bytes
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("");
    if bytes.len() > 16 {
        out.push_str("...");
    }
    out
}

pub fn hash_rect_rgba(pixels: &[u8], width: u16, height: u16, rect: ChecksumRect) -> u64 {
    let x0 = rect.x.min(width);
    let y0 = rect.y.min(height);
    let x1 = rect.x.saturating_add(rect.width).min(width);
    let y1 = rect.y.saturating_add(rect.height).min(height);
    if x1 <= x0 || y1 <= y0 {
        return fnv_hash(&[]);
    }

    let row_bytes = usize::from(x1 - x0) * 4;
    let stride = usize::from(width) * 4;
    let mut buf = Vec::with_capacity(row_bytes * usize::from(y1 - y0));
    for y in y0..y1 {
        let off = usize::from(y) * stride + usize::from(x0) * 4;
        let end = off + row_bytes;
        if end <= pixels.len() {
            buf.extend_from_slice(&pixels[off..end]);
        }
    }
    fnv_hash(&buf)
}

fn read_u64_or_eof(file: &mut File) -> io::Result<Option<u64>> {
    let mut first = [0u8; 1];
    match file.read(&mut first)? {
        0 => Ok(None),
        1 => {
            let mut rest = [0u8; 7];
            file.read_exact(&mut rest)?;
            let mut bytes = [0u8; 8];
            bytes[0] = first[0];
            bytes[1..].copy_from_slice(&rest);
            Ok(Some(u64::from_le_bytes(bytes)))
        }
        _ => unreachable!("one-byte buffer cannot read more than one byte"),
    }
}

fn read_u64(file: &mut File) -> io::Result<u64> {
    let mut bytes = [0u8; 8];
    file.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_u32(file: &mut File) -> io::Result<u32> {
    let mut bytes = [0u8; 4];
    file.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

#[derive(Default)]
struct ProbeState {
    current_record: u64,
    watch_points: Vec<WatchPoint>,
    events: Vec<WatchEvent>,
    pipeline_errors: Vec<WirePipelineError>,
}

thread_local! {
    static PROBE: RefCell<Option<ProbeState>> = const { RefCell::new(None) };
    static WIRE_TO_SURFACE2_FRAME_IDS: RefCell<VecDeque<Option<u32>>> = const { RefCell::new(VecDeque::new()) };
}

fn clear_wire_to_surface2_frame_queue() {
    WIRE_TO_SURFACE2_FRAME_IDS.with(|queue| queue.borrow_mut().clear());
}

fn push_wire_to_surface2_frame_id(frame_id: Option<u32>) {
    WIRE_TO_SURFACE2_FRAME_IDS.with(|queue| queue.borrow_mut().push_back(frame_id));
}

pub(super) fn probe_next_wire_to_surface2_frame_id() -> Option<u32> {
    WIRE_TO_SURFACE2_FRAME_IDS.with(|queue| queue.borrow_mut().pop_front().flatten())
}

fn probe_begin(watch_points: Vec<WatchPoint>) {
    PROBE.with(|probe| {
        *probe.borrow_mut() = Some(ProbeState {
            current_record: 0,
            watch_points,
            events: Vec::new(),
            pipeline_errors: Vec::new(),
        });
    });
}

fn probe_set_record(record_seq: u64) {
    PROBE.with(|probe| {
        if let Some(state) = probe.borrow_mut().as_mut() {
            state.current_record = record_seq;
        }
    });
}

fn probe_take_events() -> Vec<WatchEvent> {
    PROBE.with(|probe| {
        probe
            .borrow_mut()
            .take()
            .map_or_else(Vec::new, |state| state.events)
    })
}

fn probe_take_pipeline_errors() -> Vec<WirePipelineError> {
    PROBE.with(|probe| {
        probe
            .borrow()
            .as_ref()
            .map_or_else(Vec::new, |state| state.pipeline_errors.clone())
    })
}

pub(super) fn probe_pipeline_error(detail: &str) {
    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        let Some(state) = probe.as_mut() else {
            return;
        };
        state.pipeline_errors.push(WirePipelineError {
            record_seq: state.current_record,
            detail: detail.to_owned(),
        });
    });
}

pub(super) fn probe_surface_write(
    compositor: &Compositor,
    op: &'static str,
    detail: String,
    rects: &[SurfaceRect],
) {
    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        let Some(state) = probe.as_mut() else {
            return;
        };
        for point in state.watch_points.iter().copied() {
            for rect in rects {
                let Some((sx, sy)) = output_to_surface_point(compositor, point, rect.surface_id)
                else {
                    continue;
                };
                if !surface_point_in_rect(sx, sy, rect) {
                    continue;
                }
                let Some(value) = compositor.sample_pixel(rect.surface_id, sx, sy) else {
                    continue;
                };
                if state.events.last().is_some_and(|prev| {
                    prev.record_seq == state.current_record
                        && prev.point == point
                        && prev.value == value
                        && prev.op == op
                }) {
                    continue;
                }
                state.events.push(WatchEvent {
                    record_seq: state.current_record,
                    point,
                    op: op.to_owned(),
                    detail: detail.clone(),
                    value,
                });
            }
        }
    });
}

pub(super) fn probe_surface_to_cache(
    compositor: &Compositor,
    pdu: &ironrdp_egfx::pdu::SurfaceToCachePdu,
) {
    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        let Some(state) = probe.as_mut() else {
            return;
        };
        let rect = SurfaceRect {
            surface_id: pdu.surface_id,
            x: pdu.source_rectangle.left,
            y: pdu.source_rectangle.top,
            w: pdu
                .source_rectangle
                .right
                .saturating_sub(pdu.source_rectangle.left),
            h: pdu
                .source_rectangle
                .bottom
                .saturating_sub(pdu.source_rectangle.top),
        };
        for point in state.watch_points.iter().copied() {
            let Some((sx, sy)) = output_to_surface_point(compositor, point, pdu.surface_id) else {
                continue;
            };
            if !surface_point_in_rect(sx, sy, &rect) {
                continue;
            }
            let Some(value) = compositor.sample_pixel(pdu.surface_id, sx, sy) else {
                continue;
            };
            state.events.push(WatchEvent {
                record_seq: state.current_record,
                point,
                op: "s2c".to_owned(),
                detail: format!(
                    "slot={} surf={} src=({},{})-({},{})",
                    pdu.cache_slot,
                    pdu.surface_id,
                    pdu.source_rectangle.left,
                    pdu.source_rectangle.top,
                    pdu.source_rectangle.right,
                    pdu.source_rectangle.bottom
                ),
                value,
            });
        }
    });
}

pub(super) fn probe_surface_to_surface(
    compositor: &Compositor,
    pdu: &ironrdp_egfx::pdu::SurfaceToSurfacePdu,
    rects: &[SurfaceRect],
) {
    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        let Some(state) = probe.as_mut() else {
            return;
        };
        let width = pdu
            .source_rectangle
            .right
            .saturating_sub(pdu.source_rectangle.left);
        let height = pdu
            .source_rectangle
            .bottom
            .saturating_sub(pdu.source_rectangle.top);
        for point in state.watch_points.iter().copied() {
            for rect in rects {
                let Some((dx, dy)) =
                    output_to_surface_point(compositor, point, pdu.destination_surface_id)
                else {
                    continue;
                };
                if !surface_point_in_rect(dx, dy, rect) {
                    continue;
                }
                let Some(value) = compositor.sample_pixel(pdu.destination_surface_id, dx, dy)
                else {
                    continue;
                };
                let source = pdu.destination_points.iter().find_map(|dst| {
                    let in_x = dx >= dst.x && dx < dst.x.saturating_add(width);
                    let in_y = dy >= dst.y && dy < dst.y.saturating_add(height);
                    if in_x && in_y {
                        Some((
                            pdu.source_rectangle.left.saturating_add(dx - dst.x),
                            pdu.source_rectangle.top.saturating_add(dy - dst.y),
                            dst.x,
                            dst.y,
                        ))
                    } else {
                        None
                    }
                });
                let detail = if let Some((sx, sy, dst_x, dst_y)) = source {
                    format!(
                        "src_surf={} src=({},{}) dst_surf={} dst=({},{}) dst_origin=({},{})",
                        pdu.source_surface_id,
                        sx,
                        sy,
                        pdu.destination_surface_id,
                        dx,
                        dy,
                        dst_x,
                        dst_y
                    )
                } else {
                    format!(
                        "src_surf={} src=({},{})-({},{}) dst_surf={} n={}",
                        pdu.source_surface_id,
                        pdu.source_rectangle.left,
                        pdu.source_rectangle.top,
                        pdu.source_rectangle.right,
                        pdu.source_rectangle.bottom,
                        pdu.destination_surface_id,
                        pdu.destination_points.len()
                    )
                };
                state.events.push(WatchEvent {
                    record_seq: state.current_record,
                    point,
                    op: "s2s".to_owned(),
                    detail,
                    value,
                });
            }
        }
    });
}

fn output_to_surface_point(
    compositor: &Compositor,
    point: WatchPoint,
    surface_id: u16,
) -> Option<(u16, u16)> {
    let (ox, oy) = compositor.surface_origin(surface_id)?;
    let sx = i32::from(point.x) - ox;
    let sy = i32::from(point.y) - oy;
    if sx < 0 || sy < 0 {
        return None;
    }
    Some((sx as u16, sy as u16))
}

fn surface_point_in_rect(x: u16, y: u16, rect: &SurfaceRect) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.w)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_dump_round_trips_complete_payloads() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut writer = WireDumpWriter::create(tmp.path()).unwrap();

        writer.write_s2c(7, &[1, 2, 3, 4]).unwrap();
        writer.write_s2c(7, &[0xAA; 6]).unwrap();

        let records: Vec<_> = WireDumpReader::open(tmp.path())
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].seq, 0);
        assert_eq!(records[0].channel_id, 7);
        assert_eq!(records[0].payload, vec![1, 2, 3, 4]);
        assert_eq!(records[1].seq, 1);
        assert_eq!(records[1].payload, vec![0xAA; 6]);
    }

    #[test]
    fn text_trace_is_rejected_as_not_a_wire_dump() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"[egfx] caps_confirmed V10_7\n").unwrap();

        let err = WireDumpReader::open(tmp.path()).unwrap_err();

        assert!(
            err.to_string().contains("not an EGFX wire dump"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn replay_processes_synthetic_solid_fill_frame() {
        let tmp = synthetic_solid_fill_dump();
        let mut frames = Vec::new();

        let summary = replay_wire_dump(
            WireReplayOptions {
                dump_path: tmp.path(),
                until_seq: None,
                frame_every: 1,
                checksum_rect: Some(ChecksumRect {
                    x: 1,
                    y: 1,
                    width: 2,
                    height: 2,
                }),
                watch_points: Vec::new(),
            },
            |frame| {
                frames.push(frame.clone());
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(summary.records, 6);
        assert_eq!(summary.frames, 1);
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].rgba[(1usize * 4 + 1) * 4..][..4],
            [10, 20, 30, 255]
        );
        assert_eq!(
            frames[0].tile_hash,
            Some(hash_rect_rgba(
                &frames[0].rgba,
                4,
                4,
                frames[0].checksum_rect.unwrap()
            ))
        );
    }

    fn synthetic_solid_fill_dump() -> tempfile::NamedTempFile {
        use ironrdp_core::{Encode as _, WriteCursor};
        use ironrdp_egfx::pdu::{
            Color, CreateSurfacePdu, EndFramePdu, GfxPdu, MapSurfaceToOutputPdu, PixelFormat,
            ResetGraphicsPdu, SolidFillPdu, StartFramePdu, Timestamp,
        };
        use ironrdp_graphics::zgfx::wrap_uncompressed;
        use ironrdp_pdu::geometry::ExclusiveRectangle;

        fn wrapped(pdu: GfxPdu) -> Vec<u8> {
            let mut bytes = vec![0u8; pdu.size()];
            let mut cursor = WriteCursor::new(&mut bytes);
            pdu.encode(&mut cursor).unwrap();
            wrap_uncompressed(&bytes)
        }

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut writer = WireDumpWriter::create(tmp.path()).unwrap();
        let pdus = [
            GfxPdu::ResetGraphics(ResetGraphicsPdu {
                width: 4,
                height: 4,
                monitors: Vec::new(),
            }),
            GfxPdu::CreateSurface(CreateSurfacePdu {
                surface_id: 0,
                width: 4,
                height: 4,
                pixel_format: PixelFormat::XRgb,
            }),
            GfxPdu::MapSurfaceToOutput(MapSurfaceToOutputPdu {
                surface_id: 0,
                output_origin_x: 0,
                output_origin_y: 0,
            }),
            GfxPdu::StartFrame(StartFramePdu {
                timestamp: Timestamp {
                    milliseconds: 0,
                    seconds: 0,
                    minutes: 0,
                    hours: 0,
                },
                frame_id: 1,
            }),
            GfxPdu::SolidFill(SolidFillPdu {
                surface_id: 0,
                fill_pixel: Color {
                    b: 30,
                    g: 20,
                    r: 10,
                    xa: 0,
                },
                rectangles: vec![ExclusiveRectangle {
                    left: 1,
                    top: 1,
                    right: 3,
                    bottom: 3,
                }],
            }),
            GfxPdu::EndFrame(EndFramePdu { frame_id: 1 }),
        ];
        for pdu in pdus {
            writer.write_s2c(9, &wrapped(pdu)).unwrap();
        }
        drop(writer);
        tmp
    }

    #[test]
    fn watch_pixel_reports_surface_write_record_and_value() {
        let tmp = synthetic_solid_fill_dump();
        let mut events = Vec::new();

        let summary = replay_wire_dump(
            WireReplayOptions {
                dump_path: tmp.path(),
                until_seq: None,
                frame_every: 1,
                checksum_rect: None,
                watch_points: vec![WatchPoint { x: 1, y: 1 }],
            },
            |_| Ok(()),
        )
        .unwrap();

        events.extend(summary.watch_events);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].record_seq, 4);
        assert_eq!(events[0].point, WatchPoint { x: 1, y: 1 });
        assert_eq!(events[0].op, "fill");
        assert_eq!(events[0].value, [10, 20, 30, 255]);
    }

    #[test]
    fn inspect_pdus_lists_synthetic_solid_fill_record() {
        let tmp = synthetic_solid_fill_dump();

        let records = inspect_wire_dump_pdus(tmp.path(), &[4]).unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].seq, 4);
        assert_eq!(records[0].pdus.len(), 1);
        assert_eq!(records[0].pdus[0].kind, "SolidFill");
        assert!(
            records[0].pdus[0]
                .detail
                .contains("surf=0 color=(10,20,30) rects=(1,1)-(3,3)"),
            "{}",
            records[0].pdus[0].detail
        );
    }

    #[test]
    fn pipeline_error_probe_records_current_record() {
        probe_begin(Vec::new());
        probe_set_record(42);

        probe_pipeline_error("decode failed");

        let errors = probe_take_pipeline_errors();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].record_seq, 42);
        assert_eq!(errors[0].detail, "decode failed");
        let _ = probe_take_events();
    }

    #[test]
    fn wire_to_surface2_frame_probe_queues_current_egfx_frame_id() {
        use ironrdp_core::{Encode as _, WriteCursor};
        use ironrdp_egfx::pdu::{
            Codec2Type, EndFramePdu, GfxPdu, PixelFormat, StartFramePdu, Timestamp,
            WireToSurface2Pdu,
        };
        use ironrdp_graphics::zgfx::wrap_uncompressed;

        fn encode_many(pdus: &[GfxPdu]) -> Vec<u8> {
            let mut bytes = vec![0u8; pdus.iter().map(GfxPdu::size).sum()];
            let mut cursor = WriteCursor::new(&mut bytes);
            for pdu in pdus {
                pdu.encode(&mut cursor).unwrap();
            }
            wrap_uncompressed(&bytes)
        }

        clear_wire_to_surface2_frame_queue();
        let wts = || {
            GfxPdu::WireToSurface2(WireToSurface2Pdu {
                surface_id: 0,
                codec_id: Codec2Type::RemoteFxProgressive,
                codec_context_id: 1,
                pixel_format: PixelFormat::XRgb,
                bitmap_data: Vec::new(),
            })
        };
        let payload = encode_many(&[
            GfxPdu::StartFrame(StartFramePdu {
                timestamp: Timestamp {
                    milliseconds: 0,
                    seconds: 0,
                    minutes: 0,
                    hours: 0,
                },
                frame_id: 77,
            }),
            wts(),
            wts(),
            GfxPdu::EndFrame(EndFramePdu { frame_id: 77 }),
        ]);
        let mut probe = WireToSurface2FrameProbe::new();

        probe.prepare(&payload);

        assert_eq!(probe_next_wire_to_surface2_frame_id(), Some(77));
        assert_eq!(probe_next_wire_to_surface2_frame_id(), Some(77));
        assert_eq!(probe_next_wire_to_surface2_frame_id(), None);
    }
}
