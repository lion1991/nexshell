// rdp_section — RDP 整页 tab 的 handler + render 入口（组装 rdp_view Element 与协议层）。
// 本文件只含 impl RootView，无自由函数（几何/纯逻辑在 rdp_view，渲染 Element 在 rdp_view）。
// 面板 section 间禁互 use；跨 section 复用走 self.xxx() 方法调用。

use std::sync::{Arc, Mutex};

use pathfinder_geometry::vector::{vec2f, Vector2F, Vector2I};

use warpui::assets::asset_cache::AssetCache;
use warpui::color::ColorU;
use warpui::elements::{
    Align, Border, Container, CornerRadius, CrossAxisAlignment, DispatchEventResult, Empty,
    EventHandler, Flex, MainAxisSize, ParentElement, Radius, Text,
};
use warpui::image_cache::{CustomImageFormat, CustomImageHeader, ImageType};
use warpui::{fonts, AppContext, Element, SingletonEntity, ViewContext};

use crate::rdp_view::{rdp_desktop_size, RdpPageElement};
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{
    RdpConnectionPhase, RdpTabState, RootView, TerminalSessionKind, TITLE_BAR_BORDER_HEIGHT,
    TITLE_BAR_HEIGHT,
};
use nexshell::host_management::{HostConnectionConfig, RdpDisplayQuality};
use nexshell::rdp_session::{spawn_rdp_session, RdpEvent, RdpSessionConfig};
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
        };

        let handle = spawn_rdp_session(rdp_config.clone());
        let frame_rx = handle.frame_rx.clone();

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
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            tab.rdp = Some(RdpTabState {
                handle,
                config: rdp_config,
                phase: RdpConnectionPhase::Connecting,
                asset_id,
                last_uploaded_generation: 0,
                viewport: Arc::new(Mutex::new(None)),
            });
        }
        self.attach_rdp_frame_stream(tab_id, frame_rx, ctx);
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
        if let Some(rdp) = self
            .terminal_tabs
            .get_mut(index)
            .and_then(|t| t.rdp.as_mut())
        {
            rdp.handle = handle; // 旧 handle 被替换后 drop → 优雅断开旧会话。
            rdp.phase = RdpConnectionPhase::Connecting;
            rdp.last_uploaded_generation = 0;
            if let Ok(mut vp) = rdp.viewport.lock() {
                *vp = None;
            }
        }
        self.attach_rdp_frame_stream(tab_id, frame_rx, ctx);
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
        frame_rx: async_channel::Receiver<RdpEvent>,
        ctx: &mut ViewContext<Self>,
    ) {
        ctx.spawn_stream_local(
            frame_rx,
            move |view, event, ctx| {
                view.handle_rdp_event_for_tab(&tab_id, event, ctx);
            },
            |_, _| {},
        );
    }

    fn handle_rdp_event_for_tab(
        &mut self,
        tab_id: &str,
        event: RdpEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            RdpEvent::Connected => {
                if let Some(rdp) = self.rdp_state_mut(tab_id) {
                    rdp.phase = RdpConnectionPhase::Connected;
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
                        let bytes = CustomImageHeader::prepend_custom_header(
                            fb.rgba.clone(),
                            fb.width as u32,
                            fb.height as u32,
                            CustomImageFormat::Rgba,
                        )
                        .ok()?;
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
                }
                ctx.notify();
            }
        }
    }

    fn rdp_state_mut(&mut self, tab_id: &str) -> Option<&mut RdpTabState> {
        self.terminal_tabs
            .iter_mut()
            .find(|t| t.id == tab_id)
            .and_then(|t| t.rdp.as_mut())
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
    }

    /// RDP 整页 body：连接中 / 已连接（嵌画面 Element）/ 已断开（reason + 重连按钮）。
    pub(in crate::root_view) fn render_rdp_page(&self, _app: &AppContext) -> Box<dyn Element> {
        let colors = HostOverviewColors::from_theme(&self.cached_warp_theme);
        let Some(rdp) = self
            .terminal_tabs
            .get(self.active_tab_index)
            .and_then(|t| t.rdp.as_ref())
        else {
            return Container::new(Empty::new().finish()).finish();
        };

        match &rdp.phase {
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
                )
                .finish();
                Container::new(element)
                    .with_background_color(ColorU::new(0, 0, 0, 255))
                    .finish()
            }
        }
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
