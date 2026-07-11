// rdp_section — RDP 整页 tab 的 handler + render 入口（组装 rdp_view Element 与协议层）。
// 本文件只含 impl RootView，无自由函数（几何/纯逻辑在 rdp_view，渲染 Element 在 rdp_view）。
// 面板 section 间禁互 use；跨 section 复用走 self.xxx() 方法调用。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pathfinder_geometry::vector::{vec2f, Vector2F, Vector2I};

use warpui::assets::asset_cache::AssetCache;
use warpui::color::ColorU;
use warpui::elements::{
    Align, Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    DispatchEventResult, Empty, EventHandler, Flex, MainAxisAlignment, MainAxisSize, ParentElement,
    Radius, Stack, Text,
};
use warpui::image_cache::{CustomImageFormat, CustomImageHeader, ImageType};
use warpui::r#async::Timer;
use warpui::{fonts, AppContext, Element, SingletonEntity, ViewContext};

use crate::rdp_view::{rdp_desktop_scale_factor, rdp_desktop_size, RdpPageElement};
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{
    RdpConnectionPhase, RdpTabState, RootView, TerminalSessionKind, TITLE_BAR_BORDER_HEIGHT,
    TITLE_BAR_HEIGHT,
};
use nexshell::generation::{accepts_generation, Generation};
use nexshell::host_management::{HostConnectionConfig, RdpDisplayQuality};
use nexshell::rdp_session::{
    default_enable_egfx, spawn_rdp_session, RdpEvent, RdpResizeRequest, RdpSessionConfig,
};
use nexshell::terminal_runtime::LocalTerminalRuntime;

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
        let (width, height) = rdp_desktop_size(content_area, scale, hidpi);
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
        let asset_id = format!("rdp:{tab_id}");
        let stats = Arc::clone(&handle.stats);
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            tab.rdp = Some(RdpTabState {
                handle,
                config: rdp_config,
                session_generation,
                phase: RdpConnectionPhase::Connecting,
                asset_id,
                last_uploaded_generation: 0,
                viewport: Arc::new(Mutex::new(None)),
                last_mouse: Arc::new(Mutex::new(None)),
                mod_tracker: Arc::new(Mutex::new(Default::default())),
                current_pointer: warpui::platform::Cursor::Arrow,
                pointer_cursor_cache: std::collections::HashMap::new(),
                hidpi,
                stats,
                conn_info_open: false,
                conn_info_last_sample: None,
                conn_info_mbps: 0.0,
                conn_info_fps: 0.0,
                resize_debounce: Default::default(),
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
            rdp.handle = handle; // 旧 handle 被替换后 drop → 优雅断开旧会话。
            rdp.phase = RdpConnectionPhase::Connecting;
            rdp.last_uploaded_generation = 0;
            if let Ok(mut vp) = rdp.viewport.lock() {
                *vp = None;
            }
        }
        self.attach_rdp_frame_stream(tab_id, session_generation, frame_rx, ctx);
        ctx.notify();
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
            RdpEvent::Resized { .. } => {
                // 远端分辨率已换（framebuffer 已按新尺寸重建）：复位上传代号强制重传，
                // 复位防抖使新分辨率成为「当前」基线。桌面分辨率显示随 framebuffer 自动更新。
                if let Some(rdp) = self.rdp_state_mut(tab_id) {
                    rdp.last_uploaded_generation = 0;
                    rdp.resize_debounce = Default::default();
                }
                ctx.notify();
            }
            RdpEvent::Disconnected { reason } => {
                if let Some(rdp) = self.rdp_state_mut(tab_id) {
                    rdp.phase = RdpConnectionPhase::Disconnected { reason };
                }
                ctx.notify();
            }
            RdpEvent::FrameUpdated { .. } => {
                // 取最新帧：仅当 generation 前进时打包带自定义头的 RGBA。
                let upload = self
                    .terminal_tabs
                    .iter()
                    .find(|t| t.id == tab_id)
                    .and_then(|t| t.rdp.as_ref())
                    .and_then(|rdp| {
                        let fb = rdp.handle.framebuffer.lock();
                        let generation = fb.generation();
                        if generation <= rdp.last_uploaded_generation {
                            return None;
                        }
                        // 一次分配带头构造：绕开 clone+splice 的两次全帧搬运（len 恒 = w*h*4，无需校验）。
                        let header = CustomImageHeader {
                            width: fb.width as u32,
                            height: fb.height as u32,
                            image_format: CustomImageFormat::Rgba,
                        }
                        .create_header();
                        let mut bytes = Vec::with_capacity(header.len() + fb.rgba.len());
                        bytes.extend_from_slice(header.as_bytes());
                        bytes.extend_from_slice(&fb.rgba);
                        Some((rdp.asset_id.clone(), generation, bytes))
                    });

                let Some((asset_id, generation, bytes)) = upload else {
                    return;
                };
                // 稳定 key 覆盖同一条目：单会话仅一条 raw asset，逐帧替换，不堆积。
                AssetCache::handle(ctx).update(ctx, |cache, ctx| {
                    cache.insert_raw_asset_bytes::<ImageType>(asset_id, &bytes, ctx);
                });
                if let Some(rdp) = self.rdp_state_mut(tab_id) {
                    rdp.last_uploaded_generation = generation;
                    rdp.phase.on_frame_uploaded();
                }
                ctx.notify();
            }
            RdpEvent::PointerChanged(pointer) => {
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
                    ctx.notify();
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

    /// 切走/关闭某 tab 前，若它是 RDP tab 则抬起全部远端修饰键，防卡键（尤其 Win 键）。
    /// 非 RDP tab 或已断开静默跳过（try_send 满/断开也丢弃不阻塞）。
    pub(in crate::root_view) fn release_rdp_modifiers(&self, index: usize) {
        let Some(rdp) = self.terminal_tabs.get(index).and_then(|t| t.rdp.as_ref()) else {
            return;
        };
        for event in crate::rdp_view::keymap::modifier_release_events() {
            let _ = rdp.handle.input_tx.try_send(event);
        }
        // 全量抬起后清账，tracker 与远端复位同步（避免残留状态误判后续对账）。
        if let Ok(mut tracker) = rdp.mod_tracker.lock() {
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
            RdpConnectionPhase::Connected => {
                let (w, h) = {
                    let fb = rdp.handle.framebuffer.lock();
                    (fb.width, fb.height)
                };
                let element = RdpPageElement::new(
                    rdp.asset_id.clone(),
                    Vector2I::new(w as i32, h as i32),
                    ColorU::new(0, 0, 0, 255),
                    rdp.viewport.clone(),
                    rdp.handle.input_tx.clone(),
                    rdp.last_mouse.clone(),
                    rdp.mod_tracker.clone(),
                    rdp.current_pointer,
                )
                .finish();
                Container::new(element)
                    .with_background_color(ColorU::new(0, 0, 0, 255))
                    .finish()
            }
        };

        if !rdp.conn_info_open {
            return body;
        }
        let mut stack = Stack::new();
        stack.add_child(body);
        stack.add_overlay_child(
            Container::new(
                Align::new(self.render_rdp_conn_info_card(rdp, &colors))
                    .top_right()
                    .finish(),
            )
            .with_margin_top(16.0)
            .with_margin_right(16.0)
            .finish(),
        );
        stack.finish()
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

    /// 1s 自重排定时器：无打开面板即停表；否则各打开面板差分算 Mbps/fps 后重绘。
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
            ctx.notify();
            Self::schedule_rdp_conn_info_tick(ctx);
        });
    }

    /// 动态分辨率检测定时器（~100ms tick）：内容区尺寸稳定 ~400ms 且换算 ≠ 当前会话分辨率
    /// 时向会话发 resize 请求（防抖在 ResizeDebounce）。无已连接 RDP tab 即停表。
    fn schedule_rdp_resize_tick(ctx: &mut ViewContext<Self>) {
        ctx.spawn(Timer::after(Duration::from_millis(100)), |me, _, ctx| {
            let any_connected = me.terminal_tabs.iter().any(|t| {
                t.rdp
                    .as_ref()
                    .map_or(false, |r| matches!(r.phase, RdpConnectionPhase::Connected))
            });
            if !any_connected {
                me.rdp_resize_ticking = false;
                return;
            }
            // 内容区逻辑尺寸 + 窗口 scale（全窗口共享，随尺寸/全屏实时变化）。
            let (content_area, scale) = me.rdp_content_area(ctx);
            for tab in me.terminal_tabs.iter_mut() {
                let Some(rdp) = tab.rdp.as_mut() else {
                    continue;
                };
                // 仅 EGFX 会话支持 Display Control 动态分辨率；legacy 不请求。
                if !matches!(rdp.phase, RdpConnectionPhase::Connected) || !rdp.config.enable_egfx {
                    continue;
                }
                let target = rdp_desktop_size(content_area, scale, rdp.hidpi);
                let current = {
                    let fb = rdp.handle.framebuffer.lock();
                    (fb.width, fb.height)
                };
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
        let (w, h) = {
            let fb = rdp.handle.framebuffer.lock();
            (fb.width, fb.height)
        };
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
                        "TCP".to_string(),
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
