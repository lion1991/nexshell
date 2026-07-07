use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use nexshell::text_editor::{
    EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions,
};
use warp_core::ui::appearance::Appearance;
use warp_editor::editor::NavigationKey;
use warpui::{
    color::ColorU,
    elements::{
        Align, Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
        CornerRadius, CrossAxisAlignment, Element, Expanded, Fill, Flex, Hoverable, Icon,
        MainAxisAlignment, MainAxisSize, MouseState, MouseStateHandle, ParentElement, Radius,
        ScrollbarWidth, Stack, Text, Wrap,
    },
    fonts,
    ui_components::components::{Coords, UiComponent, UiComponentStyles},
    CursorInfo, FocusContext, ModelAsRef, ModelHandle, SingletonEntity, View, ViewContext,
    ViewHandle,
};

use crate::host_management_view::constants::*;
use crate::warp_dropdown::{
    render_warp_dropdown, render_warp_dropdown_with_top_bar, WarpDropdownCustomProps,
    WarpDropdownOption, WarpDropdownProps,
};
use nexshell::host_management::{HostSystemIcon, RdpDisplayQuality};
use warpui::{Entity, ReadModel, UpdateModel};

// RDP 端口默认值：切到 RDP 且端口仍是 SSH 默认(22) 时自动改用此值。
const RDP_DEFAULT_PORT: u16 = 3389;

// ── 数据模型 ──

#[derive(Clone, Debug)]
pub struct HostEditDraft {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub serial_baud_rate: u32,
    pub username: String,
    pub auth_method: String,
    pub password: String,
    pub private_key: String,
    pub key_passphrase: String,
    pub ca_cert: String,
    // 引用密钥库的密钥 id（key 认证用，优先于内联 private_key）
    pub key_id: Option<String>,
    pub description: String,
    pub group_id: Option<String>,
    pub tags: Vec<String>,
    pub keep_alive_enabled: bool,
    pub keep_alive_interval: u16,
    pub keep_alive_max_failures: u16,
    pub tcp_connect_timeout: u16,
    pub auth_timeout: u16,
    pub term_encoding: String,
    pub serial_data_bits: u8,
    pub serial_stop_bits: u8,
    pub serial_parity: String,
    pub serial_flow_control: String,
    pub serial_dtr: bool,
    pub serial_rts: bool,
    pub rdp_display_quality: RdpDisplayQuality,
    pub system: HostSystemIcon,
}

impl HostEditDraft {
    pub fn from_card(card: &nexshell::host_management::HostCardSnapshot) -> Self {
        let c = &card.connection;
        Self {
            id: card.id.clone(),
            name: card.name.clone(),
            protocol: card.protocol.clone(),
            host: c.host.clone(),
            port: c.port,
            serial_baud_rate: c.serial_baud_rate,
            username: c.username.clone(),
            auth_method: c.auth_method.clone(),
            password: c.password.clone().unwrap_or_default(),
            private_key: c.private_key.clone().unwrap_or_default(),
            key_passphrase: c.key_passphrase.clone().unwrap_or_default(),
            ca_cert: c.ca_cert.clone().unwrap_or_default(),
            key_id: c.key_id.clone(),
            description: card.description.clone(),
            group_id: card.group_id.clone(),
            tags: card.tags.clone(),
            keep_alive_enabled: c.keep_alive_enabled,
            keep_alive_interval: c.keep_alive_interval,
            keep_alive_max_failures: c.keep_alive_max_failures as u16,
            tcp_connect_timeout: c.tcp_connect_timeout,
            auth_timeout: c.auth_timeout,
            term_encoding: c.term_encoding.clone(),
            serial_data_bits: c.serial_data_bits,
            serial_stop_bits: c.serial_stop_bits,
            serial_parity: c.serial_parity.clone(),
            serial_flow_control: c.serial_flow_control.clone(),
            serial_dtr: c.serial_dtr,
            serial_rts: c.serial_rts,
            rdp_display_quality: c.rdp_display_quality,
            system: card.system,
        }
    }

    pub fn new_ssh() -> Self {
        Self {
            id: format!(
                "host-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ),
            name: String::new(),
            protocol: "SSH".to_string(),
            host: String::new(),
            port: 22,
            serial_baud_rate: 115_200,
            username: "root".to_string(),
            auth_method: "password".to_string(),
            password: String::new(),
            private_key: String::new(),
            key_passphrase: String::new(),
            ca_cert: String::new(),
            key_id: None,
            description: String::new(),
            group_id: None,
            tags: Vec::new(),
            keep_alive_enabled: true,
            keep_alive_interval: 30,
            keep_alive_max_failures: 3,
            tcp_connect_timeout: 15,
            auth_timeout: 30,
            term_encoding: "utf-8".to_string(),
            serial_data_bits: 8,
            serial_stop_bits: 1,
            serial_parity: "none".to_string(),
            serial_flow_control: "none".to_string(),
            serial_dtr: false,
            serial_rts: false,
            rdp_display_quality: RdpDisplayQuality::Standard,
            system: HostSystemIcon::Terminal,
        }
    }
}

// ── 焦点字段 ──

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditField {
    Name,
    Host,
    Port,
    SerialBaudRate,
    Username,
    Password,
    PrivateKey,
    KeyPassphrase,
    CaCert,
    Description,
    KeepAliveInterval,
    KeepAliveMaxFailures,
    TcpConnectTimeout,
    AuthTimeout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostDropdownKind {
    Group,
    Key,
    SerialDevice,
    SerialBaudRate,
    Encoding,
    SerialDataBits,
    SerialStopBits,
    SerialParity,
    SerialFlowControl,
}

// ── Model (跨窗口通信) ──

#[derive(Clone, Debug)]
pub enum HostEditEvent {
    Saved(HostEditDraft),
    Cancelled,
}

pub struct HostEditModel {
    pub draft: HostEditDraft,
    pub is_new: bool,
    pub group_options: Vec<(String, String)>,
    pub key_options: Vec<(String, String)>,
    pub available_tags: Vec<String>,
}

impl warpui::Entity for HostEditModel {
    type Event = HostEditEvent;
}

// ── Action ──

#[derive(Clone, Debug)]
pub enum HostEditAction {
    SelectProtocol(String),
    SelectRdpQuality(RdpDisplayQuality),
    SelectAuthMethod(String),
    ToggleAdvancedSettings,
    ToggleKeepAlive,
    ToggleDropdown(HostDropdownKind),
    SelectGroup(Option<String>),
    SelectKey(Option<String>),
    SelectSerialDevice(String),
    SelectSerialBaudRate(u32),
    SelectEncoding(String),
    SelectSerialDataBits(u8),
    SelectSerialStopBits(u8),
    SelectSerialParity(String),
    SelectSerialFlowControl(String),
    ToggleSerialDtr,
    ToggleSerialRts,
    ToggleTag(String),
    TogglePasswordVisibility,
    IncrementPort,
    DecrementPort,
    Save,
    Cancel,
}

impl Entity for HostEditView {
    type Event = ();
}

// ── View ──

struct FieldStates {
    close_btn_state: MouseStateHandle,
    protocol_ssh_state: MouseStateHandle,
    protocol_rdp_state: MouseStateHandle,
    protocol_serial_state: MouseStateHandle,
    rdp_quality_standard_state: MouseStateHandle,
    rdp_quality_hidpi_state: MouseStateHandle,
    auth_password_state: MouseStateHandle,
    auth_key_state: MouseStateHandle,
    advanced_state: MouseStateHandle,
    group_state: MouseStateHandle,
    key_state: MouseStateHandle,
    serial_device_state: MouseStateHandle,
    serial_baud_rate_state: MouseStateHandle,
    encoding_state: MouseStateHandle,
    serial_data_bits_state: MouseStateHandle,
    serial_stop_bits_state: MouseStateHandle,
    serial_parity_state: MouseStateHandle,
    serial_flow_control_state: MouseStateHandle,
    serial_dtr_state: MouseStateHandle,
    serial_rts_state: MouseStateHandle,
    keep_alive_state: MouseStateHandle,
    password_eye_state: MouseStateHandle,
    port_inc_state: MouseStateHandle,
    port_dec_state: MouseStateHandle,
    save_state: MouseStateHandle,
    cancel_state: MouseStateHandle,
    open_dropdown: Option<HostDropdownKind>,
    advanced_settings_expanded: bool,
    group_item_states: RefCell<BTreeMap<String, MouseStateHandle>>,
    key_item_states: RefCell<BTreeMap<String, MouseStateHandle>>,
    serial_device_item_states: RefCell<BTreeMap<String, MouseStateHandle>>,
    serial_baud_rate_item_states: RefCell<BTreeMap<String, MouseStateHandle>>,
    encoding_item_states: RefCell<BTreeMap<String, MouseStateHandle>>,
    serial_data_bits_item_states: RefCell<BTreeMap<String, MouseStateHandle>>,
    serial_stop_bits_item_states: RefCell<BTreeMap<String, MouseStateHandle>>,
    serial_parity_item_states: RefCell<BTreeMap<String, MouseStateHandle>>,
    serial_flow_control_item_states: RefCell<BTreeMap<String, MouseStateHandle>>,
    tag_item_states: RefCell<BTreeMap<String, MouseStateHandle>>,
}

impl FieldStates {
    fn new() -> Self {
        let ms = || Arc::new(Mutex::new(MouseState::default()));
        Self {
            close_btn_state: ms(),
            protocol_ssh_state: ms(),
            protocol_rdp_state: ms(),
            protocol_serial_state: ms(),
            rdp_quality_standard_state: ms(),
            rdp_quality_hidpi_state: ms(),
            auth_password_state: ms(),
            auth_key_state: ms(),
            advanced_state: ms(),
            group_state: ms(),
            key_state: ms(),
            serial_device_state: ms(),
            serial_baud_rate_state: ms(),
            encoding_state: ms(),
            serial_data_bits_state: ms(),
            serial_stop_bits_state: ms(),
            serial_parity_state: ms(),
            serial_flow_control_state: ms(),
            serial_dtr_state: ms(),
            serial_rts_state: ms(),
            keep_alive_state: ms(),
            password_eye_state: ms(),
            port_inc_state: ms(),
            port_dec_state: ms(),
            save_state: ms(),
            cancel_state: ms(),
            open_dropdown: None,
            advanced_settings_expanded: true,
            group_item_states: RefCell::new(BTreeMap::new()),
            key_item_states: RefCell::new(BTreeMap::new()),
            serial_device_item_states: RefCell::new(BTreeMap::new()),
            serial_baud_rate_item_states: RefCell::new(BTreeMap::new()),
            encoding_item_states: RefCell::new(BTreeMap::new()),
            serial_data_bits_item_states: RefCell::new(BTreeMap::new()),
            serial_stop_bits_item_states: RefCell::new(BTreeMap::new()),
            serial_parity_item_states: RefCell::new(BTreeMap::new()),
            serial_flow_control_item_states: RefCell::new(BTreeMap::new()),
            tag_item_states: RefCell::new(BTreeMap::new()),
        }
    }
}

/// 连接关键字段净化：去首尾空白 + 滤掉控制字符（换行/NUL 等）。
fn sanitize_conn_field(s: &str) -> String {
    s.trim().chars().filter(|c| !c.is_control()).collect()
}

pub struct HostEditView {
    model: ModelHandle<HostEditModel>,
    ui_font: fonts::FamilyId,
    states: RefCell<FieldStates>,
    name_editor: ViewHandle<EditorView>,
    host_editor: ViewHandle<EditorView>,
    port_editor: ViewHandle<EditorView>,
    serial_baud_rate_editor: ViewHandle<EditorView>,
    username_editor: ViewHandle<EditorView>,
    password_editor: ViewHandle<EditorView>,
    private_key_editor: ViewHandle<EditorView>,
    key_passphrase_editor: ViewHandle<EditorView>,
    ca_cert_editor: ViewHandle<EditorView>,
    description_editor: ViewHandle<EditorView>,
    keep_alive_interval_editor: ViewHandle<EditorView>,
    keep_alive_max_failures_editor: ViewHandle<EditorView>,
    tcp_connect_timeout_editor: ViewHandle<EditorView>,
    auth_timeout_editor: ViewHandle<EditorView>,
    scroll_state: ClippedScrollStateHandle,
}

impl HostEditView {
    pub fn new(model: ModelHandle<HostEditModel>, ctx: &mut ViewContext<Self>) -> Self {
        let ui_font = fonts::Cache::handle(ctx).update(ctx, |cache, _| {
            cache
                .load_system_font("Helvetica Neue")
                .or_else(|_| cache.load_system_font("Helvetica"))
                .or_else(|_| cache.load_system_font("Arial"))
                .expect("ui font")
        });

        let draft = ctx.read_model(&model, |m, _| m.draft.clone());
        let name_editor = Self::create_warp_text_editor(
            &rust_i18n::t!("form_placeholder_host_name"),
            &draft.name,
            ctx,
        );
        let host_editor = Self::create_warp_text_editor(
            &Self::host_placeholder(&draft.protocol),
            &draft.host,
            ctx,
        );
        let port_editor = Self::create_warp_text_editor("", &draft.port.to_string(), ctx);
        let serial_baud_rate_editor =
            Self::create_warp_text_editor("", &draft.serial_baud_rate.to_string(), ctx);
        let username_editor = Self::create_warp_text_editor("root", &draft.username, ctx);
        let password_editor = Self::create_warp_text_editor_with_password(
            &rust_i18n::t!("form_placeholder_password"),
            &draft.password,
            true,
            ctx,
        );
        let private_key_editor = Self::create_warp_text_editor(
            &rust_i18n::t!("form_placeholder_key_path"),
            &draft.private_key,
            ctx,
        );
        let key_passphrase_editor = Self::create_warp_text_editor_with_password(
            &rust_i18n::t!("form_placeholder_key_passphrase"),
            &draft.key_passphrase,
            true,
            ctx,
        );
        let ca_cert_editor = Self::create_warp_text_editor(
            &rust_i18n::t!("form_placeholder_cert"),
            &draft.ca_cert,
            ctx,
        );
        let description_editor = Self::create_warp_text_editor(
            &rust_i18n::t!("form_placeholder_description"),
            &draft.description,
            ctx,
        );
        let keep_alive_interval_editor =
            Self::create_warp_text_editor("", &draft.keep_alive_interval.to_string(), ctx);
        let keep_alive_max_failures_editor =
            Self::create_warp_text_editor("", &draft.keep_alive_max_failures.to_string(), ctx);
        let tcp_connect_timeout_editor =
            Self::create_warp_text_editor("", &draft.tcp_connect_timeout.to_string(), ctx);
        let auth_timeout_editor =
            Self::create_warp_text_editor("", &draft.auth_timeout.to_string(), ctx);

        Self::subscribe_editor(EditField::Name, &name_editor, ctx);
        Self::subscribe_editor(EditField::Host, &host_editor, ctx);
        Self::subscribe_editor(EditField::Port, &port_editor, ctx);
        Self::subscribe_editor(EditField::SerialBaudRate, &serial_baud_rate_editor, ctx);
        Self::subscribe_editor(EditField::Username, &username_editor, ctx);
        Self::subscribe_editor(EditField::Password, &password_editor, ctx);
        Self::subscribe_editor(EditField::PrivateKey, &private_key_editor, ctx);
        Self::subscribe_editor(EditField::KeyPassphrase, &key_passphrase_editor, ctx);
        Self::subscribe_editor(EditField::CaCert, &ca_cert_editor, ctx);
        Self::subscribe_editor(EditField::Description, &description_editor, ctx);
        Self::subscribe_editor(
            EditField::KeepAliveInterval,
            &keep_alive_interval_editor,
            ctx,
        );
        Self::subscribe_editor(
            EditField::KeepAliveMaxFailures,
            &keep_alive_max_failures_editor,
            ctx,
        );
        Self::subscribe_editor(
            EditField::TcpConnectTimeout,
            &tcp_connect_timeout_editor,
            ctx,
        );
        Self::subscribe_editor(EditField::AuthTimeout, &auth_timeout_editor, ctx);

        Self {
            model,
            ui_font,
            states: RefCell::new(FieldStates::new()),
            name_editor,
            host_editor,
            port_editor,
            serial_baud_rate_editor,
            username_editor,
            password_editor,
            private_key_editor,
            key_passphrase_editor,
            ca_cert_editor,
            description_editor,
            keep_alive_interval_editor,
            keep_alive_max_failures_editor,
            tcp_connect_timeout_editor,
            auth_timeout_editor,
            scroll_state: ClippedScrollStateHandle::default(),
        }
    }
}

impl View for HostEditView {
    fn ui_name() -> &'static str {
        "HostEditView"
    }

    fn render(&self, ctx: &warpui::AppContext) -> Box<dyn Element> {
        let model = ctx.model(&self.model);
        let draft = &model.draft;
        let is_new = model.is_new;
        let ui_font = self.ui_font;
        let states = self.states.borrow();
        let appearance = Appearance::as_ref(ctx);
        let hc = HostUiColors::from_theme(appearance.theme());

        let title_text = if is_new {
            rust_i18n::t!("form_new_host")
        } else {
            rust_i18n::t!("form_edit_host")
        };
        let title = &*title_text;

        let mut root = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        root.add_child(render_header(title, &states.close_btn_state, ui_font, &hc));
        let password_visible = !self.password_editor.as_ref(ctx).is_password();
        let form = render_form(
            draft,
            &model.group_options,
            &model.key_options,
            &model.available_tags,
            &states,
            ui_font,
            self,
            appearance,
            &hc,
            password_visible,
        );
        let scrollable = ClippedScrollable::vertical(
            self.scroll_state.clone(),
            form,
            ScrollbarWidth::Auto,
            Fill::None,
            Fill::None,
            Fill::None,
        )
        .finish();
        root.add_child(Expanded::new(1.0, scrollable).finish());
        root.add_child(render_footer(
            &states.save_state,
            &states.cancel_state,
            is_new,
            ui_font,
            &hc,
        ));

        let inner = Container::new(root.finish())
            .with_background_color(hc.panel_bg)
            .finish();

        inner
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        if focus_ctx.is_self_focused() {
            ctx.focus(&self.name_editor);
            ctx.notify();
        }
    }

    fn active_cursor_position(&self, ctx: &ViewContext<Self>) -> Option<CursorInfo> {
        let editor = self.editor_for_field(self.current_editor_field(ctx));
        let cursor_id = nexshell::text_editor::position_id_for_cursor(editor.id());
        let font_size = Appearance::as_ref(ctx).ui_font_size();
        ctx.element_position_by_id(cursor_id)
            .map(|position| CursorInfo {
                position,
                font_size,
            })
    }
}

impl HostEditView {
    fn host_placeholder(protocol: &str) -> String {
        if protocol == "Serial" {
            rust_i18n::t!("form_placeholder_host_serial").to_string()
        } else {
            rust_i18n::t!("form_placeholder_host_ssh").to_string()
        }
    }

