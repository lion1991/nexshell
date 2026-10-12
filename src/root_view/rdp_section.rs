// rdp_section — RDP 整页 tab 的 handler + render 入口（组装 rdp_view Element 与协议层）。
// 本文件只含 impl RootView，无自由函数（几何/纯逻辑在 rdp_view，渲染 Element 在 rdp_view）。
// 面板 section 间禁互 use；跨 section 复用走 self.xxx() 方法调用。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pathfinder_geometry::vector::{vec2f, Vector2F, Vector2I};

use warpui::color::ColorU;
use warpui::elements::{
    Align, Border, ChildAnchor, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    DispatchEventResult, Draggable, Empty, EventHandler, Flex, MainAxisAlignment, MainAxisSize,
    OffsetPositioning, ParentAnchor, ParentElement, ParentOffsetBounds, Radius, Stack, Text,
};
use warpui::r#async::Timer;
use warpui::{fonts, AppContext, Element, ViewContext};

use crate::rdp_view::{
    rdp_desktop_scale_factor, rdp_desktop_size, rdp_fixed_desktop_size, RdpLiveImage,
    RdpPageElement,
};
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{
    RdpConnectionPhase, RdpTabState, RootView, TerminalSessionKind, TITLE_BAR_BORDER_HEIGHT,
    TITLE_BAR_HEIGHT,
};
use nexshell::generation::{accepts_generation, Generation};
use nexshell::host_management::{HostConnectionConfig, RdpDisplayQuality};
use nexshell::host_overview::format_bytes_short;
use nexshell::rdp_session::{
    default_enable_egfx, default_enable_udp, spawn_rdp_session, ClipboardTransfer, RdpEvent,
    RdpResizeRequest, RdpSessionConfig,
};
use nexshell::terminal_runtime::LocalTerminalRuntime;

/// 连接信息面板默认贴页面右上角的内缩距离。
const CONN_INFO_MARGIN: f32 = 16.0;

