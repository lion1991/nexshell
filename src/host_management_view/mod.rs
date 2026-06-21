pub mod constants;
pub mod group_nav;
pub mod host_card;
pub mod key_manager_view;
pub mod search_bar;
pub mod selection_bar;
pub mod status_view;

use warpui::{
    elements::{
        ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container, CrossAxisAlignment,
        DragAxis, Draggable, DraggableState, Expanded, Fill, Flex, MainAxisSize, ParentElement,
        SavePosition, ScrollbarWidth,
    },
    fonts, Element, ViewHandle,
};

use crate::terminal_grid_element::TerminalGridAction;

use warp_core::ui::appearance::Appearance;
use warp::editor::EditorView;

use nexshell::host_management::{
    HostManagementState, HostViewMode, RecentHostSnapshot,
};
use nexshell::host_overview_fleet::HostOverviewFleet;
use nexshell::ssh_key_store::SshKeyRecord;

use constants::*;
use group_nav::{render_group_nav, GroupNavStates};
use host_card::{render_host_card, render_host_list_row, render_list_header, HostCardStates};
use search_bar::{render_search_bar, SearchBarStates};
use key_manager_view::{render_key_manager_view, KeyManagerStates};
use selection_bar::{render_reorder_bar, render_selection_bar, SelectionBarStates};
use status_view::render_status_view;

pub struct HostManagementViewStates {
    pub search_bar: SearchBarStates,
    pub group_nav: GroupNavStates,
    pub host_cards: HostCardStates,
    pub selection_bar: SelectionBarStates,
    pub key_manager: KeyManagerStates,
    pub scroll_state: ClippedScrollStateHandle,
}

impl HostManagementViewStates {
    pub fn new() -> Self {
        Self {
            search_bar: SearchBarStates::new(),
            group_nav: GroupNavStates::new(),
            host_cards: HostCardStates::new(),
            selection_bar: SelectionBarStates::new(),
            key_manager: KeyManagerStates::new(),
            scroll_state: ClippedScrollStateHandle::new(),
        }
    }
}

