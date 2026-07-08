use std::sync::{Arc, Mutex};

use warpui::{
    color::ColorU,
    elements::{
        Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
        CornerRadius, CrossAxisAlignment, Expanded, Fill, Flex, Hoverable, Icon, MainAxisSize,
        MouseState, MouseStateHandle, ParentElement, Radius, ScrollbarWidth, Text, Wrap,
    },
    fonts,
    text_layout::ClipConfig,
    Element,
};

use super::host_card::protocol_colors;
use crate::host_management_view::constants::*;
use crate::terminal_grid_element::TerminalGridAction;
use nexshell::host_management::{HostGroupSnapshot, HostViewMode, RecentHostSnapshot};

// 行高亮 pill：外层左右内缩 + 内层圆角，避免贴边整条矩形。
const NAV_PILL_INSET: f32 = 8.0;
const NAV_PILL_RADIUS: f32 = 6.0;

pub struct GroupNavStates {
    pub group_states: Vec<MouseStateHandle>,
    pub tag_states: Vec<MouseStateHandle>,
    pub recent_states: Vec<MouseStateHandle>,
    pub manage_button_state: MouseStateHandle,
    pub status_entry_state: MouseStateHandle,
    pub containers_entry_state: MouseStateHandle,
    pub keys_entry_state: MouseStateHandle,
    /// 各行 hover/选中背景 eased 过渡（key = "nav-{类}:{id}"，随导航持久）。
    pub hover_transitions: std::cell::RefCell<nexshell::ui_anim::TransitionMap<String>>,
    /// 分组/最近访问中段滚动区状态。
    pub nav_scroll_state: ClippedScrollStateHandle,
}

impl GroupNavStates {
    pub fn new() -> Self {
        Self {
            group_states: Vec::new(),
            tag_states: Vec::new(),
            recent_states: Vec::new(),
            manage_button_state: Arc::new(Mutex::new(MouseState::default())),
            status_entry_state: Arc::new(Mutex::new(MouseState::default())),
            containers_entry_state: Arc::new(Mutex::new(MouseState::default())),
            keys_entry_state: Arc::new(Mutex::new(MouseState::default())),
            hover_transitions: std::cell::RefCell::new(nexshell::ui_anim::TransitionMap::new()),
            nav_scroll_state: ClippedScrollStateHandle::new(),
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

// 取样某行的背景过渡：先 retarget 到本帧目标色再取插值，key 稳定。
fn nav_pill_bg(states: &GroupNavStates, key: &str, target: ColorU) -> ColorU {
    let now = std::time::Instant::now();
    let key = key.to_string();
    let mut t = states.hover_transitions.borrow_mut();
    t.retarget(key.clone(), target, now);
    t.sample(&key, now).unwrap_or(target)
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
    // 清理已消失行的过渡条目；底部/状态入口恒常驻。
    {
        let group_ids: std::collections::HashSet<&str> =
            groups.iter().map(|g| g.id.as_str()).collect();
        let recent_ids: std::collections::HashSet<&str> =
            recent.iter().map(|r| r.host_id.as_str()).collect();
        states
            .hover_transitions
            .borrow_mut()
            .retain(|key| match key.split_once(':') {
                Some(("nav-g", id)) => group_ids.contains(id),
                Some(("nav-r", id)) => recent_ids.contains(id),
                Some(("nav-b", _)) | Some(("nav-s", _)) => true,
                _ => false,
            });
    }

    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    // 功能区（可扩展：未来在此追加更多功能菜单入口）
    col.add_child(render_function_item(
        "状态总览".to_string(),
        ICON_ACTIVITY,
        HostViewMode::Status,
        &states.status_entry_state,
        "nav-s:status",
        states,
        view_mode == HostViewMode::Status,
        ui_font,
        hc,
    ));
    col.add_child(render_function_item(
        rust_i18n::t!("host_nav_containers").to_string(),
        ICON_GRID_VIEW,
        HostViewMode::Containers,
        &states.containers_entry_state,
        "nav-s:containers",
        states,
        view_mode == HostViewMode::Containers,
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

    // 中段（分组 + 最近访问）单独滚动，防止分组多时把底部固定区顶出屏。
    let mut mid = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    mid.add_child(
        Container::new(
            Text::new_inline(
                rust_i18n::t!("host_group_nav").to_string(),
                ui_font,
                UI_FONT_SIZE_SMALL,
            )
            .with_color(hc.text_secondary)
            .finish(),
        )
        .with_padding_left(16.0)
        .with_padding_top(14.0)
        .with_padding_bottom(6.0)
        .finish(),
    );

    for (index, group) in groups.iter().enumerate() {
        mid.add_child(render_group_item(group, index, states, ui_font, hc));
    }

    if !recent.is_empty() {
        mid.add_child(render_section_title("最近访问", ui_font, hc));
        for (index, item) in recent.iter().enumerate() {
            mid.add_child(render_recent_item(item, index, states, ui_font, hc));
        }
    }

    col.add_child(
        Expanded::new(
            1.0,
            ClippedScrollable::vertical(
                states.nav_scroll_state.clone(),
                mid.finish(),
                ScrollbarWidth::Custom(4.0),
                Fill::Solid(hc.scrollbar_thumb),
                Fill::Solid(hc.scrollbar_thumb_active),
                Fill::None,
            )
            .with_overlayed_scrollbar()
            .finish(),
        )
        .finish(),
    );

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
        "nav-b:keys",
        states,
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
        "nav-b:manage",
        states,
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
    let group_id_marker = group.id.clone();
    let is_all = group.id == "all";

    let is_hovered = state.lock().map(|s| s.is_hovered()).unwrap_or(false);
    let target = if is_selected {
        hc.group_selected_bg
    } else if is_hovered {
        hc.group_hover_bg
    } else {
        hc.sidebar_bg
    };
    let bg = nav_pill_bg(states, &format!("nav-g:{}", group.id), target);

    Hoverable::new(state, move |_mouse| {
        let text_color = if is_selected {
            hc.text_accent
        } else {
            hc.text_primary
        };
        // 计数选中态跟随 accent，其余次级色。
        let count_color = if is_selected {
            hc.text_accent
        } else {
            hc.text_secondary
        };
        // pill：外层 margin 内缩、内层承 bg + 圆角；行内 padding 16→8 保内容位置不变。
        Container::new(
            Container::new(
                ConstrainedBox::new(
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Container::new(group_marker(is_all, &group_id_marker, &hc))
                                .with_padding_left(8.0)
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
                                    .with_color(count_color)
                                    .finish(),
                            )
                            .with_padding_right(8.0)
                            .finish(),
                        )
                        .finish(),
                )
                .with_height(GROUP_ITEM_HEIGHT)
                .finish(),
            )
            .with_background_color(bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(NAV_PILL_RADIUS)))
            .finish(),
        )
        .with_margin_left(NAV_PILL_INSET)
        .with_margin_right(NAV_PILL_INSET)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_hover(|_, ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::WakeUiAnim);
    })
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostSelectGroup(group_id_click.clone()));
    })
    .finish()
}

