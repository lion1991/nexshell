//! RDP 协议层（IronRDP 纯 Rust，见 docs/adr/0007）。
//! 线程模型照抄 SSH：每连接一个专用 OS 线程 + current-thread tokio，
//! 事件循环 block_on 跑，帧解码成 RGBA framebuffer，脏矩形事件推回 UI。
//! 本步只做：TCP → NLA(CredSSP) → 能力协商 → 收图形更新 → 合成 framebuffer。
//! 输入/剪贴板留占位（RdpInputEvent + input_tx），后续步骤实现。

mod clipboard;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ironrdp_async::FramedWrite;
use ironrdp_cliprdr::backend::ClipboardMessage;
use ironrdp_cliprdr::CliprdrClient;
use ironrdp_connector::sspi::generator::NetworkRequest;
use ironrdp_connector::{
    BitmapConfig, ClientConnector, Config, ConnectorResult, Credentials, DesktopSize, ServerName,
};
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_pdu::input::fast_path::FastPathInputEvent;
use ironrdp_pdu::rdp::capability_sets::BitmapCodecs;
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{ActiveStage, ActiveStageOutput};
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
}

/// UI 侧持有的会话句柄。drop 或 close() 时优雅断开。
pub struct RdpSessionHandle {
    /// 会话事件流（unbounded：UI 慢消费也不阻塞协议线程）。
    pub frame_rx: async_channel::Receiver<RdpEvent>,
    /// 共享 framebuffer，UI 重绘时读快照。
    pub framebuffer: Arc<Mutex<RdpFramebuffer>>,
    /// 输入通道占位（本步不消费）。
    pub input_tx: async_channel::Sender<RdpInputEvent>,
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
    let (close_tx, close_rx) = async_channel::unbounded::<()>();
    let framebuffer = Arc::new(Mutex::new(RdpFramebuffer::new(config.width, config.height)));

    let thread = thread::Builder::new()
        .name(format!("nexshell-rdp-{id}"))
        .spawn({
            let framebuffer = Arc::clone(&framebuffer);
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
                    event_tx,
                    close_rx,
                    input_rx,
                ));
            }
        })
        .ok();

    RdpSessionHandle {
        frame_rx,
        framebuffer,
        input_tx,
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
        desktop_scale_factor: 0,
        bitmap: Some(BitmapConfig {
            color_depth: 32,
            lossy_compression: true,
            // 空 codec 集：不广告扩展 codec，服务端回退标准位图更新（raw/RLE/RDP6），
            // ActiveStage 可解。RemoteFX 等 codec 广告后续步骤补。
            codecs: BitmapCodecs(Vec::new()),
        }),
        client_build: 0,
        client_name: "nexshell".to_string(),
        client_dir: String::new(),
        platform: ironrdp_pdu::rdp::capability_sets::MajorPlatformType::UNSPECIFIED,
        hardware_id: None,
        license_cache: None,
        enable_server_pointer: false,
        autologon: false,
        enable_audio_playback: false,
        request_data: None,
        pointer_software_rendering: false,
        multitransport_flags: None,
        compression_type: None,
        performance_flags: ironrdp_pdu::rdp::client_info::PerformanceFlags::default(),
        timezone_info: ironrdp_pdu::rdp::client_info::TimezoneInfo::default(),
        alternate_shell: String::new(),
        work_dir: String::new(),
    }
}

/// 事件循环：连接 → NLA → 激活 → 收帧。任何阶段出错都发 Disconnected 收尾。
async fn run_rdp_event_loop(
    config: RdpSessionConfig,
    framebuffer: Arc<Mutex<RdpFramebuffer>>,
    event_tx: async_channel::Sender<RdpEvent>,
    close_rx: async_channel::Receiver<()>,
    input_rx: async_channel::Receiver<RdpInputEvent>,
) {
    let reason = match connect_and_run(&config, &framebuffer, &event_tx, &close_rx, &input_rx).await
    {
        Ok(()) => "session ended".to_string(),
        Err(error) => error,
    };
    let _ = event_tx.try_send(RdpEvent::Disconnected { reason });
}

