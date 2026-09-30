//! AVC444 抓包离线分析（docs/plans/2026-09-29-egfx-avc444.md 第 0 步）。
//! 读 `NEXSHELL_RDP_EGFX_WIRE_DUMP` 录下的 dump：统计 WireToSurface1 编码与 LC 分布、
//! NAL 构成、区域矩形与 destRect 的关系；按 surface 把主流/辅流拆成 Annex B 文件
//! （`surf<N>_main.h264` / `_aux.h264` / 线上顺序 `_wire.h264`），供 ffprobe/ffmpeg 试解。
//!
//! 用法：cargo run --example avc444_probe -- <wire.dump> <out_dir>

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use ironrdp_core::decode;
use ironrdp_egfx::pdu::{Avc420BitmapStream, Avc444BitmapStream, Codec1Type, Encoding, GfxPdu};
use ironrdp_pdu::geometry::ExclusiveRectangle;
use nexshell::rdp_session::for_each_wire_gfx_pdu;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(dump), Some(out)) = (args.next(), args.next()) else {
        eprintln!("用法: cargo run --example avc444_probe -- <wire.dump> <out_dir>");
        std::process::exit(2);
    };
    if let Err(e) = run(Path::new(&dump), &PathBuf::from(out)) {
        eprintln!("[avc444-probe] error: {e}");
        std::process::exit(1);
    }
}