// 分组色标：所有主机用 accent 列表图标，普通分组用按 id 取色的 8×8 圆角色点。
fn group_marker(is_all: bool, group_id: &str, hc: &HostUiColors) -> Box<dyn Element> {
    if is_all {
        return ConstrainedBox::new(Icon::new(ICON_LIST_VIEW, hc.text_accent).finish())
            .with_width(ICON_SIZE_SM)
            .with_height(ICON_SIZE_SM)
            .finish();
    }
    let idx = group_id.bytes().map(|b| b as usize).sum::<usize>() % hc.group_dot_palette.len();
    let dot = ConstrainedBox::new(
        Container::new(warpui::elements::Empty::new().finish())
            .with_background_color(hc.group_dot_palette[idx])
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .finish(),
    )
    .with_width(8.0)
    .with_height(8.0)
    .finish();
    ConstrainedBox::new(Container::new(dot).with_uniform_padding(4.0).finish())
        .with_width(ICON_SIZE_SM)
        .with_height(ICON_SIZE_SM)
        .finish()
}

// 区块小标题，如"最近访问"（降级为 SMALL 字号、无分隔线）。
fn render_section_title(
    label: &str,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Container::new(
        Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE_SMALL)
            .with_color(hc.text_secondary)
            .finish(),
    )
    .with_padding_left(16.0)
    .with_padding_top(14.0)
    .with_padding_bottom(6.0)
    .finish()
}

