//! EGFX wire dump 离线重放：读取 `NEXSHELL_RDP_EGFX_WIRE_DUMP` 生成的 dump，
//! 逐条喂回 EGFX pipeline，并在 FrameUpdated 后输出 hash/PNG，辅助二分首次分歧。

use std::io;
use std::path::PathBuf;

use nexshell::rdp_session::{
    inspect_wire_dump_pdus_with_points, replay_wire_dump, ChecksumRect, WatchPoint,
    WireReplayOptions,
};

fn main() {
    if let Err(e) = run() {
        eprintln!("[egfx-replay] error: {e}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let args = parse_args().map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    if let Some(dir) = &args.out_dir {
        std::fs::create_dir_all(dir)?;
    }
    if !args.list_records.is_empty() {
        for record in
            inspect_wire_dump_pdus_with_points(&args.dump, &args.list_records, &args.watch_points)?
        {
            println!(
                "[egfx-pdu] record={} channel={} payload={} decompressed={} pdus={} error={}",
                record.seq,
                record.channel_id,
                record.payload_len,
                record.decompressed_len,
                record.pdus.len(),
                record.error.as_deref().unwrap_or("None")
            );
            for pdu in &record.pdus {
                println!(
                    "[egfx-pdu] record={} index={} kind={} len={} {}",
                    record.seq, pdu.index, pdu.kind, pdu.encoded_len, pdu.detail
                );
            }
        }
        if args.inspect_only {
            return Ok(());
        }
    }

    let summary = replay_wire_dump(
        WireReplayOptions {
            dump_path: &args.dump,
            until_seq: args.until,
            frame_every: args.every,
            checksum_rect: args.tile,
            watch_points: args.watch_points.clone(),
        },
        |frame| {
            let tile = frame
                .tile_hash
                .map_or_else(|| "tile=None".to_owned(), |h| format!("tile={h:016x}"));
            println!(
                "[egfx-replay] frame={} record={} dirty=({},{} {}x{}) size={}x{} hash={:016x} {}",
                frame.frame_index,
                frame.record_seq,
                frame.dirty.x,
                frame.dirty.y,
                frame.dirty.width,
                frame.dirty.height,
                frame.width,
                frame.height,
                frame.full_hash,
                tile
            );

            if !args.no_png {
                if let Some(dir) = &args.out_dir {
                    let path = dir.join(format!(
                        "frame_{:06}_rec_{:06}_{}x{}.png",
                        frame.frame_index, frame.record_seq, frame.width, frame.height
                    ));
                    image::save_buffer_with_format(
                        &path,
                        &frame.rgba,
                        u32::from(frame.width),
                        u32::from(frame.height),
                        image::ColorType::Rgba8,
                        image::ImageFormat::Png,
                    )
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                }
            }
            Ok(())
        },
    )?;

    println!(
        "[egfx-replay] summary records={} frames={} final={}x{} hash={} tile={}",
        summary.records,
        summary.frames,
        summary.final_width,
        summary.final_height,
        summary
            .final_hash
            .map_or_else(|| "None".to_owned(), |h| format!("{h:016x}")),
        summary
            .final_tile_hash
            .map_or_else(|| "None".to_owned(), |h| format!("{h:016x}"))
    );
    for event in &summary.watch_events {
        println!(
            "[egfx-watch] record={} point=({}, {}) op={} value={:?} {}",
            event.record_seq, event.point.x, event.point.y, event.op, event.value, event.detail
        );
    }
    for error in &summary.pipeline_errors {
        println!("[egfx-error] record={} {}", error.record_seq, error.detail);
    }
    Ok(())
}

#[derive(Debug)]
struct Args {
    dump: PathBuf,
    out_dir: Option<PathBuf>,
    until: Option<u64>,
    every: u64,
    tile: Option<ChecksumRect>,
    watch_points: Vec<WatchPoint>,
    list_records: Vec<u64>,
    inspect_only: bool,
    no_png: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut raw = std::env::args().skip(1);
    let mut dump = None;
    let mut out_dir = None;
    let mut until = None;
    let mut every = 1;
    let mut tile = None;
    let mut watch_points = Vec::new();
    let mut list_records = Vec::new();
    let mut inspect_only = false;
    let mut no_png = false;

    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "--help" | "-h" => return Err(usage()),
            "--out-dir" => out_dir = Some(next_path(&mut raw, "--out-dir")?),
            "--until" => until = Some(next_parse(&mut raw, "--until")?),
            "--every" => every = next_parse(&mut raw, "--every")?,
            "--tile" => tile = Some(parse_tile(&next_string(&mut raw, "--tile")?)?),
            "--watch-pixel" => {
                watch_points.push(parse_watch_point(&next_string(&mut raw, "--watch-pixel")?)?)
            }
            "--list-record" => list_records.push(next_parse(&mut raw, "--list-record")?),
            "--list-records" => list_records.extend(parse_record_range(&next_string(
                &mut raw,
                "--list-records",
            )?)?),
            "--inspect-only" => inspect_only = true,
            "--no-png" => no_png = true,
            _ if arg.starts_with('-') => return Err(format!("未知参数 {arg}\n{}", usage())),
            _ => {
                if dump.replace(PathBuf::from(arg)).is_some() {
                    return Err(format!("只能提供一个 dump 路径\n{}", usage()));
                }
            }
        }
    }

    let dump = dump.ok_or_else(usage)?;
    Ok(Args {
        dump,
        out_dir,
        until,
        every: every.max(1),
        tile,
        watch_points,
        list_records,
        inspect_only,
        no_png,
    })
}