    fn create_warp_text_editor(
        placeholder: &str,
        initial_text: &str,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<EditorView> {
        Self::create_warp_text_editor_with_password(placeholder, initial_text, false, ctx)
    }

    fn create_warp_text_editor_with_password(
        placeholder: &str,
        initial_text: &str,
        is_password: bool,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<EditorView> {
        let initial_text = initial_text.to_string();
        let placeholder = placeholder.to_string();
        ctx.add_typed_action_view(move |ctx| {
            let font_size = Appearance::as_ref(ctx).ui_font_size();
            let options = SingleLineEditorOptions {
                text: TextOptions {
                    font_size_override: Some(font_size),
                    ..Default::default()
                },
                is_password,
                ..Default::default()
            };
            let mut editor = EditorView::single_line(options, ctx);
            editor.set_placeholder_text(&placeholder, ctx);
            if !initial_text.is_empty() {
                editor.system_reset_buffer_text(&initial_text, ctx);
            }
            editor
        })
    }

    fn subscribe_editor(
        field: EditField,
        editor: &ViewHandle<EditorView>,
        ctx: &mut ViewContext<Self>,
    ) {
        ctx.subscribe_to_view(editor, move |me, _, event: &EditorEvent, ctx| {
            me.handle_editor_event(field, event, ctx);
        });
    }

    fn editor_for_field(&self, field: EditField) -> &ViewHandle<EditorView> {
        match field {
            EditField::Name => &self.name_editor,
            EditField::Host => &self.host_editor,
            EditField::Port => &self.port_editor,
            EditField::SerialBaudRate => &self.serial_baud_rate_editor,
            EditField::Username => &self.username_editor,
            EditField::Password => &self.password_editor,
            EditField::PrivateKey => &self.private_key_editor,
            EditField::KeyPassphrase => &self.key_passphrase_editor,
            EditField::CaCert => &self.ca_cert_editor,
            EditField::Description => &self.description_editor,
            EditField::KeepAliveInterval => &self.keep_alive_interval_editor,
            EditField::KeepAliveMaxFailures => &self.keep_alive_max_failures_editor,
            EditField::TcpConnectTimeout => &self.tcp_connect_timeout_editor,
            EditField::AuthTimeout => &self.auth_timeout_editor,
        }
    }

    fn handle_editor_event(
        &mut self,
        field: EditField,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            EditorEvent::Edited(_) => {
                if field == EditField::SerialBaudRate {
                    self.sync_serial_baud_rate_from_editor(ctx);
                } else if let Some((min, max)) = number_bounds(field) {
                    self.sync_number_from_editor(field, min, max, ctx);
                }
                ctx.notify();
            }
            EditorEvent::Enter => self.save(ctx),
            EditorEvent::Escape => self.cancel(ctx),
            EditorEvent::Navigate(NavigationKey::Tab) => self.focus_next_editor(ctx),
            EditorEvent::Navigate(NavigationKey::ShiftTab) => self.focus_previous_editor(ctx),
            _ => {}
        }
    }

    fn current_editor_field(&self, ctx: &ViewContext<Self>) -> EditField {
        let focused = ctx
            .focused_view_id(ctx.window_id())
            .unwrap_or(self.name_editor.id());
        if focused == self.host_editor.id() {
            EditField::Host
        } else if focused == self.port_editor.id() {
            EditField::Port
        } else if focused == self.serial_baud_rate_editor.id() {
            EditField::SerialBaudRate
        } else if focused == self.username_editor.id() {
            EditField::Username
        } else if focused == self.password_editor.id() {
            EditField::Password
        } else if focused == self.private_key_editor.id() {
            EditField::PrivateKey
        } else if focused == self.key_passphrase_editor.id() {
            EditField::KeyPassphrase
        } else if focused == self.ca_cert_editor.id() {
            EditField::CaCert
        } else if focused == self.description_editor.id() {
            EditField::Description
        } else if focused == self.keep_alive_interval_editor.id() {
            EditField::KeepAliveInterval
        } else if focused == self.keep_alive_max_failures_editor.id() {
            EditField::KeepAliveMaxFailures
        } else if focused == self.tcp_connect_timeout_editor.id() {
            EditField::TcpConnectTimeout
        } else if focused == self.auth_timeout_editor.id() {
            EditField::AuthTimeout
        } else {
            EditField::Name
        }
    }

    fn focus_order(&self, ctx: &ViewContext<Self>) -> Vec<EditField> {
        let (protocol, is_key_auth) = ctx.read_model(&self.model, |m, _| {
            (
                m.draft.protocol.clone(),
                m.draft.auth_method.as_str() == "key",
            )
        });
        let is_ssh = protocol == "SSH";
        let is_rdp = protocol == "RDP";
        let mut fields = vec![EditField::Name, EditField::Host];

        if is_ssh {
            fields.push(EditField::Port);
            fields.push(EditField::Username);
            if is_key_auth {
                fields.push(EditField::PrivateKey);
                fields.push(EditField::KeyPassphrase);
                fields.push(EditField::CaCert);
            } else {
                fields.push(EditField::Password);
            }
        } else if is_rdp {
            fields.push(EditField::Port);
            fields.push(EditField::Username);
            fields.push(EditField::Password);
        } else {
            fields.push(EditField::SerialBaudRate);
        }

        fields.push(EditField::Description);
        if is_ssh {
            fields.push(EditField::KeepAliveInterval);
            fields.push(EditField::KeepAliveMaxFailures);
            fields.push(EditField::TcpConnectTimeout);
            fields.push(EditField::AuthTimeout);
        }
        fields
    }

    fn focus_next_editor(&self, ctx: &mut ViewContext<Self>) {
        let order = self.focus_order(ctx);
        let current = self.current_editor_field(ctx);
        let current_index = order
            .iter()
            .position(|field| *field == current)
            .unwrap_or(0);
        let next = order[(current_index + 1) % order.len()];
        let editor = self.editor_for_field(next);
        ctx.focus(editor);
        editor.update(ctx, |editor, ctx| editor.select_all(ctx));
    }

    fn focus_previous_editor(&self, ctx: &mut ViewContext<Self>) {
        let order = self.focus_order(ctx);
        let current = self.current_editor_field(ctx);
        let current_index = order
            .iter()
            .position(|field| *field == current)
            .unwrap_or(0);
        let previous = if current_index == 0 {
            *order.last().unwrap_or(&EditField::Name)
        } else {
            order[current_index - 1]
        };
        let editor = self.editor_for_field(previous);
        ctx.focus(editor);
        editor.update(ctx, |editor, ctx| editor.select_all(ctx));
    }

    fn save(&self, ctx: &mut ViewContext<Self>) {
        let name = self.name_editor.as_ref(ctx).buffer_text(ctx);
        let host = self.host_editor.as_ref(ctx).buffer_text(ctx);
        let port = self
            .parsed_port_from_editor(ctx)
            .unwrap_or_else(|| ctx.read_model(&self.model, |m, _| m.draft.port));
        let serial_baud_rate = self
            .parsed_serial_baud_rate_from_editor(ctx)
            .unwrap_or_else(|| ctx.read_model(&self.model, |m, _| m.draft.serial_baud_rate));
        let username = self.username_editor.as_ref(ctx).buffer_text(ctx);
        let password = self.password_editor.as_ref(ctx).buffer_text(ctx);
        let private_key = self.private_key_editor.as_ref(ctx).buffer_text(ctx);
        let key_passphrase = self.key_passphrase_editor.as_ref(ctx).buffer_text(ctx);
        let ca_cert = self.ca_cert_editor.as_ref(ctx).buffer_text(ctx);
        let description = self.description_editor.as_ref(ctx).buffer_text(ctx);
        let keep_alive_interval = self
            .parsed_number_from_editor(EditField::KeepAliveInterval, 10, 300, ctx)
            .unwrap_or_else(|| ctx.read_model(&self.model, |m, _| m.draft.keep_alive_interval));
        let keep_alive_max_failures = self
            .parsed_number_from_editor(EditField::KeepAliveMaxFailures, 1, 10, ctx)
            .unwrap_or_else(|| ctx.read_model(&self.model, |m, _| m.draft.keep_alive_max_failures));
        let tcp_connect_timeout = self
            .parsed_number_from_editor(EditField::TcpConnectTimeout, 5, 60, ctx)
            .unwrap_or_else(|| ctx.read_model(&self.model, |m, _| m.draft.tcp_connect_timeout));
        let auth_timeout = self
            .parsed_number_from_editor(EditField::AuthTimeout, 10, 120, ctx)
            .unwrap_or_else(|| ctx.read_model(&self.model, |m, _| m.draft.auth_timeout));

        // 净化连接关键字段：去首尾空白 + 去控制字符，防换行/NUL 注入进 SSH 连接
        let name = name.trim().to_string();
        let host = sanitize_conn_field(&host);
        let username = sanitize_conn_field(&username);

        ctx.update_model(&self.model, |m, ctx| {
            m.draft.name = name;
            m.draft.host = host;
            m.draft.port = port;
            m.draft.serial_baud_rate = serial_baud_rate;
            m.draft.username = username;
            m.draft.password = password;
            m.draft.private_key = private_key;
            m.draft.key_passphrase = key_passphrase;
            m.draft.ca_cert = ca_cert;
            m.draft.description = description;
            m.draft.keep_alive_interval = keep_alive_interval;
            m.draft.keep_alive_max_failures = keep_alive_max_failures;
            m.draft.tcp_connect_timeout = tcp_connect_timeout;
            m.draft.auth_timeout = auth_timeout;
            ctx.emit(HostEditEvent::Saved(m.draft.clone()));
        });
    }

    fn cancel(&self, ctx: &mut ViewContext<Self>) {
        ctx.update_model(&self.model, |_m, ctx| {
            ctx.emit(HostEditEvent::Cancelled);
        });
    }

    fn parsed_port_from_editor(&self, ctx: &ViewContext<Self>) -> Option<u16> {
        self.parsed_number_from_editor(EditField::Port, 1, u16::MAX, ctx)
    }

    fn parsed_serial_baud_rate_from_editor(&self, ctx: &ViewContext<Self>) -> Option<u32> {
        parse_bounded_u32_text(
            &self.serial_baud_rate_editor.as_ref(ctx).buffer_text(ctx),
            1,
            4_000_000,
        )
    }

    fn parsed_number_from_editor(
        &self,
        field: EditField,
        min: u16,
        max: u16,
        ctx: &ViewContext<Self>,
    ) -> Option<u16> {
        parse_bounded_number_text(
            &self.editor_for_field(field).as_ref(ctx).buffer_text(ctx),
            min,
            max,
        )
    }

    fn sync_number_from_editor(
        &self,
        field: EditField,
        min: u16,
        max: u16,
        ctx: &mut ViewContext<Self>,
    ) {
        let editor = self.editor_for_field(field);
        let text = editor.as_ref(ctx).buffer_text(ctx);
        let digits = digits_only(&text);
        if digits != text {
            editor.update(ctx, |editor, ctx| {
                editor.system_reset_buffer_text(&digits, ctx);
            });
            return;
        }

        if let Some(value) = parse_bounded_number_text(&digits, min, max) {
            ctx.update_model(&self.model, |m, ctx| {
                if set_draft_number_field(&mut m.draft, field, value) {
                    ctx.notify();
                }
            });
        }
    }

    fn sync_serial_baud_rate_from_editor(&self, ctx: &mut ViewContext<Self>) {
        let text = self.serial_baud_rate_editor.as_ref(ctx).buffer_text(ctx);
        let digits = digits_only(&text);
        if digits != text {
            self.serial_baud_rate_editor.update(ctx, |editor, ctx| {
                editor.system_reset_buffer_text(&digits, ctx);
            });
            return;
        }

        if let Some(value) = parse_bounded_u32_text(&digits, 1, 4_000_000) {
            ctx.update_model(&self.model, |m, ctx| {
                if m.draft.serial_baud_rate != value {
                    m.draft.serial_baud_rate = value;
                    ctx.notify();
                }
            });
        }
    }

    fn set_port_value(&self, port: u16, ctx: &mut ViewContext<Self>) {
        let text = port.to_string();
        ctx.update_model(&self.model, |m, ctx| {
            m.draft.port = port;
            ctx.notify();
        });
        self.port_editor.update(ctx, |editor, ctx| {
            if editor.buffer_text(ctx) != text {
                editor.system_reset_buffer_text(&text, ctx);
            }
        });
        ctx.notify();
    }

    fn set_serial_device_value(&self, device: &str, ctx: &mut ViewContext<Self>) {
        let text = device.to_string();
        ctx.update_model(&self.model, |m, ctx| {
            if m.draft.host != text {
                m.draft.host = text.clone();
                ctx.notify();
            }
        });
        self.host_editor.update(ctx, |editor, ctx| {
            if editor.buffer_text(ctx) != text {
                editor.system_reset_buffer_text(&text, ctx);
            }
        });
        ctx.notify();
    }

    fn set_serial_baud_rate_value(&self, baud_rate: u32, ctx: &mut ViewContext<Self>) {
        let text = baud_rate.to_string();
        ctx.update_model(&self.model, |m, ctx| {
            if m.draft.serial_baud_rate != baud_rate {
                m.draft.serial_baud_rate = baud_rate;
                ctx.notify();
            }
        });
        self.serial_baud_rate_editor.update(ctx, |editor, ctx| {
            if editor.buffer_text(ctx) != text {
                editor.system_reset_buffer_text(&text, ctx);
            }
        });
        ctx.notify();
    }

    fn current_port_value(&self, ctx: &ViewContext<Self>) -> u16 {
        self.parsed_port_from_editor(ctx)
            .unwrap_or_else(|| ctx.read_model(&self.model, |m, _| m.draft.port))
            .max(1)
    }
}

impl warpui::TypedActionView for HostEditView {
    type Action = HostEditAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            HostEditAction::SelectProtocol(target) => {
                self.states.borrow_mut().open_dropdown = None;
                let target = target.clone();
                // 切协议时按 SSH/RDP 默认端口互换（仅当端口仍是另一协议默认值），避免残留。
                let mut port_change: Option<u16> = None;
                let host_placeholder = ctx.update_model(&self.model, |m, ctx| {
                    if m.draft.protocol != target {
                        m.draft.protocol = target.clone();
                        m.draft.system = if target == "Serial" {
                            HostSystemIcon::Serial
                        } else {
                            HostSystemIcon::Terminal
                        };
                        if target == "Serial" && m.draft.host.trim().is_empty() {
                            if let Some(device) = available_serial_devices().into_iter().next() {
                                m.draft.host = device;
                            }
                        }
                        if target == "RDP" && m.draft.port == 22 {
                            port_change = Some(RDP_DEFAULT_PORT);
                        } else if target == "SSH" && m.draft.port == RDP_DEFAULT_PORT {
                            port_change = Some(22);
                        }
                        ctx.notify();
                    }
                    Self::host_placeholder(&m.draft.protocol)
                });
                if let Some(port) = port_change {
                    self.set_port_value(port, ctx);
                }
                let host_text = ctx.read_model(&self.model, |m, _| m.draft.host.clone());
                self.host_editor.update(ctx, |editor, ctx| {
                    editor.set_placeholder_text(&host_placeholder, ctx);
                    if editor.buffer_text(ctx) != host_text {
                        editor.system_reset_buffer_text(&host_text, ctx);
                    }
                });
                ctx.notify();
            }
            HostEditAction::SelectRdpQuality(quality) => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    if m.draft.rdp_display_quality != *quality {
                        m.draft.rdp_display_quality = *quality;
                        ctx.notify();
                    }
                });
                ctx.notify();
            }
            HostEditAction::SelectAuthMethod(method) => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    if matches!(method.as_str(), "password" | "key")
                        && m.draft.auth_method != *method
                    {
                        m.draft.auth_method = method.clone();
                        ctx.notify();
                    }
                });
                ctx.notify();
            }
            HostEditAction::ToggleAdvancedSettings => {
                let mut states = self.states.borrow_mut();
                states.advanced_settings_expanded = !states.advanced_settings_expanded;
                if !states.advanced_settings_expanded {
                    states.open_dropdown = None;
                }
                ctx.notify();
            }
            HostEditAction::ToggleKeepAlive => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    m.draft.keep_alive_enabled = !m.draft.keep_alive_enabled;
                    ctx.notify();
                });
            }
            HostEditAction::ToggleDropdown(kind) => {
                let mut states = self.states.borrow_mut();
                states.open_dropdown = if states.open_dropdown == Some(*kind) {
                    None
                } else {
                    Some(*kind)
                };
                ctx.notify();
            }
            HostEditAction::SelectGroup(group_id) => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    if m.draft.group_id != *group_id {
                        m.draft.group_id = group_id.clone();
                        ctx.notify();
                    }
                });
                ctx.notify();
            }
            HostEditAction::SelectKey(key_id) => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    if m.draft.key_id != *key_id {
                        m.draft.key_id = key_id.clone();
                        ctx.notify();
                    }
                });
                ctx.notify();
            }
            HostEditAction::SelectSerialDevice(device) => {
                self.states.borrow_mut().open_dropdown = None;
                self.set_serial_device_value(device, ctx);
            }
            HostEditAction::SelectSerialBaudRate(baud_rate) => {
                self.states.borrow_mut().open_dropdown = None;
                self.set_serial_baud_rate_value(*baud_rate, ctx);
            }
            HostEditAction::SelectEncoding(encoding) => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    if m.draft.term_encoding != *encoding {
                        m.draft.term_encoding = encoding.clone();
                        ctx.notify();
                    }
                });
                ctx.notify();
            }
            HostEditAction::SelectSerialDataBits(data_bits) => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    if m.draft.serial_data_bits != *data_bits {
                        m.draft.serial_data_bits = *data_bits;
                        ctx.notify();
                    }
                });
                ctx.notify();
            }
            HostEditAction::SelectSerialStopBits(stop_bits) => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    if m.draft.serial_stop_bits != *stop_bits {
                        m.draft.serial_stop_bits = *stop_bits;
                        ctx.notify();
                    }
                });
                ctx.notify();
            }
            HostEditAction::SelectSerialParity(parity) => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    if m.draft.serial_parity != *parity {
                        m.draft.serial_parity = parity.clone();
                        ctx.notify();
                    }
                });
                ctx.notify();
            }
            HostEditAction::SelectSerialFlowControl(flow_control) => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    if m.draft.serial_flow_control != *flow_control {
                        m.draft.serial_flow_control = flow_control.clone();
                        ctx.notify();
                    }
                });
                ctx.notify();
            }
            HostEditAction::ToggleSerialDtr => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    m.draft.serial_dtr = !m.draft.serial_dtr;
                    ctx.notify();
                });
            }
            HostEditAction::ToggleSerialRts => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    m.draft.serial_rts = !m.draft.serial_rts;
                    ctx.notify();
                });
            }
            HostEditAction::ToggleTag(tag) => {
                self.states.borrow_mut().open_dropdown = None;
                ctx.update_model(&self.model, |m, ctx| {
                    if let Some(index) = m.draft.tags.iter().position(|item| item == tag) {
                        m.draft.tags.remove(index);
                    } else {
                        m.draft.tags.push(tag.clone());
                        m.draft.tags.sort();
                    }
                    ctx.notify();
                });
            }
            HostEditAction::TogglePasswordVisibility => {
                self.password_editor.update(ctx, |editor, ctx| {
                    editor.set_is_password(!editor.is_password());
                    ctx.notify();
                });
                ctx.notify();
            }
            HostEditAction::IncrementPort => {
                self.states.borrow_mut().open_dropdown = None;
                let next = self.current_port_value(ctx).saturating_add(1).max(1);
                self.set_port_value(next, ctx);
            }
            HostEditAction::DecrementPort => {
                self.states.borrow_mut().open_dropdown = None;
                let next = self.current_port_value(ctx).saturating_sub(1).max(1);
                self.set_port_value(next, ctx);
            }
            HostEditAction::Save => {
                self.save(ctx);
            }
            HostEditAction::Cancel => {
                self.cancel(ctx);
            }
        }
    }
}