// 最近访问项单行：>_ 图标 + 主机名（可截断）+ 弹性空档 + 相对时间恒右，点击快连。
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
    let (icon_tint, icon_fg) = protocol_colors(&item.protocol, &hc);
    let time = relative_time(item.accessed_at);

    let is_hovered = state.lock().map(|s| s.is_hovered()).unwrap_or(false);
    let target = if is_hovered {
        hc.group_hover_bg
    } else {
        hc.sidebar_bg
    };
    let bg = nav_pill_bg(states, &format!("nav-r:{}", item.host_id), target);

    Hoverable::new(state, move |_mouse| {
        Container::new(
            Container::new(
                ConstrainedBox::new(
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Container::new(
                                // 24×24 协议色底 + 居中 14 图标（padding=(24-14)/2）。
                                Container::new(
                                    ConstrainedBox::new(Icon::new(ICON_TERMINAL, icon_fg).finish())
                                        .with_width(14.0)
                                        .with_height(14.0)
                                        .finish(),
                                )
                                .with_uniform_padding(5.0)
                                .with_background_color(icon_tint)
                                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.0)))
                                .finish(),
                            )
                            .with_padding_left(8.0)
                            .with_margin_right(10.0)
                            .finish(),
                        )
                        .with_child(
                            // 名字槽独占弹性空间（勿再加 Expanded 空档分走宽度）；
                            // 超宽才尾部渐隐（Warp tab 同款），与时间保底 8px 间距。
                            Expanded::new(
                                1.0,
                                Container::new(
                                    Text::new_inline(name.clone(), ui_font, UI_FONT_SIZE)
                                        .with_color(hc.text_primary)
                                        .with_clip(ClipConfig::end())
                                        .soft_wrap(false)
                                        .finish(),
                                )
                                .with_margin_right(8.0)
                                .finish(),
                            )
                            .finish(),
                        )
                        .with_child(
                            Container::new(
                                Text::new_inline(time.clone(), ui_font, UI_FONT_SIZE_SMALL)
                                    .with_color(hc.text_secondary)
                                    .finish(),
                            )
                            .with_padding_right(8.0)
                            .finish(),
                        )
                        .finish(),
                )
                .with_height(GROUP_ITEM_HEIGHT)
                .finish(),
            )
            .with_background_color(bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(NAV_PILL_RADIUS)))
            .finish(),
        )
        .with_margin_left(NAV_PILL_INSET)
        .with_margin_right(NAV_PILL_INSET)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_hover(|_, ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::WakeUiAnim);
    })
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
// top border 留在全宽外层（底部区分界，不随 pill 内缩）。
fn render_bottom_item(
    label: String,
    icon: &'static str,
    state: &MouseStateHandle,
    pill_key: &str,
    states: &GroupNavStates,
    action: TerminalGridAction,
    show_top_border: bool,
    is_selected: bool,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();

    let is_hovered = state.lock().map(|s| s.is_hovered()).unwrap_or(false);
    let target = if is_selected {
        hc.group_selected_bg
    } else if is_hovered {
        hc.group_hover_bg
    } else {
        hc.sidebar_bg
    };
    let bg = nav_pill_bg(states, pill_key, target);

    Hoverable::new(state, move |_mouse| {
        let fg = if is_selected {
            hc.text_accent
        } else {
            hc.text_secondary
        };

        Container::new(
            Container::new(
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
                .with_padding_left(8.0)
                .with_vertical_padding(8.0)
                .with_background_color(bg)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(NAV_PILL_RADIUS)))
                .finish(),
            )
            .with_margin_left(NAV_PILL_INSET)
            .with_margin_right(NAV_PILL_INSET)
            .finish(),
        )
        .with_border(
            Border::top(if show_top_border { 1.0 } else { 0.0 })
                .with_border_color(hc.sidebar_border),
        )
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

/// 顶部功能区入口（状态总览/容器等）：圆角卡片(图标+加粗文字)，点击切视图。
#[allow(clippy::too_many_arguments)]
fn render_function_item(
    label: String,
    icon: &'static str,
    mode: HostViewMode,
    state: &MouseStateHandle,
    pill_key: &str,
    states: &GroupNavStates,
    is_selected: bool,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();

    let is_hovered = state.lock().map(|s| s.is_hovered()).unwrap_or(false);
    // 选中时背景恒为 hover 色（不随鼠标变），否则常规 hover 切换。
    let target = if is_selected || is_hovered {
        hc.card_bg_hover
    } else {
        hc.card_bg
    };
    let bg = nav_pill_bg(states, pill_key, target);

    Hoverable::new(state, move |_mouse| {
        let text_color = if is_selected {
            hc.text_accent
        } else {
            hc.text_primary
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
                            .with_color(text_color)
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
    .on_hover(|_, ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::WakeUiAnim);
    })
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostSetViewMode(mode));
    })
    .finish()
}