fn next_string(raw: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    raw.next()
        .ok_or_else(|| format!("{flag} 需要一个参数\n{}", usage()))
}

fn next_path(raw: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf, String> {
    next_string(raw, flag).map(PathBuf::from)
}

fn next_parse<T>(raw: &mut impl Iterator<Item = String>, flag: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    let value = next_string(raw, flag)?;
    value
        .parse()
        .map_err(|_| format!("{flag} 参数格式无效: {value}"))
}

fn parse_tile(value: &str) -> Result<ChecksumRect, String> {
    let parts: Vec<_> = value.split(',').collect();
    if parts.len() != 4 {
        return Err("--tile 格式必须是 x,y,w,h".to_owned());
    }
    let parse = |s: &str| {
        s.parse::<u16>()
            .map_err(|_| format!("--tile 数字无效: {s}"))
    };
    Ok(ChecksumRect {
        x: parse(parts[0])?,
        y: parse(parts[1])?,
        width: parse(parts[2])?,
        height: parse(parts[3])?,
    })
}

fn parse_watch_point(value: &str) -> Result<WatchPoint, String> {
    let parts: Vec<_> = value.split(',').collect();
    if parts.len() != 2 {
        return Err("--watch-pixel 格式必须是 x,y".to_owned());
    }
    let parse = |s: &str| {
        s.parse::<u16>()
            .map_err(|_| format!("--watch-pixel 数字无效: {s}"))
    };
    Ok(WatchPoint {
        x: parse(parts[0])?,
        y: parse(parts[1])?,
    })
}

fn parse_record_range(value: &str) -> Result<Vec<u64>, String> {
    let Some((start, end)) = value.split_once("..") else {
        return Ok(vec![value
            .parse()
            .map_err(|_| format!("--list-records 数字无效: {value}"))?]);
    };
    let start = start
        .parse::<u64>()
        .map_err(|_| format!("--list-records 起点无效: {start}"))?;
    let end = end
        .parse::<u64>()
        .map_err(|_| format!("--list-records 终点无效: {end}"))?;
    if end < start {
        return Err("--list-records 终点不能小于起点".to_owned());
    }
    Ok((start..=end).collect())
}

fn usage() -> String {
    "用法: cargo run --example egfx_replay -- <dump> [--out-dir <dir>] [--until <seq>] [--every <n>] [--tile x,y,w,h] [--watch-pixel x,y] [--list-record <seq>] [--list-records a..b] [--inspect-only] [--no-png]".to_owned()
}
