// 密钥库管理页（ServerCat Keychain 风格）：左侧密钥卡片列表 + 右侧详情面板。
// 本文件只放纯渲染自由函数；数据来自 RootView 缓存的 ssh_key_store 记录与选中态。

use std::sync::{Arc, Mutex};

use warpui::{
    color::ColorU,
    elements::{
        Align, Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
        CornerRadius, CrossAxisAlignment, Empty, Expanded, Fill, Flex, Hoverable, Icon,
        MainAxisSize, MouseState, MouseStateHandle, ParentElement, Radius, ScrollbarWidth, Text,
    },
    fonts, Element, ViewHandle,
};

use warp::editor::EditorView;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::ui_components::text_input::TextInput;

use nexshell::file_drop_target::{DropCallback, FileDropTarget};
use nexshell::ssh_key_store::SshKeyRecord;

use crate::host_management_view::constants::*;
use crate::terminal_grid_element::TerminalGridAction;

pub struct KeyManagerStates {
    pub row_states: Vec<MouseStateHandle>,
    pub delete_state: MouseStateHandle,
    pub copy_state: MouseStateHandle,
    pub edit_state: MouseStateHandle,
    pub cancel_state: MouseStateHandle,
    pub save_state: MouseStateHandle,
    pub list_scroll: ClippedScrollStateHandle,
    pub detail_scroll: ClippedScrollStateHandle,
}

impl KeyManagerStates {
    pub fn new() -> Self {
        Self {
            row_states: Vec::new(),
            delete_state: Arc::new(Mutex::new(MouseState::default())),
            copy_state: Arc::new(Mutex::new(MouseState::default())),
            edit_state: Arc::new(Mutex::new(MouseState::default())),
            cancel_state: Arc::new(Mutex::new(MouseState::default())),
            save_state: Arc::new(Mutex::new(MouseState::default())),
            list_scroll: ClippedScrollStateHandle::new(),
            detail_scroll: ClippedScrollStateHandle::new(),
        }
    }

    pub fn ensure_count(&mut self, count: usize) {
        while self.row_states.len() < count {
            self.row_states
                .push(Arc::new(Mutex::new(MouseState::default())));
        }
    }
}

fn red() -> ColorU {
    ColorU::new(0xe5, 0x4d, 0x42, 0xff)
}

fn green() -> ColorU {
    ColorU::new(0x6d, 0xc2, 0x8a, 0xff)
}

fn transparent() -> ColorU {
    ColorU::new(0, 0, 0, 0)
}

fn bold(content: String, ui_font: fonts::FamilyId, size: f32, color: ColorU) -> Box<dyn Element> {
    Text::new_inline(content, ui_font, size)
        .with_color(color)
        .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
        .finish()
}

fn icon_box(icon: &'static str, color: ColorU, size: f32) -> Box<dyn Element> {
    ConstrainedBox::new(Icon::new(icon, color).finish())
        .with_width(size)
        .with_height(size)
        .finish()
}

fn spacer_v(height: f32) -> Box<dyn Element> {
    ConstrainedBox::new(Empty::new().finish())
        .with_height(height)
        .finish()
}

fn spacer_h(width: f32) -> Box<dyn Element> {
    ConstrainedBox::new(Empty::new().finish())
        .with_width(width)
        .finish()
}

// 列表行间细分隔线（横跨卡片内宽）。
fn divider(hc: &HostUiColors) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(Empty::new().finish())
            .with_background_color(hc.card_border)
            .finish(),
    )
    .with_height(1.0)
    .finish()
}

// 左右两栏之间的竖向分隔线。
fn vertical_divider(hc: &HostUiColors) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(Empty::new().finish())
            .with_background_color(hc.sidebar_border)
            .finish(),
    )
    .with_width(1.0)
    .finish()
}

pub fn render_key_manager_view(
    keys: &[(SshKeyRecord, usize)],
    selected_key_id: Option<&str>,
    selected_public_key: Option<&str>,
    copy_cmd_expanded: bool,
    editing: bool,
    delete_confirming: bool,
    name_editor: &ViewHandle<EditorView>,
    passphrase_editor: &ViewHandle<EditorView>,
    states: &KeyManagerStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let list = render_key_list_panel(keys, selected_key_id, states, ui_font, hc);
    let selected = selected_key_id.and_then(|id| keys.iter().find(|(key, _)| key.id == id));
    let detail = render_key_detail_panel(
        selected,
        selected_public_key,
        copy_cmd_expanded,
        editing,
        delete_confirming,
        name_editor,
        passphrase_editor,
        states,
        ui_font,
        hc,
    );

    Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            ConstrainedBox::new(list)
                .with_width(KEY_LIST_WIDTH)
                .finish(),
        )
        .with_child(vertical_divider(hc))
        .with_child(Expanded::new(1.0, detail).finish())
        .finish()
}

