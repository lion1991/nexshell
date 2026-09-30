//! RDP 协议层（IronRDP 纯 Rust，见 docs/adr/0007）。
//! 线程模型照抄 SSH：每连接一个专用 OS 线程 + current-thread tokio，
//! 事件循环 block_on 跑，帧解码成 RGBA framebuffer，脏矩形事件推回 UI。
//! 本步只做：TCP → NLA(CredSSP) → 能力协商 → 收图形更新 → 合成 framebuffer。
//! 输入/剪贴板留占位（RdpInputEvent + input_tx），后续步骤实现。

mod audio_diag;
mod clipboard;
mod egfx;
mod frame_marker;
mod rdpdr;
mod stats;
mod udp;

pub use egfx::{
    for_each_wire_gfx_pdu, inspect_wire_dump_pdus, inspect_wire_dump_pdus_with_points,
    replay_wire_dump, vt_replay_dir, ChecksumRect, WatchEvent, WatchPoint, WirePduInfo,
    WirePduRecord, WirePipelineError, WireReplayFrame, WireReplayOptions, WireReplaySummary,
};
pub use stats::{format_duration_hms, fps, mbps, RdpStats};
pub use udp::default_enable_udp;

use std::net::SocketAddr;
#[cfg(target_os = "macos")]
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ironrdp_async::FramedWrite;
use ironrdp_cliprdr::backend::ClipboardMessage;
use ironrdp_cliprdr::CliprdrClient;
use ironrdp_connector::connection_activation::{
    ConnectionActivationSequence, ConnectionActivationState,
};
use ironrdp_connector::sspi::generator::NetworkRequest;
use ironrdp_connector::{
    BitmapConfig, ClientConnector, Config, ConnectorResult, Credentials, DesktopSize, ServerName,
};
use ironrdp_core::WriteBuf;
use ironrdp_displaycontrol::pdu::MonitorLayoutEntry;
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_pdu::geometry::InclusiveRectangle;
use ironrdp_pdu::input::fast_path::FastPathInputEvent;
use ironrdp_pdu::rdp::capability_sets::BitmapCodecs;
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{ActiveStage, ActiveStageBuilder, ActiveStageOutput};
use parking_lot::Mutex;
use tokio::net::TcpStream;

use crate::rdp_cert_store;

/// 连接参数，由调用方（主机库）填。分辨率也由调用方定。
#[derive(Clone, Debug)]
pub struct RdpSessionConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub width: u16,
    pub height: u16,
    /// 开 EGFX 图形管线（MS-RDPEGFX，docs/adr/0008 第①步）。
    /// 第①步临时用 NEXSHELL_RDP_EGFX 环境变量门控，出画面后（第②步）改默认开。
    pub enable_egfx: bool,
    /// RDPSND 音频重定向开关（仅输出方向，cpal 播放 + Opus 解码）。
    pub enable_audio: bool,
    /// RDPDR 驱动器重定向开关（`~/NexShell RDP` ↔ 远端 \\tsclient\NexShell 文件互拷）。
    pub enable_drive: bool,
    /// 远端 DPI 缩放百分比（[100,500] 有效，0=不请求，HiDPI 下=物理/逻辑×100）。
    pub desktop_scale_factor: u32,
    /// 可靠 UDP 旁路（docs/adr/0014）：服务端提供时把图形等动态通道迁到 UDP，失败自动留在 TCP。
    pub enable_udp: bool,
}

fn default_enable_egfx_from_env(disable_egfx: Option<std::ffi::OsString>) -> bool {
    disable_egfx.is_none()
}

/// EGFX is the default graphics pipeline; set NEXSHELL_RDP_DISABLE_EGFX=1 for legacy fallback.
pub fn default_enable_egfx() -> bool {
    default_enable_egfx_from_env(std::env::var_os("NEXSHELL_RDP_DISABLE_EGFX"))
}

/// 连接类型默认固定 LAN；NEXSHELL_RDP_AUTODETECT=1 改报自动探测。服务端关了网络探测时
/// Autodetect 会被按最小带宽处理（帧率掉到个位数），见 docs/adr/0014。
fn connection_type_from_env(
    autodetect: Option<std::ffi::OsString>,
) -> ironrdp_pdu::gcc::ConnectionType {
    if autodetect.is_some() {
        ironrdp_pdu::gcc::ConnectionType::Autodetect
    } else {
        ironrdp_pdu::gcc::ConnectionType::Lan
    }
}

impl RdpSessionConfig {
    /// UDP 只迁动态通道，DRDYNVC 随 EGFX 注册；关 EGFX 时不声明 UDP，否则隧道建成后无通道可迁。
    fn udp_effective(&self) -> bool {
        self.enable_udp && self.enable_egfx
    }
}

/// 脏矩形（左上原点，像素）。本步图形更新统一按整帧上报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

/// 推回 UI 的会话事件。
#[derive(Clone, Debug)]
pub enum RdpEvent {
    /// 激活完成，可以开始收帧。
    Connected,
    /// framebuffer 已更新（脏区域）。UI 收到后读 Arc<Mutex<RdpFramebuffer>> 重绘。
    FrameUpdated { dirty: DirtyRect },
    /// 连接结束（正常或错误）。
    Disconnected { reason: String },
    /// 远端光标形态变化：本地据此接管/隐藏/复原鼠标（accelerated 模式，不合成进帧）。
    PointerChanged(RdpPointer),
    /// 远端分辨率已变（动态分辨率生效：EGFX ResetGraphics 或 Deactivation-Reactivation）。
    /// framebuffer 已按新尺寸重建，UI 据此刷新桌面分辨率并重置上传代号。
    Resized { width: u16, height: u16 },
}

/// UI → 会话线程的分辨率重设请求（物理像素，UI 已按 HiDPI 换算好）。
/// 走独立通道而非 RdpInputEvent（后者是 Copy 的 FastPath 输入热路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RdpResizeRequest {
    pub width: u16,
    pub height: u16,
    /// 远端 DPI 缩放百分比（[100,500] 有效，0=不请求）。
    pub scale_factor: u32,
}

/// 远端下发的光标形态。语义对齐 mstsc/FreeRDP：
/// Default=系统箭头，Hidden=隐藏，Bitmap=自定义位图（New/Cached/Color/Large）。
#[derive(Clone, Debug)]
pub enum RdpPointer {
    Default,
    Hidden,
    Bitmap {
        /// 非预乘 RGBA（accelerated target）。
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        hotspot_x: f32,
        hotspot_y: f32,
        /// 缓存/判等键（用 DecodedPointer 的 Arc 地址）。
        cache_key: u64,
    },
}

/// 鼠标按钮（左/中/右）。侧键不支持（warpui 未派发其抬起事件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RdpButton {
    Left,
    Middle,
    Right,
}

