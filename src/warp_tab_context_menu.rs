//! Warp tab right-click menu rules used by the native-shell spike.
//!
//! This mirrors the structure of `warp/app/src/tab.rs`:
//! `menu_items_with_pane_name_target` builds menu sections in order, inserting
//! separators only between non-empty sections. The native spike currently has
//! local terminal tabs only, so the Warp sections that depend on product state
//! we do not carry yet (shared sessions and saved tab configs) are kept as
//! empty sections rather than re-shaped into a custom menu.

use pathfinder_geometry::vector::Vector2F;
use crate::menu::{MenuItem, MenuItemFields};
use warp_core::ui::theme::{AnsiColorIdentifier, AnsiColors};
use warpui::Action;

pub const TAB_COLOR_ICON_PATH: &str = "bundled/svg/ellipse.svg";
pub const TAB_NO_COLOR_ICON_PATH: &str = "bundled/svg/no_color_ellipse.svg";
pub const TAB_COLOR_OPTIONS: [AnsiColorIdentifier; 6] = [
    AnsiColorIdentifier::Red,
    AnsiColorIdentifier::Green,
    AnsiColorIdentifier::Yellow,
    AnsiColorIdentifier::Blue,
    AnsiColorIdentifier::Magenta,
    AnsiColorIdentifier::Cyan,
];

pub fn selected_tab_color_after_toggle(
    selected_color: Option<AnsiColorIdentifier>,
    color: AnsiColorIdentifier,
) -> Option<AnsiColorIdentifier> {
    if selected_color == Some(color) {
        None
    } else {
        Some(color)
    }
}

pub fn custom_title_from_editor(title: &str) -> Option<String> {
    Some(title.to_string()).filter(|title| !title.is_empty())
}

pub fn tab_rename_editor_top_margin(new_tab_styling: bool) -> f32 {
    if new_tab_styling {
        8.0
    } else {
        3.0
    }
}

