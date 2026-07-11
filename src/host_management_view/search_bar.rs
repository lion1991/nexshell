use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use nexshell::text_editor::EditorView;
use warp_core::ui::appearance::Appearance;
use warpui::{
    color::ColorU,
    elements::{
        Align, Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
        EventDispatchMode, Expanded, Flex, Hoverable, Icon, MainAxisSize, MouseState,
        MouseStateHandle, ParentElement, Radius, SavePosition, Stack, Text,
    },
    fonts,
    ui_components::components::{Coords, UiComponent, UiComponentStyles},
    Element, ViewHandle,
};

use crate::host_management_view::constants::*;
use crate::terminal_grid_element::TerminalGridAction;
use crate::warp_dropdown::{render_warp_dropdown, WarpDropdownOption, WarpDropdownProps};
use nexshell::host_management::{HostViewMode, ProtocolFilter};
use nexshell::ui_anim::{SpringAnim, TransitionMap};

/// 视图切换分段控件：单格宽高，thumb 与按钮共用。
const VIEW_MODE_SEGMENT_WIDTH: f32 = 32.0;
const VIEW_MODE_SEGMENT_HEIGHT: f32 = 28.0;

pub struct SearchBarStates {
    pub search_input_state: MouseStateHandle,
    pub refresh_state: MouseStateHandle,
    pub select_all_state: MouseStateHandle,
    pub grid_state: MouseStateHandle,
    pub list_state: MouseStateHandle,
    pub status_state: MouseStateHandle,
    pub protocol_state: MouseStateHandle,
    pub protocol_dropdown_open: bool,
    pub protocol_item_states: RefCell<BTreeMap<String, MouseStateHandle>>,
    pub privacy_state: MouseStateHandle,
    pub import_state: MouseStateHandle,
    pub export_state: MouseStateHandle,
    pub sync_state: MouseStateHandle,
    pub new_host_state: MouseStateHandle,
    pub connect_selected_state: MouseStateHandle,
    pub move_selected_state: MouseStateHandle,
    pub delete_selected_state: MouseStateHandle,
    /// 视图切换 thumb 弹簧位移（液态玻璃滑动）。
    pub view_mode_thumb: RefCell<SpringAnim>,
    pub view_mode_thumb_init: Cell<bool>,
    /// 三格图标颜色过渡，key = 段索引。
    pub view_mode_icon_transitions: RefCell<TransitionMap<usize>>,
}

impl SearchBarStates {
    pub fn new() -> Self {
        Self {
            search_input_state: Arc::new(Mutex::new(MouseState::default())),
            refresh_state: Arc::new(Mutex::new(MouseState::default())),
            select_all_state: Arc::new(Mutex::new(MouseState::default())),
            grid_state: Arc::new(Mutex::new(MouseState::default())),
            list_state: Arc::new(Mutex::new(MouseState::default())),
            status_state: Arc::new(Mutex::new(MouseState::default())),
            protocol_state: Arc::new(Mutex::new(MouseState::default())),
            protocol_dropdown_open: false,
            protocol_item_states: RefCell::new(BTreeMap::new()),
            privacy_state: Arc::new(Mutex::new(MouseState::default())),
            import_state: Arc::new(Mutex::new(MouseState::default())),
            export_state: Arc::new(Mutex::new(MouseState::default())),
            sync_state: Arc::new(Mutex::new(MouseState::default())),
            new_host_state: Arc::new(Mutex::new(MouseState::default())),
            connect_selected_state: Arc::new(Mutex::new(MouseState::default())),
            move_selected_state: Arc::new(Mutex::new(MouseState::default())),
            delete_selected_state: Arc::new(Mutex::new(MouseState::default())),
            view_mode_thumb: RefCell::new(SpringAnim::new(0.0)),
            view_mode_thumb_init: Cell::new(false),
            view_mode_icon_transitions: RefCell::new(TransitionMap::new()),
        }
    }
}

