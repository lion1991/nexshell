use std::rc::Rc;
use std::sync::{Arc, Mutex};

use warp_core::ui::appearance::Appearance;
use warpui::{
    color::ColorU,
    elements::{
        Align, Border, ChildView, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
        DispatchEventResult, Element, EventHandler, Flex, Hoverable, MainAxisAlignment,
        MainAxisSize, MouseState, MouseStateHandle, ParentElement, Radius, Shrinkable, Text,
    },
    fonts::FamilyId,
    ui_components::{
        button::ButtonVariant,
        components::{Coords, UiComponent, UiComponentStyles},
        radio_buttons::{RadioButtonItem, RadioButtonLayout, RadioButtonStateHandle},
        slider::SliderStateHandle,
        switch::SwitchStateHandle,
    },
    AppContext, ViewHandle,
};

use super::super::terminal_grid_element::{
    CursorStyleChoice, GlassQualityChoice, LanguageChoice, TerminalGridAction, ThemeChoice,
};
use super::render_helpers::{
    render_category_header, render_page_title, render_setting_row, CONTENT_FONT_SIZE,
    HEADER_PADDING, MAX_PAGE_WIDTH, PAGE_PADDING,
};

// warp: appearance_page.rs 常量
const OPACITY_SLIDER_WIDTH: f32 = 200.0;
const FONT_SIZE_INPUT_WIDTH: f32 = 80.0;
const LINE_HEIGHT_INPUT_WIDTH: f32 = 80.0;
const INPUT_HEIGHT: f32 = 30.0;

pub struct AppearancePageState {
    pub current_theme_state: MouseStateHandle,
    pub reuse_view_tab_switch: SwitchStateHandle,
    pub opacity_slider_state: SliderStateHandle,
    pub glass_quality_radio_state: RadioButtonStateHandle,
    pub glass_quality_mouse_states: Vec<MouseStateHandle>,
    pub cursor_style_radio_state: RadioButtonStateHandle,
    pub cursor_style_mouse_states: Vec<MouseStateHandle>,
    // Text 区块
    pub font_size_minus_state: MouseStateHandle,
    pub font_size_plus_state: MouseStateHandle,
    pub line_height_minus_state: MouseStateHandle,
    pub line_height_plus_state: MouseStateHandle,
    pub line_height_reset_state: MouseStateHandle,
    pub view_all_fonts_checkbox_state: MouseStateHandle,
    pub view_all_fonts: bool,
    // Language 区块
    pub language_radio_state: RadioButtonStateHandle,
    pub language_radio_mouse_states: Vec<MouseStateHandle>,
}

impl Default for AppearancePageState {
    fn default() -> Self {
        Self {
            current_theme_state: Arc::new(Mutex::new(MouseState::default())),
            reuse_view_tab_switch: SwitchStateHandle::default(),
            opacity_slider_state: SliderStateHandle::default(),
            glass_quality_radio_state: RadioButtonStateHandle::default(),
            glass_quality_mouse_states: GlassQualityChoice::ALL
                .iter()
                .map(|_| Arc::new(Mutex::new(MouseState::default())))
                .collect(),
            cursor_style_radio_state: RadioButtonStateHandle::default(),
            cursor_style_mouse_states: CursorStyleChoice::ALL
                .iter()
                .map(|_| Arc::new(Mutex::new(MouseState::default())))
                .collect(),
            font_size_minus_state: Arc::new(Mutex::new(MouseState::default())),
            font_size_plus_state: Arc::new(Mutex::new(MouseState::default())),
            line_height_minus_state: Arc::new(Mutex::new(MouseState::default())),
            line_height_plus_state: Arc::new(Mutex::new(MouseState::default())),
            line_height_reset_state: Arc::new(Mutex::new(MouseState::default())),
            view_all_fonts_checkbox_state: Arc::new(Mutex::new(MouseState::default())),
            view_all_fonts: false,
            language_radio_state: RadioButtonStateHandle::default(),
            language_radio_mouse_states: LanguageChoice::ALL
                .iter()
                .map(|_| Arc::new(Mutex::new(MouseState::default())))
                .collect(),
        }
    }
}