/// UI → 会话线程的键鼠事件。坐标均为远端桌面像素（已由 viewport 反算+clamp）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RdpInputEvent {
    /// 指针移动。
    MouseMove { x: u16, y: u16 },
    /// 按钮按下/抬起。
    MouseButton {
        button: RdpButton,
        pressed: bool,
        x: u16,
        y: u16,
    },
    /// 滚轮。`horizontal=false` 为垂直；`delta` 为带符号刻度（RDP rotation units，约 ±120/格）。
    Wheel {
        horizontal: bool,
        delta: i16,
        x: u16,
        y: u16,
    },
    /// 键盘：`scancode` 为 PC set-1 单字节码；`extended` 表示需 0xE0 前缀（方向/编辑键/右修饰/Win）。
    Key {
        scancode: u8,
        extended: bool,
        pressed: bool,
    },
}

/// 单个 RdpInputEvent → IronRDP FastPath 输入事件。按钮/滚轮标志见 MS-RDPBCGR TS_FP_POINTER_EVENT。
fn to_fastpath_input(event: RdpInputEvent) -> FastPathInputEvent {
    use ironrdp_pdu::input::fast_path::KeyboardFlags;
    use ironrdp_pdu::input::mouse::PointerFlags;
    use ironrdp_pdu::input::MousePdu;

    match event {
        RdpInputEvent::MouseMove { x, y } => FastPathInputEvent::MouseEvent(MousePdu {
            flags: PointerFlags::MOVE,
            number_of_wheel_rotation_units: 0,
            x_position: x,
            y_position: y,
        }),
        RdpInputEvent::MouseButton {
            button,
            pressed,
            x,
            y,
        } => {
            let mut flags = match button {
                RdpButton::Left => PointerFlags::LEFT_BUTTON,
                RdpButton::Right => PointerFlags::RIGHT_BUTTON,
                RdpButton::Middle => PointerFlags::MIDDLE_BUTTON_OR_WHEEL,
            };
            if pressed {
                flags |= PointerFlags::DOWN;
            }
            FastPathInputEvent::MouseEvent(MousePdu {
                flags,
                number_of_wheel_rotation_units: 0,
                x_position: x,
                y_position: y,
            })
        }
        RdpInputEvent::Wheel {
            horizontal,
            delta,
            x,
            y,
        } => {
            // WHEEL_NEGATIVE 位由 MousePdu::encode 按 delta 符号自动置，这里只给方向+带符号量。
            let flags = if horizontal {
                PointerFlags::HORIZONTAL_WHEEL
            } else {
                PointerFlags::VERTICAL_WHEEL
            };
            FastPathInputEvent::MouseEvent(MousePdu {
                flags,
                number_of_wheel_rotation_units: delta,
                x_position: x,
                y_position: y,
            })
        }
        RdpInputEvent::Key {
            scancode,
            extended,
            pressed,
        } => {
            if key_trace() {
                eprintln!(
                    "[nexshell key-debug] fastpath encode scancode=0x{scancode:02X} ext={extended} pressed={pressed}"
                );
            }
            let mut flags = KeyboardFlags::empty();
            if !pressed {
                flags |= KeyboardFlags::RELEASE;
            }
            if extended {
                flags |= KeyboardFlags::EXTENDED;
            }
            FastPathInputEvent::KeyboardEvent(flags, scancode)
        }
    }
}

/// 一次 FastPath 包最多攒的事件数（协议上限 255，取保守值频繁 flush）。
const INPUT_BATCH_MAX: usize = 64;

/// RGBA framebuffer。字节序与 warpui `CustomImageFormat::Rgba` 一致（R,G,B,A）。
pub struct RdpFramebuffer {
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    /// 帧代号：每次内容变更 +1。渲染侧据此判断是否有新帧、避免重复上传纹理。
    generation: u64,
}

impl RdpFramebuffer {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            rgba: vec![0; usize::from(width) * usize::from(height) * 4],
            generation: 0,
        }
    }

    /// 当前帧代号（0 = 尚无任何帧）。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 整帧覆盖（src 必须是同尺寸 RGBA）。返回整帧脏矩形。
    pub fn apply_full(&mut self, src: &[u8]) -> DirtyRect {
        let len = self.rgba.len().min(src.len());
        self.rgba[..len].copy_from_slice(&src[..len]);
        self.generation = self.generation.wrapping_add(1);
        DirtyRect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        }
    }

    /// 把 src（整帧 RGBA）中 rect 覆盖的行拷进本 framebuffer 对应位置。
    /// 供后续增量更新用；本步事件循环走 apply_full，但单测覆盖此路径。
    pub fn apply_region(&mut self, src: &[u8], rect: DirtyRect) {
        let stride = usize::from(self.width) * 4;
        let x0 = usize::from(rect.x);
        let x1 = usize::from(rect.x + rect.width).min(usize::from(self.width));
        if x1 <= x0 {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let row_bytes = (x1 - x0) * 4;
        for row in rect.y..rect.y.saturating_add(rect.height) {
            if row >= self.height {
                break;
            }
            let off = usize::from(row) * stride + x0 * 4;
            if off + row_bytes > self.rgba.len() || off + row_bytes > src.len() {
                break;
            }
            self.rgba[off..off + row_bytes].copy_from_slice(&src[off..off + row_bytes]);
        }
    }

    /// EGFX 合成：把已映射 surface（src=src_w×src_h RGBA，映射原点 origin）落进 framebuffer，
    /// 只写 `clip`（output 坐标）范围内、且被 surface 覆盖的行。不 +generation（发布前统一 bump）。
    pub fn compose_surface(
        &mut self,
        origin_x: i32,
        origin_y: i32,
        src: &[u8],
        src_w: u16,
        src_h: u16,
        clip: DirtyRect,
    ) {
        let fb_w = i32::from(self.width);
        let fb_h = i32::from(self.height);
        let sw = i32::from(src_w);
        let sh = i32::from(src_h);
        // clip ∩ surface-output-rect ∩ framebuffer。
        let cx0 = i32::from(clip.x).max(origin_x).max(0);
        let cy0 = i32::from(clip.y).max(origin_y).max(0);
        let cx1 = (i32::from(clip.x) + i32::from(clip.width))
            .min(origin_x + sw)
            .min(fb_w);
        let cy1 = (i32::from(clip.y) + i32::from(clip.height))
            .min(origin_y + sh)
            .min(fb_h);
        if cx1 <= cx0 || cy1 <= cy0 {
            return;
        }
        let fb_stride = usize::from(self.width) * 4;
        let src_stride = usize::from(src_w) * 4;
        let n = (cx1 - cx0) as usize * 4;
        for row in cy0..cy1 {
            let sy = (row - origin_y) as usize;
            let sx = (cx0 - origin_x) as usize;
            let so = sy * src_stride + sx * 4;
            let dofs = row as usize * fb_stride + cx0 as usize * 4;
            if so + n > src.len() || dofs + n > self.rgba.len() {
                continue;
            }
            self.rgba[dofs..dofs + n].copy_from_slice(&src[so..so + n]);
        }
    }

    /// 手动推进帧代号（EGFX 合成一帧后统一调一次）。
    pub fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
}