pub fn render_search_bar(
    _query: &str,
    protocol_filter: ProtocolFilter,
    view_mode: HostViewMode,
    all_selected: bool,
    selected_count: usize,
    states: &SearchBarStates,
    search_editor: &ViewHandle<EditorView>,
    sidebar_open: bool,
    ui_font: fonts::FamilyId,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center);

    // 侧边栏收起时搜索框落到工具栏左侧，保证始终能搜
    if !sidebar_open {
        row.add_child(render_search_input(
            &states.search_input_state,
            search_editor,
            appearance,
            hc,
        ));
    }

    row.add_child(
        Container::new(render_protocol_filter(
            protocol_filter,
            &states.protocol_state,
            states.protocol_dropdown_open,
            protocol_filter_options(protocol_filter, states),
            ui_font,
            appearance,
        ))
        .with_margin_left(12.0)
        .finish(),
    );

    row.add_child(Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish());

    // 全选 + 选择操作只在选择态出现（移出常驻栏）
    if selected_count > 0 {
        row.add_child(render_select_all_button(
            &states.select_all_state,
            all_selected,
            ui_font,
            appearance,
            hc,
        ));
        row.add_child(render_selection_actions(
            selected_count,
            states,
            ui_font,
            hc,
        ));
    }

    row.add_child(
        Container::new(render_view_mode_toggle(view_mode, states, ui_font, hc))
            .with_margin_left(12.0)
            .finish(),
    );
    row.add_child(
        Container::new(render_icon_button(
            &states.refresh_state,
            ICON_REFRESH,
            TerminalGridAction::HostRefresh,
            hc,
        ))
        .with_margin_left(8.0)
        .finish(),
    );
    row.add_child(toolbar_divider(hc));
    row.add_child(render_toolbar_actions(states, ui_font, hc));

    let bar = Container::new(row.finish())
        .with_horizontal_padding(SEARCH_BAR_HORIZONTAL_PADDING)
        .with_vertical_padding(SEARCH_BAR_VERTICAL_PADDING)
        .with_border(Border::bottom(1.0).with_border_color(hc.toolbar_border))
        .finish();

    // 通栏液态玻璃：与下方内容（滚动列表）同层叠放，需自开层保证模糊采样到已画内容（同 find_section.rs）。
    let bar = nexshell::glass_backdrop::GlassBackdrop::new(bar, 0.0, hc.panel_bg)
        .with_glass(nexshell::design_tokens::Glass::popover())
        .with_own_layer()
        .finish();

    // 定高锁死：overlay 下 Align 传来的 max.y 是有限全高，行内 Expanded 占位（Empty 取 max）
    // 会把交叉轴撑满整页，玻璃跟着铺满。同 find_section.rs 的定高盒做法。
    ConstrainedBox::new(bar)
        .with_height(SEARCH_BAR_TOTAL_HEIGHT)
        .finish()
}

pub fn render_search_input(
    state: &MouseStateHandle,
    editor: &ViewHandle<EditorView>,
    appearance: &Appearance,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let input = appearance
        .ui_builder()
        .text_input(editor.clone())
        .with_style(UiComponentStyles {
            background: Some(hc.search_bar_bg.into()),
            border_width: Some(0.0),
            font_color: Some(hc.text_primary),
            height: Some(BUTTON_HEIGHT),
            padding: Some(Coords {
                top: 7.0,
                bottom: 7.0,
                left: 0.0,
                right: 0.0,
            }),
            ..Default::default()
        })
        .build()
        .finish();

    let input_shell = Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Container::new(
                    ConstrainedBox::new(Icon::new(ICON_SEARCH, hc.text_secondary).finish())
                        .with_width(ICON_SIZE_SM)
                        .with_height(ICON_SIZE_SM)
                        .finish(),
                )
                .with_margin_right(8.0)
                .finish(),
            )
            .with_child(Expanded::new(1.0, Stack::new().with_child(input).finish()).finish())
            .finish(),
    )
    .with_horizontal_padding(12.0)
    .with_vertical_padding(0.0)
    .with_background_color(hc.search_bar_bg)
    .with_border(Border::all(1.0).with_border_color(hc.search_bar_border))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
    .finish();

    ConstrainedBox::new(crate::input_cursor::text_input_ibeam_cursor_shell(
        state.clone(),
        input_shell,
    ))
    .with_width(216.0)
    .with_height(BUTTON_HEIGHT)
    .finish()
}