pub fn render_appearance_page(
    state: &AppearancePageState,
    current_theme: ThemeChoice,
    current_font_size: f32,
    line_height_ratio: f32,
    window_opacity: u8,
    current_glass_quality: GlassQualityChoice,
    cursor_style: CursorStyleChoice,
    current_font_weight: warpui::fonts::Weight,
    current_font_name: &str,
    available_fonts: &[String],
    monospace_font: FamilyId,
    font_family_dropdown: &ViewHandle<
        super::super::warp_filterable_dropdown::FilterableDropdown<TerminalGridAction>,
    >,
    font_weight_dropdown: &ViewHandle<
        super::super::warp_dropdown_view::Dropdown<TerminalGridAction>,
    >,
    open_file_editor_dropdown: &ViewHandle<
        super::super::warp_dropdown_view::Dropdown<TerminalGridAction>,
    >,
    current_language: LanguageChoice,
    reuse_view_tab: bool,
    appearance: &Appearance,
    _app: &AppContext,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui_font = appearance.ui_font_family();

    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    col.add_child(render_page_title(
        &rust_i18n::t!("appearance_title"),
        appearance,
    ));

    // --- Theme --- warp: appearance_page.rs:2630-2713
    col.add_child(render_category_header(
        &rust_i18n::t!("appearance_theme"),
        appearance,
    ));
    col.add_child(render_current_theme_row(
        &state.current_theme_state,
        current_theme,
        monospace_font,
        ui_font,
        theme,
    ));
    col.add_child(crate::settings_view::render_helpers::render_separator(
        appearance,
    ));

    // --- Window ---
    col.add_child(render_category_header(
        &rust_i18n::t!("appearance_window"),
        appearance,
    ));

    let opacity_label = Text::new_inline(
        rust_i18n::t!("appearance_window_opacity", value = window_opacity),
        ui_font,
        CONTENT_FONT_SIZE,
    )
    .with_color(theme.active_ui_text_color().into())
    .finish();

    let opacity_slider = appearance
        .ui_builder()
        .slider(state.opacity_slider_state.clone())
        .with_range(1.0..100.0)
        .with_default_value(window_opacity as f32)
        .with_step(1.0)
        .with_style(UiComponentStyles {
            width: Some(OPACITY_SLIDER_WIDTH),
            margin: Some(Coords::default().top(3.).bottom(3.)),
            ..Default::default()
        })
        .on_drag(|ctx, _, val| {
            ctx.dispatch_typed_action(TerminalGridAction::SetOpacity(val as u8));
        })
        .on_change(|ctx, _, val| {
            ctx.dispatch_typed_action(TerminalGridAction::SetOpacity(val as u8));
        })
        .build()
        .finish();

    col.add_child(render_setting_row(opacity_label, opacity_slider));

    let glass_label = Text::new_inline(
        rust_i18n::t!("appearance_glass_quality"),
        ui_font,
        CONTENT_FONT_SIZE,
    )
    .with_color(theme.active_ui_text_color().into())
    .finish();

    let glass_radio = appearance
        .ui_builder()
        .radio_buttons(
            state.glass_quality_mouse_states.clone(),
            GlassQualityChoice::ALL
                .iter()
                .map(|quality| RadioButtonItem::text(quality.label()))
                .collect(),
            state.glass_quality_radio_state.clone(),
            Some(current_glass_quality.to_index()),
            CONTENT_FONT_SIZE,
            RadioButtonLayout::Row,
        )
        .on_change(Rc::new(move |ctx, _, index| {
            if let Some(idx) = index {
                if let Some(quality) = GlassQualityChoice::from_index(idx) {
                    ctx.dispatch_typed_action(TerminalGridAction::SetGlassQuality(quality));
                }
            }
        }))
        .build()
        .finish();

    col.add_child(render_setting_row(glass_label, glass_radio));
    col.add_child(crate::settings_view::render_helpers::render_separator(
        appearance,
    ));

    // --- Text --- warp: appearance_page.rs:3909-4054
    col.add_child(render_category_header(
        &rust_i18n::t!("appearance_text"),
        appearance,
    ));
    col.add_child(render_text_section(
        state,
        current_font_name,
        available_fonts,
        current_font_size,
        line_height_ratio,
        current_font_weight,
        font_family_dropdown,
        font_weight_dropdown,
        appearance,
    ));
    col.add_child(crate::settings_view::render_helpers::render_separator(
        appearance,
    ));

    // --- Cursor ---
    col.add_child(render_category_header(
        &rust_i18n::t!("appearance_cursor"),
        appearance,
    ));

    let cursor_label = Text::new_inline(
        rust_i18n::t!("appearance_cursor_type"),
        ui_font,
        CONTENT_FONT_SIZE,
    )
    .with_color(theme.active_ui_text_color().into())
    .finish();

    let cursor_radio = appearance
        .ui_builder()
        .radio_buttons(
            state.cursor_style_mouse_states.clone(),
            CursorStyleChoice::ALL
                .iter()
                .map(|s| RadioButtonItem::text(s.label()))
                .collect(),
            state.cursor_style_radio_state.clone(),
            Some(cursor_style.to_index()),
            CONTENT_FONT_SIZE,
            RadioButtonLayout::Row,
        )
        .on_change(Rc::new(move |ctx, _, index| {
            if let Some(idx) = index {
                if let Some(style) = CursorStyleChoice::from_index(idx) {
                    ctx.dispatch_typed_action(TerminalGridAction::SetCursorStyle(style));
                }
            }
        }))
        .build()
        .finish();

    col.add_child(render_setting_row(cursor_label, cursor_radio));
    col.add_child(crate::settings_view::render_helpers::render_separator(
        appearance,
    ));

    // --- Language ---
    col.add_child(render_category_header(
        &rust_i18n::t!("appearance_language"),
        appearance,
    ));

    let lang_labels: Vec<String> = vec![
        rust_i18n::t!("appearance_language_auto").to_string(),
        rust_i18n::t!("appearance_language_en").to_string(),
        rust_i18n::t!("appearance_language_zh").to_string(),
    ];
    let lang_index = match current_language {
        LanguageChoice::Auto => 0,
        LanguageChoice::English => 1,
        LanguageChoice::Chinese => 2,
    };
    let lang_label = Text::new_inline(
        rust_i18n::t!("appearance_language"),
        ui_font,
        CONTENT_FONT_SIZE,
    )
    .with_color(theme.active_ui_text_color().into())
    .finish();
    let lang_radio = appearance
        .ui_builder()
        .radio_buttons(
            state.language_radio_mouse_states.clone(),
            lang_labels.into_iter().map(RadioButtonItem::text).collect(),
            state.language_radio_state.clone(),
            Some(lang_index),
            CONTENT_FONT_SIZE,
            RadioButtonLayout::Row,
        )
        .on_change(Rc::new(move |ctx, _, index| {
            if let Some(idx) = index {
                let choice = match idx {
                    0 => LanguageChoice::Auto,
                    1 => LanguageChoice::English,
                    2 => LanguageChoice::Chinese,
                    _ => return,
                };
                ctx.dispatch_typed_action(TerminalGridAction::SetLanguage(choice));
            }
        }))
        .build()
        .finish();
    col.add_child(render_setting_row(lang_label, lang_radio));
    col.add_child(crate::settings_view::render_helpers::render_separator(
        appearance,
    ));

    // --- 编辑器（文件面板「编辑」用）---
    col.add_child(render_category_header(
        &rust_i18n::t!("settings_editor_section"),
        appearance,
    ));
    let editor_label = Text::new_inline(
        rust_i18n::t!("settings_open_file_editor"),
        ui_font,
        CONTENT_FONT_SIZE,
    )
    .with_color(theme.active_ui_text_color().into())
    .finish();
    let editor_dropdown =
        Container::new(ChildView::new(open_file_editor_dropdown).finish()).finish();
    col.add_child(render_setting_row(editor_label, editor_dropdown));

    // diff / 代码查看器「复用单标签」开关（ADR 0002，默认开启）
    let reuse_label = Text::new_inline(
        rust_i18n::t!("settings_reuse_view_tab"),
        ui_font,
        CONTENT_FONT_SIZE,
    )
    .with_color(theme.active_ui_text_color().into())
    .finish();
    let reuse_switch = appearance
        .ui_builder()
        .switch(state.reuse_view_tab_switch.clone())
        .check(reuse_view_tab)
        .build()
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::SetReuseViewTab(!reuse_view_tab));
        })
        .finish();
    col.add_child(render_setting_row(reuse_label, reuse_switch));

    Container::new(
        Align::new(
            ConstrainedBox::new(col.finish())
                .with_max_width(MAX_PAGE_WIDTH)
                .finish(),
        )
        .top_center()
        .finish(),
    )
    .with_uniform_padding(PAGE_PADDING)
    .finish()
}

