use std::sync::{Arc, Mutex};

use warpui::{
    elements::{
        Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Flex, Hoverable, Icon,
        MainAxisAlignment, MainAxisSize, MouseState, MouseStateHandle, ParentElement, Radius, Text,
    },
    fonts, Element,
};

use crate::host_management_view::constants::*;
use crate::terminal_grid_element::TerminalGridAction;

pub struct SelectionBarStates {
    pub connect_state: MouseStateHandle,
    pub edit_state: MouseStateHandle,
    pub move_state: MouseStateHandle,
    pub delete_state: MouseStateHandle,
    pub cancel_state: MouseStateHandle,
    pub reorder_done_state: MouseStateHandle,
}

impl SelectionBarStates {
    pub fn new() -> Self {
        Self {
            connect_state: Arc::new(Mutex::new(MouseState::default())),
            edit_state: Arc::new(Mutex::new(MouseState::default())),
            move_state: Arc::new(Mutex::new(MouseState::default())),
            delete_state: Arc::new(Mutex::new(MouseState::default())),
            cancel_state: Arc::new(Mutex::new(MouseState::default())),
            reorder_done_state: Arc::new(Mutex::new(MouseState::default())),
        }
    }
}

pub fn render_selection_bar(
    _selected_count: usize,
    states: &SelectionBarStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::Center)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(render_bar_button(
            &states.connect_state,
            ICON_PLAY,
            rust_i18n::t!("host_connect").to_string(),
            hc.text_primary,
            ui_font,
            TerminalGridAction::HostConnectSelected,
            hc,
        ))
        .with_child(render_bar_button(
            &states.edit_state,
            ICON_PENCIL,
            rust_i18n::t!("host_edit").to_string(),
            hc.text_primary,
            ui_font,
            TerminalGridAction::HostEditSelected,
            hc,
        ))
        .with_child(render_bar_button(
            &states.move_state,
            ICON_SWAP,
            rust_i18n::t!("host_move").to_string(),
            hc.text_primary,
            ui_font,
            TerminalGridAction::HostEnterReorderMode,
            hc,
        ))
        .with_child(render_bar_button(
            &states.delete_state,
            ICON_TRASH,
            rust_i18n::t!("host_delete").to_string(),
            hc.text_primary,
            ui_font,
            TerminalGridAction::HostDeleteSelected,
            hc,
        ))
        .with_child(render_bar_button(
            &states.cancel_state,
            ICON_X_CIRCLE,
            rust_i18n::t!("host_cancel").to_string(),
            hc.text_secondary,
            ui_font,
            TerminalGridAction::HostClearSelection,
            hc,
        ))
        .finish();

    Container::new(
        Container::new(row)
            .with_horizontal_padding(24.0)
            .with_vertical_padding(10.0)
            .with_background_color(hc.action_bar_bg)
            .with_border(Border::all(1.0).with_border_color(hc.action_bar_border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
            .finish(),
    )
    .with_horizontal_padding(24.0)
    .with_vertical_padding(8.0)
    .with_background_color(hc.panel_bg)
    .with_border(Border::top(1.0).with_border_color(hc.toolbar_border))
    .finish()
}

fn render_bar_button(
    state: &MouseStateHandle,
    icon_path: &'static str,
    label: String,
    color: warpui::color::ColorU,
    ui_font: fonts::FamilyId,
    action: TerminalGridAction,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();

    Hoverable::new(state, move |mouse| {
        let text_color = if mouse.is_hovered() {
            hc.text_accent
        } else {
            color
        };

        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Container::new(
                        ConstrainedBox::new(Icon::new(icon_path, text_color).finish())
                            .with_width(ICON_SIZE_SM)
                            .with_height(ICON_SIZE_SM)
                            .finish(),
                    )
                    .with_margin_right(6.0)
                    .finish(),
                )
                .with_child(
                    Text::new_inline(label.clone(), ui_font, 13.0)
                        .with_color(text_color)
                        .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(16.0)
        .with_vertical_padding(6.0)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}

pub fn render_reorder_bar(
    states: &SelectionBarStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::Center)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Container::new(
                Text::new_inline(
                    rust_i18n::t!("host_reorder_hint").to_string(),
                    ui_font,
                    13.0,
                )
                .with_color(hc.text_secondary)
                .finish(),
            )
            .with_margin_right(24.0)
            .finish(),
        )
        .with_child(render_bar_button(
            &states.reorder_done_state,
            ICON_SWAP,
            rust_i18n::t!("host_reorder_done").to_string(),
            hc.text_accent,
            ui_font,
            TerminalGridAction::HostExitReorderMode,
            hc,
        ))
        .finish();

    Container::new(
        Container::new(row)
            .with_horizontal_padding(24.0)
            .with_vertical_padding(10.0)
            .with_background_color(hc.action_bar_bg)
            .with_border(Border::all(1.0).with_border_color(hc.action_bar_border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
            .finish(),
    )
    .with_horizontal_padding(24.0)
    .with_vertical_padding(8.0)
    .with_background_color(hc.panel_bg)
    .with_border(Border::top(1.0).with_border_color(hc.toolbar_border))
    .finish()
}
