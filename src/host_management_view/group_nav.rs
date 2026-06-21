use std::sync::{Arc, Mutex};

use warpui::{
    elements::{
        Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Expanded, Flex,
        Hoverable, Icon, MainAxisSize, MouseState, MouseStateHandle, ParentElement, Radius, Text,
        Wrap,
    },
    fonts, Element,
};

use crate::host_management_view::constants::*;
use crate::terminal_grid_element::TerminalGridAction;
use nexshell::host_management::{
    HostGroupSnapshot, HostViewMode, RecentHostSnapshot,
};

pub struct GroupNavStates {
    pub group_states: Vec<MouseStateHandle>,
    pub tag_states: Vec<MouseStateHandle>,
    pub recent_states: Vec<MouseStateHandle>,
    pub manage_button_state: MouseStateHandle,
    pub status_entry_state: MouseStateHandle,
    pub keys_entry_state: MouseStateHandle,
}

impl GroupNavStates {
    pub fn new() -> Self {
        Self {
            group_states: Vec::new(),
            tag_states: Vec::new(),
            recent_states: Vec::new(),
            manage_button_state: Arc::new(Mutex::new(MouseState::default())),
            status_entry_state: Arc::new(Mutex::new(MouseState::default())),
            keys_entry_state: Arc::new(Mutex::new(MouseState::default())),
        }
    }

    pub fn ensure_group_count(&mut self, count: usize) {
        while self.group_states.len() < count {
            self.group_states
                .push(Arc::new(Mutex::new(MouseState::default())));
        }
    }

    pub fn ensure_tag_count(&mut self, count: usize) {
        while self.tag_states.len() < count {
            self.tag_states
                .push(Arc::new(Mutex::new(MouseState::default())));
        }
    }

    pub fn ensure_recent_count(&mut self, count: usize) {
        while self.recent_states.len() < count {
            self.recent_states
                .push(Arc::new(Mutex::new(MouseState::default())));
        }
    }
}

pub fn render_group_nav(
    groups: &[HostGroupSnapshot],
    search_box: Box<dyn Element>,
    recent: &[RecentHostSnapshot],
    available_tags: &[String],
    selected_tags: &std::collections::BTreeSet<String>,
    view_mode: HostViewMode,
    states: &GroupNavStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    // 功能区（可扩展：未来在此追加更多功能菜单入口）
    col.add_child(render_function_item(
        "状态总览".to_string(),
        ICON_ACTIVITY,
        HostViewMode::Status,
        &states.status_entry_state,
        ui_font,
        hc,
    ));

    col.add_child(
        Container::new(search_box)
            .with_margin_left(12.0)
            .with_margin_right(12.0)
            .with_margin_bottom(4.0)
            .finish(),
    );

    col.add_child(
        Container::new(
            Text::new_inline(
                rust_i18n::t!("host_group_nav").to_string(),
                ui_font,
                UI_FONT_SIZE,
            )
            .with_color(hc.text_secondary)
            .finish(),
        )
        .with_padding_left(16.0)
        .with_padding_top(12.0)
        .with_padding_bottom(8.0)
        .with_border(Border::top(1.0).with_border_color(hc.sidebar_border))
        .finish(),
    );

    for (index, group) in groups.iter().enumerate() {
        col.add_child(render_group_item(group, index, states, ui_font, hc));
    }

    if !recent.is_empty() {
        col.add_child(render_section_title("最近访问", ui_font, hc));
        for (index, item) in recent.iter().enumerate() {
            col.add_child(render_recent_item(item, index, states, ui_font, hc));
        }
    }

    col.add_child(Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish());

    if !available_tags.is_empty() {
        col.add_child(
            Container::new(
                Text::new_inline(
                    rust_i18n::t!("host_tag_filter").to_string(),
                    ui_font,
                    UI_FONT_SIZE_SMALL,
                )
                .with_color(hc.text_secondary)
                .finish(),
            )
            .with_padding_left(16.0)
            .with_padding_top(8.0)
            .with_padding_bottom(6.0)
            .with_border(Border::top(1.0).with_border_color(hc.sidebar_border))
            .finish(),
        );

        let mut tag_wrap = Wrap::row().with_spacing(4.0).with_run_spacing(4.0);
        for (index, tag) in available_tags.iter().enumerate() {
            let is_selected = selected_tags.contains(tag);
            tag_wrap.extend(std::iter::once(render_tag_chip(
                tag,
                index,
                is_selected,
                states,
                ui_font,
                hc,
            )));
        }
        col.add_child(
            Container::new(tag_wrap.finish())
                .with_padding_left(16.0)
                .with_padding_right(8.0)
                .with_padding_bottom(8.0)
                .finish(),
        );
    }

    col.add_child(render_bottom_item(
        "密钥管理".to_string(),
        ICON_KEY,
        &states.keys_entry_state,
        TerminalGridAction::HostSetViewMode(HostViewMode::Keys),
        true,
        view_mode == HostViewMode::Keys,
        ui_font,
        hc,
    ));
    col.add_child(render_bottom_item(
        rust_i18n::t!("host_manage_groups_tags").to_string(),
        ICON_LINK,
        &states.manage_button_state,
        TerminalGridAction::HostManageGroupsTags,
        false,
        false,
        ui_font,
        hc,
    ));

    Container::new(
        ConstrainedBox::new(col.finish())
            .with_width(SIDEBAR_WIDTH)
            .finish(),
    )
    .with_background_color(hc.sidebar_bg)
    .with_border(Border::right(1.0).with_border_color(hc.sidebar_border))
    .finish()
}

