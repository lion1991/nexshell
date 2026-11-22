//! 终端页左侧常驻主机列表（ADR 0023）：按分组折叠，单击即连 / 跳到已开标签。
//! 数据由 root_view/host_list_section 按 host_list_helpers 算好后传进来，本文件只管画。

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use warpui::{
    color::ColorU,
    elements::{
        Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
        CornerRadius, CrossAxisAlignment, Empty, Expanded, Fill, Flex, Hoverable, Icon,
        MainAxisSize, MouseState, MouseStateHandle, ParentElement, Radius, ScrollbarWidth, Text,
    },
    fonts,
    text_layout::ClipConfig,
    Element,
};

use super::host_card::{protocol_colors, system_icon_path};
use crate::host_list_helpers::HostOpenState;
use crate::host_management_view::constants::*;
use crate::terminal_grid_element::TerminalGridAction;
use nexshell::host_management::HostCardSnapshot;
use nexshell::ui_anim::TransitionMap;

const ROW_HEIGHT: f32 = 28.0;
const PILL_INSET: f32 = 8.0;
const PILL_RADIUS: f32 = 6.0;
/// 有分组标题时主机行相对标题缩进，和标题文字对齐。
const ROW_INDENT: f32 = 18.0;
const OPEN_DOT_SIZE: f32 = 6.0;

#[derive(Default)]
pub struct HostListNavStates {
    group_states: HashMap<String, MouseStateHandle>,
    host_states: HashMap<String, MouseStateHandle>,
    /// 行 hover / 当前高亮背景过渡（key = "hl-g:{分组}" / "hl-h:{主机}"）。
    pub hover_transitions: RefCell<TransitionMap<String>>,
    scroll_state: ClippedScrollStateHandle,
}

fn mouse_state(map: &mut HashMap<String, MouseStateHandle>, key: &str) -> MouseStateHandle {
    map.entry(key.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
        .clone()
}

fn is_hovered(state: &MouseStateHandle) -> bool {
    state.lock().map(|s| s.is_hovered()).unwrap_or(false)
}

pub struct HostListNavRow<'a> {
    pub host: &'a HostCardSnapshot,
    pub open: Option<HostOpenState>,
    /// 当前标签对应的主机。
    pub current: bool,
}

pub struct HostListNavSection<'a> {
    pub key: &'a str,
    pub label: String,
    pub collapsed: bool,
    pub rows: Vec<HostListNavRow<'a>>,
}

pub fn render_host_list_nav(
    sections: &[HostListNavSection<'_>],
    show_headers: bool,
    states: &mut HostListNavStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    prune_states(sections, states);

    let mut list = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for (index, section) in sections.iter().enumerate() {
        if show_headers {
            let state = mouse_state(&mut states.group_states, section.key);
            list.add_child(render_section_header(
                section,
                index > 0,
                state,
                &states.hover_transitions,
                ui_font,
                hc,
            ));
            if section.collapsed {
                continue;
            }
        }
        let indent = if show_headers { ROW_INDENT } else { 0.0 };
        for row in &section.rows {
            let state = mouse_state(&mut states.host_states, &row.host.id);
            list.add_child(render_host_row(
                row,
                indent,
                state,
                &states.hover_transitions,
                ui_font,
                hc,
            ));
        }
    }

    let scroll = ClippedScrollable::vertical(
        states.scroll_state.clone(),
        Container::new(list.finish())
            .with_padding_top(8.0)
            .with_padding_bottom(8.0)
            .finish(),
        ScrollbarWidth::Custom(4.0),
        Fill::Solid(hc.scrollbar_thumb),
        Fill::Solid(hc.scrollbar_thumb_active),
        Fill::None,
    )
    .with_overlayed_scrollbar()
    .finish();

    Container::new(
        ConstrainedBox::new(
            Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(Expanded::new(1.0, scroll).finish())
                .finish(),
        )
        .with_width(SIDEBAR_WIDTH)
        .finish(),
    )
    .with_background_color(hc.sidebar_bg)
    .with_border(Border::right(1.0).with_border_color(hc.sidebar_border))
    .finish()
}

/// 删掉已消失主机 / 分组的 hover 状态和过渡条目。
fn prune_states(sections: &[HostListNavSection<'_>], states: &mut HostListNavStates) {
    let group_keys: std::collections::HashSet<&str> = sections.iter().map(|s| s.key).collect();
    let host_ids: std::collections::HashSet<&str> = sections
        .iter()
        .flat_map(|s| s.rows.iter().map(|r| r.host.id.as_str()))
        .collect();
    states
        .group_states
        .retain(|key, _| group_keys.contains(key.as_str()));
    states
        .host_states
        .retain(|id, _| host_ids.contains(id.as_str()));
    states
        .hover_transitions
        .borrow_mut()
        .retain(|key| match key.split_once(':') {
            Some(("hl-g", key)) => group_keys.contains(key),
            Some(("hl-h", id)) => host_ids.contains(id),
            _ => false,
        });
}

fn pill_bg(transitions: &RefCell<TransitionMap<String>>, key: String, target: ColorU) -> ColorU {
    let now = std::time::Instant::now();
    let mut t = transitions.borrow_mut();
    t.retarget(key.clone(), target, now);
    t.sample(&key, now).unwrap_or(target)
}

fn render_section_header(
    section: &HostListNavSection<'_>,
    gap_above: bool,
    state: MouseStateHandle,
    transitions: &RefCell<TransitionMap<String>>,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let target = if is_hovered(&state) {
        hc.group_hover_bg
    } else {
        hc.sidebar_bg
    };
    let bg = pill_bg(transitions, format!("hl-g:{}", section.key), target);
    let chevron = if section.collapsed {
        ICON_CHEVRON_RIGHT
    } else {
        ICON_CHEVRON_DOWN
    };
    let label = section.label.clone();
    let count = section.rows.len().to_string();
    let key = section.key.to_string();

    Hoverable::new(state, move |_| {
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Container::new(
                    ConstrainedBox::new(Icon::new(chevron, hc.text_secondary).finish())
                        .with_width(12.0)
                        .with_height(12.0)
                        .finish(),
                )
                .with_margin_right(6.0)
                .finish(),
            )
            .with_child(
                Expanded::new(
                    1.0,
                    Text::new_inline(label.clone(), ui_font, UI_FONT_SIZE_SMALL)
                        .with_color(hc.text_secondary)
                        .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
                        .with_clip(ClipConfig::end())
                        .soft_wrap(false)
                        .finish(),
                )
                .finish(),
            )
            .with_child(
                Text::new_inline(count.clone(), ui_font, UI_FONT_SIZE_SMALL)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .finish();
        Container::new(
            Container::new(ConstrainedBox::new(row).with_height(ROW_HEIGHT).finish())
                .with_padding_left(8.0)
                .with_padding_right(10.0)
                .with_background_color(bg)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(PILL_RADIUS)))
                .finish(),
        )
        .with_margin_left(PILL_INSET)
        .with_margin_right(PILL_INSET)
        .with_margin_top(if gap_above { 6.0 } else { 0.0 })
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_hover(|_, ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::WakeUiAnim);
    })
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostListToggleSection(key.clone()));
    })
    .finish()
}