/// UI 侧持有的会话句柄。drop 或 close() 时优雅断开。
pub struct RdpSessionHandle {
    /// 会话事件流。unbounded 是为协议线程永不阻塞；FrameUpdated 是纯通知（像素在共享
    /// framebuffer，generation 为权威），UI 侧靠 generation 早退吞掉不前进的积压，但每条
    /// 前进事件仍触发一次全帧上传。后续可改容量 1 + 满则丢（见 code-review-2026-07-23 P2-10）。
    pub frame_rx: async_channel::Receiver<RdpEvent>,
    /// 共享 framebuffer，UI 重绘时读快照。
    pub framebuffer: Arc<Mutex<RdpFramebuffer>>,
    /// 输入通道占位（本步不消费）。
    pub input_tx: async_channel::Sender<RdpInputEvent>,
    /// 分辨率重设请求通道（动态分辨率）。UI 防抖后发，会话侧只做去重。
    pub resize_tx: async_channel::Sender<RdpResizeRequest>,
    /// 运行时统计（Arc 与协议线程共享），连接信息面板只读差分。
    pub stats: Arc<RdpStats>,
    close_tx: async_channel::Sender<()>,
    _thread: Option<thread::JoinHandle<()>>,
}

impl RdpSessionHandle {
    /// 显式请求断开。事件循环 select 到后优雅退出。
    pub fn close(&self) {
        let _ = self.close_tx.try_send(());
    }
}

impl Drop for RdpSessionHandle {
    fn drop(&mut self) {
        self.close();
    }
}

static RDP_SESSION_SEQ: AtomicU64 = AtomicU64::new(0);

/// 起线程 + current-thread runtime 跑 RDP 事件循环，返回句柄。
pub fn spawn_rdp_session(config: RdpSessionConfig) -> RdpSessionHandle {
    let id = RDP_SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
    let (event_tx, frame_rx) = async_channel::unbounded::<RdpEvent>();
    let (input_tx, input_rx) = async_channel::unbounded::<RdpInputEvent>();
    let (resize_tx, resize_rx) = async_channel::unbounded::<RdpResizeRequest>();
    let (close_tx, close_rx) = async_channel::unbounded::<()>();
    let framebuffer = Arc::new(Mutex::new(RdpFramebuffer::new(config.width, config.height)));
    let stats = Arc::new(RdpStats::new());

    let thread = thread::Builder::new()
        .name(format!("nexshell-rdp-{id}"))
        .spawn({
            let framebuffer = Arc::clone(&framebuffer);
            let stats = Arc::clone(&stats);
            move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(error) => {
                        let _ = event_tx.try_send(RdpEvent::Disconnected {
                            reason: format!("failed to start RDP runtime: {error}"),
                        });
                        return;
                    }
                };
                runtime.block_on(run_rdp_event_loop(
                    config,
                    framebuffer,
                    stats,
                    event_tx,
                    close_rx,
                    input_rx,
                    resize_rx,
                ));
            }
        })
        .ok();

    RdpSessionHandle {
        frame_rx,
        framebuffer,
        input_tx,
        resize_tx,
        stats,
        close_tx,
        _thread: thread,
    }
}

/// `DOMAIN\user` 拆成 (Some(domain), user)；无反斜杠则本地账户 (None, user)。
pub fn split_domain_user(raw: &str) -> (Option<String>, String) {
    match raw.split_once('\\') {
        Some((domain, user)) if !domain.is_empty() => (Some(domain.to_string()), user.to_string()),
        _ => (None, raw.to_string()),
    }
}

/// CredSSP 的网络客户端占位。仅 Kerberos KDC 代理会调 send；
/// 我们只做密码(NTLM)认证，不触发网络调用，故返回错误即可。
struct NoopNetworkClient;

impl ironrdp_async::NetworkClient for NoopNetworkClient {
    fn send(
        &mut self,
        _request: &NetworkRequest,
    ) -> impl std::future::Future<Output = ConnectorResult<Vec<u8>>> {
        async {
            Err(ironrdp_connector::general_err!(
                "Kerberos network client not available (password/NTLM only)"
            ))
        }
    }
}