fn render_group_item(
    group: &HostGroupSnapshot,
    index: usize,
    states: &GroupNavStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = states.group_states[index].clone();
    let is_selected = group.selected;
    let label = group.label.clone();
    let count = group.count;
    let group_id_click = group.id.clone();
    let is_all = group.id == "all";

    Hoverable::new(state, move |mouse| {
        let bg = if is_selected {
            hc.group_selected_bg
        } else if mouse.is_hovered() {
            hc.group_hover_bg
        } else {
            hc.sidebar_bg
        };

        let text_color = if is_selected {
            hc.text_accent
        } else {
            hc.text_primary
        };

        Container::new(
            ConstrainedBox::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Container::new(group_marker(is_all, &hc))
                            .with_padding_left(16.0)
                            .with_margin_right(10.0)
                            .finish(),
                    )
                    .with_child(
                        Text::new_inline(label.clone(), ui_font, UI_FONT_SIZE)
                            .with_color(text_color)
                            .finish(),
                    )
                    .with_child(
                        Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish(),
                    )
                    .with_child(
                        Container::new(
                            Text::new_inline(count.to_string(), ui_font, UI_FONT_SIZE)
                                .with_color(hc.text_secondary)
                                .finish(),
                        )
                        .with_padding_right(16.0)
                        .finish(),
                    )
                    .finish(),
            )
            .with_height(GROUP_ITEM_HEIGHT)
            .finish(),
        )
        .with_background_color(bg)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostSelectGroup(group_id_click.clone()));
    })
    .finish()
}

// 分组色标：所有主机用蓝色列表图标，普通分组用文件夹图标。
fn group_marker(is_all: bool, hc: &HostUiColors) -> Box<dyn Element> {
    let (icon, color) = if is_all {
        (ICON_LIST_VIEW, hc.text_accent)
    } else {
        (ICON_FOLDER, hc.text_secondary)
    };
    ConstrainedBox::new(Icon::new(icon, color).finish())
        .with_width(ICON_SIZE_SM)
        .with_height(ICON_SIZE_SM)
        .finish()
}

// 区块小标题（带顶部分隔线），如"最近访问"。
fn render_section_title(label: &str, ui_font: fonts::FamilyId, hc: &HostUiColors) -> Box<dyn Element> {
    Container::new(
        Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE)
            .with_color(hc.text_secondary)
            .finish(),
    )
    .with_padding_left(16.0)
    .with_padding_top(12.0)
    .with_padding_bottom(8.0)
    .with_border(Border::top(1.0).with_border_color(hc.sidebar_border))
    .finish()
}

// 最近访问项：>_ 图标 + 主机名 + (分组名 · 相对时间)，点击快连。
fn render_recent_item(
    item: &RecentHostSnapshot,
    index: usize,
    states: &GroupNavStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = states.recent_states[index].clone();
    let host_id = item.host_id.clone();
    let name = item.name.clone();
    let sub = match &item.group_name {
        Some(group) => format!("{} · {}", group, relative_time(item.accessed_at)),
        None => relative_time(item.accessed_at),
    };
    Hoverable::new(state, move |mouse| {
        let bg = if mouse.is_hovered() {
            hc.group_hover_bg
        } else {
            hc.sidebar_bg
        };
        Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Container::new(
                        ConstrainedBox::new(Icon::new(ICON_TERMINAL, hc.text_secondary).finish())
                            .with_width(ICON_SIZE_SM)
                            .with_height(ICON_SIZE_SM)
                            .finish(),
                    )
                    .with_padding_left(16.0)
                    .with_margin_right(10.0)
                    .finish(),
                )
                .with_child(
                    Flex::column()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_child(
                            Text::new_inline(name.clone(), ui_font, UI_FONT_SIZE)
                                .with_color(hc.text_primary)
                                .finish(),
                        )
                        .with_child(
                            Text::new_inline(sub.clone(), ui_font, UI_FONT_SIZE_SMALL)
                                .with_color(hc.text_secondary)
                                .finish(),
                        )
                        .finish(),
                )
                .finish(),
        )
        .with_vertical_padding(6.0)
        .with_background_color(bg)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostQuickConnect(host_id.clone()));
    })
    .finish()
}

