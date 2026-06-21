use std::sync::{Arc, Mutex};

use warpui::{
    color::ColorU,
    elements::{
        Align, Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, DraggableState,
        Fill, Flex, Hoverable, Icon, MainAxisAlignment, MainAxisSize, MouseState, MouseStateHandle,
        ParentElement, Radius, Text,
    },
    fonts, Element, ViewHandle,
};

use crate::host_management_view::constants::*;
use crate::terminal_grid_element::TerminalGridAction;
use warp::editor::EditorView;
use warpui::elements::Expanded;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::ui_components::text_input::TextInput;

use nexshell::host_management::{
    HostCardSnapshot, HostSystemIcon, HostViewMode,
};

pub struct HostCardStates {
    pub card_states: Vec<MouseStateHandle>,
    pub connect_states: Vec<MouseStateHandle>,
    pub checkbox_states: Vec<MouseStateHandle>,
    pub draggable_states: Vec<DraggableState>,
}

impl HostCardStates {
    pub fn new() -> Self {
        Self {
            card_states: Vec::new(),
            connect_states: Vec::new(),
            checkbox_states: Vec::new(),
            draggable_states: Vec::new(),
        }
    }

    pub fn ensure_count(&mut self, count: usize) {
        while self.card_states.len() < count {
            self.card_states
                .push(Arc::new(Mutex::new(MouseState::default())));
            self.connect_states
                .push(Arc::new(Mutex::new(MouseState::default())));
            self.checkbox_states
                .push(Arc::new(Mutex::new(MouseState::default())));
            self.draggable_states.push(DraggableState::default());
        }
    }
}

// 内联重命名：该卡处于重命名态时，name 处渲染单行输入框替代静态文本
fn render_name_element(
    name: &str,
    is_renaming: bool,
    rename_editor: &ViewHandle<EditorView>,
    ui_font: fonts::FamilyId,
    font_size: f32,
    color: ColorU,
) -> Box<dyn Element> {
    if is_renaming {
        // editor buffer 不接受无限宽度约束，必须包在固定宽度容器内
        ConstrainedBox::new(
            TextInput::new(
                rename_editor.clone(),
                UiComponentStyles::default()
                    .set_background(Fill::None)
                    .set_border_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                    .set_border_width(1.0),
            )
            .build()
            .finish(),
        )
        .with_width(180.0)
        .finish()
    } else {
        Text::new_inline(name.to_string(), ui_font, font_size)
            .with_color(color)
            .finish()
    }
}

pub fn render_host_card(
    host: &HostCardSnapshot,
    index: usize,
    states: &HostCardStates,
    ui_font: fonts::FamilyId,
    privacy_mode: bool,
    selected: bool,
    _view_mode: HostViewMode,
    rename_target: Option<&str>,
    rename_editor: &ViewHandle<EditorView>,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let is_renaming = rename_target == Some(host.id.as_str());
    let rename_editor = rename_editor.clone();
    let card_state = states.card_states[index].clone();
    let connect_state = states.connect_states[index].clone();
    let checkbox_state = states.checkbox_states[index].clone();
    let host_id = host.id.clone();
    let host_id_for_menu = host.id.clone();
    let host_id_for_connect = host.id.clone();
    let host_id_for_checkbox = host.id.clone();
    let name = host.name.clone();
    let protocol = host.protocol.clone();
    let endpoint = if privacy_mode {
        mask_endpoint(&host.endpoint)
    } else {
        host.endpoint.clone()
    };
    let description = host.description.clone();
    let tags = host.tags.clone();
    let system = host.system;

    Hoverable::new(card_state, move |mouse| {
        let is_hovered = mouse.is_hovered();
        let bg = if is_hovered {
            hc.card_bg_hover
        } else {
            hc.card_bg
        };
        let border_color = if selected {
            hc.text_accent
        } else if is_hovered {
            hc.card_border_hover
        } else {
            hc.card_border
        };

        let mut card_col = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start);

        // 顶部行：左侧系统图标 + 右侧多选 checkbox
        let top_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(render_system_icon(system, &hc))
            .with_child(Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish())
            .with_child(render_select_checkbox(
                checkbox_state.clone(),
                host_id_for_checkbox.clone(),
                selected,
                &hc,
            ))
            .finish();
        card_col.add_child(top_row);

        let mut name_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Container::new(render_name_element(
                    &name,
                    is_renaming,
                    &rename_editor,
                    ui_font,
                    14.0,
                    hc.text_primary,
                ))
                .with_margin_right(8.0)
                .finish(),
            );
        name_row.add_child(render_protocol_badge(&protocol, ui_font, &hc));
        card_col.add_child(
            Container::new(name_row.finish())
                .with_margin_top(12.0)
                .finish(),
        );

        card_col.add_child(
            Container::new(
                Text::new_inline(endpoint.clone(), ui_font, UI_FONT_SIZE)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .with_margin_top(6.0)
            .finish(),
        );

        card_col.add_child(
            Container::new(
                Text::new_inline(description.clone(), ui_font, UI_FONT_SIZE_SMALL)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .with_margin_top(4.0)
            .finish(),
        );

        card_col.add_child(render_tags_row(&tags, ui_font, &hc));

        card_col.add_child(
            Container::new(render_connect_button(
                connect_state.clone(),
                host_id_for_connect.clone(),
                ui_font,
                &hc,
            ))
            .with_margin_top(16.0)
            .finish(),
        );

        Container::new(
            Container::new(card_col.finish())
                .with_uniform_padding(CARD_PADDING)
                .finish(),
        )
        .with_background_color(bg)
        .with_border(Border::all(1.0).with_border_color(border_color))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(CARD_CORNER_RADIUS)))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .with_defer_events_to_children()
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostSelectSingle(host_id.clone()));
    })
    .on_right_click(move |ctx, _, position| {
        ctx.dispatch_typed_action(TerminalGridAction::HostShowContextMenu {
            host_id: host_id_for_menu.clone(),
            position,
        });
    })
    .finish()
}

