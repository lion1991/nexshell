//! RDP 协议层探针（无 UI）。连接 → NLA → 收帧约 5 秒，把最终 framebuffer
//! 编码成 PNG 落 /tmp/rdp_probe.png，并打印各阶段日志。
//!
//! 用法（凭据只走环境变量，绝不入代码/文件）：
//!   RDP_HOST=1.2.3.4 RDP_USER='DOMAIN\me' RDP_PASS='***' cargo run --example rdp_probe
//! 可选：RDP_PORT(默认3389) RDP_WIDTH(默认1280) RDP_HEIGHT(默认800)

use std::time::{Duration, Instant};

use nexshell::rdp_session::{spawn_rdp_session, RdpEvent, RdpSessionConfig};

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let host = std::env::var("RDP_HOST").expect("set RDP_HOST");
    let username = std::env::var("RDP_USER").expect("set RDP_USER");
    let password = std::env::var("RDP_PASS").expect("set RDP_PASS");
    let port: u16 = env_or("RDP_PORT", 3389);
    let width: u16 = env_or("RDP_WIDTH", 1280);
    let height: u16 = env_or("RDP_HEIGHT", 800);

    println!("[probe] connecting {host}:{port} as {username} ({width}x{height})");
    let started = Instant::now();
    let handle = spawn_rdp_session(RdpSessionConfig {
        host,
        port,
        username,
        password,
        width,
        height,
        // 与主程序同门控（docs/adr/0008 第①步）：开 NEXSHELL_RDP_EGFX=1 可无 UI 验证
        // EGFX 通道链路（看 stderr 的 [egfx] 日志；此时 PNG 会是黑屏，合成第②步做）。
        enable_egfx: std::env::var("NEXSHELL_RDP_EGFX").is_ok(),
    });

    // RDP_DURATION：采集秒数，缺省 5（长跑抓 EGFX dump 用）。
    let deadline = Instant::now() + Duration::from_secs(env_or("RDP_DURATION", 5));
    let mut connected_at: Option<Duration> = None;
    let mut first_frame_at: Option<Duration> = None;
    let mut frame_count = 0usize;

    // RDP_JIGGLE=1：每 500ms 抖一下鼠标，模拟交互会话（防服务端闲置掐线 + 触发持续重绘）。
    if std::env::var("RDP_JIGGLE").is_ok() {
        let input_tx = handle.input_tx.clone();
        std::thread::spawn(move || {
            let mut flip = false;
            loop {
                std::thread::sleep(Duration::from_millis(500));
                flip = !flip;
                let x = if flip { 500 } else { 520 };
                if input_tx
                    .send_blocking(nexshell::rdp_session::RdpInputEvent::MouseMove { x, y: 400 })
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match handle.frame_rx.recv_blocking() {
            Ok(RdpEvent::Connected) => {
                connected_at = Some(started.elapsed());
                println!(
                    "[probe] activated (NLA+capabilities) at {:?}",
                    connected_at.unwrap()
                );
            }
            Ok(RdpEvent::FrameUpdated { dirty }) => {
                frame_count += 1;
                if first_frame_at.is_none() {
                    first_frame_at = Some(started.elapsed());
                    println!(
                        "[probe] first frame at {:?} dirty={:?}",
                        first_frame_at.unwrap(),
                        dirty
                    );
                }
            }
            Ok(RdpEvent::Disconnected { reason }) => {
                println!("[probe] disconnected: {reason}");
                break;
            }
            Err(_) => break, // sender 关闭
        }
    }

    println!(
        "[probe] summary: connected={:?} first_frame={:?} frames={}",
        connected_at, first_frame_at, frame_count
    );

    // 落盘最终 framebuffer。
    let (w, h, rgba) = {
        let fb = handle.framebuffer.lock();
        (u32::from(fb.width), u32::from(fb.height), fb.rgba.clone())
    };
    match image::RgbaImage::from_raw(w, h, rgba) {
        Some(img) => match img.save("/tmp/rdp_probe.png") {
            Ok(()) => println!("[probe] wrote /tmp/rdp_probe.png ({w}x{h})"),
            Err(e) => println!("[probe] PNG save failed: {e}"),
        },
        None => println!("[probe] framebuffer size mismatch, skip PNG"),
    }

    handle.close();
}