impl RootView {
    /// 主机库点 RDP 主机：算分辨率 → spawn 协议会话 → 开整页 tab → 起帧事件消费。
    pub(in crate::root_view) fn connect_rdp_host(
        &mut self,
        host_id: &str,
        session_id: &str,
        title: String,
        config: HostConnectionConfig,
        ctx: &mut ViewContext<Self>,
    ) {
        let (content_area, scale) = self.rdp_content_area(ctx);
        let hidpi = matches!(config.rdp_display_quality, RdpDisplayQuality::Hidpi);
        // 固定分辨率：按用户设定连接，不随窗口；否则按内容区推导。
        let fixed = config.rdp_resolution.fixed_size();
        let (width, height) = match fixed {
            Some((w, h)) => rdp_fixed_desktop_size(w, h),
            None => rdp_desktop_size(content_area, scale, hidpi),
        };
        let rdp_config = RdpSessionConfig {
            host: config.host.trim().to_string(),
            port: config.port,
            username: config.username.trim().to_string(),
            password: config.password.clone().unwrap_or_default(),
            width,
            height,
            // EGFX 默认开启；必要时用 NEXSHELL_RDP_DISABLE_EGFX=1 回退旧管线。
            enable_egfx: default_enable_egfx(),
            // RDPSND 音频重定向默认开启。
            enable_audio: true,
            // RDPDR 驱动器重定向默认开启（~/NexShell RDP ↔ \\tsclient\NexShell 文件互拷）。
            enable_drive: true,
            // HiDPI 下请求远端 DPI 缩放（对齐 Windows App）；标准画质=0 不请求。
            desktop_scale_factor: rdp_desktop_scale_factor(scale, hidpi),
            // 可靠 UDP 旁路默认开启；NEXSHELL_RDP_DISABLE_UDP=1 退回纯 TCP。
            enable_udp: default_enable_udp(),
        };

        let handle = spawn_rdp_session(rdp_config.clone());
        let frame_rx = handle.frame_rx.clone();
        let session_generation = self.async_generations.allocate();

        // 占位终端（failed，connected 恒 false）：RDP 不走 PTY，仅让 tab 结构成立。
        let placeholder = LocalTerminalRuntime::failed(session_id, "rdp session");
        self.push_terminal_tab(
            placeholder,
            session_id,
            title.clone(),
            TerminalSessionKind::Rdp,
            Some(host_id.to_string()),
            None,
            ctx,
        );

        let tab_id = self.terminal_tabs[self.active_tab_index].id.clone();
        let stats = Arc::clone(&handle.stats);
        let live_image = self.rdp_live_image(&handle, ctx);
        let desktop_size = (rdp_config.width, rdp_config.height);
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            tab.rdp = Some(RdpTabState {
                handle,
                config: rdp_config,
                session_generation,
                phase: RdpConnectionPhase::Connecting,
                live_image,
                has_frame: false,
                desktop_size,
                viewport: Arc::new(Mutex::new(None)),
                last_mouse: Arc::new(Mutex::new(None)),
                mod_tracker: Arc::new(Mutex::new(Default::default())),
                current_pointer: warpui::platform::Cursor::Arrow,
                pointer_cursor_cache: std::collections::HashMap::new(),
                hidpi,
                fixed_resolution: fixed.is_some(),
                stats,
                conn_info_open: false,
                conn_info_offset: Vector2F::zero(),
                conn_info_drag: Default::default(),
                conn_info_drag_origin: Default::default(),
                conn_info_last_sample: None,
                conn_info_mbps: 0.0,
                conn_info_fps: 0.0,
                resize_debounce: Default::default(),
                clipboard_transfer: None,
            });
        }
        self.attach_rdp_frame_stream(tab_id, session_generation, frame_rx, ctx);
        self.host_state.notice = Some(rust_i18n::t!("toast_connecting", title = title).to_string());
    }

    /// 断开态「重连」：用 tab 里存的 config 重新 spawn（沿用连接时定的分辨率），旧 handle drop 断开。
    pub(in crate::root_view) fn reconnect_rdp_tab(
        &mut self,
        index: usize,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(tab) = self.terminal_tabs.get(index) else {
            return;
        };
        let Some(rdp) = tab.rdp.as_ref() else {
            return;
        };
        let config = rdp.config.clone();
        let tab_id = tab.id.clone();

        let handle = spawn_rdp_session(config.clone());
        let frame_rx = handle.frame_rx.clone();
        let session_generation = self.async_generations.allocate();
        let live_image = self.rdp_live_image(&handle, ctx);
        if let Some(rdp) = self
            .terminal_tabs
            .get_mut(index)
            .and_then(|t| t.rdp.as_mut())
        {
            rdp.stats = Arc::clone(&handle.stats); // 新会话新统计（旧的随旧 handle drop）。
            rdp.session_generation = session_generation;
            rdp.conn_info_last_sample = None;
            rdp.conn_info_mbps = 0.0;
            rdp.conn_info_fps = 0.0;
            rdp.clipboard_transfer = None;
            rdp.handle = handle; // 旧 handle 被替换后 drop → 优雅断开旧会话。
            rdp.phase = RdpConnectionPhase::Connecting;
            rdp.live_image = live_image;
            rdp.has_frame = false;
            rdp.desktop_size = (config.width, config.height);
            if let Ok(mut vp) = rdp.viewport.lock() {
                *vp = None;
            }
        }
        self.attach_rdp_frame_stream(tab_id, session_generation, frame_rx, ctx);
        ctx.notify();
    }

    /// 会话画面的像素源。渲染器读不到（会话线程正在写）时经 redraw 通道补一次重绘。
    fn rdp_live_image(
        &mut self,
        handle: &nexshell::rdp_session::RdpSessionHandle,
        ctx: &mut ViewContext<Self>,
    ) -> Arc<dyn warpui::rendering::live_image::LiveImageSource> {
        let (redraw_tx, redraw_rx) = async_channel::bounded(1);
        ctx.spawn_stream_local(redraw_rx, |_, (), ctx| ctx.notify(), |_, _| {});
        Arc::new(RdpLiveImage::new(
            Arc::clone(&handle.framebuffer),
            redraw_tx,
        ))
    }

    /// 内容区逻辑尺寸（窗口逻辑高减去标题栏）+ 窗口 scale factor。取不到窗口时给保守默认。
    fn rdp_content_area(&self, ctx: &ViewContext<Self>) -> (Vector2F, f32) {
        let mut logical = vec2f(1280.0, 800.0);
        let mut scale = 1.0;
        if let Some(window) = ctx.windows().platform_window(ctx.window_id()) {
            logical = window.as_ref().size();
            scale = window.as_ref().backing_scale_factor();
        }
        let chrome = TITLE_BAR_HEIGHT + TITLE_BAR_BORDER_HEIGHT;
        let content = vec2f(logical.x().max(1.0), (logical.y() - chrome).max(1.0));
        (content, scale)
    }

    /// 帧事件流：不节流（Connected/Disconnected 状态事件不可丢），
    /// 纹理上传按 generation 门控——只有更新的帧代号才 insert，避免重复上传。
    fn attach_rdp_frame_stream(
        &mut self,
        tab_id: String,
        session_generation: Generation,
        frame_rx: async_channel::Receiver<RdpEvent>,
        ctx: &mut ViewContext<Self>,
    ) {
        ctx.spawn_stream_local(
            frame_rx,
            move |view, event, ctx| {
                view.handle_rdp_event_for_tab(&tab_id, session_generation, event, ctx);
            },
            |_, _| {},
        );
    }

    fn handle_rdp_event_for_tab(
        &mut self,
        tab_id: &str,
        session_generation: Generation,
        event: RdpEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        let current = self
            .terminal_tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .and_then(|tab| tab.rdp.as_ref())
            .map(|rdp| rdp.session_generation);
        if !accepts_generation(current, session_generation) {
            return;
        }
        match event {
            RdpEvent::Connected => {
                if let Some(rdp) = self.rdp_state_mut(tab_id) {
                    rdp.phase.on_transport_connected();
                }
                // 已连接：启动动态分辨率检测（窗口尺寸/全屏变化 → 重设远端分辨率）。
                if !self.rdp_resize_ticking {
                    self.rdp_resize_ticking = true;
                    Self::schedule_rdp_resize_tick(ctx);
                }
                ctx.notify();
            }
            RdpEvent::Resized { width, height } => {
                // 远端分辨率已换（framebuffer 已按新尺寸重建）：记下新尺寸，
                // 复位防抖使新分辨率成为「当前」基线。
                if let Some(rdp) = self.rdp_state_mut(tab_id) {
                    rdp.desktop_size = (width, height);
                    rdp.resize_debounce = Default::default();
                }
                ctx.notify();
            }
            RdpEvent::Disconnected { reason } => {
                if let Some(rdp) = self.rdp_state_mut(tab_id) {
                    rdp.phase = RdpConnectionPhase::Disconnected { reason };
                    rdp.clipboard_transfer = None;
                }
                ctx.notify();
            }
            RdpEvent::Reconnecting {
                attempt,
                max,
                after_sleep,
            } => {
                if let Some(rdp) = self.rdp_state_mut(tab_id) {
                    rdp.phase = RdpConnectionPhase::Reconnecting {
                        attempt,
                        max,
                        after_sleep,
                    };
                    rdp.clipboard_transfer = None;
                }
                ctx.notify();
            }
            RdpEvent::ClipboardTransfer(progress) => {
                if let Some(rdp) = self.rdp_state_mut(tab_id) {
                    rdp.clipboard_transfer = progress;
                }
                ctx.notify();
            }
            // 只触发重绘：渲染器从 framebuffer 按帧代号上传脏区。画面不在屏上时只在首帧重绘（状态翻转）。
            RdpEvent::FrameUpdated { .. } => {
                let on_screen = self.rdp_tab_on_screen(tab_id);
                if let Some(rdp) = self.rdp_state_mut(tab_id) {
                    let first =
                        !rdp.has_frame || matches!(rdp.phase, RdpConnectionPhase::Connecting);
                    rdp.has_frame = true;
                    rdp.phase.on_frame_uploaded();
                    if on_screen || first {
                        ctx.notify();
                    }
                }
            }
            RdpEvent::PointerChanged(pointer) => {
                let on_screen = self.rdp_tab_on_screen(tab_id);
                let Some(rdp) = self.rdp_state_mut(tab_id) else {
                    return;
                };
                // 光标点尺寸随画面缩放（viewport 未就绪时按 1.0，后续指针更新会带上正确值）。
                let scale = rdp
                    .viewport
                    .lock()
                    .ok()
                    .and_then(|vp| vp.map(|v| v.scale))
                    .filter(|s| s.is_finite() && *s > 0.0)
                    .unwrap_or(1.0);
                // 备忘命中（同 cache_key 且 scale 未变）则复用已注册光标，避免重复注册打穿 LRU。
                let cursor = match &pointer {
                    nexshell::rdp_session::RdpPointer::Bitmap { cache_key, .. } => {
                        match rdp.pointer_cursor_cache.get(cache_key) {
                            Some((s, c)) if *s == scale => *c,
                            _ => {
                                let c = crate::rdp_view::pointer_to_cursor(&pointer, scale);
                                rdp.pointer_cursor_cache.insert(*cache_key, (scale, c));
                                c
                            }
                        }
                    }
                    _ => crate::rdp_view::pointer_to_cursor(&pointer, scale),
                };
                if std::env::var_os("NEXSHELL_RDP_PTR_TRACE").is_some() {
                    eprintln!("[rdp-ptr] view 收到指针 → {cursor:?} scale={scale}");
                }
                if rdp.current_pointer != cursor {
                    rdp.current_pointer = cursor;
                    // 鼠标可能正悬停画面内：重绘触发合成 MouseMoved → 重设光标。
                    if on_screen {
                        ctx.notify();
                    }
                }
            }
        }
    }

    fn rdp_state_mut(&mut self, tab_id: &str) -> Option<&mut RdpTabState> {
        self.terminal_tabs
            .iter_mut()
            .find(|t| t.id == tab_id)
            .and_then(|t| t.rdp.as_mut())
    }

    /// 同步 RDP 符号热键接管态：活动 tab 是 RDP、终端页、且窗口有键盘焦点时 Push，否则 Pop。
    /// 幂等（HotkeyGuardSlot 只在翻转时动作），render 每帧调 + on_focus/on_blur 调，自愈不叠 Push。
    pub(in crate::root_view) fn sync_rdp_hotkey_guard(&self) {
        let desired = self.app_page == crate::AppPage::Terminal
            && self.window_key_focused.get()
            && self
                .terminal_tabs
                .get(self.active_tab_index)
                .map_or(false, |t| matches!(t.kind, TerminalSessionKind::Rdp));
        self.rdp_hotkey_guard.borrow_mut().set_engaged(desired);
    }

    /// RDP 画面正显示在屏上：终端页的活动 tab 且应用可见。
    /// 远端始终照常出画面（SuppressOutput 会让 Windows 会话停止渲染，见 ADR 0020），
    /// 只是画面不在屏上时新帧不触发重绘，切回时渲染器整帧补传。
    fn rdp_tab_on_screen(&self, tab_id: &str) -> bool {
        self.app_visible
            && self.app_page == crate::AppPage::Terminal
            && self
                .terminal_tabs
                .get(self.active_tab_index)
                .is_some_and(|tab| tab.id == tab_id)
    }

    /// 订阅应用可见性（macOS 遮挡状态）；其他平台没有这个信号，视为始终可见。
    pub(in crate::root_view) fn watch_app_visibility(ctx: &mut ViewContext<Self>) {
        #[cfg(target_os = "macos")]
        ctx.spawn_stream_local(
            crate::macos_window_util::app_visibility_events(),
            |view, visible, ctx| {
                view.app_visible = visible;
                ctx.notify();
            },
            |_, _| {},
        );
        #[cfg(not(target_os = "macos"))]
        let _ = ctx;
    }

    /// 活动页是 RDP 时，把被本地菜单截走的 ⌘ 快捷键转成 Ctrl+键发往远端，返回是否已转发。
    pub(in crate::root_view) fn forward_rdp_command_shortcut(&self, key: &str) -> bool {
        if self.app_page != crate::AppPage::Terminal {
            return false;
        }
        let Some(rdp) = self
            .terminal_tabs
            .get(self.active_tab_index)
            .and_then(|t| t.rdp.as_ref())
        else {
            return false;
        };
        let Some((scancode, _)) = crate::rdp_view::keymap::scancode_for_key(key) else {
            return false;
        };
        if let Ok(mut tracker) = rdp.mod_tracker.lock() {
            for event in tracker.command_shortcut(scancode) {
                let _ = rdp.handle.input_tx.try_send(event);
            }
        }
        true
    }

    /// 离开某 RDP tab（切 tab / 切页 / 新建 tab / 失焦）前抬起远端全部修饰键、仍按住的普通键和左键，
    /// 防卡键（尤其 Win 键）和远端停在拖拽态。非 RDP tab 或已断开静默跳过（try_send 满/断开也丢弃不阻塞）。
    pub(in crate::root_view) fn release_rdp_held_input(&self, index: usize) {
        let Some(rdp) = self.terminal_tabs.get(index).and_then(|t| t.rdp.as_ref()) else {
            return;
        };
        for event in crate::rdp_view::keymap::modifier_release_events() {
            let _ = rdp.handle.input_tx.try_send(event);
        }
        // 全量抬起后清账，tracker 与远端复位同步（避免残留状态误判后续对账）。
        if let Ok(mut tracker) = rdp.mod_tracker.lock() {
            for event in tracker.drain_held_keys() {
                let _ = rdp.handle.input_tx.try_send(event);
            }
            let at = rdp
                .last_mouse
                .lock()
                .ok()
                .and_then(|m| *m)
                .unwrap_or_default();
            if let Some(event) = tracker.drain_left_button(at) {
                let _ = rdp.handle.input_tx.try_send(event);
            }
            tracker.clear();
        }
    }

    /// RDP 整页 body：连接中 / 已连接（嵌画面 Element）/ 已断开（reason + 重连按钮）。
    /// conn_info_open 时右上角叠加实时连接信息浮层。
    pub(in crate::root_view) fn render_rdp_page(&self, _app: &AppContext) -> Box<dyn Element> {
        let colors = self.design_tokens.overview;
        let Some(rdp) = self
            .terminal_tabs
            .get(self.active_tab_index)
            .and_then(|t| t.rdp.as_ref())
        else {
            return Container::new(Empty::new().finish()).finish();
        };

        let body = match &rdp.phase {
            RdpConnectionPhase::Connecting => {
                self.render_rdp_status(rust_i18n::t!("rdp_connecting").to_string(), None, &colors)
            }
            RdpConnectionPhase::Disconnected { reason } => self.render_rdp_status(
                rust_i18n::t!("rdp_disconnected", reason = reason).to_string(),
                Some(self.active_tab_index),
                &colors,
            ),
            // 还没传过帧就没有旧画面可留，只显示文字。
            RdpConnectionPhase::Reconnecting { .. } if !rdp.has_frame => {
                self.render_rdp_status(Self::rdp_reconnecting_message(&rdp.phase), None, &colors)
            }
            RdpConnectionPhase::Connected | RdpConnectionPhase::Reconnecting { .. } => {
                let (w, h) = rdp.desktop_size;
                let element = RdpPageElement::new(
                    rdp.has_frame.then(|| Arc::clone(&rdp.live_image)),
                    Vector2I::new(w as i32, h as i32),
                    ColorU::new(0, 0, 0, 255),
                    rdp.viewport.clone(),
                    rdp.handle.input_tx.clone(),
                    rdp.last_mouse.clone(),
                    rdp.mod_tracker.clone(),
                    rdp.current_pointer,
                )
                .finish();
                let page = Container::new(element)
                    .with_background_color(ColorU::new(0, 0, 0, 255))
                    .finish();
                if rdp.has_frame {
                    page
                } else {
                    // 首帧到之前继续盖着「连接中」，免得 文字 → 黑屏 → 画面 连切两次。
                    // 画面元素仍在底下参与布局，视口尺寸照常上报。
                    let mut stack = Stack::new();
                    stack.add_child(page);
                    stack.add_child(self.render_rdp_status(
                        rust_i18n::t!("rdp_connecting").to_string(),
                        None,
                        &colors,
                    ));
                    stack.finish()
                }
            }
        };

        let mut stack = Stack::new();
        stack.add_child(body);
        if matches!(rdp.phase, RdpConnectionPhase::Reconnecting { .. }) && rdp.has_frame {
            let card = Container::new(
                Text::new_inline(
                    Self::rdp_reconnecting_message(&rdp.phase),
                    self.ui_font,
                    13.0,
                )
                .with_color(colors.text_primary)
                .finish(),
            )
            .with_horizontal_padding(16.0)
            .with_vertical_padding(10.0)
            .with_background_color(colors.card_bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
            .with_border(Border::all(1.0).with_border_color(colors.panel_border))
            .finish();
            stack.add_positioned_overlay_child(
                card,
                OffsetPositioning::offset_from_parent(
                    Vector2F::zero(),
                    ParentOffsetBounds::ParentByPosition,
                    ParentAnchor::Center,
                    ChildAnchor::Center,
                ),
            );
        }
        if let Some(progress) = rdp.clipboard_transfer {
            stack.add_positioned_overlay_child(
                self.render_rdp_clipboard_transfer(progress, &colors),
                OffsetPositioning::offset_from_parent(
                    vec2f(0.0, -CONN_INFO_MARGIN),
                    ParentOffsetBounds::ParentByPosition,
                    ParentAnchor::BottomMiddle,
                    ChildAnchor::BottomMiddle,
                ),
            );
        }
        if !rdp.conn_info_open {
            return stack.finish();
        }
        // 按住面板任意处拖动；松手时把位移记进 conn_info_offset。
        let index = self.active_tab_index;
        let drag_origin = rdp.conn_info_drag_origin.clone();
        let drop_origin = drag_origin.clone();
        let card = Draggable::new(
            rdp.conn_info_drag.clone(),
            self.render_rdp_conn_info_card(rdp, &colors),
        )
        .on_drag_start(move |_, _, rect| drag_origin.set(Some(rect.origin())))
        .on_drop(move |ctx, _, rect, _| {
            if let Some(origin) = drop_origin.take() {
                ctx.dispatch_typed_action(TerminalGridAction::MoveRdpConnectionInfo {
                    index,
                    delta: rect.origin() - origin,
                    card: rect.size(),
                });
            }
        })
        .finish();
        stack.add_positioned_overlay_child(
            card,
            OffsetPositioning::offset_from_parent(
                vec2f(-CONN_INFO_MARGIN, CONN_INFO_MARGIN) + rdp.conn_info_offset,
                ParentOffsetBounds::ParentByPosition,
                ParentAnchor::TopRight,
                ChildAnchor::TopRight,
            ),
        );
        stack.finish()
    }

    pub(in crate::root_view) fn handle_cancel_rdp_clipboard_transfer(&self, index: usize) {
        if let Some(rdp) = self.terminal_tabs.get(index).and_then(|t| t.rdp.as_ref()) {
            rdp.handle.cancel_clipboard_transfer();
        }
    }

    /// 连接信息面板拖动结束：累加位移，并夹在内容区内（窗口缩小后也能拖回来）。
    pub(in crate::root_view) fn handle_move_rdp_connection_info(
        &mut self,
        index: usize,
        delta: Vector2F,
        card: Vector2F,
        ctx: &mut ViewContext<Self>,
    ) {
        let (area, _) = self.rdp_content_area(ctx);
        let Some(rdp) = self
            .terminal_tabs
            .get_mut(index)
            .and_then(|t| t.rdp.as_mut())
        else {
            return;
        };
        let max_left = (area.x() - card.x() - 2.0 * CONN_INFO_MARGIN).max(0.0);
        let max_down = (area.y() - card.y() - 2.0 * CONN_INFO_MARGIN).max(0.0);
        let next = rdp.conn_info_offset + delta;
        rdp.conn_info_offset = vec2f(
            next.x().clamp(-max_left, 0.0),
            next.y().clamp(0.0, max_down),
        );
        ctx.notify();
    }

    /// 切换连接信息浮层。打开时建采样基线并起 1s 差分定时器（已在跑则复用）。
    pub(in crate::root_view) fn handle_toggle_rdp_connection_info(
        &mut self,
        index: usize,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(rdp) = self
            .terminal_tabs
            .get_mut(index)
            .and_then(|t| t.rdp.as_mut())
        else {
            return;
        };
        rdp.conn_info_open = !rdp.conn_info_open;
        if rdp.conn_info_open {
            rdp.conn_info_last_sample =
                Some((rdp.stats.bytes(), rdp.stats.frames(), Instant::now()));
            rdp.conn_info_mbps = 0.0;
            rdp.conn_info_fps = 0.0;
            if !self.rdp_conn_info_refreshing {
                self.rdp_conn_info_refreshing = true;
                Self::schedule_rdp_conn_info_tick(ctx);
            }
        }
        ctx.notify();
    }

    /// 1s 自重排定时器：无打开面板即停表；否则各打开面板差分算 Mbps/fps，屏上那个开着才重绘。
    fn schedule_rdp_conn_info_tick(ctx: &mut ViewContext<Self>) {
        ctx.spawn(Timer::after(Duration::from_secs(1)), |me, _, ctx| {
            let any_open = me
                .terminal_tabs
                .iter()
                .any(|t| t.rdp.as_ref().map_or(false, |r| r.conn_info_open));
            if !any_open {
                me.rdp_conn_info_refreshing = false;
                return;
            }
            let now = Instant::now();
            for tab in me.terminal_tabs.iter_mut() {
                let Some(rdp) = tab.rdp.as_mut() else {
                    continue;
                };
                if !rdp.conn_info_open {
                    continue;
                }
                let (bytes, frames) = (rdp.stats.bytes(), rdp.stats.frames());
                if let Some((pb, pf, pat)) = rdp.conn_info_last_sample {
                    let dt = now.duration_since(pat).as_secs_f64();
                    rdp.conn_info_mbps = nexshell::rdp_session::mbps(bytes.saturating_sub(pb), dt);
                    rdp.conn_info_fps = nexshell::rdp_session::fps(frames.saturating_sub(pf), dt);
                }
                rdp.conn_info_last_sample = Some((bytes, frames, now));
            }
            let on_screen_open = me.terminal_tabs.get(me.active_tab_index).is_some_and(|t| {
                t.rdp.as_ref().is_some_and(|r| r.conn_info_open) && me.rdp_tab_on_screen(&t.id)
            });
            if on_screen_open {
                ctx.notify();
            }
            Self::schedule_rdp_conn_info_tick(ctx);
        });
    }

    /// 动态分辨率检测定时器（~100ms tick）：内容区尺寸稳定 ~400ms 且换算 ≠ 当前会话分辨率
    /// 时向会话发 resize 请求（防抖在 ResizeDebounce）。无已连接 RDP tab 即停表。
    fn schedule_rdp_resize_tick(ctx: &mut ViewContext<Self>) {
        ctx.spawn(Timer::after(Duration::from_millis(100)), |me, _, ctx| {
            // 与下方循环同一门槛：固定分辨率 / 非 EGFX 的 tab 不参与轮询，全都不需要就停表。
            let any_pollable = me.terminal_tabs.iter().any(|t| {
                t.rdp.as_ref().map_or(false, |r| {
                    matches!(r.phase, RdpConnectionPhase::Connected)
                        && r.config.enable_egfx
                        && !r.fixed_resolution
                })
            });
            if !any_pollable {
                me.rdp_resize_ticking = false;
                return;
            }
            // 内容区逻辑尺寸 + 窗口 scale（全窗口共享，随尺寸/全屏实时变化）。
            let (content_area, scale) = me.rdp_content_area(ctx);
            for tab in me.terminal_tabs.iter_mut() {
                let Some(rdp) = tab.rdp.as_mut() else {
                    continue;
                };
                // 仅 EGFX 会话支持 Display Control 动态分辨率；legacy / 固定分辨率不请求。
                if !matches!(rdp.phase, RdpConnectionPhase::Connected)
                    || !rdp.config.enable_egfx
                    || rdp.fixed_resolution
                {
                    continue;
                }
                let target = rdp_desktop_size(content_area, scale, rdp.hidpi);
                let current = rdp.desktop_size;
                // 4 tick ≈ 400ms 稳定才发。
                if let Some((width, height)) = rdp.resize_debounce.tick(target, current, 4) {
                    let _ = rdp.handle.resize_tx.try_send(RdpResizeRequest {
                        width,
                        height,
                        scale_factor: rdp_desktop_scale_factor(scale, rdp.hidpi),
                    });
                }
            }
            Self::schedule_rdp_resize_tick(ctx);
        });
    }

    /// 连接信息浮层卡片：分组标题 + 键值行（网络 / 连接 / 图形管线），参照 host_overview 风格。
    fn render_rdp_conn_info_card(
        &self,
        rdp: &RdpTabState,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let na = rust_i18n::t!("rdp_info_na").to_string();
        let rtt = rdp
            .stats
            .rtt_ms()
            .map(|v| format!("{v:.0} ms"))
            .unwrap_or_else(|| na.clone());
        let recv = format!("{:.1} Mbps", rdp.conn_info_mbps);
        let fps = format!("{:.0} fps", rdp.conn_info_fps);
        let (w, h) = rdp.desktop_size;
        let quality = if rdp.hidpi {
            rust_i18n::t!("rdp_quality_hidpi")
        } else {
            rust_i18n::t!("rdp_quality_standard")
        };
        let resolution = format!("{w}×{h} · {quality}");
        let addr = format!("{}:{}", rdp.config.host, rdp.config.port);
        let duration =
            nexshell::rdp_session::format_duration_hms(rdp.stats.connected_at().elapsed());
        let pipeline = match rdp.stats.pipeline() {
            2 => rust_i18n::t!("rdp_info_pipeline_egfx"),
            1 => rust_i18n::t!("rdp_info_pipeline_remotefx"),
            _ => rust_i18n::t!("rdp_info_pipeline_bitmap"),
        }
        .to_string();

        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_rdp_conn_info_header(colors));

        for (title, rows) in [
            (
                rust_i18n::t!("rdp_info_group_network").to_string(),
                vec![
                    (
                        rust_i18n::t!("rdp_info_transport").to_string(),
                        if rdp.stats.udp_active() {
                            "TCP + UDP"
                        } else {
                            "TCP"
                        }
                        .to_string(),
                    ),
                    (rust_i18n::t!("rdp_info_rtt").to_string(), rtt),
                    (rust_i18n::t!("rdp_info_recv_rate").to_string(), recv),
                ],
            ),
            (
                rust_i18n::t!("rdp_info_group_connection").to_string(),
                vec![
                    (rust_i18n::t!("rdp_info_remote_addr").to_string(), addr),
                    (rust_i18n::t!("rdp_info_resolution").to_string(), resolution),
                    (rust_i18n::t!("rdp_info_duration").to_string(), duration),
                ],
            ),
            (
                rust_i18n::t!("rdp_info_group_graphics").to_string(),
                vec![
                    (rust_i18n::t!("rdp_info_pipeline").to_string(), pipeline),
                    (rust_i18n::t!("rdp_info_publish_fps").to_string(), fps),
                ],
            ),
        ] {
            column.add_child(
                Container::new(self.render_rdp_conn_info_section(title, rows, colors))
                    .with_padding_top(12.0)
                    .finish(),
            );
        }

        let card = Container::new(column.finish())
            .with_horizontal_padding(14.0)
            .with_vertical_padding(14.0)
            .with_background_color(colors.card_bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
            .with_border(Border::all(1.0).with_border_color(colors.panel_border))
            .finish();
        ConstrainedBox::new(card).with_width(240.0).finish()
    }

    /// 卡片标题行「连接信息」+ 关闭按钮（再切 conn_info_open）。
    fn render_rdp_conn_info_header(&self, colors: &HostOverviewColors) -> Box<dyn Element> {
        let index = self.active_tab_index;
        let title = Text::new_inline(
            rust_i18n::t!("rdp_info_title").to_string(),
            self.ui_font,
            13.0,
        )
        .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
        .with_color(colors.text_primary)
        .finish();
        let close = EventHandler::new(
            Text::new_inline("✕".to_string(), self.ui_font, 13.0)
                .with_color(colors.text_muted)
                .finish(),
        )
        .on_left_mouse_down(move |ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::ToggleRdpConnectionInfo(index));
            DispatchEventResult::StopPropagation
        })
        .finish();
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_child(title)
            .with_child(close)
            .finish()
    }

    /// 一个分组：小标题 + 若干键值行。
    fn render_rdp_conn_info_section(
        &self,
        title: String,
        rows: Vec<(String, String)>,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                Text::new_inline(title, self.ui_font, 11.0)
                    .with_style(fonts::Properties::default().weight(fonts::Weight::Medium))
                    .with_color(colors.section_title)
                    .finish(),
            );
        for (key, value) in rows {
            column.add_child(
                Container::new(self.render_rdp_conn_info_row(key, value, colors))
                    .with_padding_top(5.0)
                    .finish(),
            );
        }
        column.finish()
    }

    /// 单条键值行：左键 muted，右值 primary，两端对齐。
    fn render_rdp_conn_info_row(
        &self,
        key: String,
        value: String,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_child(
                Text::new_inline(key, self.ui_font, 12.0)
                    .with_color(colors.text_muted)
                    .finish(),
            )
            .with_child(
                Text::new_inline(value, self.ui_font, 12.0)
                    .with_color(colors.text_primary)
                    .finish(),
            )
            .finish()
    }

    /// 远端文件下载浮条：进度 +「取消」。下载期间 Finder 的粘贴会一直等着，取消后它拿到空结果。
    fn render_rdp_clipboard_transfer(
        &self,
        progress: ClipboardTransfer,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let index = self.active_tab_index;
        let percent = (progress.done * 100)
            .checked_div(progress.total)
            .unwrap_or(0)
            .min(100);
        let message = rust_i18n::t!(
            "rdp_clipboard_transfer",
            files = progress.files,
            percent = percent,
            done = format_bytes_short(progress.done),
            total = format_bytes_short(progress.total)
        )
        .to_string();
        let cancel = EventHandler::new(
            Text::new_inline(
                rust_i18n::t!("rdp_clipboard_cancel").to_string(),
                self.ui_font,
                12.0,
            )
            .with_style(fonts::Properties::default().weight(fonts::Weight::Medium))
            .with_color(colors.text_primary)
            .finish(),
        )
        .on_left_mouse_down(move |ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::CancelRdpClipboardTransfer(index));
            DispatchEventResult::StopPropagation
        })
        .finish();
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(message, self.ui_font, 12.0)
                    .with_color(colors.text_primary)
                    .finish(),
            )
            .with_child(Container::new(cancel).with_padding_left(14.0).finish())
            .finish();
        Container::new(row)
            .with_horizontal_padding(14.0)
            .with_vertical_padding(8.0)
            .with_background_color(colors.card_bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
            .with_border(Border::all(1.0).with_border_color(colors.panel_border))
            .finish()
    }

    /// 连接中 / 断开态的居中提示；断开态附「重连」按钮（dispatch ReconnectTab）。
    fn render_rdp_status(
        &self,
        message: String,
        reconnect_index: Option<usize>,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(message, self.ui_font, 13.0)
                    .with_color(colors.text_primary)
                    .finish(),
            );

        if let Some(index) = reconnect_index {
            column.add_child(
                Container::new(self.render_rdp_reconnect_button(index, colors))
                    .with_padding_top(16.0)
                    .finish(),
            );
        }

        Container::new(Align::new(column.finish()).finish())
            .with_background_color(colors.panel_bg)
            .finish()
    }

    fn rdp_reconnecting_message(phase: &RdpConnectionPhase) -> String {
        let RdpConnectionPhase::Reconnecting {
            attempt,
            max,
            after_sleep,
        } = phase
        else {
            return String::new();
        };
        let key = if *after_sleep {
            "rdp_reconnecting_after_sleep"
        } else {
            "rdp_reconnecting"
        };
        rust_i18n::t!(key, attempt = attempt, max = max).to_string()
    }

    fn render_rdp_reconnect_button(
        &self,
        index: usize,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let label = Text::new_inline(
            rust_i18n::t!("rdp_reconnect").to_string(),
            self.ui_font,
            12.0,
        )
        .with_style(fonts::Properties::default().weight(fonts::Weight::Medium))
        .with_color(colors.text_primary)
        .finish();

        let button = Container::new(Align::new(label).finish())
            .with_horizontal_padding(16.0)
            .with_vertical_padding(7.0)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .with_border(Border::all(1.0).with_border_color(colors.panel_border))
            .finish();

        EventHandler::new(button)
            .on_left_mouse_down(move |ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::ReconnectTab(index));
                DispatchEventResult::StopPropagation
            })
            .finish()
    }
}