fn render_protocol_filter(
    filter: ProtocolFilter,
    state: &MouseStateHandle,
    is_open: bool,
    options: Vec<WarpDropdownOption<TerminalGridAction>>,
    _ui_font: fonts::FamilyId,
    appearance: &Appearance,
) -> Box<dyn Element> {
    ConstrainedBox::new(render_warp_dropdown(WarpDropdownProps {
        position_id: "host_management_protocol_dropdown_top_bar",
        label: filter.label().to_string(),
        state,
        is_open,
        options,
        toggle_action: TerminalGridAction::HostToggleProtocolDropdown,
        appearance,
        menu_width: 120.0,
        top_bar_height: BUTTON_HEIGHT,
    }))
    .with_width(120.0)
    .with_height(BUTTON_HEIGHT)
    .finish()
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

fn protocol_filter_options(
    current: ProtocolFilter,
    states: &SearchBarStates,
) -> Vec<WarpDropdownOption<TerminalGridAction>> {
    [
        ProtocolFilter::All,
        ProtocolFilter::Ssh,
        ProtocolFilter::Serial,
        ProtocolFilter::Rdp,
    ]
    .into_iter()
    .map(|filter| {
        let key = filter.label().to_string();
        WarpDropdownOption {
            label: key.clone(),
            action: TerminalGridAction::HostSetProtocolFilter(filter),
            selected: filter == current,
            state: dropdown_item_state(&states.protocol_item_states, key),
            icon_path: None,
            shortcut: None,
        }
    })
    .collect()
}

fn render_icon_button(
    state: &MouseStateHandle,
    icon_path: &'static str,
    action: TerminalGridAction,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();

    Hoverable::new(state, move |mouse| {
        let bg = if mouse.is_hovered() {
            hc.group_hover_bg
        } else {
            ColorU::transparent_black()
        };

        Container::new(
            ConstrainedBox::new(
                Align::new(
                    ConstrainedBox::new(Icon::new(icon_path, hc.text_secondary).finish())
                        .with_width(ICON_SIZE_SM)
                        .with_height(ICON_SIZE_SM)
                        .finish(),
                )
                .finish(),
            )
            .with_width(BUTTON_HEIGHT)
            .with_height(BUTTON_HEIGHT)
            .finish(),
        )
        .with_background_color(bg)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
        .with_border(Border::all(1.0).with_border_color(if mouse.is_hovered() {
            hc.search_bar_border
        } else {
            ColorU::transparent_black()
        }))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}

fn render_select_all_button(
    state: &MouseStateHandle,
    all_selected: bool,
    _ui_font: fonts::FamilyId,
    appearance: &Appearance,
    _hc: &HostUiColors,
) -> Box<dyn Element> {
    Container::new(
        appearance
            .ui_builder()
            .checkbox(state.clone(), None)
            .check(all_selected)
            .with_label(
                appearance
                    .ui_builder()
                    .span(rust_i18n::t!("host_select_all")),
            )
            .build()
            .with_cursor(warpui::platform::Cursor::PointingHand)
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::HostToggleSelectAll);
            })
            .finish(),
    )
    .with_margin_left(-7.0)
    .finish()
}

fn render_view_mode_toggle(
    view_mode: HostViewMode,
    states: &SearchBarStates,
    _ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    // Keys/Containers 视图不属于这三格，thumb 隐藏且弹簧保持不动。
    let active_index = match view_mode {
        HostViewMode::Grid => Some(0),
        HostViewMode::List => Some(1),
        HostViewMode::Status => Some(2),
        HostViewMode::Containers | HostViewMode::Keys => None,
    };

    let grid_btn = render_view_mode_button(
        &states.grid_state,
        ICON_GRID_VIEW,
        0,
        active_index == Some(0),
        TerminalGridAction::HostSetViewMode(HostViewMode::Grid),
        states,
        hc,
    );
    let list_btn = render_view_mode_button(
        &states.list_state,
        ICON_LIST_VIEW,
        1,
        active_index == Some(1),
        TerminalGridAction::HostSetViewMode(HostViewMode::List),
        states,
        hc,
    );
    let status_btn = render_view_mode_button(
        &states.status_state,
        ICON_ACTIVITY,
        2,
        active_index == Some(2),
        TerminalGridAction::HostSetViewMode(HostViewMode::Status),
        states,
        hc,
    );
    let button_row = Flex::row()
        .with_child(grid_btn)
        .with_child(list_btn)
        .with_child(status_btn)
        .finish();

    let mut stack = Stack::new().with_event_dispatch_mode(EventDispatchMode::Waterfall);
    if let Some(index) = active_index {
        let target_x = index as f32 * VIEW_MODE_SEGMENT_WIDTH;
        let x = {
            let mut thumb = states.view_mode_thumb.borrow_mut();
            if states.view_mode_thumb_init.get() {
                thumb.set_target(target_x);
            } else {
                // 首帧直接落位，不从 0 滑过去。
                thumb.snap(target_x);
                states.view_mode_thumb_init.set(true);
            }
            // 取整到整像素，防止收敛尾部亚像素抖动发虚（同 tab 底条）。
            thumb.sample(Instant::now()).round()
        };
        // thumb 底层，尺寸/圆角与按钮自画背景块同规格，margin_left 定位。
        stack.add_child(
            Container::new(
                ConstrainedBox::new(warpui::elements::Empty::new().finish())
                    .with_width(VIEW_MODE_SEGMENT_WIDTH)
                    .with_height(VIEW_MODE_SEGMENT_HEIGHT)
                    .finish(),
            )
            .with_margin_left(x)
            .with_background_color(hc.card_bg_hover)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(
                BUTTON_CORNER_RADIUS - 1.0,
            )))
            .finish(),
        );
    }
    stack.add_child(button_row);

    Container::new(
        // 锁死内容高度，保证 thumb 与三按钮顶边对齐，消除 Stack 约束歧义。
        ConstrainedBox::new(stack.finish())
            .with_height(VIEW_MODE_SEGMENT_HEIGHT)
            .finish(),
    )
    // 玻璃工具栏上用半透明 inset 底，实色块会挡玻璃穿透。
    .with_background_color(hc.toolbar_inset_bg)
    .with_border(Border::all(1.0).with_border_color(hc.search_bar_border))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
    .finish()
}