/// 组装 connector Config。字段取值对齐 IronRDP 官方 client 默认。
fn build_connector_config(config: &RdpSessionConfig) -> Config {
    let (domain, username) = split_domain_user(&config.username);
    Config {
        credentials: Credentials::UsernamePassword {
            username,
            password: config.password.clone(),
        },
        domain,
        enable_tls: true,
        enable_credssp: true,
        keyboard_type: ironrdp_pdu::gcc::KeyboardType::IBM_ENHANCED,
        keyboard_subtype: 0,
        keyboard_layout: 0,
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        desktop_size: DesktopSize {
            width: config.width,
            height: config.height,
        },
        desktop_scale_factor: config.desktop_scale_factor,
        bitmap: Some(BitmapConfig {
            color_depth: 32,
            lossy_compression: true,
            // 广告 RemoteFX：服务端整帧 RFX 编码而非 GDI 横条带，消除自上而下扫描。
            // 失败回退空集（标准位图更新，ActiveStage 可解）。
            codecs: ironrdp_pdu::rdp::capability_sets::client_codecs_capabilities(&[])
                .unwrap_or(BitmapCodecs(Vec::new())),
        }),
        client_build: 0,
        client_name: "nexshell".to_string(),
        client_dir: String::new(),
        platform: ironrdp_pdu::rdp::capability_sets::MajorPlatformType::UNSPECIFIED,
        hardware_id: None,
        license_cache: None,
        // 开启服务端光标：本地做「远端光标接管」。accelerated 模式（下一行 false）→
        // IronRDP 只发 PointerBitmap 事件、不把光标合成进 framebuffer（避免双光标）。
        enable_server_pointer: true,
        autologon: false,
        // false 会在 Client Info 带 INFO_NOAUDIOPLAYBACK，服务端不建音频端点。
        enable_audio_playback: config.enable_audio,
        request_data: None,
        // false=accelerated：产出非预乘 RGBA 的 PointerBitmap，用系统光标绘制。
        pointer_software_rendering: false,
        multitransport_flags: udp::multitransport_flags(config.udp_effective()),
        compression_type: None,
        performance_flags: ironrdp_pdu::rdp::client_info::PerformanceFlags::default(),
        timezone_info: ironrdp_pdu::rdp::client_info::TimezoneInfo::default(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        // EGFX 早期能力标志（fork patch，docs/adr/0008）：只在门控开时广告，
        // 让服务端可协商 Microsoft::Windows::RDS::Graphics 通道。
        support_dyn_vc_gfx_protocol: config.enable_egfx,
        // 自动探测（mstsc 默认）：服务端实测 RTT/带宽后调整画质与帧率，弱网不再按局域网画质硬推。
        connection_type: connection_type_from_env(std::env::var_os("NEXSHELL_RDP_AUTODETECT")),
        // 其余取与旧行为等价的默认：不开标准 RDP 安全、无音频采集、无 RAIL。
        enable_standard_rdp_security: false,
        enable_audio_capture: false,
        monitor_layout: None,
        remote_application_mode: false,
        rail_support_level: ironrdp_pdu::rdp::capability_sets::RailSupportLevel::empty(),
    }
}

/// rdpsnd 伴随规则：audio 或 drive 任一开都必须挂 rdpsnd。
/// MS-RDPEFS（IronRDP rdpdr/src/lib.rs:29）——rdpdr 须与 rdpsnd 同时 advertise，
/// 否则服务端不回 rdpdr 响应，盘符静默失效。
fn needs_rdpsnd(enable_audio: bool, enable_drive: bool) -> bool {
    enable_audio || enable_drive
}

fn attach_audio_static_channels(
    connector: ClientConnector,
    rdpsnd: ironrdp_rdpsnd::client::Rdpsnd,
) -> ClientConnector {
    // rdpdr（rdpsnd 依赖）已拆到独立门控 rdpdr::build_channel，此处只挂 rdpsnd。
    if audio_diag::enabled() {
        eprintln!("[rdp-audio] registering legacy rdpsnd static channel");
    }

    connector.with_static_channel(rdpsnd)
}

/// 事件循环：连接 → NLA → 激活 → 收帧。任何阶段出错都发 Disconnected 收尾。
async fn run_rdp_event_loop(
    config: RdpSessionConfig,
    framebuffer: Arc<Mutex<RdpFramebuffer>>,
    stats: Arc<RdpStats>,
    event_tx: async_channel::Sender<RdpEvent>,
    close_rx: async_channel::Receiver<()>,
    input_rx: async_channel::Receiver<RdpInputEvent>,
    resize_rx: async_channel::Receiver<RdpResizeRequest>,
) {
    let reason = match connect_and_run(
        &config,
        &framebuffer,
        &stats,
        &event_tx,
        &close_rx,
        &input_rx,
        &resize_rx,
    )
    .await
    {
        Ok(()) => "session ended".to_string(),
        Err(error) => error,
    };
    // 断开原因同时打到 stderr，便于命令行复现时与 IronRDP tracing 日志对齐时间线。
    eprintln!("[rdp] disconnected: {reason}");
    let _ = event_tx.try_send(RdpEvent::Disconnected { reason });
}

async fn connect_and_run(
    config: &RdpSessionConfig,
    framebuffer: &Arc<Mutex<RdpFramebuffer>>,
    stats: &Arc<RdpStats>,
    event_tx: &async_channel::Sender<RdpEvent>,
    close_rx: &async_channel::Receiver<()>,
    input_rx: &async_channel::Receiver<RdpInputEvent>,
    resize_rx: &async_channel::Receiver<RdpResizeRequest>,
) -> Result<(), String> {
    // rustls 0.23 需显式选 provider（树内 ring/aws-lc-rs 共存），已装则忽略。
    let _ = rustls::crypto::ring::default_provider().install_default();

    let addr = format!("{}:{}", config.host, config.port);
    let tcp = TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("TCP connect failed: {e}"))?;
    // 关 Nagle：rdpdr 是请求-响应式小包，Nagle+延迟ACK 把每个响应压 ~40ms，
    // 驱动器传文件吞吐锁死在百 KB/s。mstsc/FreeRDP/IronRDP 官方 client 同样开。
    tcp.set_nodelay(true)
        .map_err(|e| format!("set TCP_NODELAY failed: {e}"))?;
    let client_addr: SocketAddr = tcp
        .local_addr()
        .map_err(|e| format!("local_addr failed: {e}"))?;
    let udp_peer = if config.udp_effective() {
        Some(
            tcp.peer_addr()
                .map_err(|e| format!("peer_addr failed: {e}"))?,
        )
    } else {
        None
    };
    // TLS 升级前 dup 一份底层 fd 供 RTT 探测（原 stream 随后被 TLS 吃掉）。
    #[cfg(target_os = "macos")]
    stats.capture_fd(tcp.as_raw_fd());

    // cliprdr：注册文本剪贴板静态通道。backend 经 clip_tx 回递要发的 PDU，
    // 轮询/回调经 shared 桥接（见 clipboard 模块）。
    let clip_shared = clipboard::ClipboardShared::new();
    let (clip_tx, clip_rx) = async_channel::unbounded::<ClipboardMessage>();
    let clip_backend = clipboard::TextCliprdrBackend::new(clip_tx, &clip_shared);

    let connector_config = build_connector_config(config);
    if audio_diag::enabled() {
        eprintln!(
            "[rdp-audio] config enable_audio={} enable_egfx={} desktop={}x{}",
            config.enable_audio, config.enable_egfx, config.width, config.height
        );
    }
    let mut connector = ClientConnector::new(connector_config, client_addr)
        .with_static_channel(CliprdrClient::new(Box::new(clip_backend)));
    // EGFX：门控开时挂 drdynvc 静态通道 + EGFX 合成动态通道（docs/adr/0008 第②步，出画面）。
    // handler 直接往共享 framebuffer 写并发 FrameUpdated；ActiveStage 自动路由并回发 FrameAck。
    if config.enable_egfx {
        connector = connector.with_static_channel(egfx::build_dvc_client(
            Arc::clone(framebuffer),
            event_tx.clone(),
            Arc::clone(stats),
            config.width,
            config.height,
        ));
    }

    // RDPSND：MS-RDPEFS 要求 rdpdr 必须与 rdpsnd 一起 advertise，否则服务端不回 rdpdr。
    // 故 audio 或 drive 任一开就挂：audio 用真实 cpal 播放后端，仅 drive 用 Noop 静默伴随。
    if needs_rdpsnd(config.enable_audio, config.enable_drive) {
        let rdpsnd = if config.enable_audio {
            ironrdp_rdpsnd::client::Rdpsnd::new(Box::new(
                ironrdp_rdpsnd_native::cpal::RdpsndBackend::new(),
            ))
        } else {
            ironrdp_rdpsnd::client::Rdpsnd::new(Box::new(ironrdp_rdpsnd::client::NoopRdpsndBackend))
        };
        connector = attach_audio_static_channels(connector, rdpsnd);
    } else if audio_diag::enabled() {
        eprintln!("[rdp-audio] rdpsnd static channel disabled (no audio, no drive)");
    }

    // RDPDR：驱动器重定向（文件互拷）独立门控；audio 开时也需它满足 rdpsnd 依赖。
    // 只注册一次，与 audio 解耦。
    if let Some(rdpdr_channel) = rdpdr::build_channel(config.enable_drive, config.enable_audio) {
        connector = connector.with_static_channel(rdpdr_channel);
    }

    audio_diag::log_advertised_static_channels(&connector.static_channels);

    // 阶段一：明文协商到 TLS 升级点。
    let mut framed = ironrdp_tokio::TokioFramed::new(tcp);
    let should_upgrade = ironrdp_tokio::connect_begin(&mut framed, &mut connector)
        .await
        .map_err(|e| format!("connect_begin failed: {e}"))?;

    // 阶段二：TLS 升级 + 取服务端公钥（CredSSP 绑定用）。
    // 证书优先走平台根校验；自签（RDP 常态）落到回调里按 host:port 做指纹 TOFU。
    let initial_stream = framed.into_inner_no_leftover();
    let endpoint = rdp_cert_store::endpoint_key(&config.host, config.port);
    let trust_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let callback: ironrdp_tls::CertificateValidationCallback = {
        let trust_error = Arc::clone(&trust_error);
        Arc::new(move |cert_der: &[u8], endpoint: &str, error: &str| {
            match rdp_cert_store::verify_or_pin(endpoint, cert_der) {
                Ok(rdp_cert_store::CertTrustVerdict::Trusted)
                | Ok(rdp_cert_store::CertTrustVerdict::Pinned) => true,
                Ok(rdp_cert_store::CertTrustVerdict::Mismatch { expected }) => {
                    let actual = rdp_cert_store::sha256_fingerprint(cert_der);
                    *trust_error.lock() = Some(format!(
                        "RDP server certificate for {endpoint} changed (expected {expected}, got {actual}); \
                         connection aborted. Edit and save this host to trust the new certificate."
                    ));
                    false
                }
                Err(store_error) => {
                    *trust_error.lock() = Some(format!(
                        "RDP certificate check failed for {endpoint}: {store_error} (TLS error: {error})"
                    ));
                    false
                }
            }
        })
    };
    // UDP 旁路的 TLS 与 TCP 同策略：平台根校验 + 同一 endpoint 的 TOFU 指纹。
    let mut udp = udp_peer.map(|peer| {
        udp::UdpSideband::new(
            peer,
            config.host.clone(),
            ironrdp_rdpeudp_tokio::UdpTlsConfig {
                certificate_validation: ironrdp_tls::CertificateValidation::Strict,
                certificate_validation_callback: Some(Arc::clone(&callback)),
                certificate_validation_endpoint: endpoint.clone(),
            },
            Arc::clone(stats),
        )
    });
    let (upgraded_stream, server_cert) =
        ironrdp_tls::upgrade_with_certificate_validation_callback_for_endpoint(
            initial_stream,
            &config.host,
            &endpoint,
            callback,
        )
        .await
        .map_err(|e| {
            trust_error
                .lock()
                .take()
                .unwrap_or_else(|| format!("TLS upgrade failed: {e}"))
        })?;
    let server_public_key = ironrdp_tls::extract_tls_server_public_key(&server_cert)
        .ok_or_else(|| "extract server public key failed".to_string())?
        .to_owned();

    // 阶段三：CredSSP(NLA) + 能力协商，收尾得到 ConnectionResult。
    let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
    let mut upgraded_framed = ironrdp_tokio::TokioFramed::new(upgraded_stream);
    let mut network_client = NoopNetworkClient;
    let server_name = ServerName::new(config.host.clone());
    let connection_result = match udp.as_mut() {
        // 连接期的多传输请求在此内联建隧道（握手超时已压短，见 udp 模块）。
        Some(sideband) => {
            ironrdp_tokio::connect_finalize_with_multitransport(
                upgraded,
                connector,
                &mut upgraded_framed,
                &mut network_client,
                server_name,
                server_public_key,
                None,
                async |request, soft_sync| Ok(sideband.handle_request(&request, soft_sync).await.0),
            )
            .await
        }
        None => {
            ironrdp_tokio::connect_finalize(
                upgraded,
                connector,
                &mut upgraded_framed,
                &mut network_client,
                server_name,
                server_public_key,
                None,
            )
            .await
        }
    }
    .map_err(|e| format!("connect_finalize (NLA) failed: {e}"))?;
    audio_diag::log_negotiated_rdpsnd_channel(&connection_result.static_channels);
    let soft_sync = connection_result.multitransport_soft_sync();

    // 激活分辨率可能被服务端改；据此重建 framebuffer + DecodedImage。
    let desktop = connection_result.desktop_size;
    {
        let mut fb = framebuffer.lock();
        if fb.width != desktop.width || fb.height != desktop.height {
            *fb = RdpFramebuffer::new(desktop.width, desktop.height);
        }
    }
    let mut image = DecodedImage::new(PixelFormat::RgbA32, desktop.width, desktop.height);
    // 重激活序列工厂（上游 session 已与 connector 解耦，DeactivateAll 后由应用自建序列）。
    let activation_factory = connection_result.activation_factory;
    let mut active_stage = ActiveStageBuilder {
        static_channels: connection_result.static_channels,
        user_channel_id: connection_result.user_channel_id,
        io_channel_id: connection_result.io_channel_id,
        message_channel_id: connection_result.message_channel_id,
        share_id: connection_result.share_id,
        compression_type: connection_result.compression_type,
        enable_server_pointer: connection_result.enable_server_pointer,
        pointer_software_rendering: connection_result.pointer_software_rendering,
    }
    .build();
    active_stage.set_window_support_level(connection_result.window_support_level);
    // 隧道已建：等服务端 Soft-Sync 把动态通道迁过来。
    if udp.as_ref().is_some_and(udp::UdpSideband::has_transport) {
        active_stage
            .enable_reliable_udp_dvc_tunnel()
            .map_err(|e| format!("enable UDP tunnel failed: {e}"))?;
    }
    let _ = event_tx.try_send(RdpEvent::Connected);
    // Mac 剪贴板轮询挪出帧循环：独立 OS 线程 1s tick 同步读 NSPasteboard，有变化经 channel 回递。
    // 事件循环收到才编码发送，期间不再因读剪贴板卡住收帧。receiver 随本函数返回 drop → 线程 ~1s 内自退。
    let (clip_poll_tx, clip_poll_rx) =
        async_channel::unbounded::<Vec<ironrdp_cliprdr::pdu::ClipboardFormat>>();
    {
        let clip_shared = clip_shared.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(1));
            if clip_poll_tx.is_closed() {
                break;
            }
            if let Some(formats) = clipboard::poll_local_change(&clip_shared) {
                if clip_poll_tx.send_blocking(formats).is_err() {
                    break;
                }
            }
        });
    }

    // 阶段四：收图形更新解码合成 + 并发消费键鼠输入，编码成 FastPath 发回。
    // dw/dh 为可变权威桌面尺寸（Deactivation-Reactivation 后更新）。
    let (mut dw, mut dh) = (desktop.width, desktop.height);
    // 分辨率去重：记上次已请求的目标尺寸，相同不重发（防抖在 UI 侧）。初值=当前分辨率。
    let mut last_requested_size = (dw, dh);
    // 帧聚合状态提升到循环外持久化：acc 跨迭代累积脏区，真帧边界/兜底才发布。
    let mut acc: Option<DirtyRect> = None;
    // 远端光标去重：记上次发出的 PointerBitmap cache_key，连续同指针不重发。
    let mut last_pointer_key: Option<u64> = None;
    // 连接内一旦 peek 到任何 FrameMarker 即永久走 marker 模式（按真帧边界发布）。
    let mut marker_support = false;
    // acc 从空转非空时设 now+50ms；服务端只发 Begin 不发 End 的异常由它兜底发布。
    let mut frame_deadline: Option<tokio::time::Instant> = None;
    // 每会话一次的管线诊断日志（surface-command 含 marker / 位图回退）。
    let mut pipeline_logged = false;
    // NEXSHELL_RDP_EGFX_DUMP 开时，2s 打一次累计收字节/发帧，核实接收码率统计（面板 0.0 Mbps 排查）。
    let egfx_dbg = std::env::var_os("NEXSHELL_RDP_EGFX_DUMP").is_some();
    let mut dbg_last = std::time::Instant::now();
    loop {
        if egfx_dbg && dbg_last.elapsed() >= Duration::from_secs(2) {
            eprintln!(
                "[rdp] recv_bytes={} frames={}",
                stats.bytes(),
                stats.frames()
            );
            dbg_last = std::time::Instant::now();
        }
        udp::drain_pending(&mut udp, &mut active_stage, &mut upgraded_framed).await?;
        // 有在途累积且未武装截止时武装：单一武装点覆盖帧/输入两路（输入路 continue 后由此处补武装）。
        if acc.is_some() && frame_deadline.is_none() {
            frame_deadline = Some(tokio::time::Instant::now() + Duration::from_millis(50));
        }
        // select：关闭 / cliprdr 回递 / 剪贴板轮询 / 输入 / 截止兜底 / 帧。前几路自成闭环 continue。
        let (action, payload) = tokio::select! {
            _ = close_rx.recv() => return Ok(()),
            msg = clip_rx.recv() => {
                let Ok(msg) = msg else { continue; };
                send_clipboard_pdu(&mut active_stage, &mut upgraded_framed, msg).await?;
                continue;
            }
            formats = clip_poll_rx.recv() => {
                let Ok(formats) = formats else { continue; };
                if let Some(cliprdr) = active_stage.get_svc_processor_mut::<CliprdrClient>() {
                    let msgs = cliprdr
                        .initiate_copy(&formats)
                        .map_err(|e| format!("cliprdr initiate_copy failed: {e}"))?;
                    let data = active_stage
                        .process_svc_processor_messages(msgs)
                        .map_err(|e| format!("cliprdr encode failed: {e}"))?;
                    upgraded_framed
                        .write_all(&data)
                        .await
                        .map_err(|e| format!("write cliprdr failed: {e}"))?;
                }
                continue;
            }
            input = input_rx.recv() => {
                let Ok(first) = input else { return Ok(()); };
                let mut events = Vec::with_capacity(8);
                events.push(to_fastpath_input(first));
                while events.len() < INPUT_BATCH_MAX {
                    match input_rx.try_recv() {
                        Ok(event) => events.push(to_fastpath_input(event)),
                        Err(_) => break,
                    }
                }
                let outputs = active_stage
                    .process_fastpath_input(&mut image, &events)
                    .map_err(|e| format!("encode input failed: {e}"))?;
                // 输入产出的脏区并入 acc，不在此发布——marker/截止兜底会兜住。
                let mut signals = OutputSignals::default();
                if drain_outputs(&mut upgraded_framed, outputs, &mut acc, dw, dh, event_tx, &mut last_pointer_key, &mut signals).await? {
                    return Ok(());
                }
                continue;
            }
            // 分辨率重设：UI 防抖后发；与上次请求相同则去重。经 Display Control 发 MonitorLayout；
            // 通道未就绪（legacy/未协商）时 encode_resize 返回 None，静默忽略。
            req = resize_rx.recv() => {
                let Ok(req) = req else { continue; };
                if (req.width, req.height) == last_requested_size {
                    continue;
                }
                let (aw, ah) =
                    MonitorLayoutEntry::adjust_display_size(u32::from(req.width), u32::from(req.height));
                let scale = (req.scale_factor > 0).then_some(req.scale_factor);
                // Soft-Sync 后 Display Control 在 UDP 上，须按通道分流，不能固定走 TCP。
                match active_stage.prepare_resize(aw, ah, scale, None) {
                    Some(Ok(batch)) => {
                        udp::route_dvc_batch(udp.as_ref(), &mut active_stage, &mut upgraded_framed, batch)
                            .await?;
                        last_requested_size = (req.width, req.height);
                    }
                    Some(Err(e)) => eprintln!("[rdp] encode_resize failed: {e}"),
                    None => {} // Display Control 通道未就绪，忽略。
                }
                continue;
            }
            // 截止兜底：仅在 frame_deadline 已武装时启用（if guard 保证 unwrap 安全）。
            _ = tokio::time::sleep_until(frame_deadline.unwrap_or_else(tokio::time::Instant::now)),
                if frame_deadline.is_some() =>
            {
                publish_frame(framebuffer, &image, &mut acc, stats, event_tx);
                frame_deadline = None;
                continue;
            }
            payload = udp::recv(&mut udp) => {
                udp::handle_recv(&mut udp, &mut active_stage, &mut upgraded_framed, payload).await?;
                continue;
            }
            frame = upgraded_framed.read_pdu() => {
                frame.map_err(|e| format!("read frame failed: {e}"))?
            }
        };

        stats.add_bytes(payload.len() as u64);

        // FastPath 才含 surface command / marker（x224 慢速路径不含），先只读 peek 真帧边界。
        let mut saw_end = false;
        if action == ironrdp_pdu::Action::FastPath {
            let peek = frame_marker::peek_frame_markers(&payload);
            if peek.saw_marker {
                marker_support = true;
                stats.set_marker_mode();
            }
            saw_end = peek.saw_end;
            if !pipeline_logged {
                if peek.saw_marker {
                    eprintln!("[rdp] frame pipeline: surface-commands+frame-marker");
                    pipeline_logged = true;
                } else if peek.saw_bitmap {
                    eprintln!("[rdp] frame pipeline: legacy bitmap updates");
                    pipeline_logged = true;
                }
            }
        }

        // 本轮捕获的重激活 / 多传输请求（见 drain_outputs）。
        let mut signals = OutputSignals::default();
        let outputs = active_stage
            .process(&mut image, action, &payload)
            .map_err(|e| format!("process frame failed: {e}"))?;
        if drain_outputs(
            &mut upgraded_framed,
            outputs,
            &mut acc,
            dw,
            dh,
            event_tx,
            &mut last_pointer_key,
            &mut signals,
        )
        .await?
        {
            return Ok(());
        }
        udp::answer_requests(
            &mut udp,
            &mut active_stage,
            &mut upgraded_framed,
            std::mem::take(&mut signals.multitransport),
            soft_sync,
        )
        .await?;
        if std::mem::take(&mut signals.reactivate) {
            let seq = activation_factory.create();
            let (nw, nh) =
                run_reactivation(&mut upgraded_framed, &mut active_stage, seq, &mut image).await?;
            reset_after_resize(framebuffer, nw, nh, event_tx);
            dw = nw;
            dh = nh;
            last_requested_size = (nw, nh);
            acc = None;
            frame_deadline = None;
            continue;
        }

        if marker_support {
            // marker 模式：真帧边界发布，见本 PDU 的 FrameMarker(End) 即发。不跑 drain 探测。
            if saw_end {
                publish_frame(framebuffer, &image, &mut acc, stats, event_tx);
                frame_deadline = None;
            }
        } else {
            // 非 marker 模式：drain 掉 socket 里已就绪的后续 PDU，读空且有累积时给 ≤2 次 1.5ms 宽限
            // 再探（减少大帧在途字节被腰斩），仍无数据才一次性发布。
            let mut drained = 0;
            let mut grace = 0;
            while drained < DRAIN_MAX {
                let more = tokio::select! {
                    biased;
                    res = upgraded_framed.read_pdu() => Some(res),
                    _ = std::future::ready(()) => None,
                };
                match more {
                    Some(res) => {
                        let (action, payload) =
                            res.map_err(|e| format!("read frame failed: {e}"))?;
                        stats.add_bytes(payload.len() as u64);
                        let outputs = active_stage
                            .process(&mut image, action, &payload)
                            .map_err(|e| format!("process frame failed: {e}"))?;
                        if drain_outputs(
                            &mut upgraded_framed,
                            outputs,
                            &mut acc,
                            dw,
                            dh,
                            event_tx,
                            &mut last_pointer_key,
                            &mut signals,
                        )
                        .await?
                        {
                            return Ok(());
                        }
                        udp::answer_requests(
                            &mut udp,
                            &mut active_stage,
                            &mut upgraded_framed,
                            std::mem::take(&mut signals.multitransport),
                            soft_sync,
                        )
                        .await?;
                        if signals.reactivate {
                            break; // 收到 DeactivateAll：跳出 drain，下方走重激活。
                        }
                        drained += 1;
                    }
                    None => {
                        if acc.is_some() && grace < 2 {
                            grace += 1;
                            tokio::time::sleep(Duration::from_micros(1500)).await;
                            continue;
                        }
                        break;
                    }
                }
            }
            if std::mem::take(&mut signals.reactivate) {
                let seq = activation_factory.create();
                let (nw, nh) =
                    run_reactivation(&mut upgraded_framed, &mut active_stage, seq, &mut image)
                        .await?;
                reset_after_resize(framebuffer, nw, nh, event_tx);
                dw = nw;
                dh = nh;
                last_requested_size = (nw, nh);
                acc = None;
                frame_deadline = None;
                continue;
            }
            publish_frame(framebuffer, &image, &mut acc, stats, event_tx);
            frame_deadline = None;
        }
    }
}