// 左栏：整面板即拖入导入区；密钥行收在一个大圆角卡片里，行间分隔线。
fn render_key_list_panel(
    keys: &[(SshKeyRecord, usize)],
    selected_key_id: Option<&str>,
    states: &KeyManagerStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let body: Box<dyn Element> = if keys.is_empty() {
        Container::new(
            Text::new_inline(
                "还没有导入密钥。把私钥文件拖到这里即可导入。".to_string(),
                ui_font,
                UI_FONT_SIZE,
            )
            .with_color(hc.text_secondary)
            .finish(),
        )
        .with_vertical_padding(24.0)
        .finish()
    } else {
        let mut col = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for (index, (key, usage)) in keys.iter().enumerate() {
            if index > 0 {
                col.add_child(divider(hc));
            }
            let selected = selected_key_id == Some(key.id.as_str());
            col.add_child(render_key_row(key, *usage, index, selected, states, ui_font, hc));
        }
        Container::new(col.finish())
            .with_background_color(hc.card_bg)
            .with_border(Border::all(1.0).with_border_color(hc.card_border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.0)))
            .finish()
    };

    let scroll = ClippedScrollable::vertical(
        states.list_scroll.clone(),
        body,
        ScrollbarWidth::Custom(6.0),
        Fill::Solid(hc.scrollbar_thumb),
        Fill::Solid(hc.scrollbar_thumb_active),
        Fill::None,
    )
    .with_overlayed_scrollbar()
    .finish();

    let padded = Container::new(scroll)
        .with_horizontal_padding(20.0)
        .with_vertical_padding(20.0)
        .with_background_color(hc.panel_bg)
        .finish();

    let callback: DropCallback = Arc::new(|ctx, paths| {
        if let Some(path) = paths.into_iter().next() {
            ctx.dispatch_typed_action(TerminalGridAction::HostImportKeyFile(path));
        }
    });
    FileDropTarget::new(padded, callback).intercept().finish()
}

fn render_key_row(
    key: &SshKeyRecord,
    usage: usize,
    index: usize,
    selected: bool,
    states: &KeyManagerStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let row_state = states.row_states[index].clone();
    let name = key.name.clone();
    let key_type = if key.key_type.is_empty() {
        "key".to_string()
    } else {
        key.key_type.clone()
    };
    let usage_text = usage.to_string();
    let key_id = key.id.clone();

    Hoverable::new(row_state, move |mouse| {
        let bg = if selected {
            hc.badge_ssh_bg
        } else if mouse.is_hovered() {
            hc.card_bg_hover
        } else {
            transparent()
        };

        let meta = Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(host_count_chip(&usage_text, ui_font, &hc))
            .with_child(spacer_h(8.0))
            .with_child(type_chip(&key_type, ui_font, &hc))
            .finish();

        let info = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(bold(
                name.clone(),
                ui_font,
                UI_FONT_SIZE + 1.0,
                hc.text_primary,
            ))
            .with_child(spacer_v(8.0))
            .with_child(meta)
            .finish();

        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(1.0, info).finish())
            .with_child(spacer_h(8.0))
            .with_child(icon_box(ICON_CHEVRON_RIGHT, hc.text_secondary, ICON_SIZE_SM))
            .finish();

        Container::new(row)
            .with_horizontal_padding(14.0)
            .with_vertical_padding(14.0)
            .with_background_color(bg)
            .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostSelectKey(key_id.clone()));
    })
    .finish()
}

// 中性深色小标签（主机数 / 类型），区别于主色 tag。
fn host_count_chip(count: &str, ui_font: fonts::FamilyId, hc: &HostUiColors) -> Box<dyn Element> {
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(icon_box(ICON_TERMINAL, hc.text_secondary, 13.0))
            .with_child(spacer_h(5.0))
            .with_child(
                Text::new_inline(count.to_string(), ui_font, UI_FONT_SIZE_SMALL)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .finish(),
    )
    .with_horizontal_padding(7.0)
    .with_vertical_padding(3.0)
    .with_background_color(hc.panel_bg)
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(5.0)))
    .finish()
}