#[derive(Default)]
struct Stats {
    pdu_kinds: BTreeMap<&'static str, u64>,
    wire1_codecs: BTreeMap<String, u64>,
    lc: BTreeMap<(String, u8), u64>,
    lc_pairs: BTreeMap<(u8, u8), u64>,
    last_lc: BTreeMap<u16, u8>,
    nal_types: BTreeMap<(&'static str, u8), u64>,
    param_sets: BTreeMap<&'static str, Vec<Vec<u8>>>,
    rect_rel: BTreeMap<&'static str, u64>,
    surfaces: BTreeMap<u16, (u16, u16)>,
    samples: Vec<String>,
}

struct Outputs {
    dir: PathBuf,
    files: BTreeMap<String, File>,
}

impl Outputs {
    fn write(&mut self, name: String, annex_b: &[u8]) -> io::Result<()> {
        if !self.files.contains_key(&name) {
            let f = File::create(self.dir.join(&name))?;
            self.files.insert(name.clone(), f);
        }
        self.files
            .get_mut(&name)
            .expect("inserted")
            .write_all(annex_b)
    }
}

fn run(dump: &Path, out_dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let mut index = File::create(out_dir.join("index.tsv"))?;
    writeln!(
        index,
        "seq\tframe\tsurf\tcodec\tlc\tdest\ts1_len\ts1_rects\ts1_qp\ts1_nals\ts2_len\ts2_rects\ts2_nals"
    )?;
    let mut outputs = Outputs {
        dir: out_dir.to_owned(),
        files: BTreeMap::new(),
    };
    let mut stats = Stats::default();
    let mut frame_id = 0u32;
    let mut failure: Option<io::Error> = None;

    for_each_wire_gfx_pdu(dump, |seq, pdu| {
        if failure.is_some() {
            return;
        }
        let kind = pdu_kind(pdu);
        *stats.pdu_kinds.entry(kind).or_default() += 1;
        match pdu {
            GfxPdu::StartFrame(p) => frame_id = p.frame_id,
            GfxPdu::CapabilitiesConfirm(p) => println!("[caps] confirm {:?}", p.0),
            GfxPdu::ResetGraphics(p) => println!("[reset] {}x{}", p.width, p.height),
            GfxPdu::CreateSurface(p) => {
                stats.surfaces.insert(p.surface_id, (p.width, p.height));
                println!(
                    "[surface] create {} {}x{} {:?}",
                    p.surface_id, p.width, p.height, p.pixel_format
                );
            }
            GfxPdu::MapSurfaceToOutput(p) => println!(
                "[surface] map {} -> ({}, {})",
                p.surface_id, p.output_origin_x, p.output_origin_y
            ),
            GfxPdu::WireToSurface1(p) => {
                *stats
                    .wire1_codecs
                    .entry(format!("{:?}", p.codec_id))
                    .or_default() += 1;
                let r = on_wire1(
                    &mut stats,
                    &mut outputs,
                    &mut index,
                    seq,
                    frame_id,
                    p.surface_id,
                    p.codec_id,
                    &p.destination_rectangle,
                    &p.bitmap_data,
                );
                if let Err(e) = r {
                    failure = Some(e);
                }
            }
            _ => {}
        }
    })?;
    if let Some(e) = failure {
        return Err(e);
    }
    print_summary(&stats);
    Ok(())
}

#[expect(clippy::too_many_arguments)]
fn on_wire1(
    stats: &mut Stats,
    outputs: &mut Outputs,
    index: &mut File,
    seq: u64,
    frame_id: u32,
    surface_id: u16,
    codec: Codec1Type,
    dest: &ExclusiveRectangle,
    data: &[u8],
) -> io::Result<()> {
    let codec_name = format!("{codec:?}");
    let (lc, main, aux) = match codec {
        Codec1Type::Avc420 => {
            let s = decode::<Avc420BitmapStream<'_>>(data).map_err(invalid)?;
            (None, Some(s), None)
        }
        Codec1Type::Avc444 | Codec1Type::Avc444v2 => {
            let s = decode::<Avc444BitmapStream<'_>>(data).map_err(invalid)?;
            let lc = s.encoding.bits();
            let (main, aux) = if s.encoding == Encoding::CHROMA {
                (None, Some(s.stream1))
            } else {
                (Some(s.stream1), s.stream2)
            };
            (Some(lc), main, aux)
        }
        _ => return Ok(()),
    };

    if let Some(lc) = lc {
        *stats.lc.entry((codec_name.clone(), lc)).or_default() += 1;
        if let Some(prev) = stats.last_lc.insert(surface_id, lc) {
            *stats.lc_pairs.entry((prev, lc)).or_default() += 1;
        }
    }

    let surface = stats.surfaces.get(&surface_id).copied();
    let main_role = if lc.is_some() { "main" } else { "avc420" };
    let mut cols = Vec::new();
    for (role, stream) in [(main_role, main.as_ref()), ("aux", aux.as_ref())] {
        let Some(stream) = stream else {
            cols.extend(["-".to_owned(), "-".to_owned(), "-".to_owned()]);
            if role != "aux" {
                cols.push("-".to_owned());
            }
            continue;
        };
        let annex_b = to_annex_b(stream.data);
        let nals = nal_units(&annex_b);
        for nal in &nals {
            let t = nal.first().map_or(0, |b| b & 0x1f);
            *stats.nal_types.entry((role, t)).or_default() += 1;
            if t == 7 || t == 8 {
                let key = if t == 7 { "sps" } else { "pps" };
                let key: &'static str = match (role, key) {
                    ("main", "sps") => "main sps",
                    ("main", _) => "main pps",
                    ("aux", "sps") => "aux sps",
                    ("aux", _) => "aux pps",
                    (_, "sps") => "avc420 sps",
                    _ => "avc420 pps",
                };
                let seen = stats.param_sets.entry(key).or_default();
                if !seen.iter().any(|s| s == nal) {
                    seen.push(nal.to_vec());
                }
            }
        }
        classify_rects(stats, dest, &stream.rectangles, surface);
        if stats.samples.len() < 12 {
            stats.samples.push(format!(
                "seq={seq} surf={surface_id} {codec_name} lc={lc:?} {role} dest={} rects={}",
                rect_desc(dest),
                rects_desc(&stream.rectangles)
            ));
        }
        outputs.write(format!("surf{surface_id}_{role}.h264"), &annex_b)?;
        if lc.is_some() {
            outputs.write(format!("surf{surface_id}_wire.h264"), &annex_b)?;
        }
        cols.push(annex_b.len().to_string());
        cols.push(rects_desc(&stream.rectangles));
        if role != "aux" {
            let qp: Vec<String> = stream
                .quant_qual_vals
                .iter()
                .map(|q| format!("{}/{}", q.quantization_parameter, q.quality))
                .collect();
            cols.push(qp.join(","));
        }
        let types: Vec<String> = nals
            .iter()
            .map(|n| n.first().map_or(0, |b| b & 0x1f).to_string())
            .collect();
        cols.push(types.join(","));
    }
    writeln!(
        index,
        "{seq}\t{frame_id}\t{surface_id}\t{codec_name}\t{}\t{}\t{}",
        lc.map_or("-".to_owned(), |l| l.to_string()),
        rect_desc(dest),
        cols.join("\t")
    )
}

/// 区域矩形是 surface 绝对坐标、相对 destRect，还是两者都说得通/都不对。
fn classify_rects(
    stats: &mut Stats,
    dest: &ExclusiveRectangle,
    rects: &[ExclusiveRectangle],
    surface: Option<(u16, u16)>,
) {
    let (dw, dh) = (dest.right - dest.left, dest.bottom - dest.top);
    for r in rects {
        let absolute = r.left >= dest.left
            && r.top >= dest.top
            && r.right <= dest.right
            && r.bottom <= dest.bottom;
        let relative = r.right <= dw && r.bottom <= dh;
        let key = match (absolute, relative) {
            (true, true) => "rect fits both",
            (true, false) => "rect absolute only",
            (false, true) => "rect relative only",
            (false, false) => "rect outside dest",
        };
        *stats.rect_rel.entry(key).or_default() += 1;
    }
    let full = surface.is_some_and(|(w, h)| {
        dest.left == 0 && dest.top == 0 && dest.right == w && dest.bottom == h
    });
    *stats
        .rect_rel
        .entry(if full {
            "dest full surface"
        } else {
            "dest partial"
        })
        .or_default() += 1;
}