pub fn render_host_management_panel(
    state: &HostManagementState,
    view_states: &mut HostManagementViewStates,
    search_editor: &ViewHandle<EditorView>,
    rename_target: Option<&str>,
    rename_editor: &ViewHandle<EditorView>,
    ui_font: fonts::FamilyId,
    appearance: &Appearance,
    sidebar_open: bool,
    fleet: &HostOverviewFleet,
    keys: &[(SshKeyRecord, usize)],
    selected_key_id: Option<&str>,
    selected_key_public: Option<&str>,
    copy_cmd_expanded: bool,
    key_editing: bool,
    key_delete_confirming: bool,
    key_name_editor: &ViewHandle<EditorView>,
    key_passphrase_editor: &ViewHandle<EditorView>,
    recent: &[RecentHostSnapshot],
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let groups = state.groups_for_render();
    let filtered = state.filtered_hosts();

    view_states.group_nav.ensure_group_count(groups.len());
    view_states
        .group_nav
        .ensure_tag_count(state.snapshot.available_tags.len());
    view_states.host_cards.ensure_count(filtered.len());
    view_states.key_manager.ensure_count(keys.len());
    view_states.group_nav.ensure_recent_count(recent.len());

    let search = render_search_bar(
        &state.query,
        state.protocol_filter,
        state.view_mode,
        state.all_filtered_selected(),
        state.selected_count(),
        &view_states.search_bar,
        search_editor,
        sidebar_open,
        ui_font,
        appearance,
        hc,
    );

    let body: Box<dyn Element> = if state.view_mode == HostViewMode::Keys {
        render_key_manager_view(
            keys,
            selected_key_id,
            selected_key_public,
            copy_cmd_expanded,
            key_editing,
            key_delete_confirming,
            key_name_editor,
            key_passphrase_editor,
            &view_states.key_manager,
            ui_font,
            hc,
        )
    } else if filtered.is_empty() {
        Container::new(render_empty_state(ui_font, hc))
            .with_background_color(hc.panel_bg)
            .finish()
    } else {
        let card_grid = match state.view_mode {
            HostViewMode::Grid => render_card_grid(
                &filtered,
                state,
                &view_states.host_cards,
                ui_font,
                state.reorder_mode,
                rename_target,
                rename_editor,
                hc,
            ),
            HostViewMode::List => render_host_list(
                &filtered,
                state,
                &view_states.host_cards,
                ui_font,
                state.reorder_mode,
                rename_target,
                rename_editor,
                hc,
            ),
            HostViewMode::Status => {
                render_status_view(&filtered, fleet, &view_states.host_cards, ui_font, hc)
            }
            HostViewMode::Keys => warpui::elements::Empty::new().finish(),
        };

        let scrollbar_thumb = Fill::Solid(hc.scrollbar_thumb);
        let scrollbar_thumb_active = Fill::Solid(hc.scrollbar_thumb_active);
        let scrollbar_track = Fill::None;

        ClippedScrollable::vertical(
            view_states.scroll_state.clone(),
            card_grid,
            ScrollbarWidth::Custom(6.0),
            scrollbar_thumb,
            scrollbar_thumb_active,
            scrollbar_track,
        )
        .with_overlayed_scrollbar()
        .finish()
    };

    let show_bar = state.reorder_mode || state.selected_count() > 0;

    let mut content = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    content.add_child(search);
    content.add_child(Expanded::new(1.0, body).finish());
    if show_bar {
        let bar = if state.reorder_mode {
            render_reorder_bar(&view_states.selection_bar, ui_font, hc)
        } else {
            render_selection_bar(
                state.selected_count(),
                &view_states.selection_bar,
                ui_font,
                hc,
            )
        };
        content.add_child(bar);
    }

    let content_col = Container::new(content.finish())
        .with_background_color(hc.panel_bg)
        .finish();

    if sidebar_open {
        let search_box = search_bar::render_search_input(
            &view_states.search_bar.search_input_state,
            search_editor,
            appearance,
            hc,
        );
        let nav = render_group_nav(
            &groups,
            search_box,
            recent,
            &state.snapshot.available_tags,
            &state.selected_tags,
            state.view_mode,
            &view_states.group_nav,
            ui_font,
            hc,
        );

        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(nav)
            .with_child(Expanded::new(1.0, content_col).finish())
            .finish()
    } else {
        content_col
    }
}

const GRID_COLUMNS: usize = 3;

fn render_card_grid(
    hosts: &[nexshell::host_management::HostCardSnapshot],
    state: &HostManagementState,
    card_states: &HostCardStates,
    ui_font: fonts::FamilyId,
    reorder_mode: bool,
    rename_target: Option<&str>,
    rename_editor: &ViewHandle<EditorView>,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    if hosts.is_empty() {
        return render_empty_state(ui_font, hc);
    }

    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    for row_start in (0..hosts.len()).step_by(GRID_COLUMNS) {
        if row_start > 0 {
            col.add_child(
                ConstrainedBox::new(warpui::elements::Empty::new().finish())
                    .with_height(CARD_SPACING)
                    .finish(),
            );
        }

        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start);

        let row_end = (row_start + GRID_COLUMNS).min(hosts.len());
        for index in row_start..row_end {
            if index > row_start {
                row.add_child(
                    ConstrainedBox::new(warpui::elements::Empty::new().finish())
                        .with_width(CARD_SPACING)
                        .finish(),
                );
            }
            let host = &hosts[index];
            let selected = state.selected_host_ids.contains(&host.id)
                || state.context_menu_target.as_deref() == Some(host.id.as_str());
            let card = render_host_card(
                host,
                index,
                card_states,
                ui_font,
                state.privacy_mode,
                selected,
                state.view_mode,
                rename_target,
                rename_editor,
                hc,
            );

            let card = if reorder_mode {
                wrap_draggable_card(card, index, &host.id, &card_states.draggable_states[index])
            } else {
                card
            };

            row.add_child(Expanded::new(1.0, card).finish());
        }

        for _ in 0..(GRID_COLUMNS - (row_end - row_start)) {
            row.add_child(
                ConstrainedBox::new(warpui::elements::Empty::new().finish())
                    .with_width(CARD_SPACING)
                    .finish(),
            );
            row.add_child(Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish());
        }

        col.add_child(row.finish());
    }

    Container::new(col.finish())
        .with_horizontal_padding(24.0)
        .with_vertical_padding(CARD_SPACING)
        .with_background_color(hc.panel_bg)
        .finish()
}