// warp: appearance_page.rs:3909-4054 — Text 区块四列布局
fn render_text_section(
    state: &AppearancePageState,
    _current_font_name: &str,
    _available_fonts: &[String],
    current_font_size: f32,
    line_height_ratio: f32,
    _current_font_weight: warpui::fonts::Weight,
    font_family_dropdown: &ViewHandle<
        super::super::warp_filterable_dropdown::FilterableDropdown<TerminalGridAction>,
    >,
    font_weight_dropdown: &ViewHandle<
        super::super::warp_dropdown_view::Dropdown<TerminalGridAction>,
    >,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();

    let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Start);

    // (1) Terminal font — warp: appearance_page.rs:3927-3971
    let mut terminal_font = Flex::column();
    terminal_font.add_child(
        appearance
            .ui_builder()
            .label(rust_i18n::t!("appearance_terminal_font"))
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                ..Default::default()
            })
            .build()
            .finish(),
    );
    terminal_font.add_child(
        Container::new(ChildView::new(font_family_dropdown).finish())
            .with_margin_bottom(10.0)
            .finish(),
    );

    // warp: appearance_page.rs:3932-3968 — "View all available system fonts" checkbox
    terminal_font.add_child(
        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Container::new(
                        appearance
                            .ui_builder()
                            .checkbox(state.view_all_fonts_checkbox_state.clone(), None)
                            .check(state.view_all_fonts)
                            .build()
                            .on_click(move |ctx, _, _| {
                                ctx.dispatch_typed_action(TerminalGridAction::ToggleViewAllFonts);
                            })
                            .finish(),
                    )
                    .with_margin_left(-7.0)
                    .finish(),
                )
                .with_child(
                    Shrinkable::new(
                        1.0,
                        appearance
                            .ui_builder()
                            .span(rust_i18n::t!("appearance_view_all_fonts"))
                            .build()
                            .with_margin_left(2.0)
                            .finish(),
                    )
                    .finish(),
                )
                .finish(),
        )
        .with_margin_bottom(16.0)
        .finish(),
    );

    row.add_child(Shrinkable::new(1.0, terminal_font.finish()).finish());

    // (2) Font weight — warp: appearance_page.rs:3988-3992
    let mut font_weight = Flex::column();
    font_weight.add_child(
        appearance
            .ui_builder()
            .label(rust_i18n::t!("appearance_font_weight"))
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                ..Default::default()
            })
            .build()
            .with_margin_left(12.0)
            .finish(),
    );
    font_weight.add_child(
        Container::new(ChildView::new(font_weight_dropdown).finish())
            .with_margin_left(12.0)
            .finish(),
    );
    row.add_child(Container::new(font_weight.finish()).finish());

    // (3) Font size (px) — warp: appearance_page.rs:3996-4050
    let mut font_size = Flex::column();
    font_size.add_child(
        appearance
            .ui_builder()
            .label(rust_i18n::t!("appearance_font_size"))
            .with_style(UiComponentStyles {
                margin: Some(Coords {
                    left: 2.0,
                    ..Default::default()
                }),
                font_size: Some(CONTENT_FONT_SIZE),
                ..Default::default()
            })
            .build()
            .finish(),
    );
    font_size.add_child(
        Container::new(render_value_stepper(
            format!("{:.0}", current_font_size),
            &state.font_size_minus_state,
            &state.font_size_plus_state,
            TerminalGridAction::SetTerminalFontSize(current_font_size - 1.0),
            TerminalGridAction::SetTerminalFontSize(current_font_size + 1.0),
            FONT_SIZE_INPUT_WIDTH,
            appearance,
        ))
        .with_padding_top(4.0)
        .finish(),
    );
    row.add_child(
        Container::new(font_size.finish())
            .with_margin_left(12.0)
            .finish(),
    );

    // (4) Line height — warp: appearance_page.rs:3806-3893
    let mut line_height = Flex::column();
    line_height.add_child(
        appearance
            .ui_builder()
            .label(rust_i18n::t!("appearance_line_height"))
            .with_style(UiComponentStyles {
                margin: Some(Coords {
                    left: 12.0,
                    ..Default::default()
                }),
                font_size: Some(CONTENT_FONT_SIZE),
                ..Default::default()
            })
            .build()
            .finish(),
    );
    line_height.add_child(
        Container::new(render_value_stepper(
            format!("{:.1}", line_height_ratio),
            &state.line_height_minus_state,
            &state.line_height_plus_state,
            TerminalGridAction::SetLineHeight(line_height_ratio - 0.1),
            TerminalGridAction::SetLineHeight(line_height_ratio + 0.1),
            LINE_HEIGHT_INPUT_WIDTH,
            appearance,
        ))
        .with_margin_left(12.0)
        .with_padding_top(4.0)
        .finish(),
    );

    // warp: appearance_page.rs:3861-3891 — "Reset to default"
    let is_default = (line_height_ratio - 1.2).abs() < 0.01;
    let disabled_text_color = theme.disabled_text_color(theme.surface_2()).into();
    line_height.add_child(
        appearance
            .ui_builder()
            .reset_button(
                ButtonVariant::Text,
                state.line_height_reset_state.clone(),
                !is_default,
                disabled_text_color,
            )
            .with_style(UiComponentStyles {
                padding: Some(Coords::default().bottom(HEADER_PADDING).top(4.0)),
                margin: Some(Coords {
                    top: 2.0,
                    left: 8.0,
                    ..Default::default()
                }),
                font_size: Some(appearance.ui_builder().ui_font_size() * 0.8),
                ..Default::default()
            })
            .with_text_label(rust_i18n::t!("appearance_reset_default").to_string())
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::ResetLineHeight);
            })
            .finish(),
    );
    row.add_child(line_height.finish());

    Container::new(row.finish())
        .with_padding_bottom(HEADER_PADDING)
        .finish()
}

