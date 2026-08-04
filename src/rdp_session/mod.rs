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

pub use egfx::{
    inspect_wire_dump_pdus, inspect_wire_dump_pdus_with_points, replay_wire_dump, vt_replay_dir,
    ChecksumRect, WatchEvent, WatchPoint, WirePduInfo, WirePduRecord, WirePipelineError,
    WireReplayFrame, WireReplayOptions, WireReplaySummary,
};
pub use stats::{format_duration_hms, fps, mbps, RdpStats};

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
use ironrdp_session::{fast_path, ActiveStage, ActiveStageOutput};
use parking_lot::Mutex;
use tokio::net::TcpStream;

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
}

fn default_enable_egfx_from_env(disable_egfx: Option<std::ffi::OsString>) -> bool {
    disable_egfx.is_none()
}

/// EGFX is the default graphics pipeline; set NEXSHELL_RDP_DISABLE_EGFX=1 for legacy fallback.
pub fn default_enable_egfx() -> bool {
    default_enable_egfx_from_env(std::env::var_os("NEXSHELL_RDP_DISABLE_EGFX"))
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
    /// 会话事件流（unbounded：UI 慢消费也不阻塞协议线程）。
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
        keyboard_type: ironrdp_pdu::gcc::KeyboardType::IbmEnhanced,
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
        multitransport_flags: None,
        compression_type: None,
        performance_flags: ironrdp_pdu::rdp::client_info::PerformanceFlags::default(),
        timezone_info: ironrdp_pdu::rdp::client_info::TimezoneInfo::default(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        // EGFX 早期能力标志（fork patch，docs/adr/0008）：只在门控开时广告，
        // 让服务端可协商 Microsoft::Windows::RDS::Graphics 通道。
        support_dyn_vc_gfx_protocol: config.enable_egfx,
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

    // 证书无条件接受（与 ssh_session check_server_key 恒 Ok 同姿态）：
    // ironrdp-tls 的 upgrade 不做链校验，天然接受任意服务端证书。
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
    let initial_stream = framed.into_inner_no_leftover();
    let (upgraded_stream, server_cert) = ironrdp_tls::upgrade(initial_stream, &config.host)
        .await
        .map_err(|e| format!("TLS upgrade failed: {e}"))?;
    let server_public_key = ironrdp_tls::extract_tls_server_public_key(&server_cert)
        .ok_or_else(|| "extract server public key failed".to_string())?
        .to_owned();

    // 阶段三：CredSSP(NLA) + 能力协商，收尾得到 ConnectionResult。
    let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
    let mut upgraded_framed = ironrdp_tokio::TokioFramed::new(upgraded_stream);
    let mut network_client = NoopNetworkClient;
    let connection_result = ironrdp_tokio::connect_finalize(
        upgraded,
        connector,
        &mut upgraded_framed,
        &mut network_client,
        ServerName::new(config.host.clone()),
        server_public_key,
        None,
    )
    .await
    .map_err(|e| format!("connect_finalize (NLA) failed: {e}"))?;
    audio_diag::log_negotiated_rdpsnd_channel(&connection_result.static_channels);

    // 激活分辨率可能被服务端改；据此重建 framebuffer + DecodedImage。
    let desktop = connection_result.desktop_size;
    {
        let mut fb = framebuffer.lock();
        if fb.width != desktop.width || fb.height != desktop.height {
            *fb = RdpFramebuffer::new(desktop.width, desktop.height);
        }
    }
    let mut image = DecodedImage::new(PixelFormat::RgbA32, desktop.width, desktop.height);
    let mut active_stage = ActiveStage::new(connection_result);
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
                let mut reactivate = None;
                if drain_outputs(&mut upgraded_framed, outputs, &mut acc, dw, dh, event_tx, &mut last_pointer_key, &mut reactivate).await? {
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
                match active_stage.encode_resize(aw, ah, scale, None) {
                    Some(Ok(frame)) => {
                        upgraded_framed
                            .write_all(&frame)
                            .await
                            .map_err(|e| format!("write resize failed: {e}"))?;
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

        // 本轮捕获的 Deactivation-Reactivation 序列（服务端换分辨率的兜底路径，见 drain_outputs）。
        let mut pending_reactivation: Option<Box<ConnectionActivationSequence>> = None;
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
            &mut pending_reactivation,
        )
        .await?
        {
            return Ok(());
        }
        if let Some(seq) = pending_reactivation.take() {
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
                            &mut pending_reactivation,
                        )
                        .await?
                        {
                            return Ok(());
                        }
                        if pending_reactivation.is_some() {
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
            if let Some(seq) = pending_reactivation.take() {
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
    mut sequence: Box<ConnectionActivationSequence>,
    image: &mut DecodedImage,
) -> Result<(u16, u16), String>
where
    S: Send + Sync + Unpin + tokio::io::AsyncRead + tokio::io::AsyncWrite,
{
    let mut buf = WriteBuf::new();
    loop {
        let written = ironrdp_tokio::single_sequence_step_read(framed, &mut *sequence, &mut buf)
            .await
            .map_err(|e| format!("reactivation step read failed: {e}"))?;
        if written.size().is_some() {
            framed
                .write_all(buf.filled())
                .await
                .map_err(|e| format!("reactivation step write failed: {e}"))?;
        }
        if let ConnectionActivationState::Finalized {
            io_channel_id,
            user_channel_id,
            desktop_size,
            share_id,
            enable_server_pointer,
            pointer_software_rendering,
        } = sequence.connection_activation_state()
        {
            *image =
                DecodedImage::new(PixelFormat::RgbA32, desktop_size.width, desktop_size.height);
            active_stage.set_fastpath_processor(
                fast_path::ProcessorBuilder {
                    io_channel_id,
                    user_channel_id,
                    share_id,
                    enable_server_pointer,
                    pointer_software_rendering,
                    bulk_decompressor: None,
                }
                .build(),
            );
            active_stage.set_share_id(share_id);
            active_stage.set_enable_server_pointer(enable_server_pointer);
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
async fn drain_outputs<W: FramedWrite>(
    framed: &mut W,
    outputs: Vec<ActiveStageOutput>,
    acc: &mut Option<DirtyRect>,
    desktop_w: u16,
    desktop_h: u16,
    event_tx: &async_channel::Sender<RdpEvent>,
    last_pointer_key: &mut Option<u64>,
    reactivation: &mut Option<Box<ConnectionActivationSequence>>,
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
                let _ = event_tx.try_send(RdpEvent::Disconnected {
                    reason: format!("terminated: {reason}"),
                });
                return Ok(true);
            }
            // 动态分辨率兜底：服务端以重激活换分辨率时回传序列，交主循环 run_reactivation 走完。
            ActiveStageOutput::DeactivateAll(sequence) => {
                *reactivation = Some(sequence);
            }
            // PointerPosition（本地光标本就跟手）等忽略。
            _ => {}
        }
    }
    Ok(false)
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
mod tests {
    use super::*;

    #[test]
    fn domain_user_splits_on_backslash() {
        assert_eq!(
            split_domain_user("CORP\\alice"),
            (Some("CORP".to_string()), "alice".to_string())
        );
    }

    #[test]
    fn plain_user_has_no_domain() {
        assert_eq!(split_domain_user("bob"), (None, "bob".to_string()));
    }

    #[test]
    fn empty_domain_falls_back_to_local() {
        assert_eq!(split_domain_user("\\svc"), (None, "\\svc".to_string()));
    }

    #[test]
    fn egfx_defaults_on_when_not_disabled() {
        assert!(default_enable_egfx_from_env(None));
    }

    #[test]
    fn egfx_can_be_disabled_for_legacy_fallback() {
        assert!(!default_enable_egfx_from_env(Some(
            std::ffi::OsString::from("1")
        )));
    }

    #[test]
    fn audio_static_channels_include_rdpdr_dependency() {
        // 重构后 rdpsnd 与 rdpdr 分别注册：照 connect_and_run 的组装方式验证「audio 开时 rdpdr 在场」。
        let config = RdpSessionConfig {
            host: "127.0.0.1".to_string(),
            port: 3389,
            username: "alice".to_string(),
            password: "secret".to_string(),
            width: 1024,
            height: 768,
            enable_egfx: false,
            enable_audio: true,
            enable_drive: false,
            desktop_scale_factor: 100,
        };
        let connector = ClientConnector::new(
            build_connector_config(&config),
            "127.0.0.1:0".parse().expect("valid loopback socket addr"),
        );
        let mut connector = attach_audio_static_channels(
            connector,
            ironrdp_rdpsnd::client::Rdpsnd::new(Box::new(
                ironrdp_rdpsnd::client::NoopRdpsndBackend,
            )),
        );
        if let Some(channel) = rdpdr::build_channel(config.enable_drive, config.enable_audio) {
            connector = connector.with_static_channel(channel);
        }

        let names = audio_diag::static_channel_names(&connector.static_channels);

        assert!(names.iter().any(|name| name == "rdpsnd"));
        assert!(
            names.iter().any(|name| name == "rdpdr"),
            "FreeRDP enables rdpdr when rdpsnd is present; got {names:?}"
        );
    }

    #[test]
    fn rdpsnd_companion_follows_audio_or_drive() {
        // 核心修复：drive 单开（audio 关）也必须挂 rdpsnd 伴随通道。
        assert!(needs_rdpsnd(true, false));
        assert!(needs_rdpsnd(false, true));
        assert!(needs_rdpsnd(true, true));
        assert!(!needs_rdpsnd(false, false));
    }

    #[test]
    fn drive_only_advertises_rdpsnd_and_rdpdr() {
        // audio 关、drive 开：照 connect_and_run 组装，验证 rdpsnd 伴随 + rdpdr 同时在场。
        let config = RdpSessionConfig {
            host: "127.0.0.1".to_string(),
            port: 3389,
            username: "alice".to_string(),
            password: "secret".to_string(),
            width: 1024,
            height: 768,
            enable_egfx: false,
            enable_audio: false,
            enable_drive: true,
            desktop_scale_factor: 100,
        };
        let mut connector = ClientConnector::new(
            build_connector_config(&config),
            "127.0.0.1:0".parse().expect("valid loopback socket addr"),
        );
        // drive 开、audio 关 → Noop rdpsnd 静默伴随（单测里避免真实 cpal 设备）。
        if needs_rdpsnd(config.enable_audio, config.enable_drive) {
            connector = attach_audio_static_channels(
                connector,
                ironrdp_rdpsnd::client::Rdpsnd::new(Box::new(
                    ironrdp_rdpsnd::client::NoopRdpsndBackend,
                )),
            );
        }
        if let Some(channel) = rdpdr::build_channel(config.enable_drive, config.enable_audio) {
            connector = connector.with_static_channel(channel);
        }

        let names = audio_diag::static_channel_names(&connector.static_channels);
        assert!(
            names.iter().any(|name| name == "rdpsnd"),
            "drive needs rdpsnd companion (MS-RDPEFS); got {names:?}"
        );
        assert!(
            names.iter().any(|name| name == "rdpdr"),
            "drive registers rdpdr; got {names:?}"
        );
    }

    #[test]
    fn apply_full_copies_whole_frame() {
        let mut fb = RdpFramebuffer::new(2, 2);
        let src = vec![9u8; 2 * 2 * 4];
        let dirty = fb.apply_full(&src);
        assert_eq!(
            dirty,
            DirtyRect {
                x: 0,
                y: 0,
                width: 2,
                height: 2
            }
        );
        assert!(fb.rgba.iter().all(|&b| b == 9));
    }

    #[test]
    fn generation_advances_on_each_apply() {
        let mut fb = RdpFramebuffer::new(2, 2);
        assert_eq!(fb.generation(), 0);
        fb.apply_full(&vec![1u8; 2 * 2 * 4]);
        assert_eq!(fb.generation(), 1);
        fb.apply_region(
            &vec![2u8; 2 * 2 * 4],
            DirtyRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        assert_eq!(fb.generation(), 2);
    }

    #[test]
    fn key_press_and_release_flags() {
        use ironrdp_pdu::input::fast_path::KeyboardFlags;
        match to_fastpath_input(RdpInputEvent::Key {
            scancode: 0x1E,
            extended: false,
            pressed: true,
        }) {
            FastPathInputEvent::KeyboardEvent(flags, code) => {
                assert_eq!(code, 0x1E);
                assert!(!flags.contains(KeyboardFlags::RELEASE));
                assert!(!flags.contains(KeyboardFlags::EXTENDED));
            }
            other => panic!("expected KeyboardEvent, got {other:?}"),
        }
        match to_fastpath_input(RdpInputEvent::Key {
            scancode: 0x48,
            extended: true,
            pressed: false,
        }) {
            FastPathInputEvent::KeyboardEvent(flags, _) => {
                assert!(flags.contains(KeyboardFlags::RELEASE));
                assert!(flags.contains(KeyboardFlags::EXTENDED));
            }
            other => panic!("expected KeyboardEvent, got {other:?}"),
        }
    }

    #[test]
    fn mouse_move_and_button_flags() {
        use ironrdp_pdu::input::mouse::PointerFlags;
        match to_fastpath_input(RdpInputEvent::MouseMove { x: 10, y: 20 }) {
            FastPathInputEvent::MouseEvent(pdu) => {
                assert!(pdu.flags.contains(PointerFlags::MOVE));
                assert_eq!((pdu.x_position, pdu.y_position), (10, 20));
            }
            other => panic!("expected MouseEvent, got {other:?}"),
        }
        match to_fastpath_input(RdpInputEvent::MouseButton {
            button: RdpButton::Left,
            pressed: true,
            x: 1,
            y: 2,
        }) {
            FastPathInputEvent::MouseEvent(pdu) => {
                assert!(pdu.flags.contains(PointerFlags::LEFT_BUTTON));
                assert!(pdu.flags.contains(PointerFlags::DOWN));
            }
            other => panic!("expected MouseEvent, got {other:?}"),
        }
        // 右键抬起：RIGHT_BUTTON 且无 DOWN。
        match to_fastpath_input(RdpInputEvent::MouseButton {
            button: RdpButton::Right,
            pressed: false,
            x: 0,
            y: 0,
        }) {
            FastPathInputEvent::MouseEvent(pdu) => {
                assert!(pdu.flags.contains(PointerFlags::RIGHT_BUTTON));
                assert!(!pdu.flags.contains(PointerFlags::DOWN));
            }
            other => panic!("expected MouseEvent, got {other:?}"),
        }
    }

    #[test]
    fn wheel_direction_and_sign() {
        use ironrdp_pdu::input::mouse::PointerFlags;
        match to_fastpath_input(RdpInputEvent::Wheel {
            horizontal: false,
            delta: -120,
            x: 5,
            y: 6,
        }) {
            FastPathInputEvent::MouseEvent(pdu) => {
                assert!(pdu.flags.contains(PointerFlags::VERTICAL_WHEEL));
                assert_eq!(pdu.number_of_wheel_rotation_units, -120);
            }
            other => panic!("expected MouseEvent, got {other:?}"),
        }
        match to_fastpath_input(RdpInputEvent::Wheel {
            horizontal: true,
            delta: 120,
            x: 0,
            y: 0,
        }) {
            FastPathInputEvent::MouseEvent(pdu) => {
                assert!(pdu.flags.contains(PointerFlags::HORIZONTAL_WHEEL));
            }
            other => panic!("expected MouseEvent, got {other:?}"),
        }
    }

    fn incl(left: u16, top: u16, right: u16, bottom: u16) -> InclusiveRectangle {
        InclusiveRectangle {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn inclusive_to_dirty_uses_inclusive_bounds() {
        // 0..=1 两轴 → 2x2 起点(0,0)。
        assert_eq!(
            inclusive_to_dirty(&incl(0, 0, 1, 1), 100, 100),
            DirtyRect {
                x: 0,
                y: 0,
                width: 2,
                height: 2
            }
        );
        // 单像素 rect：右=左、下=上 → 1x1。
        assert_eq!(
            inclusive_to_dirty(&incl(5, 7, 5, 7), 100, 100),
            DirtyRect {
                x: 5,
                y: 7,
                width: 1,
                height: 1
            }
        );
    }

    #[test]
    fn inclusive_to_dirty_clamps_to_desktop() {
        // 越界的 right/bottom 收敛到 max-1，宽高不超出画面。
        let d = inclusive_to_dirty(&incl(3, 3, 99, 99), 10, 10);
        assert_eq!(d.x, 3);
        assert_eq!(d.y, 3);
        assert_eq!(d.x + d.width, 10);
        assert_eq!(d.y + d.height, 10);
        // 零尺寸桌面不 panic，返回空矩形。
        assert_eq!(inclusive_to_dirty(&incl(0, 0, 1, 1), 0, 0).width, 0);
    }

    #[test]
    fn union_dirty_bounding_box() {
        let a = DirtyRect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        };
        let b = DirtyRect {
            x: 5,
            y: 6,
            width: 3,
            height: 4,
        };
        assert_eq!(
            union_dirty(a, b),
            DirtyRect {
                x: 0,
                y: 0,
                width: 8,
                height: 10
            }
        );
        // 自并保持不变。
        assert_eq!(union_dirty(a, a), a);
    }

    #[test]
    fn apply_region_copies_only_target_rows() {
        // 4x4 全 0，src 全 7，只覆盖右下角 2x2。
        let mut fb = RdpFramebuffer::new(4, 4);
        let src = vec![7u8; 4 * 4 * 4];
        fb.apply_region(
            &src,
            DirtyRect {
                x: 2,
                y: 2,
                width: 2,
                height: 2,
            },
        );
        let stride = 4 * 4;
        // 顶部两行仍全 0。
        assert!(fb.rgba[..2 * stride].iter().all(|&b| b == 0));
        // 第 3 行前 2 像素(0..8) 仍 0，后 2 像素(8..16) 被覆盖成 7。
        let row2 = &fb.rgba[2 * stride..3 * stride];
        assert!(row2[..8].iter().all(|&b| b == 0));
        assert!(row2[8..16].iter().all(|&b| b == 7));
    }

    fn make_pointer(px: u8) -> Arc<ironrdp_graphics::pointer::DecodedPointer> {
        Arc::new(ironrdp_graphics::pointer::DecodedPointer {
            width: 2,
            height: 2,
            hotspot_x: 1,
            hotspot_y: 0,
            bitmap_data: vec![px; 2 * 2 * 4],
        })
    }

    #[test]
    fn pointer_bitmap_maps_fields() {
        let mut last = None;
        let p = make_pointer(9);
        let event = pointer_to_event(&p, &mut last).expect("first pointer emitted");
        match event {
            RdpPointer::Bitmap {
                rgba,
                width,
                height,
                hotspot_x,
                hotspot_y,
                cache_key,
            } => {
                assert_eq!(rgba, vec![9u8; 16]);
                assert_eq!((width, height), (2, 2));
                assert_eq!((hotspot_x, hotspot_y), (1.0, 0.0));
                assert_eq!(last, Some(cache_key));
                // 内容 hash：不同 Arc、同内容 → 同 key（地址语义做不到）。
                let mut last2 = None;
                match pointer_to_event(&make_pointer(9), &mut last2)
                    .expect("second pointer emitted")
                {
                    RdpPointer::Bitmap { cache_key: k2, .. } => assert_eq!(cache_key, k2),
                    other => panic!("expected Bitmap, got {other:?}"),
                }
            }
            other => panic!("expected Bitmap, got {other:?}"),
        }
    }

    #[test]
    fn pointer_same_arc_dedups() {
        let mut last = None;
        let p = make_pointer(1);
        assert!(pointer_to_event(&p, &mut last).is_some());
        // 同一 Arc 再来一次 → 去重返回 None。
        assert!(pointer_to_event(&p, &mut last).is_none());
    }

    #[test]
    fn pointer_different_arc_reemits() {
        let mut last = None;
        let a = make_pointer(1);
        let b = make_pointer(2);
        assert!(pointer_to_event(&a, &mut last).is_some());
        // 内容不同 → 重新发送。
        assert!(pointer_to_event(&b, &mut last).is_some());
    }

    #[test]
    fn pointer_same_content_different_arc_dedups() {
        let mut last = None;
        assert!(pointer_to_event(&make_pointer(1), &mut last).is_some());
        // 新 Arc 但内容相同 → 内容 hash 去重返回 None（地址语义会误重发）。
        assert!(pointer_to_event(&make_pointer(1), &mut last).is_none());
    }
}