fn render_host_list(
    hosts: &[nexshell::host_management::HostCardSnapshot],
    state: &HostManagementState,
    card_states: &HostCardStates,
    ui_font: fonts::FamilyId,
    reorder_mode: bool,
    rename_target: Option<&str>,
    rename_editor: &ViewHandle<EditorView>,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    if hosts.is_empty() {
        return render_empty_state(ui_font, hc);
    }

    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    col.add_child(render_list_header(ui_font, hc));

    for (index, host) in hosts.iter().enumerate() {
        let selected = state.selected_host_ids.contains(&host.id)
            || state.context_menu_target.as_deref() == Some(host.id.as_str());
        let row = render_host_list_row(
            host,
            index,
            card_states,
            ui_font,
            state.privacy_mode,
            selected,
            rename_target,
            rename_editor,
            hc,
        );

        let row = if reorder_mode {
            wrap_draggable_list_row(row, index, &host.id, &card_states.draggable_states[index])
        } else {
            row
        };

        col.add_child(row);
    }

    Container::new(col.finish())
        .with_background_color(hc.panel_bg)
        .finish()
}

fn wrap_draggable_card(
    card: Box<dyn Element>,
    index: usize,
    host_id: &str,
    draggable_state: &DraggableState,
) -> Box<dyn Element> {
    let host_id = host_id.to_string();
    let draggable = Draggable::new(draggable_state.clone(), card)
        .on_drag_start(|ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::HostStartCardDrag);
        })
        .on_drag(move |ctx, _, card_position, _| {
            ctx.dispatch_typed_action(TerminalGridAction::HostDragCard {
                host_id: host_id.clone(),
                card_position,
            });
        })
        .on_drop(|ctx, _, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::HostDropCard);
        })
        .finish();
    let position_id = format!("host_card_position_{index}");
    SavePosition::new(draggable, &position_id).finish()
}

fn wrap_draggable_list_row(
    row: Box<dyn Element>,
    index: usize,
    host_id: &str,
    draggable_state: &DraggableState,
) -> Box<dyn Element> {
    let host_id = host_id.to_string();
    let draggable = Draggable::new(draggable_state.clone(), row)
        .on_drag_start(|ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::HostStartCardDrag);
        })
        .on_drag(move |ctx, _, card_position, _| {
            ctx.dispatch_typed_action(TerminalGridAction::HostDragCard {
                host_id: host_id.clone(),
                card_position,
            });
        })
        .on_drop(|ctx, _, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::HostDropCard);
        })
        .with_drag_axis(DragAxis::VerticalOnly)
        .finish();
    let position_id = format!("host_card_position_{index}");
    SavePosition::new(draggable, &position_id).finish()
}

fn render_empty_state(ui_font: fonts::FamilyId, hc: &HostUiColors) -> Box<dyn Element> {
    warpui::elements::Align::new(
        Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                warpui::elements::Text::new_inline(
                    rust_i18n::t!("host_empty").to_string(),
                    ui_font,
                    16.0,
                )
                .with_color(hc.text_secondary)
                .finish(),
            )
            .with_child(
                warpui::elements::Container::new(
                    warpui::elements::Text::new_inline(
                        rust_i18n::t!("host_empty_hint").to_string(),
                        ui_font,
                        UI_FONT_SIZE,
                    )
                    .with_color(hc.text_secondary)
                    .finish(),
                )
                .with_margin_top(8.0)
                .finish(),
            )
            .finish(),
    )
    .finish()
}
