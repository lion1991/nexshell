//! VideoToolbox H.264 离线回放：读 AVC dump 目录逐帧喂 VtH264Decoder，打印 OK/FAIL。
//! 先真机采样：NEXSHELL_RDP_EGFX_DUMP=/tmp/avc cargo run --example rdp_probe ...
//! 再离线迭代：cargo run --example vt_replay /tmp/avc [--ppm <out.ppm>]
//! --ppm：把首个成功解码帧落成 P6 PPM，供人工目检解码色彩（NV12→RGBA 的 U/V 次序）。

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut dir: Option<PathBuf> = None;
    let mut ppm: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--ppm" => {
                ppm = args.next().map(PathBuf::from).or_else(|| {
                    eprintln!("--ppm 需要一个输出路径参数");
                    std::process::exit(2);
                });
            }
            _ => dir = Some(PathBuf::from(a)),
        }
    }
    let Some(dir) = dir else {
        eprintln!("用法: cargo run --example vt_replay <dump_dir> [--ppm <out.ppm>]");
        std::process::exit(2);
    };
    if let Err(e) = nexshell::rdp_session::vt_replay_dir(&dir, ppm.as_deref()) {
        eprintln!("[replay] error: {e}");
        std::process::exit(1);
    }
}