fn render_host_row(
    row: &HostListNavRow<'_>,
    indent: f32,
    state: MouseStateHandle,
    transitions: &RefCell<TransitionMap<String>>,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let host = row.host;
    let target = if row.current {
        hc.group_selected_bg
    } else if is_hovered(&state) {
        hc.group_hover_bg
    } else {
        hc.sidebar_bg
    };
    let bg = pill_bg(transitions, format!("hl-h:{}", host.id), target);
    let (icon_bg, icon_fg) = protocol_colors(&host.protocol, &hc);
    let icon_path = system_icon_path(host.system);
    let name = host.name.clone();
    let name_color = if row.current {
        hc.text_accent
    } else {
        hc.text_primary
    };
    let dot_color = row.open.map(|open| match open {
        HostOpenState::Connected => hc.semantic.ok,
        HostOpenState::Disconnected => hc.semantic.danger,
    });
    let host_id = host.id.clone();

    Hoverable::new(state, move |_| {
        let mut line = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Container::new(
                    Container::new(
                        ConstrainedBox::new(Icon::new(icon_path, icon_fg).finish())
                            .with_width(12.0)
                            .with_height(12.0)
                            .finish(),
                    )
                    .with_uniform_padding(4.0)
                    .with_background_color(icon_bg)
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(5.0)))
                    .finish(),
                )
                .with_margin_right(8.0)
                .finish(),
            )
            .with_child(
                Expanded::new(
                    1.0,
                    Text::new_inline(name.clone(), ui_font, UI_FONT_SIZE)
                        .with_color(name_color)
                        .with_clip(ClipConfig::end())
                        .soft_wrap(false)
                        .finish(),
                )
                .finish(),
            );
        if let Some(color) = dot_color {
            line.add_child(
                Container::new(
                    ConstrainedBox::new(
                        Container::new(Empty::new().finish())
                            .with_background_color(color)
                            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(
                                OPEN_DOT_SIZE / 2.0,
                            )))
                            .finish(),
                    )
                    .with_width(OPEN_DOT_SIZE)
                    .with_height(OPEN_DOT_SIZE)
                    .finish(),
                )
                .with_margin_left(6.0)
                .finish(),
            );
        }
        Container::new(
            Container::new(
                ConstrainedBox::new(line.finish())
                    .with_height(ROW_HEIGHT)
                    .finish(),
            )
            .with_padding_left(8.0 + indent)
            .with_padding_right(10.0)
            .with_background_color(bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(PILL_RADIUS)))
            .finish(),
        )
        .with_margin_left(PILL_INSET)
        .with_margin_right(PILL_INSET)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_hover(|_, ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::WakeUiAnim);
    })
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostListOpen(host_id.clone()));
    })
    .finish()
}