async fn connect_and_run(
    config: &RdpSessionConfig,
    framebuffer: &Arc<Mutex<RdpFramebuffer>>,
    event_tx: &async_channel::Sender<RdpEvent>,
    close_rx: &async_channel::Receiver<()>,
    input_rx: &async_channel::Receiver<RdpInputEvent>,
) -> Result<(), String> {
    // rustls 0.23 需显式选 provider（树内 ring/aws-lc-rs 共存），已装则忽略。
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 证书无条件接受（与 ssh_session check_server_key 恒 Ok 同姿态）：
    // ironrdp-tls 的 upgrade 不做链校验，天然接受任意服务端证书。
    let addr = format!("{}:{}", config.host, config.port);
    let tcp = TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("TCP connect failed: {e}"))?;
    let client_addr: SocketAddr = tcp
        .local_addr()
        .map_err(|e| format!("local_addr failed: {e}"))?;

    // cliprdr：注册文本剪贴板静态通道。backend 经 clip_tx 回递要发的 PDU，
    // 轮询/回调经 shared 桥接（见 clipboard 模块）。
    let clip_shared = clipboard::ClipboardShared::new();
    let (clip_tx, clip_rx) = async_channel::unbounded::<ClipboardMessage>();
    let clip_backend = clipboard::TextCliprdrBackend::new(clip_tx, &clip_shared);

    let connector_config = build_connector_config(config);
    let mut connector = ClientConnector::new(connector_config, client_addr)
        .with_static_channel(CliprdrClient::new(Box::new(clip_backend)));

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
    // Mac 剪贴板变化轮询（仅连接存活时跑；随事件循环退出而 drop，无残留任务）。
    let mut clip_poll = tokio::time::interval(Duration::from_secs(1));

    // 阶段四：收图形更新解码合成 + 并发消费键鼠输入，编码成 FastPath 发回。
    loop {
        // select：关闭 / 输入 / cliprdr 回递 / 剪贴板轮询 / 帧。前四路自成闭环 continue，不阻塞帧路径。
        // 输入分支：攒批→编码→写回。
        let (action, payload) = tokio::select! {
            _ = close_rx.recv() => return Ok(()),
            msg = clip_rx.recv() => {
                let Ok(msg) = msg else { continue; };
                send_clipboard_pdu(&mut active_stage, &mut upgraded_framed, msg).await?;
                continue;
            }
            _ = clip_poll.tick() => {
                if let Some(formats) = clipboard::poll_local_change(&clip_shared) {
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
                for out in outputs {
                    if let ActiveStageOutput::ResponseFrame(frame) = out {
                        upgraded_framed
                            .write_all(&frame)
                            .await
                            .map_err(|e| format!("write input failed: {e}"))?;
                    }
                }
                continue;
            }
            frame = upgraded_framed.read_pdu() => {
                frame.map_err(|e| format!("read frame failed: {e}"))?
            }
        };

        let outputs = active_stage
            .process(&mut image, action, &payload)
            .map_err(|e| format!("process frame failed: {e}"))?;

        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(frame) => {
                    upgraded_framed
                        .write_all(&frame)
                        .await
                        .map_err(|e| format!("write response failed: {e}"))?;
                }
                ActiveStageOutput::GraphicsUpdate(_region) => {
                    // 整帧覆盖 framebuffer（本步不做增量），上报整帧脏矩形。
                    let dirty = framebuffer.lock().apply_full(image.data());
                    let _ = event_tx.try_send(RdpEvent::FrameUpdated { dirty });
                }
                ActiveStageOutput::Terminate(reason) => {
                    let _ = event_tx.try_send(RdpEvent::Disconnected {
                        reason: format!("terminated: {reason}"),
                    });
                    return Ok(());
                }
                // 指针/DeactivateAll/输入等本步忽略。
                _ => {}
            }
        }
    }
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
}