fn render_view_mode_button(
    state: &MouseStateHandle,
    icon_path: &'static str,
    index: usize,
    is_active: bool,
    action: TerminalGridAction,
    states: &SearchBarStates,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();
    let target_color = if is_active {
        hc.text_primary
    } else {
        hc.text_secondary
    };
    let icon_color = {
        let now = Instant::now();
        let mut transitions = states.view_mode_icon_transitions.borrow_mut();
        transitions.retarget(index, target_color, now);
        transitions.sample(&index, now).unwrap_or(target_color)
    };

    Hoverable::new(state, move |mouse| {
        // active 底色交给 thumb，这里只在非 active 时画 hover 底。
        let bg = if !is_active && mouse.is_hovered() {
            hc.group_hover_bg
        } else {
            ColorU::transparent_black()
        };

        Container::new(
            ConstrainedBox::new(
                Align::new(
                    ConstrainedBox::new(Icon::new(icon_path, icon_color).finish())
                        .with_width(ICON_SIZE_SM)
                        .with_height(ICON_SIZE_SM)
                        .finish(),
                )
                .finish(),
            )
            .with_width(VIEW_MODE_SEGMENT_WIDTH)
            .with_height(VIEW_MODE_SEGMENT_HEIGHT)
            .finish(),
        )
        .with_background_color(bg)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_hover(|_, ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::WakeUiAnim);
    })
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}

fn render_toolbar_actions(
    states: &SearchBarStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Container::new(render_toolbar_action_button(
                &states.privacy_state,
                ICON_EYE,
                rust_i18n::t!("host_privacy").to_string(),
                ui_font,
                TerminalGridAction::HostTogglePrivacy,
                hc,
            ))
            .with_margin_left(4.0)
            .finish(),
        )
        .with_child(
            Container::new(render_toolbar_action_button(
                &states.import_state,
                ICON_DOWNLOAD,
                rust_i18n::t!("host_import").to_string(),
                ui_font,
                TerminalGridAction::HostImport,
                hc,
            ))
            .with_margin_left(4.0)
            .finish(),
        )
        .with_child(
            Container::new(render_toolbar_action_button(
                &states.export_state,
                ICON_UPLOAD,
                rust_i18n::t!("host_export").to_string(),
                ui_font,
                TerminalGridAction::HostExport,
                hc,
            ))
            .with_margin_left(4.0)
            .finish(),
        )
        .with_child(
            Container::new(render_toolbar_action_button(
                &states.sync_state,
                ICON_CLOUD,
                rust_i18n::t!("host_cloud_sync").to_string(),
                ui_font,
                TerminalGridAction::HostCloudSync,
                hc,
            ))
            .with_margin_left(4.0)
            .finish(),
        )
        .with_child(toolbar_divider(hc))
        .with_child(render_new_host_button(&states.new_host_state, ui_font, hc))
        .finish()
}

// 工具栏竖向细分隔线，分区用。
fn toolbar_divider(hc: &HostUiColors) -> Box<dyn Element> {
    Container::new(
        ConstrainedBox::new(
            Container::new(warpui::elements::Empty::new().finish())
                .with_background_color(hc.toolbar_border)
                .finish(),
        )
        .with_width(1.0)
        .with_height(20.0)
        .finish(),
    )
    .with_margin_left(8.0)
    .with_margin_right(8.0)
    .finish()
}