// ── 渲染函数 ──

const TEXT_FIELD_HEIGHT: f32 = 38.0;
const SETTINGS_TEXT_FIELD_HEIGHT: f32 = TEXT_FIELD_HEIGHT;

fn digits_only(text: &str) -> String {
    text.chars().filter(|ch| ch.is_ascii_digit()).collect()
}

fn parse_bounded_number_text(text: &str, min: u16, max: u16) -> Option<u16> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let value = trimmed.parse::<u32>().ok()?;
    Some(value.clamp(u32::from(min), u32::from(max)) as u16)
}

fn parse_bounded_u32_text(text: &str, min: u32, max: u32) -> Option<u32> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let value = trimmed.parse::<u32>().ok()?;
    Some(value.clamp(min, max))
}

fn known_serial_baud_rates() -> Vec<u32> {
    vec![
        300, 1_200, 2_400, 4_800, 9_600, 19_200, 38_400, 57_600, 115_200, 230_400, 460_800, 921_600,
    ]
}

fn available_serial_devices() -> Vec<String> {
    let Ok(ports) = serialport::available_ports() else {
        return Vec::new();
    };
    normalize_serial_device_names(ports.into_iter().map(|port| port.port_name))
}

fn normalize_serial_device_names<I>(names: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut devices = BTreeSet::new();
    for name in names {
        let device = name.trim();
        if !device.is_empty() && is_serial_device_name(device) {
            devices.insert(device.to_string());
        }
    }
    devices.into_iter().collect()
}