fn type_chip(key_type: &str, ui_font: fonts::FamilyId, hc: &HostUiColors) -> Box<dyn Element> {
    Container::new(
        Text::new_inline(key_type.to_string(), ui_font, UI_FONT_SIZE_SMALL)
            .with_color(hc.text_secondary)
            .finish(),
    )
    .with_horizontal_padding(7.0)
    .with_vertical_padding(3.0)
    .with_background_color(hc.panel_bg)
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(5.0)))
    .finish()
}

// 右栏：未选中显示空态，选中显示详情（可滚动）。
fn render_key_detail_panel(
    selected: Option<&(SshKeyRecord, usize)>,
    public_key: Option<&str>,
    copy_cmd_expanded: bool,
    editing: bool,
    delete_confirming: bool,
    name_editor: &ViewHandle<EditorView>,
    passphrase_editor: &ViewHandle<EditorView>,
    states: &KeyManagerStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    match selected {
        None => render_detail_empty(ui_font, hc),
        Some((key, _usage)) => render_detail_content(
            key,
            public_key,
            copy_cmd_expanded,
            editing,
            delete_confirming,
            name_editor,
            passphrase_editor,
            states,
            ui_font,
            hc,
        ),
    }
}

fn render_detail_empty(ui_font: fonts::FamilyId, hc: &HostUiColors) -> Box<dyn Element> {
    Container::new(
        Align::new(
            Flex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(icon_box(ICON_KEY, hc.text_secondary, 40.0))
                .with_child(spacer_v(18.0))
                .with_child(bold("选择密钥".to_string(), ui_font, 20.0, hc.text_secondary))
                .with_child(spacer_v(8.0))
                .with_child(
                    Text::new_inline(
                        "选择一个密钥查看详情".to_string(),
                        ui_font,
                        UI_FONT_SIZE + 1.0,
                    )
                    .with_color(hc.text_secondary)
                    .finish(),
                )
                .finish(),
        )
        .finish(),
    )
    .with_background_color(hc.panel_bg)
    .finish()
}

fn render_detail_content(
    key: &SshKeyRecord,
    public_key: Option<&str>,
    copy_cmd_expanded: bool,
    editing: bool,
    delete_confirming: bool,
    name_editor: &ViewHandle<EditorView>,
    passphrase_editor: &ViewHandle<EditorView>,
    states: &KeyManagerStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    if editing {
        return render_detail_editing(name_editor, passphrase_editor, states, ui_font, hc);
    }

    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    // 顶部「编辑」按钮（靠右），与编辑态的 Cancel/Save 位置呼应
    col.add_child(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
            .with_child(pill_button(
                &states.edit_state,
                "编辑",
                hc.card_bg,
                hc.card_bg_hover,
                hc.text_primary,
                Some(hc.card_border),
                TerminalGridAction::HostEditKey,
                ui_font,
            ))
            .finish(),
    );
    col.add_child(spacer_v(14.0));

    // 信息卡：名称 + 密码短语
    let passphrase = match key.passphrase.as_deref() {
        Some(p) if !p.is_empty() => "••••••••".to_string(),
        _ => "—".to_string(),
    };
    let mut info = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    info.add_child(info_row("名称", &key.name, ui_font, hc));
    info.add_child(divider(hc));
    info.add_child(info_row("密码短语", &passphrase, ui_font, hc));
    col.add_child(
        Container::new(info.finish())
            .with_horizontal_padding(16.0)
            .with_background_color(hc.card_bg)
            .with_border(Border::all(1.0).with_border_color(hc.card_border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.0)))
            .finish(),
    );
    col.add_child(spacer_v(22.0));

    // 复制公钥到服务器：生成 echo >> authorized_keys 命令，可选中复制去服务器执行
    if let Some(pubkey) = public_key {
        col.add_child(render_copy_to_server(
            pubkey,
            copy_cmd_expanded,
            &states.copy_state,
            ui_font,
            hc,
        ));
        col.add_child(spacer_v(22.0));
    }

    // 公钥
    col.add_child(section_title("公钥", ui_font, hc));
    col.add_child(spacer_v(8.0));
    let pub_text = public_key.unwrap_or("无法推导公钥（私钥已加密，需要口令）");
    col.add_child(code_block(pub_text, ui_font, hc));
    col.add_child(spacer_v(22.0));

    // 私钥
    col.add_child(section_title("私钥", ui_font, hc));
    col.add_child(spacer_v(8.0));
    col.add_child(code_block(&key.content, ui_font, hc));
    col.add_child(spacer_v(22.0));

    col.add_child(render_delete_section(
        delete_confirming,
        &key.id,
        states,
        ui_font,
        hc,
    ));

    let scroll = ClippedScrollable::vertical(
        states.detail_scroll.clone(),
        Container::new(col.finish())
            .with_horizontal_padding(28.0)
            .with_vertical_padding(24.0)
            .finish(),
        ScrollbarWidth::Custom(6.0),
        Fill::Solid(hc.scrollbar_thumb),
        Fill::Solid(hc.scrollbar_thumb_active),
        Fill::None,
    )
    .with_overlayed_scrollbar()
    .finish();

    Container::new(scroll)
        .with_background_color(hc.panel_bg)
        .finish()
}