pub fn should_finish_tab_rename_on_external_mouse_down(tab_being_renamed: Option<usize>) -> bool {
    tab_being_renamed.is_some()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabContextMenuAnchor {
    Pointer(Vector2F),
    VerticalTabsKebab,
}

#[derive(Clone, Debug)]
pub struct HorizontalTabColorOptions<A: Action + Clone> {
    pub selected_color: Option<AnsiColorIdentifier>,
    pub terminal_colors: AnsiColors,
    pub toggle_tab_color_actions: [A; TAB_COLOR_OPTIONS.len()],
}

#[derive(Clone, Debug)]
pub struct HorizontalTabContextMenuActions<A: Action + Clone> {
    pub rename_tab: Option<A>,
    pub reset_tab_name: Option<A>,
    pub duplicate_tab: Option<A>,
    pub move_tab_right: A,
    pub move_tab_left: A,
    pub close_tab: Option<A>,
    pub close_other_tabs: A,
    pub close_tabs_right: A,
    pub reconnect_tab: Option<A>,
    pub disconnect_tab: Option<A>,
    pub toggle_recording: Option<A>,
    /// 决定录制菜单项显示「开始录制」还是「停止录制」。
    pub is_recording: bool,
    pub save_current_tab_as_new_config: Option<A>,
    pub color_options: Option<HorizontalTabColorOptions<A>>,
}

#[derive(Clone, Debug)]
pub struct HorizontalTabContextMenuState<A: Action + Clone> {
    pub index: usize,
    pub tabs_len: usize,
    pub actions: HorizontalTabContextMenuActions<A>,
}

pub fn horizontal_tab_context_menu_items<A: Action + Clone>(
    index: usize,
    tabs_len: usize,
    close_window_enabled: bool,
    mut actions: HorizontalTabContextMenuActions<A>,
) -> Vec<MenuItem<A>> {
    if !close_window_enabled && tabs_len == 1 {
        actions.close_tab = None;
    }
    horizontal_tab_context_menu_items_with_state(HorizontalTabContextMenuState {
        index,
        tabs_len,
        actions,
    })
}

pub fn horizontal_tab_context_menu_items_with_state<A: Action + Clone>(
    state: HorizontalTabContextMenuState<A>,
) -> Vec<MenuItem<A>> {
    let mut menu_items = vec![];

    for section_items in [
        session_sharing_menu_items(),
        modify_tab_menu_items(&state),
        close_tab_menu_items(&state),
        reconnect_menu_items(&state),
        recording_menu_items(&state),
        save_config_menu_items(&state),
        color_option_menu_items(&state),
    ] {
        if section_items.is_empty() {
            continue;
        }
        if menu_items
            .last()
            .is_some_and(|item| !matches!(item, MenuItem::Separator))
        {
            menu_items.push(MenuItem::Separator);
        }
        menu_items.extend(section_items);
    }

    menu_items
}

fn session_sharing_menu_items<A: Action + Clone>() -> Vec<MenuItem<A>> {
    Vec::new()
}

fn modify_tab_menu_items<A: Action + Clone>(
    state: &HorizontalTabContextMenuState<A>,
) -> Vec<MenuItem<A>> {
    let mut menu_items = vec![];

    if let Some(rename_tab) = &state.actions.rename_tab {
        menu_items.push(
            MenuItemFields::new(rust_i18n::t!("tab_ctx_rename"))
                .with_on_select_action(rename_tab.clone())
                .into_item(),
        );
    }

    if let Some(reset_tab_name) = &state.actions.reset_tab_name {
        menu_items.push(
            MenuItemFields::new(rust_i18n::t!("tab_ctx_reset_name"))
                .with_on_select_action(reset_tab_name.clone())
                .into_item(),
        );
    }

    if let Some(duplicate_tab) = &state.actions.duplicate_tab {
        menu_items.push(
            MenuItemFields::new(rust_i18n::t!("tab_ctx_duplicate"))
                .with_on_select_action(duplicate_tab.clone())
                .into_item(),
        );
    }

    let not_last_tab = state.index != state.tabs_len.saturating_sub(1);
    if not_last_tab {
        menu_items.push(
            MenuItemFields::new(rust_i18n::t!("tab_ctx_move_right"))
                .with_on_select_action(state.actions.move_tab_right.clone())
                .into_item(),
        );
    }
    if state.index != 0 {
        menu_items.push(
            MenuItemFields::new(rust_i18n::t!("tab_ctx_move_left"))
                .with_on_select_action(state.actions.move_tab_left.clone())
                .into_item(),
        );
    }

    menu_items
}

fn close_tab_menu_items<A: Action + Clone>(
    state: &HorizontalTabContextMenuState<A>,
) -> Vec<MenuItem<A>> {
    let mut menu_items = vec![];

    if let Some(close_tab) = &state.actions.close_tab {
        menu_items.push(
            MenuItemFields::new(rust_i18n::t!("tab_ctx_close"))
                .with_on_select_action(close_tab.clone())
                .into_item(),
        );
    }
    if state.tabs_len > 1 {
        menu_items.push(
            MenuItemFields::new(rust_i18n::t!("tab_ctx_close_others"))
                .with_on_select_action(state.actions.close_other_tabs.clone())
                .into_item(),
        );
    }
    let not_last_tab = state.index != state.tabs_len.saturating_sub(1);
    if not_last_tab {
        menu_items.push(
            MenuItemFields::new(rust_i18n::t!("tab_ctx_close_right"))
                .with_on_select_action(state.actions.close_tabs_right.clone())
                .into_item(),
        );
    }

    menu_items
}

fn reconnect_menu_items<A: Action + Clone>(
    state: &HorizontalTabContextMenuState<A>,
) -> Vec<MenuItem<A>> {
    let mut items = Vec::new();
    if let Some(action) = &state.actions.disconnect_tab {
        items.push(
            MenuItemFields::new(rust_i18n::t!("tab_ctx_disconnect"))
                .with_on_select_action(action.clone())
                .into_item(),
        );
    }
    if let Some(action) = &state.actions.reconnect_tab {
        items.push(
            MenuItemFields::new(rust_i18n::t!("tab_ctx_reconnect"))
                .with_on_select_action(action.clone())
                .into_item(),
        );
    }
    items
}

fn recording_menu_items<A: Action + Clone>(
    state: &HorizontalTabContextMenuState<A>,
) -> Vec<MenuItem<A>> {
    let Some(action) = &state.actions.toggle_recording else {
        return Vec::new();
    };
    let label = if state.actions.is_recording {
        rust_i18n::t!("tab_ctx_record_stop")
    } else {
        rust_i18n::t!("tab_ctx_record_start")
    };
    vec![MenuItemFields::new(label)
        .with_on_select_action(action.clone())
        .into_item()]
}

fn save_config_menu_items<A: Action + Clone>(
    state: &HorizontalTabContextMenuState<A>,
) -> Vec<MenuItem<A>> {
    let Some(action) = &state.actions.save_current_tab_as_new_config else {
        return Vec::new();
    };
    vec![MenuItemFields::new(rust_i18n::t!("tab_ctx_save_config"))
        .with_on_select_action(action.clone())
        .into_item()]
}

fn color_option_menu_items<A: Action + Clone>(
    state: &HorizontalTabContextMenuState<A>,
) -> Vec<MenuItem<A>> {
    let Some(options) = &state.actions.color_options else {
        return Vec::new();
    };

    vec![MenuItem::ItemsRow {
        items: TAB_COLOR_OPTIONS
            .iter()
            .zip(options.toggle_tab_color_actions.iter())
            .map(|(color_option, action)| {
                let color = color_option.to_ansi_color(&options.terminal_colors);
                MenuItemFields::new_with_icon(
                    if options.selected_color == Some(*color_option) {
                        TAB_NO_COLOR_ICON_PATH
                    } else {
                        TAB_COLOR_ICON_PATH
                    },
                    color.into(),
                    color_option.to_string(),
                )
                .no_highlight_on_hover()
                .with_on_select_action(action.clone())
            })
            .collect(),
    }]
}