/// Deactivation-Reactivation 序列（照 ironrdp-client）：动态分辨率的兜底路径，
/// 服务端不走 EGFX ResetGraphics 而是重激活时用。逐步读写握手 PDU，Finalized 后
/// 按新桌面尺寸重建 DecodedImage 并复位 fastpath processor，返回新 (宽, 高)。
async fn run_reactivation<S>(
    framed: &mut ironrdp_tokio::TokioFramed<S>,
    active_stage: &mut ActiveStage,
    mut sequence: ConnectionActivationSequence,
    image: &mut DecodedImage,
) -> Result<(u16, u16), String>
where
    S: Send + Sync + Unpin + tokio::io::AsyncRead + tokio::io::AsyncWrite,
{
    let mut buf = WriteBuf::new();
    loop {
        let written = ironrdp_tokio::single_sequence_step_read(framed, &mut sequence, &mut buf)
            .await
            .map_err(|e| format!("reactivation step read failed: {e}"))?;
        if written.size().is_some() {
            framed
                .write_all(buf.filled())
                .await
                .map_err(|e| format!("reactivation step write failed: {e}"))?;
        }
        if let ConnectionActivationState::Finalized {
            desktop_size,
            share_id,
            enable_server_pointer,
            pointer_software_rendering,
            static_channel_chunk_size,
            window_support_level,
            ..
        } = sequence.connection_activation_state()
        {
            *image =
                DecodedImage::new(PixelFormat::RgbA32, desktop_size.width, desktop_size.height);
            // 照 ironrdp-client：一次性复位 fastpath processor / share_id / 指针 / 静态通道分块。
            if !active_stage.reactivate(
                sequence.io_channel_id(),
                sequence.user_channel_id(),
                share_id,
                enable_server_pointer,
                pointer_software_rendering,
                static_channel_chunk_size,
            ) {
                return Err("reactivation: invalid static channel chunk size".to_string());
            }
            active_stage.set_window_support_level(window_support_level);
            return Ok((desktop_size.width, desktop_size.height));
        }
    }
}