fn is_serial_device_name(device: &str) -> bool {
    let name = device
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(device);
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "cu.bluetooth-incoming-port" | "tty.bluetooth-incoming-port" | "cu.debug-console"
    ) {
        return false;
    }

    lower.starts_with("cu.")
        || lower.starts_with("ttyusb")
        || lower.starts_with("ttyacm")
        || lower.starts_with("ttyama")
        || lower.starts_with("ttys")
        || lower.starts_with("rfcomm")
        || is_windows_com_port_name(&lower)
}

fn is_windows_com_port_name(name: &str) -> bool {
    name.len() > 3 && name.starts_with("com") && name[3..].chars().all(|ch| ch.is_ascii_digit())
}

fn number_bounds(field: EditField) -> Option<(u16, u16)> {
    match field {
        EditField::Port => Some((1, u16::MAX)),
        EditField::KeepAliveInterval => Some((10, 300)),
        EditField::KeepAliveMaxFailures => Some((1, 10)),
        EditField::TcpConnectTimeout => Some((5, 60)),
        EditField::AuthTimeout => Some((10, 120)),
        _ => None,
    }
}

fn set_draft_number_field(draft: &mut HostEditDraft, field: EditField, value: u16) -> bool {
    let slot = match field {
        EditField::Port => &mut draft.port,
        EditField::KeepAliveInterval => &mut draft.keep_alive_interval,
        EditField::KeepAliveMaxFailures => &mut draft.keep_alive_max_failures,
        EditField::TcpConnectTimeout => &mut draft.tcp_connect_timeout,
        EditField::AuthTimeout => &mut draft.auth_timeout,
        _ => return false,
    };
    if *slot == value {
        return false;
    }
    *slot = value;
    true
}

fn render_header(
    title: &str,
    close_state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let close_state = close_state.clone();
    let hc = *hc;

    let close_btn = Hoverable::new(close_state, move |mouse| {
        let color = if mouse.is_hovered() {
            hc.text_primary
        } else {
            hc.text_secondary
        };
        ConstrainedBox::new(Icon::new("icons/close.svg", color).finish())
            .with_width(16.0)
            .with_height(16.0)
            .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::Cancel);
    })
    .finish();

    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(title.to_string(), ui_font, TOOLBAR_TITLE_SIZE)
                    .with_color(hc.text_primary)
                    .finish(),
            )
            .with_child(Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish())
            .with_child(close_btn)
            .finish(),
    )
    .with_horizontal_padding(20.0)
    .with_vertical_padding(14.0)
    .with_background_color(hc.toolbar_bg)
    .with_border(Border::bottom(1.0).with_border_color(hc.toolbar_border))
    .finish()
}

fn render_form(
    draft: &HostEditDraft,
    group_options: &[(String, String)],
    key_options: &[(String, String)],
    available_tags: &[String],
    states: &FieldStates,
    ui_font: fonts::FamilyId,
    view: &HostEditView,
    appearance: &Appearance,
    hc: &HostUiColors,
    password_visible: bool,
) -> Box<dyn Element> {
    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    col.add_child(render_field_label(
        &rust_i18n::t!("form_protocol"),
        ui_font,
        hc,
    ));
    col.add_child(render_protocol_toggle(&draft.protocol, states, ui_font, hc));

    col.add_child(render_field_label(&rust_i18n::t!("form_name"), ui_font, hc));
    col.add_child(render_text_field(&view.name_editor, appearance));

    if draft.protocol == "RDP" {
        col.add_child(render_field_label(
            &rust_i18n::t!("form_host_address"),
            ui_font,
            hc,
        ));
        col.add_child(render_text_field(&view.host_editor, appearance));

        col.add_child(render_field_label(&rust_i18n::t!("form_port"), ui_font, hc));
        col.add_child(render_port_stepper(
            draft.port,
            &view.port_editor,
            states,
            ui_font,
            appearance,
            hc,
        ));

        col.add_child(render_field_label(
            &rust_i18n::t!("form_username"),
            ui_font,
            hc,
        ));
        col.add_child(render_text_field(&view.username_editor, appearance));

        col.add_child(render_field_label(
            &rust_i18n::t!("form_password"),
            ui_font,
            hc,
        ));
        col.add_child(render_password_field(
            &view.password_editor,
            &states.password_eye_state,
            password_visible,
            appearance,
            hc,
        ));

        col.add_child(render_field_label(
            &rust_i18n::t!("form_display_quality"),
            ui_font,
            hc,
        ));
        col.add_child(render_rdp_quality_toggle(
            draft.rdp_display_quality,
            &states.rdp_quality_standard_state,
            &states.rdp_quality_hidpi_state,
            ui_font,
            hc,
        ));
    } else if draft.protocol == "SSH" {
        col.add_child(render_field_label(
            &rust_i18n::t!("form_host_address"),
            ui_font,
            hc,
        ));
        col.add_child(render_text_field(&view.host_editor, appearance));

        col.add_child(render_field_label(&rust_i18n::t!("form_port"), ui_font, hc));
        col.add_child(render_port_stepper(
            draft.port,
            &view.port_editor,
            states,
            ui_font,
            appearance,
            hc,
        ));

        col.add_child(render_field_label(
            &rust_i18n::t!("form_username"),
            ui_font,
            hc,
        ));
        col.add_child(render_text_field(&view.username_editor, appearance));

        col.add_child(render_field_label(
            &rust_i18n::t!("form_auth_method"),
            ui_font,
            hc,
        ));
        col.add_child(render_auth_method_toggle(
            &draft.auth_method,
            &states.auth_password_state,
            &states.auth_key_state,
            ui_font,
            hc,
        ));

        if draft.auth_method == "key" {
            col.add_child(render_field_label("私钥", ui_font, hc));
            col.add_child(render_dropdown_select_field(
                HostDropdownKind::Key,
                key_label(draft.key_id.as_deref(), key_options),
                &states.key_state,
                states.open_dropdown == Some(HostDropdownKind::Key),
                key_dropdown_options(draft.key_id.as_deref(), key_options, states),
                ui_font,
                appearance,
                480.0,
            ));

            col.add_child(render_field_label(
                &rust_i18n::t!("form_certificate"),
                ui_font,
                hc,
            ));
            col.add_child(render_text_field(&view.ca_cert_editor, appearance));
        } else {
            col.add_child(render_field_label(
                &rust_i18n::t!("form_password"),
                ui_font,
                hc,
            ));
            col.add_child(render_password_field(
                &view.password_editor,
                &states.password_eye_state,
                password_visible,
                appearance,
                hc,
            ));
        }
    } else {
        col.add_child(render_field_label(
            &rust_i18n::t!("form_serial_device"),
            ui_font,
            hc,
        ));
        col.add_child(render_text_field_with_dropdown(
            HostDropdownKind::SerialDevice,
            &view.host_editor,
            &states.serial_device_state,
            states.open_dropdown == Some(HostDropdownKind::SerialDevice),
            serial_device_dropdown_options(&draft.host, states),
            appearance,
            480.0,
            hc,
        ));

        col.add_child(render_field_label(
            &rust_i18n::t!("form_baud_rate"),
            ui_font,
            hc,
        ));
        col.add_child(render_text_field_with_dropdown(
            HostDropdownKind::SerialBaudRate,
            &view.serial_baud_rate_editor,
            &states.serial_baud_rate_state,
            states.open_dropdown == Some(HostDropdownKind::SerialBaudRate),
            serial_baud_rate_dropdown_options(draft.serial_baud_rate, states),
            appearance,
            260.0,
            hc,
        ));
    }

    col.add_child(render_field_label(
        &rust_i18n::t!("form_description"),
        ui_font,
        hc,
    ));
    col.add_child(render_text_field(&view.description_editor, appearance));

    col.add_child(render_field_label(
        &rust_i18n::t!("form_group"),
        ui_font,
        hc,
    ));
    col.add_child(render_dropdown_select_field(
        HostDropdownKind::Group,
        group_label(draft.group_id.as_deref(), group_options),
        &states.group_state,
        states.open_dropdown == Some(HostDropdownKind::Group),
        group_dropdown_options(draft.group_id.as_deref(), group_options, states),
        ui_font,
        appearance,
        480.0,
    ));

    col.add_child(render_field_label(&rust_i18n::t!("form_tags"), ui_font, hc));
    col.add_child(render_tag_box(
        available_tags,
        &draft.tags,
        &states.tag_item_states,
        ui_font,
        hc,
    ));

    // 高级设置（keep-alive/超时/编码）是 SSH/串口概念，RDP 不展示。
    if draft.protocol != "RDP" {
        col.add_child(render_advanced_settings(
            draft, states, ui_font, view, appearance, hc,
        ));
    }

    Container::new(col.finish())
        .with_uniform_padding(20.0)
        .finish()
}