fn info_row(
    label: &str,
    value: &str,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE + 1.0)
                    .with_color(hc.text_primary)
                    .finish(),
            )
            .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
            .with_child(
                Text::new_inline(value.to_string(), ui_font, UI_FONT_SIZE + 1.0)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .finish(),
    )
    .with_vertical_padding(14.0)
    .finish()
}

fn section_title(title: &str, ui_font: fonts::FamilyId, hc: &HostUiColors) -> Box<dyn Element> {
    bold(title.to_string(), ui_font, UI_FONT_SIZE + 3.0, hc.text_primary)
}

// 复制公钥到服务器卡：默认折叠成可点击入口；点击复制 echo >> authorized_keys 命令到
// 剪贴板并展开命令（绿色等宽、可再次选中复制）。
fn render_copy_to_server(
    public_key: &str,
    expanded: bool,
    state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();

    let header = Hoverable::new(state, move |mouse| {
        let title_color = if mouse.is_hovered() {
            hc.text_accent
        } else {
            hc.text_primary
        };
        let trailing: Box<dyn Element> = if expanded {
            Text::new_inline("已复制".to_string(), ui_font, UI_FONT_SIZE_SMALL)
                .with_color(green())
                .finish()
        } else {
            icon_box(ICON_COPY, hc.text_secondary, ICON_SIZE_SM)
        };
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(bold(
                "复制公钥到服务器".to_string(),
                ui_font,
                UI_FONT_SIZE + 2.0,
                title_color,
            ))
            .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
            .with_child(trailing)
            .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostCopyKeyToServer);
    })
    .finish();

    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    col.add_child(header);
    if expanded {
        let command = format!("echo '{}' >> ~/.ssh/authorized_keys", public_key.trim());
        col.add_child(spacer_v(12.0));
        col.add_child(divider(&hc));
        col.add_child(spacer_v(12.0));
        col.add_child(
            Text::new(command, ui_font, UI_FONT_SIZE_SMALL)
                .soft_wrap(true)
                .with_selectable(true)
                .with_color(green())
                .finish(),
        );
    }

    Container::new(col.finish())
        .with_horizontal_padding(16.0)
        .with_vertical_padding(16.0)
        .with_background_color(hc.card_bg)
        .with_border(Border::all(1.0).with_border_color(hc.card_border))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.0)))
        .finish()
}

// 代码块：等宽小字号，soft-wrap 按宽度断行，可选中复制。
fn code_block(text: &str, ui_font: fonts::FamilyId, hc: &HostUiColors) -> Box<dyn Element> {
    Container::new(
        Text::new(text.to_string(), ui_font, UI_FONT_SIZE_SMALL)
            .soft_wrap(true)
            .with_selectable(true)
            .with_color(hc.text_primary)
            .finish(),
    )
    .with_horizontal_padding(14.0)
    .with_vertical_padding(12.0)
    .with_background_color(hc.card_bg)
    .with_border(Border::all(1.0).with_border_color(hc.card_border))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
    .finish()
}

// 删除入口（点击进入二次确认）。
fn render_delete_button(
    state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = state.clone();
    Hoverable::new(state, move |mouse| {
        let color = if mouse.is_hovered() {
            red()
        } else {
            hc.text_secondary
        };
        let bg = if mouse.is_hovered() {
            ColorU::new(0xe5, 0x4d, 0x42, 0x18)
        } else {
            transparent()
        };
        Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(icon_box(ICON_TRASH, color, ICON_SIZE_SM))
                .with_child(spacer_h(8.0))
                .with_child(
                    Text::new_inline("删除密钥".to_string(), ui_font, UI_FONT_SIZE)
                        .with_color(color)
                        .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(14.0)
        .with_vertical_padding(12.0)
        .with_background_color(bg)
        .with_border(Border::all(1.0).with_border_color(hc.card_border))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostDeleteKeyPrompt);
    })
    .finish()
}