/// 分辨率变更后按新尺寸重建共享 framebuffer 并通知 UI（重激活路径用；EGFX 路径在 handler 内做）。
fn reset_after_resize(
    framebuffer: &Arc<Mutex<RdpFramebuffer>>,
    width: u16,
    height: u16,
    event_tx: &async_channel::Sender<RdpEvent>,
) {
    {
        let mut fb = framebuffer.lock();
        if fb.width != width || fb.height != height {
            *fb = RdpFramebuffer::new(width, height);
        }
    }
    let _ = event_tx.try_send(RdpEvent::Resized { width, height });
}

/// 把 backend 回递的 cliprdr 消息编码成 SVC 帧写回服务端。
/// initiate_copy/paste 需 &mut、submit_format_data 需 &self，get_svc_processor_mut 皆可。
async fn send_clipboard_pdu<W: FramedWrite>(
    active_stage: &mut ActiveStage,
    framed: &mut W,
    msg: ClipboardMessage,
) -> Result<(), String> {
    let Some(cliprdr) = active_stage.get_svc_processor_mut::<CliprdrClient>() else {
        return Ok(());
    };
    let messages = match msg {
        ClipboardMessage::SendInitiateCopy(formats) => cliprdr.initiate_copy(&formats),
        ClipboardMessage::SendInitiatePaste(format_id) => cliprdr.initiate_paste(format_id),
        ClipboardMessage::SendFormatData(response) => cliprdr.submit_format_data(response),
        // 文件/错误等 v1 不产出。
        _ => return Ok(()),
    }
    .map_err(|e| format!("cliprdr encode failed: {e}"))?;
    let data = active_stage
        .process_svc_processor_messages(messages)
        .map_err(|e| format!("cliprdr svc encode failed: {e}"))?;
    framed
        .write_all(&data)
        .await
        .map_err(|e| format!("write cliprdr failed: {e}"))
}