fn render_field_label(
    label: &str,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Container::new(
        Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE)
            .with_color(hc.text_secondary)
            .finish(),
    )
    .with_margin_top(16.0)
    .with_margin_bottom(6.0)
    .finish()
}

fn group_label(group_id: Option<&str>, group_options: &[(String, String)]) -> String {
    let Some(group_id) = group_id else {
        return rust_i18n::t!("form_no_group").to_string();
    };
    group_options
        .iter()
        .find_map(|(id, label)| (id == group_id).then(|| label.clone()))
        .unwrap_or_else(|| rust_i18n::t!("form_no_group").to_string())
}

fn render_auth_method_toggle(
    method: &str,
    password_state: &MouseStateHandle,
    key_state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let password_state = password_state.clone();
    let key_state = key_state.clone();
    let hc = *hc;
    let is_key = method == "key";
    let password_bg = if !is_key {
        hc.badge_ssh_bg
    } else {
        hc.panel_bg
    };
    let key_bg = if is_key { hc.badge_ssh_bg } else { hc.panel_bg };
    let password_border = if !is_key {
        hc.text_accent
    } else {
        hc.card_border
    };
    let key_border = if is_key {
        hc.text_accent
    } else {
        hc.card_border
    };

    let password_button = Hoverable::new(password_state, move |_mouse| {
        ConstrainedBox::new(
            Container::new(
                Align::new(
                    Text::new_inline(
                        rust_i18n::t!("form_password_auth").to_string(),
                        ui_font,
                        UI_FONT_SIZE,
                    )
                    .with_color(hc.text_primary)
                    .finish(),
                )
                .finish(),
            )
            .with_background_color(password_bg)
            .with_border(Border::all(1.0).with_border_color(password_border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
            .finish(),
        )
        .with_height(TEXT_FIELD_HEIGHT)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::SelectAuthMethod("password".to_string()));
    })
    .finish();

    let key_button = Hoverable::new(key_state, move |_mouse| {
        ConstrainedBox::new(
            Container::new(
                Align::new(
                    Text::new_inline(
                        rust_i18n::t!("form_key_auth").to_string(),
                        ui_font,
                        UI_FONT_SIZE,
                    )
                    .with_color(hc.text_primary)
                    .finish(),
                )
                .finish(),
            )
            .with_background_color(key_bg)
            .with_border(Border::all(1.0).with_border_color(key_border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
            .finish(),
        )
        .with_height(TEXT_FIELD_HEIGHT)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::SelectAuthMethod("key".to_string()));
    })
    .finish();

    ConstrainedBox::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                Expanded::new(
                    1.0,
                    Container::new(password_button)
                        .with_margin_right(8.0)
                        .finish(),
                )
                .finish(),
            )
            .with_child(Expanded::new(1.0, key_button).finish())
            .finish(),
    )
    .with_height(TEXT_FIELD_HEIGHT)
    .finish()
}

fn render_dropdown_select_field(
    kind: HostDropdownKind,
    label: String,
    state: &MouseStateHandle,
    is_open: bool,
    options: Vec<WarpDropdownOption<HostEditAction>>,
    _ui_font: fonts::FamilyId,
    appearance: &Appearance,
    menu_width: f32,
) -> Box<dyn Element> {
    render_warp_dropdown(WarpDropdownProps {
        position_id: dropdown_position_id(kind),
        label,
        state,
        is_open,
        options,
        toggle_action: HostEditAction::ToggleDropdown(kind),
        appearance,
        menu_width,
        top_bar_height: TEXT_FIELD_HEIGHT,
    })
}

fn dropdown_position_id(kind: HostDropdownKind) -> &'static str {
    match kind {
        HostDropdownKind::Group => "host_edit_group_dropdown_top_bar",
        HostDropdownKind::Key => "host_edit_key_dropdown_top_bar",
        HostDropdownKind::SerialDevice => "host_edit_serial_device_dropdown_top_bar",
        HostDropdownKind::SerialBaudRate => "host_edit_serial_baud_dropdown_top_bar",
        HostDropdownKind::Encoding => "host_edit_encoding_dropdown_top_bar",
        HostDropdownKind::SerialDataBits => "host_edit_serial_data_bits_dropdown_top_bar",
        HostDropdownKind::SerialStopBits => "host_edit_serial_stop_bits_dropdown_top_bar",
        HostDropdownKind::SerialParity => "host_edit_serial_parity_dropdown_top_bar",
        HostDropdownKind::SerialFlowControl => "host_edit_serial_flow_dropdown_top_bar",
    }
}

fn dropdown_item_state(
    states: &RefCell<BTreeMap<String, MouseStateHandle>>,
    key: impl Into<String>,
) -> MouseStateHandle {
    let key = key.into();
    states
        .borrow_mut()
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
        .clone()
}

fn group_dropdown_options(
    current_group_id: Option<&str>,
    group_options: &[(String, String)],
    states: &FieldStates,
) -> Vec<WarpDropdownOption<HostEditAction>> {
    let mut options = Vec::with_capacity(group_options.len() + 1);
    options.push(WarpDropdownOption {
        label: rust_i18n::t!("form_no_group").to_string(),
        action: HostEditAction::SelectGroup(None),
        selected: current_group_id.is_none(),
        state: dropdown_item_state(&states.group_item_states, "__none__"),
        icon_path: None,
        shortcut: None,
    });
    options.extend(group_options.iter().map(|(id, label)| WarpDropdownOption {
        label: label.clone(),
        action: HostEditAction::SelectGroup(Some(id.clone())),
        selected: current_group_id == Some(id.as_str()),
        state: dropdown_item_state(&states.group_item_states, id.clone()),
        icon_path: None,
        shortcut: None,
    }));
    options
}

fn key_label(key_id: Option<&str>, key_options: &[(String, String)]) -> String {
    let Some(key_id) = key_id else {
        return "未选择密钥".to_string();
    };
    key_options
        .iter()
        .find_map(|(id, label)| (id == key_id).then(|| label.clone()))
        .unwrap_or_else(|| "未选择密钥".to_string())
}

fn key_dropdown_options(
    current_key_id: Option<&str>,
    key_options: &[(String, String)],
    states: &FieldStates,
) -> Vec<WarpDropdownOption<HostEditAction>> {
    let mut options = Vec::with_capacity(key_options.len() + 1);
    options.push(WarpDropdownOption {
        label: "未选择密钥".to_string(),
        action: HostEditAction::SelectKey(None),
        selected: current_key_id.is_none(),
        state: dropdown_item_state(&states.key_item_states, "__none__"),
        icon_path: None,
        shortcut: None,
    });
    options.extend(key_options.iter().map(|(id, label)| WarpDropdownOption {
        label: label.clone(),
        action: HostEditAction::SelectKey(Some(id.clone())),
        selected: current_key_id == Some(id.as_str()),
        state: dropdown_item_state(&states.key_item_states, id.clone()),
        icon_path: None,
        shortcut: None,
    }));
    options
}

fn serial_device_dropdown_options(
    current_device: &str,
    states: &FieldStates,
) -> Vec<WarpDropdownOption<HostEditAction>> {
    let current_device = current_device.trim();
    let mut devices = available_serial_devices();
    if !current_device.is_empty() && !devices.iter().any(|device| device == current_device) {
        devices.insert(0, current_device.to_string());
    }

    if devices.is_empty() {
        return vec![WarpDropdownOption {
            label: rust_i18n::t!("form_no_serial").to_string(),
            action: HostEditAction::SelectSerialDevice(current_device.to_string()),
            selected: false,
            state: dropdown_item_state(&states.serial_device_item_states, "__empty__"),
            icon_path: None,
            shortcut: None,
        }];
    }

    devices
        .into_iter()
        .map(|device| WarpDropdownOption {
            label: device.clone(),
            action: HostEditAction::SelectSerialDevice(device.clone()),
            selected: device == current_device,
            state: dropdown_item_state(&states.serial_device_item_states, device),
            icon_path: None,
            shortcut: None,
        })
        .collect()
}

fn serial_baud_rate_dropdown_options(
    current: u32,
    states: &FieldStates,
) -> Vec<WarpDropdownOption<HostEditAction>> {
    let mut rates = known_serial_baud_rates();
    if current > 0 && !rates.contains(&current) {
        rates.insert(0, current);
    }

    rates
        .into_iter()
        .map(|rate| WarpDropdownOption {
            label: rate.to_string(),
            action: HostEditAction::SelectSerialBaudRate(rate),
            selected: current == rate,
            state: dropdown_item_state(&states.serial_baud_rate_item_states, rate.to_string()),
            icon_path: None,
            shortcut: None,
        })
        .collect()
}

fn encoding_dropdown_options(
    current_encoding: &str,
    states: &FieldStates,
) -> Vec<WarpDropdownOption<HostEditAction>> {
    ["utf-8", "gbk", "big5", "shift_jis"]
        .into_iter()
        .map(|encoding| WarpDropdownOption {
            label: encoding_label(encoding),
            action: HostEditAction::SelectEncoding(encoding.to_string()),
            selected: current_encoding.eq_ignore_ascii_case(encoding),
            state: dropdown_item_state(&states.encoding_item_states, encoding),
            icon_path: None,
            shortcut: None,
        })
        .collect()
}

fn serial_data_bits_dropdown_options(
    current: u8,
    states: &FieldStates,
) -> Vec<WarpDropdownOption<HostEditAction>> {
    [5_u8, 6, 7, 8]
        .into_iter()
        .map(|data_bits| WarpDropdownOption {
            label: serial_data_bits_label(data_bits),
            action: HostEditAction::SelectSerialDataBits(data_bits),
            selected: current == data_bits,
            state: dropdown_item_state(&states.serial_data_bits_item_states, data_bits.to_string()),
            icon_path: None,
            shortcut: None,
        })
        .collect()
}

fn serial_stop_bits_dropdown_options(
    current: u8,
    states: &FieldStates,
) -> Vec<WarpDropdownOption<HostEditAction>> {
    [1_u8, 2]
        .into_iter()
        .map(|stop_bits| WarpDropdownOption {
            label: serial_stop_bits_label(stop_bits),
            action: HostEditAction::SelectSerialStopBits(stop_bits),
            selected: current == stop_bits,
            state: dropdown_item_state(&states.serial_stop_bits_item_states, stop_bits.to_string()),
            icon_path: None,
            shortcut: None,
        })
        .collect()
}