// 删除区：未确认显示入口；确认态显示提示 + 「确认删除」(红) / 「取消」。
fn render_delete_section(
    confirming: bool,
    key_id: &str,
    states: &KeyManagerStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    if !confirming {
        return render_delete_button(&states.delete_state, ui_font, hc);
    }

    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    col.add_child(
        Text::new_inline(
            "确定删除此密钥？引用它的主机会被解除引用。".to_string(),
            ui_font,
            UI_FONT_SIZE,
        )
        .with_color(hc.text_secondary)
        .finish(),
    );
    col.add_child(spacer_v(10.0));
    col.add_child(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(pill_button(
                &states.delete_state,
                "确认删除",
                red(),
                red(),
                ColorU::new(0xff, 0xff, 0xff, 0xff),
                None,
                TerminalGridAction::HostDeleteKey(key_id.to_string()),
                ui_font,
            ))
            .with_child(spacer_h(10.0))
            .with_child(pill_button(
                &states.cancel_state,
                "取消",
                hc.card_bg,
                hc.card_bg_hover,
                hc.text_primary,
                Some(hc.card_border),
                TerminalGridAction::HostDeleteKeyCancel,
                ui_font,
            ))
            .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
            .finish(),
    );
    col.finish()
}

// 编辑态：顶部 取消/保存，信息卡里名称/口令变可输入。
fn render_detail_editing(
    name_editor: &ViewHandle<EditorView>,
    passphrase_editor: &ViewHandle<EditorView>,
    states: &KeyManagerStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    let bar = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(pill_button(
            &states.cancel_state,
            "取消",
            hc.card_bg,
            hc.card_bg_hover,
            hc.text_primary,
            Some(hc.card_border),
            TerminalGridAction::HostKeyEditCancel,
            ui_font,
        ))
        .with_child(spacer_h(10.0))
        .with_child(pill_button(
            &states.save_state,
            "保存",
            red(),
            red(),
            ColorU::new(0xff, 0xff, 0xff, 0xff),
            None,
            TerminalGridAction::HostKeyEditSave,
            ui_font,
        ))
        .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
        .finish();
    col.add_child(bar);
    col.add_child(spacer_v(22.0));

    let mut info = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    info.add_child(edit_field_row("名称", name_editor, ui_font, hc));
    info.add_child(divider(hc));
    info.add_child(edit_field_row("密码短语", passphrase_editor, ui_font, hc));
    col.add_child(
        Container::new(info.finish())
            .with_horizontal_padding(16.0)
            .with_background_color(hc.card_bg)
            .with_border(Border::all(1.0).with_border_color(hc.card_border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.0)))
            .finish(),
    );

    Container::new(
        Container::new(col.finish())
            .with_horizontal_padding(28.0)
            .with_vertical_padding(24.0)
            .finish(),
    )
    .with_background_color(hc.panel_bg)
    .finish()
}

fn edit_field_row(
    label: &str,
    editor: &ViewHandle<EditorView>,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE + 1.0)
                    .with_color(hc.text_primary)
                    .finish(),
            )
            .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
            .with_child(key_text_input(editor, hc))
            .finish(),
    )
    .with_vertical_padding(10.0)
    .finish()
}

// editor buffer 不接受无限宽度约束，须包在固定宽度容器内（同主机改名）。
fn key_text_input(editor: &ViewHandle<EditorView>, hc: &HostUiColors) -> Box<dyn Element> {
    ConstrainedBox::new(
        TextInput::new(
            editor.clone(),
            UiComponentStyles {
                background: Some(Fill::None),
                border_width: Some(0.0),
                font_color: Some(hc.text_primary),
                ..Default::default()
            },
        )
        .build()
        .finish(),
    )
    .with_width(200.0)
    .finish()
}

#[allow(clippy::too_many_arguments)]
fn pill_button(
    state: &MouseStateHandle,
    label: &str,
    bg: ColorU,
    bg_hover: ColorU,
    text_color: ColorU,
    border: Option<ColorU>,
    action: TerminalGridAction,
    ui_font: fonts::FamilyId,
) -> Box<dyn Element> {
    let state = state.clone();
    let label = label.to_string();
    Hoverable::new(state, move |mouse| {
        let b = if mouse.is_hovered() { bg_hover } else { bg };
        let mut container = Container::new(bold(label.clone(), ui_font, UI_FONT_SIZE, text_color))
            .with_horizontal_padding(16.0)
            .with_vertical_padding(7.0)
            .with_background_color(b)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)));
        if let Some(border_color) = border {
            container = container.with_border(Border::all(1.0).with_border_color(border_color));
        }
        container.finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}