// warp: stepper 风格的值控件，外观模仿 surface_2 input field
fn render_value_stepper(
    value_text: String,
    minus_state: &MouseStateHandle,
    plus_state: &MouseStateHandle,
    minus_action: TerminalGridAction,
    plus_action: TerminalGridAction,
    width: f32,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui_font = appearance.ui_font_family();
    let text_color: ColorU = theme.active_ui_text_color().into();
    let outline = theme.outline();
    let minus_state = minus_state.clone();
    let plus_state = plus_state.clone();

    let minus_btn = render_inline_stepper_btn(
        "\u{2212}",
        minus_state,
        minus_action,
        text_color,
        outline,
        ui_font,
    );

    let value = Shrinkable::new(
        1.0,
        Align::new(
            Text::new_inline(value_text, ui_font, CONTENT_FONT_SIZE)
                .with_color(text_color)
                .finish(),
        )
        .finish(),
    )
    .finish();

    let plus_btn =
        render_inline_stepper_btn("+", plus_state, plus_action, text_color, outline, ui_font);

    Container::new(
        ConstrainedBox::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(minus_btn)
                .with_child(value)
                .with_child(plus_btn)
                .finish(),
        )
        .with_width(width)
        .with_height(INPUT_HEIGHT)
        .finish(),
    )
    .with_background(theme.surface_2())
    .with_border(Border::all(1.0).with_border_fill(outline))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(5.0)))
    .finish()
}