fn print_summary(stats: &Stats) {
    println!("\n== PDU kinds");
    for (k, n) in &stats.pdu_kinds {
        println!("{k:>28} {n}");
    }
    println!("\n== WireToSurface1 codecs");
    for (k, n) in &stats.wire1_codecs {
        println!("{k:>28} {n}");
    }
    println!("\n== LC by codec (0=both 1=luma 2=chroma)");
    for ((c, lc), n) in &stats.lc {
        println!("{c:>20} lc={lc} {n}");
    }
    println!("\n== LC transitions per surface (prev -> cur)");
    for ((a, b), n) in &stats.lc_pairs {
        println!("{a} -> {b}: {n}");
    }
    println!("\n== NAL types by stream (1=slice 5=IDR 6=SEI 7=SPS 8=PPS 9=AUD)");
    for ((role, t), n) in &stats.nal_types {
        println!("{role:>8} type={t:<2} {n}");
    }
    println!("\n== distinct parameter sets");
    for (k, v) in &stats.param_sets {
        let hex: Vec<String> = v.iter().map(|s| hex(s)).collect();
        println!("{k:>10} x{}: {}", v.len(), hex.join(" | "));
    }
    println!("\n== rect vs dest (per stream)");
    for (k, n) in &stats.rect_rel {
        println!("{k:>22} {n}");
    }
    println!("\n== samples");
    for s in &stats.samples {
        println!("{s}");
    }
}

fn pdu_kind(pdu: &GfxPdu) -> &'static str {
    match pdu {
        GfxPdu::WireToSurface1(_) => "WireToSurface1",
        GfxPdu::WireToSurface2(_) => "WireToSurface2",
        GfxPdu::SolidFill(_) => "SolidFill",
        GfxPdu::SurfaceToSurface(_) => "SurfaceToSurface",
        GfxPdu::SurfaceToCache(_) => "SurfaceToCache",
        GfxPdu::CacheToSurface(_) => "CacheToSurface",
        GfxPdu::CreateSurface(_) => "CreateSurface",
        GfxPdu::DeleteSurface(_) => "DeleteSurface",
        GfxPdu::StartFrame(_) => "StartFrame",
        GfxPdu::EndFrame(_) => "EndFrame",
        GfxPdu::ResetGraphics(_) => "ResetGraphics",
        GfxPdu::MapSurfaceToOutput(_) => "MapSurfaceToOutput",
        GfxPdu::MapSurfaceToScaledOutput(_) => "MapSurfaceToScaledOutput",
        GfxPdu::CapabilitiesConfirm(_) => "CapabilitiesConfirm",
        GfxPdu::EvictCacheEntry(_) => "EvictCacheEntry",
        GfxPdu::CacheImportReply(_) => "CacheImportReply",
        GfxPdu::DeleteEncodingContext(_) => "DeleteEncodingContext",
        _ => "Other",
    }
}

fn invalid(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

fn is_annex_b(data: &[u8]) -> bool {
    data.starts_with(&[0, 0, 1]) || data.starts_with(&[0, 0, 0, 1])
}

/// 统一成 Annex B；AVCC（4 字节长度前缀）逐个 NAL 换成起始码。
fn to_annex_b(data: &[u8]) -> Vec<u8> {
    if is_annex_b(data) {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut rest = data;
    while rest.len() >= 4 {
        let len = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        let Some(nal) = rest.get(4..4 + len) else {
            break;
        };
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
        rest = &rest[4 + len..];
    }
    out
}

fn nal_units(annex_b: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= annex_b.len() {
        if annex_b[i] == 0 && annex_b[i + 1] == 0 && annex_b[i + 2] == 1 {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut nals = Vec::with_capacity(starts.len());
    for (k, &s) in starts.iter().enumerate() {
        let mut e = starts.get(k + 1).map_or(annex_b.len(), |&n| n - 3);
        while e > s && annex_b[e - 1] == 0 {
            e -= 1;
        }
        nals.push(&annex_b[s..e]);
    }
    nals
}

fn rect_desc(r: &ExclusiveRectangle) -> String {
    format!("{},{}-{},{}", r.left, r.top, r.right, r.bottom)
}

fn rects_desc(rects: &[ExclusiveRectangle]) -> String {
    let mut parts: Vec<String> = rects.iter().take(4).map(rect_desc).collect();
    if rects.len() > 4 {
        parts.push(format!("+{}", rects.len() - 4));
    }
    format!("[{}]", parts.join(" "))
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(24)
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}