fn serial_parity_dropdown_options(
    current: &str,
    states: &FieldStates,
) -> Vec<WarpDropdownOption<HostEditAction>> {
    ["none", "odd", "even"]
        .into_iter()
        .map(|parity| WarpDropdownOption {
            label: serial_parity_label(parity),
            action: HostEditAction::SelectSerialParity(parity.to_string()),
            selected: current.eq_ignore_ascii_case(parity),
            state: dropdown_item_state(&states.serial_parity_item_states, parity),
            icon_path: None,
            shortcut: None,
        })
        .collect()
}

fn serial_flow_control_dropdown_options(
    current: &str,
    states: &FieldStates,
) -> Vec<WarpDropdownOption<HostEditAction>> {
    ["none", "hardware", "software"]
        .into_iter()
        .map(|flow_control| WarpDropdownOption {
            label: serial_flow_control_label(flow_control),
            action: HostEditAction::SelectSerialFlowControl(flow_control.to_string()),
            selected: current.eq_ignore_ascii_case(flow_control),
            state: dropdown_item_state(&states.serial_flow_control_item_states, flow_control),
            icon_path: None,
            shortcut: None,
        })
        .collect()
}

fn render_tag_box(
    available_tags: &[String],
    selected_tags: &[String],
    tag_states: &RefCell<BTreeMap<String, MouseStateHandle>>,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let mut tags: Vec<String> = available_tags.to_vec();
    for selected in selected_tags {
        if !tags.iter().any(|tag| tag == selected) {
            tags.push(selected.clone());
        }
    }
    tags.sort();

    if tags.is_empty() {
        return Container::new(
            Text::new_inline(rust_i18n::t!("form_no_tags").to_string(), ui_font, 13.0)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_horizontal_padding(10.0)
        .with_vertical_padding(8.0)
        .with_background_color(hc.search_bar_bg)
        .with_border(Border::all(1.0).with_border_color(hc.card_border))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
        .finish();
    }

    let mut wrap = Wrap::row().with_spacing(4.0).with_run_spacing(4.0);
    let mut states = tag_states.borrow_mut();
    for tag in tags {
        let state = states
            .entry(tag.clone())
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone();
        let selected = selected_tags.iter().any(|item| item == &tag);
        wrap.extend(std::iter::once(render_tag_chip(
            tag, selected, state, ui_font, hc,
        )));
    }

    Container::new(wrap.finish())
        .with_horizontal_padding(8.0)
        .with_vertical_padding(6.0)
        .with_background_color(hc.search_bar_bg)
        .with_border(Border::all(1.0).with_border_color(hc.card_border))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
        .finish()
}

fn render_tag_chip(
    tag: String,
    selected: bool,
    state: MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let tag_for_render = tag.clone();
    let tag_for_action = tag;
    let hc = *hc;
    Hoverable::new(state, move |mouse| {
        let (bg, border_color, dot_color, text_color) = if selected {
            (hc.tag_bg, hc.accent_bg, hc.accent_bg, hc.text_primary)
        } else if mouse.is_hovered() {
            (
                hc.card_bg_hover,
                hc.card_border,
                hc.text_secondary,
                hc.text_secondary,
            )
        } else {
            (
                ColorU::transparent_black(),
                hc.card_border,
                hc.text_secondary,
                hc.text_secondary,
            )
        };

        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Text::new_inline(
                        if selected { "✓" } else { "●" }.to_string(),
                        ui_font,
                        if selected { 11.0 } else { 9.0 },
                    )
                    .with_color(dot_color)
                    .finish(),
                )
                .with_child(
                    Container::new(
                        Text::new_inline(tag_for_render.clone(), ui_font, 13.0)
                            .with_color(text_color)
                            .finish(),
                    )
                    .with_margin_left(6.0)
                    .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(10.0)
        .with_vertical_padding(4.0)
        .with_margin_right(8.0)
        .with_background_color(bg)
        .with_border(Border::all(1.0).with_border_color(border_color))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::ToggleTag(tag_for_action.clone()));
    })
    .finish()
}

fn render_text_field(editor: &ViewHandle<EditorView>, appearance: &Appearance) -> Box<dyn Element> {
    let input = appearance
        .ui_builder()
        .text_input(editor.clone())
        .with_style(UiComponentStyles {
            background: Some(appearance.theme().background().into()),
            font_color: Some(
                appearance
                    .theme()
                    .main_text_color(appearance.theme().background())
                    .into_solid(),
            ),
            height: Some(TEXT_FIELD_HEIGHT),
            ..Default::default()
        })
        .build()
        .finish();

    ConstrainedBox::new(Stack::new().with_child(input).finish())
        .with_height(TEXT_FIELD_HEIGHT)
        .finish()
}

fn render_password_field(
    editor: &ViewHandle<EditorView>,
    eye_state: &MouseStateHandle,
    password_visible: bool,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let input = appearance
        .ui_builder()
        .text_input(editor.clone())
        .with_style(UiComponentStyles {
            background: Some(appearance.theme().background().into()),
            font_color: Some(
                appearance
                    .theme()
                    .main_text_color(appearance.theme().background())
                    .into_solid(),
            ),
            height: Some(TEXT_FIELD_HEIGHT),
            padding: Some(Coords {
                top: 10.0,
                bottom: 10.0,
                left: 10.0,
                right: 38.0,
            }),
            ..Default::default()
        })
        .build()
        .finish();

    let icon_path = if password_visible {
        ICON_EYE
    } else {
        ICON_EYE_OFF
    };
    let hc = *hc;
    let eye_state = eye_state.clone();
    let eye_btn = Hoverable::new(eye_state, move |mouse| {
        let color = if mouse.is_hovered() {
            hc.text_primary
        } else {
            hc.text_secondary
        };
        ConstrainedBox::new(
            Align::new(
                ConstrainedBox::new(Icon::new(icon_path, color).finish())
                    .with_width(16.0)
                    .with_height(16.0)
                    .finish(),
            )
            .finish(),
        )
        .with_width(TEXT_FIELD_HEIGHT)
        .with_height(TEXT_FIELD_HEIGHT)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::TogglePasswordVisibility);
    })
    .finish();

    ConstrainedBox::new(
        Stack::new()
            .with_child(input)
            .with_child(Align::new(eye_btn).right().finish())
            .finish(),
    )
    .with_height(TEXT_FIELD_HEIGHT)
    .finish()
}

fn render_text_field_with_dropdown(
    kind: HostDropdownKind,
    editor: &ViewHandle<EditorView>,
    state: &MouseStateHandle,
    is_open: bool,
    options: Vec<WarpDropdownOption<HostEditAction>>,
    appearance: &Appearance,
    menu_width: f32,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let input = appearance
        .ui_builder()
        .text_input(editor.clone())
        .with_style(UiComponentStyles {
            background: Some(appearance.theme().background().into()),
            font_color: Some(
                appearance
                    .theme()
                    .main_text_color(appearance.theme().background())
                    .into_solid(),
            ),
            height: Some(TEXT_FIELD_HEIGHT),
            padding: Some(Coords {
                top: 10.0,
                bottom: 10.0,
                left: 10.0,
                right: 42.0,
            }),
            ..Default::default()
        })
        .build()
        .finish();

    let top_bar = ConstrainedBox::new(
        Stack::new()
            .with_child(input)
            .with_child(
                Align::new(render_dropdown_trigger(kind, state, appearance, hc))
                    .right()
                    .finish(),
            )
            .finish(),
    )
    .with_height(TEXT_FIELD_HEIGHT)
    .finish();

    render_warp_dropdown_with_top_bar(WarpDropdownCustomProps {
        position_id: dropdown_position_id(kind),
        top_bar,
        is_open,
        options,
        appearance,
        menu_width,
        top_bar_height: TEXT_FIELD_HEIGHT,
    })
}

fn render_dropdown_trigger(
    kind: HostDropdownKind,
    state: &MouseStateHandle,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let state = state.clone();
    let icon_path = "icons/chevron-down.svg";
    let icon_color = appearance.theme().active_ui_text_color().into_solid();
    let hc = *hc;

    Hoverable::new(state, move |mouse| {
        let color = if mouse.is_hovered() {
            hc.text_primary
        } else {
            icon_color
        };
        ConstrainedBox::new(
            Align::new(
                ConstrainedBox::new(Icon::new(icon_path, color).finish())
                    .with_width(15.0)
                    .with_height(15.0)
                    .finish(),
            )
            .finish(),
        )
        .with_width(TEXT_FIELD_HEIGHT)
        .with_height(TEXT_FIELD_HEIGHT)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_mouse_down(move |ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::ToggleDropdown(kind));
    })
    .finish()
}

fn render_inline_number_field(
    value: u16,
    editor: &ViewHandle<EditorView>,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let digit_width = (value.to_string().chars().count() as f32 * 8.0 + 4.0).max(24.0);
    let input = appearance
        .ui_builder()
        .text_input(editor.clone())
        .with_style(UiComponentStyles {
            background: Some(hc.search_bar_bg.into()),
            border_width: Some(0.0),
            font_color: Some(hc.text_primary),
            height: Some(TEXT_FIELD_HEIGHT),
            width: Some(digit_width),
            padding: Some(Coords::uniform(0.0).top(10.0).bottom(10.0)),
            ..Default::default()
        })
        .build()
        .finish();

    ConstrainedBox::new(Align::new(Stack::new().with_child(input).finish()).finish())
        .with_height(TEXT_FIELD_HEIGHT)
        .finish()
}

fn render_protocol_toggle(
    protocol: &str,
    states: &FieldStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    ConstrainedBox::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                Expanded::new(
                    1.0,
                    Container::new(protocol_segment(
                        "SSH",
                        protocol == "SSH",
                        &states.protocol_ssh_state,
                        ui_font,
                        hc,
                    ))
                    .with_margin_right(8.0)
                    .finish(),
                )
                .finish(),
            )
            .with_child(
                Expanded::new(
                    1.0,
                    Container::new(protocol_segment(
                        "RDP",
                        protocol == "RDP",
                        &states.protocol_rdp_state,
                        ui_font,
                        hc,
                    ))
                    .with_margin_right(8.0)
                    .finish(),
                )
                .finish(),
            )
            .with_child(
                Expanded::new(
                    1.0,
                    protocol_segment(
                        "Serial",
                        protocol == "Serial",
                        &states.protocol_serial_state,
                        ui_font,
                        hc,
                    ),
                )
                .finish(),
            )
            .finish(),
    )
    .with_height(TEXT_FIELD_HEIGHT)
    .finish()
}

// 协议单段按钮：选中态复用认证方式切换的高亮样式。
fn protocol_segment(
    label: &str,
    selected: bool,
    state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let state = state.clone();
    let hc = *hc;
    let target = label.to_string();
    let label = label.to_string();
    let (bg, border) = if selected {
        (hc.badge_ssh_bg, hc.text_accent)
    } else {
        (hc.panel_bg, hc.card_border)
    };

    Hoverable::new(state, move |_mouse| {
        ConstrainedBox::new(
            Container::new(
                Align::new(
                    Text::new_inline(label.clone(), ui_font, UI_FONT_SIZE)
                        .with_color(hc.text_primary)
                        .finish(),
                )
                .finish(),
            )
            .with_background_color(bg)
            .with_border(Border::all(1.0).with_border_color(border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
            .finish(),
        )
        .with_height(TEXT_FIELD_HEIGHT)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::SelectProtocol(target.clone()));
    })
    .finish()
}

// RDP 显示质量二选一：标准 / 高清 HiDPI。
fn render_rdp_quality_toggle(
    current: RdpDisplayQuality,
    standard_state: &MouseStateHandle,
    hidpi_state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let standard = rdp_quality_segment(
        &rust_i18n::t!("form_display_quality_standard"),
        RdpDisplayQuality::Standard,
        current == RdpDisplayQuality::Standard,
        standard_state,
        ui_font,
        hc,
    );
    let hidpi = rdp_quality_segment(
        &rust_i18n::t!("form_display_quality_hidpi"),
        RdpDisplayQuality::Hidpi,
        current == RdpDisplayQuality::Hidpi,
        hidpi_state,
        ui_font,
        hc,
    );

    ConstrainedBox::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                Expanded::new(
                    1.0,
                    Container::new(standard).with_margin_right(8.0).finish(),
                )
                .finish(),
            )
            .with_child(Expanded::new(1.0, hidpi).finish())
            .finish(),
    )
    .with_height(TEXT_FIELD_HEIGHT)
    .finish()
}