fn render_inline_stepper_btn(
    label: &str,
    mouse_state: MouseStateHandle,
    action: TerminalGridAction,
    text_color: ColorU,
    outline: warp_core::ui::theme::Fill,
    ui_font: FamilyId,
) -> Box<dyn Element> {
    let label = label.to_string();

    EventHandler::new(
        Hoverable::new(mouse_state, move |mouse| {
            let mut c = Container::new(
                Align::new(
                    Text::new_inline(label.clone(), ui_font, CONTENT_FONT_SIZE)
                        .with_color(text_color)
                        .finish(),
                )
                .finish(),
            )
            .with_horizontal_padding(6.0);
            if mouse.is_hovered() {
                c = c.with_background(outline);
            }
            c.finish()
        })
        .finish(),
    )
    .on_left_mouse_down(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
        DispatchEventResult::StopPropagation
    })
    .finish()
}

// warp: appearance_page.rs:2630-2713 — "Current theme" 行
fn render_current_theme_row(
    mouse_state: &MouseStateHandle,
    current_theme: ThemeChoice,
    monospace_font: FamilyId,
    ui_font: FamilyId,
    ui_theme: &warp_core::ui::theme::WarpTheme,
) -> Box<dyn Element> {
    let label_text = current_theme.label().to_string();
    let card_theme = ui_theme.clone();
    let mouse_state = mouse_state.clone();
    let text_color: ColorU = ui_theme.active_ui_text_color().into();
    let outline = ui_theme.outline();

    let row = EventHandler::new(
        Hoverable::new(mouse_state, move |mouse| {
            let bg = if mouse.is_hovered() {
                Some(outline)
            } else {
                None
            };

            let preview =
                nexshell::themes::theme::render_preview(&card_theme, monospace_font, Some(0.6));

            let mut container = Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Container::new(
                            Text::new_inline(
                                "Current theme".to_string(),
                                ui_font,
                                CONTENT_FONT_SIZE,
                            )
                            .with_color(text_color)
                            .finish(),
                        )
                        .with_margin_right(16.0)
                        .finish(),
                    )
                    .with_child(
                        Container::new(preview)
                            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                            .with_margin_right(16.0)
                            .finish(),
                    )
                    .with_child(
                        Text::new_inline(label_text.clone(), ui_font, CONTENT_FONT_SIZE)
                            .with_color(text_color)
                            .finish(),
                    )
                    .finish(),
            )
            .with_horizontal_padding(12.0)
            .with_vertical_padding(12.0)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)));

            if let Some(bg) = bg {
                container = container.with_background(bg);
            }
            container.finish()
        })
        .finish(),
    )
    .on_left_mouse_down(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::ShowThemeChooser);
        DispatchEventResult::StopPropagation
    })
    .finish();

    Container::new(row)
        .with_padding_bottom(HEADER_PADDING)
        .finish()
}
