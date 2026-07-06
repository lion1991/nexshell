//! RDP 协议层探针（无 UI）。连接 → NLA → 收帧约 5 秒，把最终 framebuffer
//! 编码成 PNG 落 /tmp/rdp_probe.png，并打印各阶段日志。
//!
//! 用法（凭据只走环境变量，绝不入代码/文件）：
//!   RDP_HOST=1.2.3.4 RDP_USER='DOMAIN\me' RDP_PASS='***' cargo run --example rdp_probe
//! 可选：RDP_PORT(默认3389) RDP_WIDTH(默认1280) RDP_HEIGHT(默认800) RDP_SCALE(远端DPI%,默认0=不请求)

use std::time::{Duration, Instant};

use nexshell::rdp_session::{default_enable_egfx, spawn_rdp_session, RdpEvent, RdpSessionConfig};

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
    // RDP_SCALE：请求远端 DPI 缩放百分比（如 200），默认 0=不请求。
    let desktop_scale_factor: u32 = env_or("RDP_SCALE", 0);

    println!("[probe] connecting {host}:{port} as {username} ({width}x{height})");
    let started = Instant::now();
    let handle = spawn_rdp_session(RdpSessionConfig {
        host,
        port,
        username,
        password,
        width,
        height,
        // 与主程序同策略：默认走 EGFX；设 NEXSHELL_RDP_DISABLE_EGFX=1 可回退旧管线对照。
        enable_egfx: default_enable_egfx(),
        // RDPSND 音频重定向：默认开，RDP_AUDIO=0 可关闭对照。
        enable_audio: std::env::var("RDP_AUDIO").map(|v| v != "0").unwrap_or(true),
        desktop_scale_factor,
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

    // RDP_RESIZE="WxH[,delay_ms]"：到点（默认3000ms）发一次动态分辨率请求，验证 MS-RDPEDISP。
    if let Ok(spec) = std::env::var("RDP_RESIZE") {
        let (wh, delay) = spec.split_once(',').unwrap_or((spec.as_str(), "3000"));
        if let Some((w, h)) = wh.split_once('x') {
            if let (Ok(w), Ok(h), Ok(delay)) = (
                w.trim().parse::<u16>(),
                h.trim().parse::<u16>(),
                delay.trim().parse::<u64>(),
            ) {
                let tx = handle.resize_tx.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(delay));
                    let _ = tx.send_blocking(nexshell::rdp_session::RdpResizeRequest {
                        width: w,
                        height: h,
                        scale_factor: desktop_scale_factor,
                    });
                    println!("[probe] sent resize {w}x{h}");
                });
            }
        }
    }

    // RDP_SNAPSHOT_DIR=<dir>：每 RDP_SNAPSHOT_MS（默认200）把 framebuffer 落 PNG，闪烁/残留判定用。
    if let Ok(dir) = std::env::var("RDP_SNAPSHOT_DIR") {
        let fb = handle.framebuffer.clone();
        let interval: u64 = env_or("RDP_SNAPSHOT_MS", 200);
        std::thread::spawn(move || {
            let _ = std::fs::create_dir_all(&dir);
            for i in 0.. {
                std::thread::sleep(Duration::from_millis(interval));
                let (w, h, rgba) = {
                    let fb = fb.lock();
                    (u32::from(fb.width), u32::from(fb.height), fb.rgba.clone())
                };
                if let Some(img) = image::RgbaImage::from_raw(w, h, rgba) {
                    let _ = img.save(format!("{dir}/frame_{i:04}.png"));
                }
            }
        });
    }

    // RDP_WIN_E=<ms>：到点发 Win+E 开资源管理器（拖窗复现的窗口来源）。
    if let Ok(at) = std::env::var("RDP_WIN_E") {
        let at: u64 = at.parse().unwrap_or(2000);
        let tx = handle.input_tx.clone();
        std::thread::spawn(move || {
            use nexshell::rdp_session::RdpInputEvent::Key;
            std::thread::sleep(Duration::from_millis(at));
            for (sc, ext, dn) in [
                (0x5Bu8, true, true), // Win down
                (0x12, false, true),  // E down
                (0x12, false, false), // E up
                (0x5B, true, false),  // Win up
            ] {
                let _ = tx.send_blocking(Key {
                    scancode: sc,
                    extended: ext,
                    pressed: dn,
                });
                std::thread::sleep(Duration::from_millis(60));
            }
            println!("[probe] sent Win+E");
        });
    }

    // RDP_WINMOVE="start_ms,cycles,steps"：Alt+Space→M 进键盘移窗模式，方向键左右往返
    // cycles 次、每程 steps 步，Enter 收尾（拖动窗口的确定性替代，图形路径相同）。
    if let Ok(spec) = std::env::var("RDP_WINMOVE") {
        let v: Vec<u64> = spec
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if v.len() == 3 {
            let (start_ms, cycles, steps) = (v[0], v[1], v[2]);
            let tx = handle.input_tx.clone();
            std::thread::spawn(move || {
                use nexshell::rdp_session::RdpInputEvent::Key;
                let tap = |sc: u8, ext: bool| {
                    let _ = tx.send_blocking(Key {
                        scancode: sc,
                        extended: ext,
                        pressed: true,
                    });
                    std::thread::sleep(Duration::from_millis(25));
                    let _ = tx.send_blocking(Key {
                        scancode: sc,
                        extended: ext,
                        pressed: false,
                    });
                    std::thread::sleep(Duration::from_millis(25));
                };
                std::thread::sleep(Duration::from_millis(start_ms));
                // RDP_UNMAX=1：先 Win+Down 还原最大化窗口为浮动窗口（否则 Move 菜单项禁用）。
                if std::env::var("RDP_UNMAX").is_ok() {
                    let _ = tx.send_blocking(Key {
                        scancode: 0x5B,
                        extended: true,
                        pressed: true,
                    });
                    tap(0x50, true); // Down
                    let _ = tx.send_blocking(Key {
                        scancode: 0x5B,
                        extended: true,
                        pressed: false,
                    });
                    std::thread::sleep(Duration::from_millis(500));
                }
                // Alt+Space（系统菜单）
                let _ = tx.send_blocking(Key {
                    scancode: 0x38,
                    extended: false,
                    pressed: true,
                });
                std::thread::sleep(Duration::from_millis(40));
                tap(0x39, false);
                let _ = tx.send_blocking(Key {
                    scancode: 0x38,
                    extended: false,
                    pressed: false,
                });
                std::thread::sleep(Duration::from_millis(400));
                tap(0x32, false); // M = 移动
                std::thread::sleep(Duration::from_millis(300));
                for c in 0..cycles {
                    let (sc, down) = if c % 2 == 0 {
                        (0x4Du8, 0x50u8)
                    } else {
                        (0x4B, 0x48)
                    };
                    for _ in 0..steps {
                        tap(sc, true); // 左/右
                    }
                    for _ in 0..steps / 2 {
                        tap(down, true); // 下/上
                    }
                }
                tap(0x1C, false); // Enter 确认
                println!("[probe] winmove done");
            });
        }
    }

    // RDP_RUN="start_ms:命令行"：到点 Win+R → 敲入命令 → Enter（造动态内容窗口用，
    // 如 "cmd /k dir c:\\windows\\system32 /s" 得到持续滚动的窗口）。仅覆盖常用 ASCII。
    if let Ok(spec) = std::env::var("RDP_RUN") {
        if let Some((at, cmdline)) = spec.split_once(':') {
            let at: u64 = at.trim().parse().unwrap_or(2000);
            let cmdline = cmdline.to_string();
            let tx = handle.input_tx.clone();
            std::thread::spawn(move || {
                use nexshell::rdp_session::RdpInputEvent::Key;
                let key = |sc: u8, ext: bool, dn: bool| {
                    let _ = tx.send_blocking(Key {
                        scancode: sc,
                        extended: ext,
                        pressed: dn,
                    });
                    std::thread::sleep(Duration::from_millis(20));
                };
                let tap = |sc: u8| {
                    key(sc, false, true);
                    key(sc, false, false);
                };
                std::thread::sleep(Duration::from_millis(at));
                key(0x5B, true, true); // Win+R
                tap(0x13);
                key(0x5B, true, false);
                std::thread::sleep(Duration::from_millis(700));
                for ch in cmdline.chars() {
                    let (sc, shift) = match ch.to_ascii_lowercase() {
                        'a' => (0x1E, false),
                        'b' => (0x30, false),
                        'c' => (0x2E, false),
                        'd' => (0x20, false),
                        'e' => (0x12, false),
                        'f' => (0x21, false),
                        'g' => (0x22, false),
                        'h' => (0x23, false),
                        'i' => (0x17, false),
                        'j' => (0x24, false),
                        'k' => (0x25, false),
                        'l' => (0x26, false),
                        'm' => (0x32, false),
                        'n' => (0x31, false),
                        'o' => (0x18, false),
                        'p' => (0x19, false),
                        'q' => (0x10, false),
                        'r' => (0x13, false),
                        's' => (0x1F, false),
                        't' => (0x14, false),
                        'u' => (0x16, false),
                        'v' => (0x2F, false),
                        'w' => (0x11, false),
                        'x' => (0x2D, false),
                        'y' => (0x15, false),
                        'z' => (0x2C, false),
                        '1' => (0x02, false),
                        '2' => (0x03, false),
                        '3' => (0x04, false),
                        '4' => (0x05, false),
                        '5' => (0x06, false),
                        '6' => (0x07, false),
                        '7' => (0x08, false),
                        '8' => (0x09, false),
                        '9' => (0x0A, false),
                        '0' => (0x0B, false),
                        ' ' => (0x39, false),
                        '.' => (0x34, false),
                        '/' => (0x35, false),
                        '\\' => (0x2B, false),
                        '-' => (0x0C, false),
                        ':' => (0x27, true),
                        '_' => (0x0C, true),
                        '"' => (0x28, true),
                        _ => continue,
                    };
                    if shift {
                        key(0x2A, false, true);
                    }
                    tap(sc);
                    if shift {
                        key(0x2A, false, false);
                    }
                }
                tap(0x1C); // Enter
                println!("[probe] run sent: {cmdline}");
            });
        }
    }

    // RDP_HOLDDRAG="x0,y0,dx,dy,start_ms,hold_ms"：(x0,y0) 左键按下 → 8px/30ms 微移
    // (dx,dy) 触发窗口拖动 → **按住悬停** hold_ms 不动 → 释放。复现"动态窗口按住不放持续抖动"。
    if let Ok(spec) = std::env::var("RDP_HOLDDRAG") {
        let v: Vec<i32> = spec
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if v.len() == 6 {
            let (x0, y0, dx, dy, start_ms, hold_ms) =
                (v[0], v[1], v[2], v[3], v[4] as u64, v[5] as u64);
            let tx = handle.input_tx.clone();
            std::thread::spawn(move || {
                use nexshell::rdp_session::{RdpButton, RdpInputEvent};
                std::thread::sleep(Duration::from_millis(start_ms));
                let _ = tx.send_blocking(RdpInputEvent::MouseMove {
                    x: x0 as u16,
                    y: y0 as u16,
                });
                std::thread::sleep(Duration::from_millis(100));
                let _ = tx.send_blocking(RdpInputEvent::MouseButton {
                    button: RdpButton::Left,
                    pressed: true,
                    x: x0 as u16,
                    y: y0 as u16,
                });
                std::thread::sleep(Duration::from_millis(120));
                let steps = (dx.abs().max(dy.abs()) / 8).max(1);
                for s in 1..=steps {
                    let _ = tx.send_blocking(RdpInputEvent::MouseMove {
                        x: (x0 + dx * s / steps) as u16,
                        y: (y0 + dy * s / steps) as u16,
                    });
                    std::thread::sleep(Duration::from_millis(30));
                }
                println!("[probe] holddrag holding at ({},{})", x0 + dx, y0 + dy);
                // RDP_HOLDJITTER=amp：hold 期间每 16ms 发一次 MouseMove，在 hx..hx+amp 间振荡
                // （amp=0 即重复发同一坐标）。复现"物理鼠标静止但客户端仍发移动"导致的窗口抖动。
                let amp: i32 = std::env::var("RDP_HOLDJITTER")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(-1);
                let (hx, hy) = (x0 + dx, y0 + dy);
                if amp >= 0 {
                    let ticks = hold_ms / 16;
                    let mut f = false;
                    for _ in 0..ticks {
                        f = !f;
                        let jx = if f { hx + amp } else { hx };
                        let _ = tx.send_blocking(RdpInputEvent::MouseMove {
                            x: jx as u16,
                            y: hy as u16,
                        });
                        std::thread::sleep(Duration::from_millis(16));
                    }
                } else {
                    std::thread::sleep(Duration::from_millis(hold_ms));
                }
                let _ = tx.send_blocking(RdpInputEvent::MouseButton {
                    button: RdpButton::Left,
                    pressed: false,
                    x: (x0 + dx) as u16,
                    y: (y0 + dy) as u16,
                });
                println!("[probe] holddrag released");
            });
        }
    }

    // RDP_CLICK="x,y,ms"：到点在 (x,y) 左键单击（输入链路可见性对照）。
    if let Ok(spec) = std::env::var("RDP_CLICK") {
        let v: Vec<i32> = spec
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if v.len() == 3 {
            let (x, y, at) = (v[0] as u16, v[1] as u16, v[2] as u64);
            let tx = handle.input_tx.clone();
            std::thread::spawn(move || {
                use nexshell::rdp_session::{RdpButton, RdpInputEvent};
                std::thread::sleep(Duration::from_millis(at));
                let _ = tx.send_blocking(RdpInputEvent::MouseMove { x, y });
                std::thread::sleep(Duration::from_millis(80));
                for pressed in [true, false] {
                    let _ = tx.send_blocking(RdpInputEvent::MouseButton {
                        button: RdpButton::Left,
                        pressed,
                        x,
                        y,
                    });
                    std::thread::sleep(Duration::from_millis(80));
                }
                println!("[probe] click done ({x},{y})");
            });
        }
    }

    // RDP_DRAG="x0,y0,x1,y1,start_ms,cycles"：start_ms 时刻在 (x0,y0) 左键按下，
    // 以 30ms/步、32px/步在两点间往返 cycles 次后释放（模拟拖动窗口）。
    if let Ok(spec) = std::env::var("RDP_DRAG") {
        let v: Vec<i32> = spec
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if v.len() == 6 {
            let (x0, y0, x1, y1, start_ms, cycles) = (v[0], v[1], v[2], v[3], v[4] as u64, v[5]);
            let tx = handle.input_tx.clone();
            std::thread::spawn(move || {
                use nexshell::rdp_session::{RdpButton, RdpInputEvent};
                let mv = |tx: &async_channel::Sender<RdpInputEvent>, x: i32, y: i32| {
                    let _ = tx.send_blocking(RdpInputEvent::MouseMove {
                        x: x as u16,
                        y: y as u16,
                    });
                };
                std::thread::sleep(Duration::from_millis(start_ms));
                mv(&tx, x0, y0);
                std::thread::sleep(Duration::from_millis(100));
                let _ = tx.send_blocking(RdpInputEvent::MouseButton {
                    button: RdpButton::Left,
                    pressed: true,
                    x: x0 as u16,
                    y: y0 as u16,
                });
                std::thread::sleep(Duration::from_millis(150));
                let steps = ((x1 - x0).abs().max((y1 - y0).abs()) / 32).max(1);
                for c in 0..cycles {
                    let (fx, fy, txx, tyy) = if c % 2 == 0 {
                        (x0, y0, x1, y1)
                    } else {
                        (x1, y1, x0, y0)
                    };
                    for s in 1..=steps {
                        mv(
                            &tx,
                            fx + (txx - fx) * s / steps,
                            fy + (tyy - fy) * s / steps,
                        );
                        std::thread::sleep(Duration::from_millis(30));
                    }
                }
                let (ex, ey) = if cycles % 2 == 0 { (x0, y0) } else { (x1, y1) };
                let _ = tx.send_blocking(RdpInputEvent::MouseButton {
                    button: RdpButton::Left,
                    pressed: false,
                    x: ex as u16,
                    y: ey as u16,
                });
                println!("[probe] drag done");
            });
        } else {
            println!("[probe] RDP_DRAG malformed (need 6 ints), ignored");
        }
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
            Ok(RdpEvent::PointerChanged(_)) => {} // probe 不关心指针形状
            Ok(RdpEvent::Resized { width, height }) => {
                println!("[probe] resized {width}x{height}");
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