fn rdp_quality_segment(
    label: &str,
    quality: RdpDisplayQuality,
    selected: bool,
    state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let state = state.clone();
    let hc = *hc;
    let label = label.to_string();
    let (bg, border) = if selected {
        (hc.badge_ssh_bg, hc.text_accent)
    } else {
        (hc.panel_bg, hc.card_border)
    };

    Hoverable::new(state, move |_mouse| {
        ConstrainedBox::new(
            Container::new(
                Align::new(
                    Text::new_inline(label.clone(), ui_font, UI_FONT_SIZE)
                        .with_color(hc.text_primary)
                        .finish(),
                )
                .finish(),
            )
            .with_background_color(bg)
            .with_border(Border::all(1.0).with_border_color(border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
            .finish(),
        )
        .with_height(TEXT_FIELD_HEIGHT)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::SelectRdpQuality(quality));
    })
    .finish()
}

fn render_port_stepper(
    port: u16,
    editor: &ViewHandle<EditorView>,
    states: &FieldStates,
    ui_font: fonts::FamilyId,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let dec_state = states.port_dec_state.clone();
    let inc_state = states.port_inc_state.clone();
    let hc = *hc;

    let dec_btn = Hoverable::new(dec_state, move |mouse| {
        let color = if mouse.is_hovered() {
            hc.text_primary
        } else {
            hc.text_secondary
        };
        Container::new(
            Text::new_inline("−".to_string(), ui_font, 14.0)
                .with_color(color)
                .finish(),
        )
        .with_horizontal_padding(10.0)
        .with_vertical_padding(4.0)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::DecrementPort);
    })
    .finish();

    let inc_btn = Hoverable::new(inc_state, move |mouse| {
        let color = if mouse.is_hovered() {
            hc.text_primary
        } else {
            hc.text_secondary
        };
        Container::new(
            Text::new_inline("+".to_string(), ui_font, 14.0)
                .with_color(color)
                .finish(),
        )
        .with_horizontal_padding(10.0)
        .with_vertical_padding(4.0)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::IncrementPort);
    })
    .finish();

    ConstrainedBox::new(
        Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(dec_btn)
                .with_child(
                    Expanded::new(
                        1.0,
                        render_inline_number_field(port, editor, appearance, &hc),
                    )
                    .finish(),
                )
                .with_child(inc_btn)
                .finish(),
        )
        .with_background_color(hc.search_bar_bg)
        .with_border(Border::all(1.0).with_border_color(hc.card_border))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
        .finish(),
    )
    .with_height(TEXT_FIELD_HEIGHT)
    .finish()
}

fn render_advanced_settings(
    draft: &HostEditDraft,
    states: &FieldStates,
    ui_font: fonts::FamilyId,
    view: &HostEditView,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let mut section = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    section.add_child(
        Container::new(warpui::elements::Empty::new().finish())
            .with_margin_top(26.0)
            .with_border(Border::top(1.0).with_border_color(hc.card_border))
            .finish(),
    );

    let advanced_state = states.advanced_state.clone();
    let is_expanded = states.advanced_settings_expanded;
    let hc_copy = *hc;
    let header = Hoverable::new(advanced_state, move |mouse| {
        let text_color = if mouse.is_hovered() {
            hc_copy.text_primary
        } else {
            hc.text_secondary
        };
        Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Text::new_inline("☷".to_string(), ui_font, UI_FONT_SIZE)
                        .with_color(text_color)
                        .finish(),
                )
                .with_child(
                    Container::new(
                        Text::new_inline(
                            rust_i18n::t!("form_advanced").to_string(),
                            ui_font,
                            UI_FONT_SIZE,
                        )
                        .with_color(text_color)
                        .finish(),
                    )
                    .with_margin_left(10.0)
                    .finish(),
                )
                .with_child(Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish())
                .with_child(
                    Text::new_inline(
                        if is_expanded { "⌃" } else { "⌄" }.to_string(),
                        ui_font,
                        UI_FONT_SIZE,
                    )
                    .with_color(text_color)
                    .finish(),
                )
                .finish(),
        )
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_mouse_down(|ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::ToggleAdvancedSettings);
    })
    .finish();

    section.add_child(Container::new(header).with_margin_top(20.0).finish());

    if states.advanced_settings_expanded {
        section.add_child(
            Container::new(render_advanced_card(
                draft, states, ui_font, view, appearance, hc,
            ))
            .with_margin_top(14.0)
            .finish(),
        );
    }

    section.finish()
}

fn render_advanced_card(
    draft: &HostEditDraft,
    states: &FieldStates,
    ui_font: fonts::FamilyId,
    view: &HostEditView,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    if draft.protocol == "Serial" {
        return render_serial_advanced_card(draft, states, ui_font, appearance, hc);
    }

    let mut card = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    card.add_child(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline("Keep-Alive".to_string(), ui_font, UI_FONT_SIZE)
                    .with_color(hc.text_primary)
                    .finish(),
            )
            .with_child(Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish())
            .with_child(render_keep_alive_toggle(
                draft.keep_alive_enabled,
                &states.keep_alive_state,
                ui_font,
                appearance,
                hc,
            ))
            .finish(),
    );

    card.add_child(
        Container::new(render_two_column_row(
            render_settings_number_item(
                &rust_i18n::t!("form_heartbeat_interval"),
                draft.keep_alive_interval,
                &view.keep_alive_interval_editor,
                "10-300 秒",
                ui_font,
                appearance,
                hc,
            ),
            render_settings_number_item(
                &rust_i18n::t!("form_max_failures"),
                draft.keep_alive_max_failures,
                &view.keep_alive_max_failures_editor,
                "1-10 次",
                ui_font,
                appearance,
                hc,
            ),
        ))
        .with_margin_top(18.0)
        .finish(),
    );

    card.add_child(render_card_divider(hc));
    card.add_child(render_settings_section_title(
        &rust_i18n::t!("form_connection_timeout"),
        ui_font,
        hc,
    ));
    card.add_child(
        Container::new(render_two_column_row(
            render_settings_number_item(
                &rust_i18n::t!("form_tcp_timeout"),
                draft.tcp_connect_timeout,
                &view.tcp_connect_timeout_editor,
                "5-60 秒",
                ui_font,
                appearance,
                hc,
            ),
            render_settings_number_item(
                &rust_i18n::t!("form_auth_timeout"),
                draft.auth_timeout,
                &view.auth_timeout_editor,
                "10-120 秒",
                ui_font,
                appearance,
                hc,
            ),
        ))
        .with_margin_top(12.0)
        .finish(),
    );

    card.add_child(render_card_divider(hc));
    card.add_child(render_settings_section_title(
        &rust_i18n::t!("form_terminal_encoding"),
        ui_font,
        hc,
    ));
    card.add_child(
        Container::new(render_dropdown_select_field(
            HostDropdownKind::Encoding,
            encoding_label(&draft.term_encoding),
            &states.encoding_state,
            states.open_dropdown == Some(HostDropdownKind::Encoding),
            encoding_dropdown_options(&draft.term_encoding, states),
            ui_font,
            appearance,
            440.0,
        ))
        .with_margin_top(10.0)
        .finish(),
    );
    card.add_child(
        Container::new(
            Text::new_inline(
                rust_i18n::t!("form_encoding_hint").to_string(),
                ui_font,
                12.0,
            )
            .with_color(hc.text_secondary)
            .finish(),
        )
        .with_margin_top(8.0)
        .finish(),
    );

    Container::new(card.finish())
        .with_uniform_padding(16.0)
        .with_background_color(hc.card_bg)
        .with_border(Border::all(1.0).with_border_color(hc.card_border))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
        .finish()
}

fn render_serial_advanced_card(
    draft: &HostEditDraft,
    states: &FieldStates,
    ui_font: fonts::FamilyId,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let mut card = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    card.add_child(render_settings_section_title(
        &rust_i18n::t!("form_serial_params"),
        ui_font,
        hc,
    ));
    card.add_child(
        Container::new(render_two_column_row(
            render_settings_dropdown_item(
                &rust_i18n::t!("form_data_bits"),
                render_dropdown_select_field(
                    HostDropdownKind::SerialDataBits,
                    serial_data_bits_label(draft.serial_data_bits),
                    &states.serial_data_bits_state,
                    states.open_dropdown == Some(HostDropdownKind::SerialDataBits),
                    serial_data_bits_dropdown_options(draft.serial_data_bits, states),
                    ui_font,
                    appearance,
                    210.0,
                ),
                ui_font,
                hc,
            ),
            render_settings_dropdown_item(
                &rust_i18n::t!("form_stop_bits"),
                render_dropdown_select_field(
                    HostDropdownKind::SerialStopBits,
                    serial_stop_bits_label(draft.serial_stop_bits),
                    &states.serial_stop_bits_state,
                    states.open_dropdown == Some(HostDropdownKind::SerialStopBits),
                    serial_stop_bits_dropdown_options(draft.serial_stop_bits, states),
                    ui_font,
                    appearance,
                    210.0,
                ),
                ui_font,
                hc,
            ),
        ))
        .with_margin_top(12.0)
        .finish(),
    );

    card.add_child(
        Container::new(render_two_column_row(
            render_settings_dropdown_item(
                &rust_i18n::t!("form_parity"),
                render_dropdown_select_field(
                    HostDropdownKind::SerialParity,
                    serial_parity_label(&draft.serial_parity),
                    &states.serial_parity_state,
                    states.open_dropdown == Some(HostDropdownKind::SerialParity),
                    serial_parity_dropdown_options(&draft.serial_parity, states),
                    ui_font,
                    appearance,
                    210.0,
                ),
                ui_font,
                hc,
            ),
            render_settings_dropdown_item(
                &rust_i18n::t!("form_flow_control"),
                render_dropdown_select_field(
                    HostDropdownKind::SerialFlowControl,
                    serial_flow_control_label(&draft.serial_flow_control),
                    &states.serial_flow_control_state,
                    states.open_dropdown == Some(HostDropdownKind::SerialFlowControl),
                    serial_flow_control_dropdown_options(&draft.serial_flow_control, states),
                    ui_font,
                    appearance,
                    210.0,
                ),
                ui_font,
                hc,
            ),
        ))
        .with_margin_top(14.0)
        .finish(),
    );

    card.add_child(
        Container::new(render_two_column_row(
            render_serial_toggle_item(
                "DTR",
                draft.serial_dtr,
                &states.serial_dtr_state,
                HostEditAction::ToggleSerialDtr,
                ui_font,
                hc,
            ),
            render_serial_toggle_item(
                "RTS",
                draft.serial_rts,
                &states.serial_rts_state,
                HostEditAction::ToggleSerialRts,
                ui_font,
                hc,
            ),
        ))
        .with_margin_top(18.0)
        .finish(),
    );

    Container::new(card.finish())
        .with_uniform_padding(16.0)
        .with_background_color(hc.card_bg)
        .with_border(Border::all(1.0).with_border_color(hc.card_border))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
        .finish()
}

fn render_keep_alive_toggle(
    enabled: bool,
    state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Text::new_inline(
                if enabled {
                    rust_i18n::t!("form_keep_alive_enabled").to_string()
                } else {
                    rust_i18n::t!("form_keep_alive_disabled").to_string()
                },
                ui_font,
                13.0,
            )
            .with_color(hc.text_secondary)
            .finish(),
        )
        .with_child(
            appearance
                .ui_builder()
                .checkbox(state.clone(), None)
                .check(enabled)
                .build()
                .with_cursor(warpui::platform::Cursor::PointingHand)
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(HostEditAction::ToggleKeepAlive);
                })
                .finish(),
        )
        .finish()
}