// 卡片右上角的多选 checkbox：点击 = 多选切换；卡片本体点击 = 单选
fn render_select_checkbox(
    state: MouseStateHandle,
    host_id: String,
    selected: bool,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    Hoverable::new(state, move |mouse| {
        let is_hovered = mouse.is_hovered();
        let border_color = if selected || is_hovered {
            hc.text_accent
        } else {
            hc.text_secondary
        };
        let bg = if selected {
            hc.text_accent
        } else {
            ColorU::transparent_black()
        };
        ConstrainedBox::new(
            Container::new(warpui::elements::Empty::new().finish())
                .with_border(Border::all(1.5).with_border_color(border_color))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.0)))
                .with_background_color(bg)
                .finish(),
        )
        .with_width(16.0)
        .with_height(16.0)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostToggleSelect(host_id.clone()));
    })
    .finish()
}

fn render_system_icon(system: HostSystemIcon, hc: &HostUiColors) -> Box<dyn Element> {
    let icon_path = match system {
        HostSystemIcon::Terminal => ICON_TERMINAL,
        HostSystemIcon::Linux => ICON_LINUX,
        HostSystemIcon::Serial => ICON_SERIAL,
    };

    Container::new(
        ConstrainedBox::new(
            Align::new(
                ConstrainedBox::new(Icon::new(icon_path, hc.text_primary).finish())
                    .with_width(24.0)
                    .with_height(24.0)
                    .finish(),
            )
            .finish(),
        )
        .with_width(CARD_ICON_SIZE)
        .with_height(CARD_ICON_SIZE)
        .finish(),
    )
    .with_background_color(hc.search_bar_bg)
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
    .finish()
}

fn render_protocol_badge(
    protocol: &str,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let (bg, text_color, label) = match protocol {
        "SSH" => (hc.badge_ssh_bg, hc.badge_ssh_text, "SSH"),
        "Serial" => (hc.badge_serial_bg, hc.badge_serial_text, "Serial"),
        _ => (hc.badge_ssh_bg, hc.badge_ssh_text, protocol),
    };

    Container::new(
        Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE_SMALL)
            .with_color(text_color)
            .finish(),
    )
    .with_horizontal_padding(8.0)
    .with_vertical_padding(2.0)
    .with_background_color(bg)
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
    .finish()
}

fn render_tags_row(
    tags: &[String],
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    if tags.is_empty() {
        return Container::new(
            Text::new_inline("无标签".to_string(), ui_font, UI_FONT_SIZE_SMALL)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_margin_top(4.0)
        .finish();
    }

    let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
    for tag in tags {
        row.add_child(
            Container::new(
                Text::new_inline(tag.clone(), ui_font, UI_FONT_SIZE_SMALL)
                    .with_color(hc.tag_text)
                    .finish(),
            )
            .with_horizontal_padding(6.0)
            .with_vertical_padding(1.0)
            .with_margin_right(4.0)
            .with_background_color(hc.tag_bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.0)))
            .finish(),
        );
    }
    Container::new(row.finish()).with_margin_top(4.0).finish()
}