// 次级操作纯图标按钮（隐私/导入/导出/云同步），hover 出背景 + tooltip 显示名字。
fn render_toolbar_action_button(
    state: &MouseStateHandle,
    icon_path: &'static str,
    label: String,
    ui_font: fonts::FamilyId,
    action: TerminalGridAction,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();

    Hoverable::new(state, move |mouse| {
        let color = if mouse.is_hovered() {
            hc.text_primary
        } else {
            hc.text_secondary
        };
        let button = SavePosition::new(
            Container::new(
                ConstrainedBox::new(Icon::new(icon_path, color).finish())
                    .with_width(ICON_SIZE_SM)
                    .with_height(ICON_SIZE_SM)
                    .finish(),
            )
            .with_horizontal_padding(7.0)
            .with_vertical_padding(7.0)
            .with_background_color(if mouse.is_hovered() {
                hc.group_hover_bg
            } else {
                ColorU::transparent_black()
            })
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
            .finish(),
            icon_path,
        )
        .finish();
        if mouse.is_hovered() {
            crate::file_panel_view_helpers::file_panel_name_tooltip(
                button,
                icon_path,
                label.clone(),
                ui_font,
                hc.card_bg,
                hc.text_primary,
            )
        } else {
            button
        }
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}

fn render_new_host_button(
    state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();

    Hoverable::new(state, move |mouse| {
        let bg = if mouse.is_hovered() {
            ColorU::new(
                (hc.accent_bg.r as u16 + 20).min(255) as u8,
                (hc.accent_bg.g as u16 + 20).min(255) as u8,
                (hc.accent_bg.b as u16 + 20).min(255) as u8,
                255,
            )
        } else {
            hc.accent_bg
        };

        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    ConstrainedBox::new(Icon::new(ICON_PLUS, hc.accent_text).finish())
                        .with_width(ICON_SIZE_SM)
                        .with_height(ICON_SIZE_SM)
                        .finish(),
                )
                .with_child(
                    Container::new(
                        Text::new_inline(
                            rust_i18n::t!("host_new").to_string(),
                            ui_font,
                            UI_FONT_SIZE,
                        )
                        .with_color(hc.accent_text)
                        .finish(),
                    )
                    .with_margin_left(6.0)
                    .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(14.0)
        .with_vertical_padding(7.0)
        .with_background_color(bg)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BUTTON_CORNER_RADIUS)))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostNewHost);
    })
    .finish()
}

fn render_selection_actions(
    count: usize,
    states: &SearchBarStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let border_color = hc.search_bar_border;
    let sep = || -> Box<dyn Element> {
        Container::new(
            ConstrainedBox::new(
                Container::new(warpui::elements::Empty::new().finish())
                    .with_background_color(border_color)
                    .finish(),
            )
            .with_width(1.0)
            .with_height(20.0)
            .finish(),
        )
        .with_horizontal_padding(8.0)
        .finish()
    };

    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(sep())
        .with_child(
            Container::new(
                Text::new_inline(
                    rust_i18n::t!("host_selected_count", count = count).to_string(),
                    ui_font,
                    UI_FONT_SIZE,
                )
                .with_color(hc.text_accent)
                .finish(),
            )
            .with_margin_right(8.0)
            .finish(),
        )
        .with_child(render_action_btn(
            &states.connect_selected_state,
            ICON_PLAY,
            rust_i18n::t!("host_connect").to_string(),
            ui_font,
            TerminalGridAction::HostConnectSelected,
            hc,
        ))
        .with_child(render_action_btn(
            &states.move_selected_state,
            ICON_SWAP,
            rust_i18n::t!("host_move").to_string(),
            ui_font,
            TerminalGridAction::HostEnterReorderMode,
            hc,
        ))
        .with_child(render_action_btn(
            &states.delete_selected_state,
            ICON_TRASH,
            rust_i18n::t!("host_delete").to_string(),
            ui_font,
            TerminalGridAction::HostDeleteSelected,
            hc,
        ))
        .finish()
}

fn render_action_btn(
    state: &MouseStateHandle,
    icon_path: &'static str,
    label: String,
    ui_font: fonts::FamilyId,
    action: TerminalGridAction,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();

    Hoverable::new(state, move |mouse| {
        let color = if mouse.is_hovered() {
            hc.text_primary
        } else {
            hc.text_secondary
        };

        let mut inner = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                ConstrainedBox::new(Icon::new(icon_path, color).finish())
                    .with_width(14.0)
                    .with_height(14.0)
                    .finish(),
            );

        if !label.is_empty() {
            inner.add_child(
                Container::new(
                    Text::new_inline(label.clone(), ui_font, UI_FONT_SIZE)
                        .with_color(color)
                        .finish(),
                )
                .with_margin_left(4.0)
                .finish(),
            );
        }

        Container::new(inner.finish())
            .with_horizontal_padding(8.0)
            .with_vertical_padding(4.0)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .with_background_color(if mouse.is_hovered() {
                hc.card_bg_hover
            } else {
                ColorU::transparent_black()
            })
            .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}