fn render_two_column_row(left: Box<dyn Element>, right: Box<dyn Element>) -> Box<dyn Element> {
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Expanded::new(1.0, Container::new(left).with_margin_right(12.0).finish()).finish(),
        )
        .with_child(Expanded::new(1.0, right).finish())
        .finish()
}

fn render_settings_section_title(
    title: &str,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Text::new_inline(title.to_string(), ui_font, 13.0)
        .with_color(hc.text_primary)
        .finish()
}

fn render_card_divider(hc: &HostUiColors) -> Box<dyn Element> {
    Container::new(warpui::elements::Empty::new().finish())
        .with_margin_top(20.0)
        .with_margin_bottom(20.0)
        .with_border(Border::top(1.0).with_border_color(hc.card_border))
        .finish()
}

fn render_settings_number_item(
    label: &str,
    value: u16,
    editor: &ViewHandle<EditorView>,
    range: &str,
    ui_font: fonts::FamilyId,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Text::new_inline(label.to_string(), ui_font, 13.0)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_child(
            Container::new(render_number_text_field(value, editor, appearance, hc))
                .with_margin_top(8.0)
                .finish(),
        )
        .with_child(
            Container::new(
                Text::new_inline(range.to_string(), ui_font, 12.0)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .with_margin_top(8.0)
            .finish(),
        )
        .finish()
}

fn render_settings_dropdown_item(
    label: &str,
    dropdown: Box<dyn Element>,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Text::new_inline(label.to_string(), ui_font, 13.0)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_child(Container::new(dropdown).with_margin_top(8.0).finish())
        .finish()
}

fn render_serial_toggle_item(
    label: &str,
    enabled: bool,
    state: &MouseStateHandle,
    action: HostEditAction,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let state = state.clone();
    let hc = *hc;
    Hoverable::new(state, move |mouse| {
        let bg = if mouse.is_hovered() {
            hc.panel_bg
        } else {
            hc.panel_bg
        };
        Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Text::new_inline(label.to_string(), ui_font, 13.0)
                        .with_color(hc.text_secondary)
                        .finish(),
                )
                .with_child(Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish())
                .with_child(
                    Text::new_inline(
                        if enabled {
                            "已启用".to_string()
                        } else {
                            "已停用".to_string()
                        },
                        ui_font,
                        13.0,
                    )
                    .with_color(if enabled {
                        hc.text_primary
                    } else {
                        hc.text_secondary
                    })
                    .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(12.0)
        .with_vertical_padding(9.0)
        .with_background_color(bg)
        .with_border(Border::all(1.0).with_border_color(hc.card_border))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.0)))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}

fn render_number_text_field(
    _value: u16,
    editor: &ViewHandle<EditorView>,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let input = appearance
        .ui_builder()
        .text_input(editor.clone())
        .with_style(UiComponentStyles {
            background: Some(hc.panel_bg.into()),
            border_width: Some(0.0),
            font_color: Some(hc.text_primary),
            height: Some(SETTINGS_TEXT_FIELD_HEIGHT),
            padding: Some(
                Coords::uniform(0.0)
                    .left(12.0)
                    .right(34.0)
                    .top(9.0)
                    .bottom(9.0),
            ),
            ..Default::default()
        })
        .build()
        .finish();

    let spinner = Container::new(
        Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline("⌃".to_string(), appearance.ui_font_family(), 8.0)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .with_child(
                Text::new_inline("⌄".to_string(), appearance.ui_font_family(), 8.0)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .finish(),
    )
    .with_margin_right(12.0)
    .finish();

    ConstrainedBox::new(
        Stack::new()
            .with_child(input)
            .with_child(Align::new(spinner).right().finish())
            .finish(),
    )
    .with_height(SETTINGS_TEXT_FIELD_HEIGHT)
    .finish()
}

fn encoding_label(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "utf-8" => "UTF-8 (推荐)".to_string(),
        "gbk" => "GBK".to_string(),
        "big5" => "Big5".to_string(),
        "shift_jis" => "Shift-JIS".to_string(),
        _ => value.to_string(),
    }
}

fn serial_data_bits_label(value: u8) -> String {
    format!("{value} bits")
}

fn serial_stop_bits_label(value: u8) -> String {
    format!("{value} bit")
}

fn serial_parity_label(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "odd" => "Odd".to_string(),
        "even" => "Even".to_string(),
        _ => "None".to_string(),
    }
}

fn serial_flow_control_label(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "hardware" => "Hardware".to_string(),
        "software" => "Software".to_string(),
        _ => "None".to_string(),
    }
}

fn render_footer(
    save_state: &MouseStateHandle,
    cancel_state: &MouseStateHandle,
    is_new: bool,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let save_state = save_state.clone();
    let cancel_state = cancel_state.clone();
    let hc = *hc;
    let save_label = if is_new {
        rust_i18n::t!("form_create").to_string()
    } else {
        rust_i18n::t!("form_save").to_string()
    };

    let cancel_btn = Hoverable::new(cancel_state, move |mouse| {
        let color = if mouse.is_hovered() {
            hc.text_primary
        } else {
            hc.text_secondary
        };
        Container::new(
            Text::new_inline(rust_i18n::t!("form_cancel").to_string(), ui_font, 13.0)
                .with_color(color)
                .finish(),
        )
        .with_horizontal_padding(20.0)
        .with_vertical_padding(8.0)
        .with_border(Border::all(1.0).with_border_color(hc.card_border))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::Cancel);
    })
    .finish();

    let save_btn = Hoverable::new(save_state, move |mouse| {
        let bg = if mouse.is_hovered() {
            hc.text_accent
        } else {
            hc.accent_bg
        };
        Container::new(
            Text::new_inline(save_label.to_string(), ui_font, 13.0)
                .with_color(hc.accent_text)
                .finish(),
        )
        .with_horizontal_padding(24.0)
        .with_vertical_padding(8.0)
        .with_background_color(bg)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(HostEditAction::Save);
    })
    .finish();

    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Container::new(cancel_btn).with_margin_right(12.0).finish())
            .with_child(save_btn)
            .finish(),
    )
    .with_horizontal_padding(20.0)
    .with_vertical_padding(14.0)
    .with_background_color(hc.toolbar_bg)
    .with_border(Border::top(1.0).with_border_color(hc.toolbar_border))
    .finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexshell::host_management::{HostCardSnapshot, HostConnectionConfig};

    #[test]
    fn from_card_ssh_round_trips_all_fields() {
        let connection = {
            let mut c = HostConnectionConfig::ssh("10.0.0.1", 2222, "admin");
            c.auth_method = "key".to_string();
            c.password = Some("secret".to_string());
            c.private_key = Some("~/.ssh/id_ed25519".to_string());
            c.key_passphrase = Some("phrase".to_string());
            c.ca_cert = Some("/tmp/ca.pem".to_string());
            c.keep_alive_enabled = false;
            c.keep_alive_interval = 60;
            c.keep_alive_max_failures = 5;
            c.tcp_connect_timeout = 10;
            c.auth_timeout = 20;
            c.term_encoding = "gbk".to_string();
            c
        };
        let card = HostCardSnapshot {
            id: "host-42".to_string(),
            name: "My Server".to_string(),
            protocol: "SSH".to_string(),
            endpoint: "admin@10.0.0.1:2222".to_string(),
            description: "production".to_string(),
            connection,
            group_id: Some("servers".to_string()),
            tags: vec!["prod".to_string(), "us-east".to_string()],
            system: HostSystemIcon::Linux,
            sort_order: 5,
        };

        let draft = HostEditDraft::from_card(&card);

        assert_eq!(draft.id, "host-42");
        assert_eq!(draft.name, "My Server");
        assert_eq!(draft.protocol, "SSH");
        assert_eq!(draft.host, "10.0.0.1");
        assert_eq!(draft.port, 2222);
        assert_eq!(draft.username, "admin");
        assert_eq!(draft.auth_method, "key");
        assert_eq!(draft.password, "secret");
        assert_eq!(draft.private_key, "~/.ssh/id_ed25519");
        assert_eq!(draft.key_passphrase, "phrase");
        assert_eq!(draft.ca_cert, "/tmp/ca.pem");
        assert_eq!(draft.description, "production");
        assert_eq!(draft.group_id, Some("servers".to_string()));
        assert_eq!(draft.tags, vec!["prod".to_string(), "us-east".to_string()]);
        assert!(!draft.keep_alive_enabled);
        assert_eq!(draft.keep_alive_interval, 60);
        assert_eq!(draft.keep_alive_max_failures, 5);
        assert_eq!(draft.tcp_connect_timeout, 10);
        assert_eq!(draft.auth_timeout, 20);
        assert_eq!(draft.term_encoding, "gbk");
        assert_eq!(draft.system, HostSystemIcon::Linux);
    }

    #[test]
    fn from_card_serial_round_trips_all_fields() {
        let mut connection = HostConnectionConfig::serial("/dev/cu.usbserial", 9600);
        connection.serial_data_bits = 7;
        connection.serial_stop_bits = 2;
        connection.serial_parity = "even".to_string();
        connection.serial_flow_control = "hardware".to_string();
        connection.serial_dtr = true;
        connection.serial_rts = true;

        let card = HostCardSnapshot {
            id: "host-99".to_string(),
            name: "Serial Device".to_string(),
            protocol: "Serial".to_string(),
            endpoint: "/dev/cu.usbserial @ 9600".to_string(),
            description: "debug port".to_string(),
            connection,
            group_id: None,
            tags: vec![],
            system: HostSystemIcon::Serial,
            sort_order: 0,
        };

        let draft = HostEditDraft::from_card(&card);

        assert_eq!(draft.protocol, "Serial");
        assert_eq!(draft.host, "/dev/cu.usbserial");
        assert_eq!(draft.serial_baud_rate, 9600);
        assert_eq!(draft.serial_data_bits, 7);
        assert_eq!(draft.serial_stop_bits, 2);
        assert_eq!(draft.serial_parity, "even");
        assert_eq!(draft.serial_flow_control, "hardware");
        assert!(draft.serial_dtr);
        assert!(draft.serial_rts);
        assert_eq!(draft.system, HostSystemIcon::Serial);
    }

    #[test]
    fn from_card_optional_fields_default_to_empty() {
        let connection = HostConnectionConfig::ssh("host", 22, "root");
        let card = HostCardSnapshot {
            id: "h1".to_string(),
            name: "test".to_string(),
            protocol: "SSH".to_string(),
            endpoint: "root@host:22".to_string(),
            description: String::new(),
            connection,
            group_id: None,
            tags: vec![],
            system: HostSystemIcon::Terminal,
            sort_order: 0,
        };

        let draft = HostEditDraft::from_card(&card);

        assert_eq!(draft.password, "");
        assert_eq!(draft.private_key, "");
        assert_eq!(draft.key_passphrase, "");
        assert_eq!(draft.ca_cert, "");
        assert_eq!(draft.group_id, None);
        assert!(draft.tags.is_empty());
    }

    #[test]
    fn serial_device_names_filter_platform_noise_and_sort() {
        let devices = normalize_serial_device_names([
            "/dev/cu.usbserial-AR0K468T".to_string(),
            "/dev/cu.debug-console".to_string(),
            "/dev/cu.Bluetooth-Incoming-Port".to_string(),
            "/dev/ttyS0".to_string(),
            "COM4".to_string(),
            "COMX".to_string(),
            "COM3".to_string(),
            "/dev/cu.usbserial-AR0K468T".to_string(),
        ]);

        assert_eq!(
            devices,
            vec![
                "/dev/cu.usbserial-AR0K468T".to_string(),
                "/dev/ttyS0".to_string(),
                "COM3".to_string(),
                "COM4".to_string(),
            ]
        );
    }
}