fn render_connect_button(
    state: MouseStateHandle,
    host_id: String,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    Hoverable::new(state, move |mouse| {
        let bg = if mouse.is_hovered() {
            hc.connect_btn_bg_hover
        } else {
            hc.connect_btn_bg
        };

        Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Container::new(
                        ConstrainedBox::new(Icon::new(ICON_LINK, hc.text_accent).finish())
                            .with_width(ICON_SIZE_SM)
                            .with_height(ICON_SIZE_SM)
                            .finish(),
                    )
                    .with_margin_right(6.0)
                    .finish(),
                )
                .with_child(
                    Text::new_inline("快速连接".to_string(), ui_font, UI_FONT_SIZE)
                        .with_color(hc.text_secondary)
                        .finish(),
                )
                .finish(),
        )
        .with_vertical_padding(8.0)
        .with_background_color(bg)
        .with_border(Border::all(1.0).with_border_color(hc.connect_btn_border))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostQuickConnect(host_id.clone()));
    })
    .finish()
}

const LIST_STATUS_COL_W: f32 = 80.0;
const LIST_LATENCY_COL_W: f32 = 80.0;
const LIST_PROTOCOL_COL_W: f32 = 80.0;
const LIST_GROUP_COL_W: f32 = 80.0;
const LIST_TAG_COL_W: f32 = 100.0;

pub fn render_list_header(ui_font: fonts::FamilyId, hc: &HostUiColors) -> Box<dyn Element> {
    let text_secondary = hc.text_secondary;
    let header_text = |label: &str| -> Box<dyn Element> {
        Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE)
            .with_color(text_secondary)
            .finish()
    };

    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            ConstrainedBox::new(header_text("状态"))
                .with_width(LIST_STATUS_COL_W)
                .finish(),
        )
        .with_child(Expanded::new(1.0, header_text("主机信息")).finish())
        .with_child(
            ConstrainedBox::new(header_text("延迟"))
                .with_width(LIST_LATENCY_COL_W)
                .finish(),
        )
        .with_child(
            ConstrainedBox::new(header_text("协议"))
                .with_width(LIST_PROTOCOL_COL_W)
                .finish(),
        )
        .with_child(
            ConstrainedBox::new(header_text("分组"))
                .with_width(LIST_GROUP_COL_W)
                .finish(),
        )
        .with_child(
            ConstrainedBox::new(header_text("标签"))
                .with_width(LIST_TAG_COL_W)
                .finish(),
        )
        .finish();

    Container::new(row)
        .with_horizontal_padding(16.0)
        .with_vertical_padding(10.0)
        .with_background_color(hc.sidebar_bg)
        .with_border(Border::bottom(1.0).with_border_color(hc.card_border))
        .finish()
}