/// drain 时单帧最多累积的 PDU 数：防服务端持续推流饿死 input/close 分支。
const DRAIN_MAX: usize = 32;

/// InclusiveRectangle（右/下含端）→ DirtyRect，clamp 到桌面尺寸。
fn inclusive_to_dirty(rect: &InclusiveRectangle, max_w: u16, max_h: u16) -> DirtyRect {
    if max_w == 0 || max_h == 0 {
        return DirtyRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }
    let left = rect.left.min(max_w - 1);
    let top = rect.top.min(max_h - 1);
    let right = rect.right.min(max_w - 1).max(left);
    let bottom = rect.bottom.min(max_h - 1).max(top);
    DirtyRect {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    }
}

/// 两脏矩形的包围盒（累积一逻辑帧内多个 GraphicsUpdate）。
fn union_dirty(a: DirtyRect, b: DirtyRect) -> DirtyRect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.width).max(b.x + b.width);
    let y1 = (a.y + a.height).max(b.y + b.height);
    DirtyRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

/// 处理一批 outputs：ResponseFrame 立即写回；GraphicsUpdate 转脏区并入 acc（不拷贝像素）；
/// Terminate 发 Disconnected 并返回 true（需退出）。
/// drain_outputs 交回主循环处理的事件（需要 ActiveStage / UDP 状态，drain 内拿不到）。
#[derive(Default)]
struct OutputSignals {
    /// 服务端要求 Deactivation-Reactivation。
    reactivate: bool,
    /// 会话期到达的 Initiate Multitransport Request。
    multitransport: Vec<ironrdp_pdu::rdp::multitransport::MultitransportRequestPdu>,
}