// Unix 秒 → 相对时间中文。
fn relative_time(accessed_at: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(accessed_at);
    let diff = (now - accessed_at).max(0);
    if diff < 60 {
        "刚刚".to_string()
    } else if diff < 3600 {
        format!("{} 分钟前", diff / 60)
    } else if diff < 86_400 {
        format!("{} 小时前", diff / 3600)
    } else if diff < 172_800 {
        "昨天".to_string()
    } else {
        format!("{} 天前", diff / 86_400)
    }
}

fn render_tag_chip(
    tag: &str,
    index: usize,
    is_selected: bool,
    states: &GroupNavStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = states.tag_states[index].clone();
    let tag_str = tag.to_string();
    let tag_for_click = tag.to_string();

    let (bg, text_color) = if is_selected {
        (hc.tag_bg, hc.tag_text)
    } else {
        (hc.group_hover_bg, hc.text_secondary)
    };

    Hoverable::new(state, move |_| {
        Container::new(
            Text::new_inline(tag_str.clone(), ui_font, UI_FONT_SIZE_SMALL)
                .with_color(text_color)
                .finish(),
        )
        .with_horizontal_padding(8.0)
        .with_vertical_padding(3.0)
        .with_background_color(bg)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.0)))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostToggleTag(tag_for_click.clone()));
    })
    .finish()
}

// 侧栏底部按钮：图标+文字左对齐。密钥管理(可选中) / 管理分组标签共用。
fn render_bottom_item(
    label: String,
    icon: &'static str,
    state: &MouseStateHandle,
    action: TerminalGridAction,
    show_top_border: bool,
    is_selected: bool,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();

    Hoverable::new(state, move |mouse| {
        let bg = if is_selected {
            hc.group_selected_bg
        } else if mouse.is_hovered() {
            hc.group_hover_bg
        } else {
            hc.sidebar_bg
        };
        let fg = if is_selected {
            hc.text_accent
        } else {
            hc.text_secondary
        };

        Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Container::new(
                        ConstrainedBox::new(Icon::new(icon, fg).finish())
                            .with_width(ICON_SIZE_SM)
                            .with_height(ICON_SIZE_SM)
                            .finish(),
                    )
                    .with_margin_right(10.0)
                    .finish(),
                )
                .with_child(
                    Text::new_inline(label.clone(), ui_font, UI_FONT_SIZE)
                        .with_color(fg)
                        .finish(),
                )
                .finish(),
        )
        .with_padding_left(16.0)
        .with_vertical_padding(10.0)
        .with_background_color(bg)
        .with_border(
            Border::top(if show_top_border { 1.0 } else { 0.0 })
                .with_border_color(hc.sidebar_border),
        )
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}

/// 顶部状态总览入口：圆角卡片(云图标+加粗文字)，点击切到状态总览视图。
fn render_function_item(
    label: String,
    icon: &'static str,
    mode: HostViewMode,
    state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();
    Hoverable::new(state, move |mouse| {
        let bg = if mouse.is_hovered() {
            hc.card_bg_hover
        } else {
            hc.card_bg
        };
        Container::new(
            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Container::new(
                            ConstrainedBox::new(Icon::new(icon, hc.text_accent).finish())
                                .with_width(ICON_SIZE_SM)
                                .with_height(ICON_SIZE_SM)
                                .finish(),
                        )
                        .with_margin_right(10.0)
                        .finish(),
                    )
                    .with_child(
                        Text::new_inline(label.clone(), ui_font, UI_FONT_SIZE)
                            .with_color(hc.text_primary)
                            .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
                            .finish(),
                    )
                    .finish(),
            )
            .with_horizontal_padding(14.0)
            .with_vertical_padding(12.0)
            .with_background_color(bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
            .finish(),
        )
        .with_margin_top(12.0)
        .with_margin_left(12.0)
        .with_margin_right(12.0)
        .with_margin_bottom(8.0)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostSetViewMode(mode));
    })
    .finish()
}