pub fn render_host_list_row(
    host: &HostCardSnapshot,
    index: usize,
    states: &HostCardStates,
    ui_font: fonts::FamilyId,
    privacy_mode: bool,
    selected: bool,
    rename_target: Option<&str>,
    rename_editor: &ViewHandle<EditorView>,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let is_renaming = rename_target == Some(host.id.as_str());
    let rename_editor = rename_editor.clone();
    let card_state = states.card_states[index].clone();
    let checkbox_state = states.checkbox_states[index].clone();
    let host_id = host.id.clone();
    let host_id_for_menu = host.id.clone();
    let host_id_for_checkbox = host.id.clone();
    let name = host.name.clone();
    let protocol = host.protocol.clone();
    let endpoint = if privacy_mode {
        mask_endpoint(&host.endpoint)
    } else {
        host.endpoint.clone()
    };
    let system = host.system;
    let tags = host.tags.clone();
    let group_name = host.group_id.clone().unwrap_or_default();

    Hoverable::new(card_state, move |mouse| {
        let is_hovered = mouse.is_hovered();
        let bg = if selected {
            hc.group_selected_bg
        } else if is_hovered {
            hc.card_bg_hover
        } else {
            hc.panel_bg
        };

        let status_col = ConstrainedBox::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Container::new(render_list_select_checkbox(
                        checkbox_state.clone(),
                        host_id_for_checkbox.clone(),
                        selected,
                        &hc,
                    ))
                    .with_margin_right(8.0)
                    .finish(),
                )
                .with_child(
                    Container::new(render_system_icon_small(system, &hc))
                        .with_margin_right(8.0)
                        .finish(),
                )
                .finish(),
        )
        .with_width(LIST_STATUS_COL_W)
        .finish();

        let info_col = Expanded::new(
            1.0,
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(render_name_element(
                    &name,
                    is_renaming,
                    &rename_editor,
                    ui_font,
                    13.0,
                    hc.text_primary,
                ))
                .with_child(
                    Container::new(
                        Text::new_inline(endpoint.clone(), ui_font, UI_FONT_SIZE)
                            .with_color(hc.text_secondary)
                            .finish(),
                    )
                    .with_margin_top(2.0)
                    .finish(),
                )
                .finish(),
        )
        .finish();

        let latency_col = ConstrainedBox::new(
            Text::new_inline("—".to_string(), ui_font, UI_FONT_SIZE)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_width(LIST_LATENCY_COL_W)
        .finish();

        let protocol_col = ConstrainedBox::new(render_protocol_badge(&protocol, ui_font, &hc))
            .with_width(LIST_PROTOCOL_COL_W)
            .finish();

        let group_label = if group_name.is_empty() {
            "—".to_string()
        } else {
            group_name.clone()
        };
        let group_col = ConstrainedBox::new(
            Text::new_inline(group_label, ui_font, UI_FONT_SIZE)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_width(LIST_GROUP_COL_W)
        .finish();

        let tag_label = if tags.is_empty() {
            "—".to_string()
        } else {
            tags.join(", ")
        };
        let tag_col = ConstrainedBox::new(
            Text::new_inline(tag_label, ui_font, UI_FONT_SIZE)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_width(LIST_TAG_COL_W)
        .finish();

        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(status_col)
            .with_child(info_col)
            .with_child(latency_col)
            .with_child(protocol_col)
            .with_child(group_col)
            .with_child(tag_col)
            .finish();

        Container::new(row)
            .with_horizontal_padding(16.0)
            .with_vertical_padding(12.0)
            .with_background_color(bg)
            .with_border(Border::bottom(1.0).with_border_color(hc.card_border))
            .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .with_defer_events_to_children()
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostSelectSingle(host_id.clone()));
    })
    .on_right_click(move |ctx, _, position| {
        ctx.dispatch_typed_action(TerminalGridAction::HostShowContextMenu {
            host_id: host_id_for_menu.clone(),
            position,
        });
    })
    .finish()
}

// 列表行的多选 checkbox：与卡片视图同语义
fn render_list_select_checkbox(
    state: MouseStateHandle,
    host_id: String,
    selected: bool,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    Hoverable::new(state, move |_mouse| {
        ConstrainedBox::new(
            Container::new(warpui::elements::Empty::new().finish())
                .with_border(Border::all(1.5).with_border_color(if selected {
                    hc.text_accent
                } else {
                    hc.text_secondary
                }))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.0)))
                .with_background_color(if selected {
                    hc.text_accent
                } else {
                    ColorU::transparent_black()
                })
                .finish(),
        )
        .with_width(14.0)
        .with_height(14.0)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostToggleSelect(host_id.clone()));
    })
    .finish()
}

fn render_system_icon_small(system: HostSystemIcon, hc: &HostUiColors) -> Box<dyn Element> {
    let icon_path = match system {
        HostSystemIcon::Terminal => ICON_TERMINAL,
        HostSystemIcon::Linux => ICON_LINUX,
        HostSystemIcon::Serial => ICON_SERIAL,
    };

    Container::new(
        ConstrainedBox::new(
            Align::new(
                ConstrainedBox::new(Icon::new(icon_path, hc.text_primary).finish())
                    .with_width(16.0)
                    .with_height(16.0)
                    .finish(),
            )
            .finish(),
        )
        .with_width(28.0)
        .with_height(28.0)
        .finish(),
    )
    .with_background_color(hc.search_bar_bg)
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.0)))
    .finish()
}

fn mask_endpoint(endpoint: &str) -> String {
    if let Some((user_host, port)) = endpoint.rsplit_once(':') {
        if let Some((user, host)) = user_host.split_once('@') {
            let masked_host = if host.len() > 4 {
                format!("{}****", &host[..4])
            } else {
                "****".to_string()
            };
            return format!("{user}@{masked_host}:{port}");
        }
    }
    "****".to_string()
}