async fn drain_outputs<W: FramedWrite>(
    framed: &mut W,
    outputs: Vec<ActiveStageOutput>,
    acc: &mut Option<DirtyRect>,
    desktop_w: u16,
    desktop_h: u16,
    event_tx: &async_channel::Sender<RdpEvent>,
    last_pointer_key: &mut Option<u64>,
    signals: &mut OutputSignals,
) -> Result<bool, String> {
    for out in outputs {
        match out {
            ActiveStageOutput::ResponseFrame(frame) => {
                framed
                    .write_all(&frame)
                    .await
                    .map_err(|e| format!("write response failed: {e}"))?;
            }
            ActiveStageOutput::GraphicsUpdate(region) => {
                let dirty = inclusive_to_dirty(&region, desktop_w, desktop_h);
                *acc = Some(match *acc {
                    Some(prev) => union_dirty(prev, dirty),
                    None => dirty,
                });
            }
            ActiveStageOutput::PointerDefault => {
                *last_pointer_key = None;
                if ptr_trace() {
                    eprintln!("[rdp-ptr] default");
                }
                let _ = event_tx.try_send(RdpEvent::PointerChanged(RdpPointer::Default));
            }
            ActiveStageOutput::PointerHidden => {
                *last_pointer_key = None;
                if ptr_trace() {
                    eprintln!("[rdp-ptr] hidden");
                }
                let _ = event_tx.try_send(RdpEvent::PointerChanged(RdpPointer::Hidden));
            }
            ActiveStageOutput::PointerBitmap(pointer) => {
                if let Some(p) = pointer_to_event(&pointer, last_pointer_key) {
                    if ptr_trace() {
                        eprintln!(
                            "[rdp-ptr] bitmap {}x{} hs=({},{})",
                            pointer.width, pointer.height, pointer.hotspot_x, pointer.hotspot_y
                        );
                    }
                    // 发送失败（通道满）回滚去重键，下次同指针可重试，否则光标永久丢失。
                    if event_tx.try_send(RdpEvent::PointerChanged(p)).is_err() {
                        *last_pointer_key = None;
                    }
                }
            }
            ActiveStageOutput::Terminate(reason) => {
                eprintln!("[rdp] server terminated session: {reason}");
                let _ = event_tx.try_send(RdpEvent::Disconnected {
                    reason: format!("terminated: {reason}"),
                });
                return Ok(true);
            }
            // 动态分辨率兜底：服务端以重激活换分辨率时回传序列，交主循环 run_reactivation 走完。
            ActiveStageOutput::DeactivateAll => {
                signals.reactivate = true;
            }
            ActiveStageOutput::MultitransportRequest(request) => {
                signals.multitransport.push(request);
            }
            // 服务端实测的网络特征（连接类型自动探测的结果），诊断用。
            ActiveStageOutput::AutoDetect(request) => {
                if net_trace() {
                    eprintln!("[rdp-net] {request:?}");
                }
            }
            // PointerPosition（本地光标本就跟手）等忽略。
            _ => {}
        }
    }
    Ok(false)
}

/// 网络探测结果追踪开关（NEXSHELL_RDP_NET_TRACE=1）。
fn net_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NEXSHELL_RDP_NET_TRACE").is_ok_and(|v| v == "1"))
}

/// 指针链路追踪开关（NEXSHELL_RDP_PTR_TRACE=1）。
fn ptr_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NEXSHELL_RDP_PTR_TRACE").is_ok_and(|v| v == "1"))
}

/// 按键链路追踪开关（NEXSHELL_DEBUG_KEYS=1，与 warpui 平台层同开关）。
fn key_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NEXSHELL_DEBUG_KEYS").is_ok_and(|v| v == "1"))
}

/// DecodedPointer(Arc) → RdpPointer::Bitmap；与上次同一指针（同 cache_key）则返回 None 去重。
/// cache_key 取内容 hash 而非 Arc 地址：地址在 Arc 释放后可被新指针复用，
/// 会让 UI 光标缓存/去重误命中旧指针。位图仅数 KB 且只在指针变化时触发，开销可忽略。
fn pointer_to_event(
    pointer: &std::sync::Arc<ironrdp_graphics::pointer::DecodedPointer>,
    last_pointer_key: &mut Option<u64>,
) -> Option<RdpPointer> {
    let cache_key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (
            pointer.width,
            pointer.height,
            pointer.hotspot_x,
            pointer.hotspot_y,
        )
            .hash(&mut hasher);
        pointer.bitmap_data.hash(&mut hasher);
        hasher.finish()
    };
    if *last_pointer_key == Some(cache_key) {
        return None;
    }
    *last_pointer_key = Some(cache_key);
    Some(RdpPointer::Bitmap {
        rgba: pointer.bitmap_data.clone(),
        width: pointer.width as u32,
        height: pointer.height as u32,
        hotspot_x: pointer.hotspot_x as f32,
        hotspot_y: pointer.hotspot_y as f32,
        cache_key,
    })
}

/// 发布累积脏区：apply_region 拷一次 + 发一条 FrameUpdated。acc 为 None 则空操作。
fn publish_frame(
    framebuffer: &Arc<Mutex<RdpFramebuffer>>,
    image: &DecodedImage,
    acc: &mut Option<DirtyRect>,
    stats: &Arc<RdpStats>,
    event_tx: &async_channel::Sender<RdpEvent>,
) {
    if let Some(dirty) = acc.take() {
        if dirty.width == 0 || dirty.height == 0 {
            return;
        }
        framebuffer.lock().apply_region(image.data(), dirty);
        stats.inc_frame();
        let _ = event_tx.try_send(RdpEvent::FrameUpdated { dirty });
    }
}

#[cfg(test)]
mod tests;
