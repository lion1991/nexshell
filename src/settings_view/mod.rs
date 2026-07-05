pub mod appearance_page;
pub mod keybindings_page;
pub mod render_helpers;

use std::sync::{Arc, Mutex};

use warp_core::ui::appearance::Appearance;
use warpui::{
    color::ColorU,
    elements::{
        Align, Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
        CornerRadius, CrossAxisAlignment, DispatchEventResult, Element, EventHandler, Expanded,
        Fill, Flex, Hoverable, Icon, MainAxisAlignment, MainAxisSize, MouseState, MouseStateHandle,
        ParentElement, Radius, ScrollbarWidth, Shrinkable, Text,
    },
    fonts::{FamilyId, Properties, Weight},
    ui_components::{
        button::ButtonVariant,
        components::{Coords, UiComponent, UiComponentStyles},
    },
    AppContext, SingletonEntity as _, ViewHandle,
};

use self::appearance_page::{render_appearance_page, AppearancePageState};
use self::keybindings_page::render_keybindings_page;
use self::render_helpers::{CONTENT_FONT_SIZE, HEADER_PADDING, SIDEBAR_WIDTH};
use super::terminal_grid_element::{
    self, CursorStyleChoice, NexSettingsSection, TerminalGridAction, ThemeChoice,
};

// warp: theme_chooser.rs 常量
fn theme_chooser_title() -> String {
    rust_i18n::t!("appearance_themes").to_string()
}
const THEME_CHOOSER_WIDTH: f32 = 280.0;
const THEME_TITLE_FONT_SIZE: f32 = 16.0;
const THEME_TITLE_MARGIN: f32 = 12.0;
const THEME_NAME_FONT_SIZE: f32 = 14.0;
const THEME_NAME_MARGIN_LEFT: f32 = 16.0;
const THEME_ITEM_PADDING: f32 = 16.0;
const CLOSE_ICON: &str = "icons/close.svg";

impl NexSettingsSection {
    fn label(self) -> String {
        match self {
            Self::Appearance => rust_i18n::t!("settings_appearance").to_string(),
            Self::Keybindings => rust_i18n::t!("settings_keybindings").to_string(),
        }
    }

    const ALL: [Self; 2] = [Self::Appearance, Self::Keybindings];
}

pub struct NexSettingsViewState {
    pub current_page: NexSettingsSection,
    pub nav_button_states: [MouseStateHandle; 2],
    pub appearance_state: AppearancePageState,
    pub content_scroll_state: ClippedScrollStateHandle,
    // warp: theme_chooser 状态（WarpTheme 预缓存，避免每帧重建）
    pub theme_chooser_open: bool,
    pub theme_chooser_card_states: Vec<MouseStateHandle>,
    pub theme_chooser_cached_themes: Vec<warp_core::ui::theme::WarpTheme>,
    pub theme_chooser_scroll_state: ClippedScrollStateHandle,
    pub theme_chooser_close_state: MouseStateHandle,
}

impl Default for NexSettingsViewState {
    fn default() -> Self {
        Self {
            current_page: NexSettingsSection::Appearance,
            nav_button_states: [
                Arc::new(Mutex::new(MouseState::default())),
                Arc::new(Mutex::new(MouseState::default())),
            ],
            appearance_state: AppearancePageState::default(),
            content_scroll_state: ClippedScrollStateHandle::new(),
            theme_chooser_open: false,
            theme_chooser_card_states: ThemeChoice::ALL
                .iter()
                .map(|_| Arc::new(Mutex::new(MouseState::default())))
                .collect(),
            theme_chooser_cached_themes: ThemeChoice::ALL
                .iter()
                .map(|c| c.to_warp_theme())
                .collect(),
            theme_chooser_scroll_state: ClippedScrollStateHandle::new(),
            theme_chooser_close_state: Arc::new(Mutex::new(MouseState::default())),
        }
    }
}

// warp/app/src/settings_view/mod.rs:2265-2432
pub fn render_settings_view(
    state: &NexSettingsViewState,
    current_theme: ThemeChoice,
    current_font_size: f32,
    line_height_ratio: f32,
    window_opacity: u8,
    cursor_style: CursorStyleChoice,
    current_font_weight: warpui::fonts::Weight,
    current_font_name: &str,
    available_fonts: &[String],
    monospace_font: FamilyId,
    font_family_dropdown: &ViewHandle<
        super::warp_filterable_dropdown::FilterableDropdown<TerminalGridAction>,
    >,
    font_weight_dropdown: &ViewHandle<super::warp_dropdown_view::Dropdown<TerminalGridAction>>,
    open_file_editor_dropdown: &ViewHandle<super::warp_dropdown_view::Dropdown<TerminalGridAction>>,
    current_language: terminal_grid_element::LanguageChoice,
    reuse_view_tab: bool,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();

    // --- Sidebar ---
    let mut nav_col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    for (i, section) in NexSettingsSection::ALL.iter().enumerate() {
        let is_active = state.current_page == *section;
        let mouse_state = state.nav_button_states[i].clone();
        let section_val = *section;

        let variant = if is_active {
            ButtonVariant::Accent
        } else {
            ButtonVariant::Text
        };

        let button = appearance
            .ui_builder()
            .button(variant, mouse_state)
            .with_text_label(section.label())
            .with_style(
                UiComponentStyles::default()
                    .set_border_width(0.)
                    .set_margin(Coords::default().left(12.))
                    .set_padding(Coords::uniform(8.))
                    .set_font_size(CONTENT_FONT_SIZE),
            )
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::SettingsSelectPage(section_val));
            })
            .finish();

        nav_col.add_child(button);
    }

    let sidebar = ConstrainedBox::new(
        Container::new(
            Container::new(nav_col.finish())
                .with_padding_top(HEADER_PADDING * 2.0 + CONTENT_FONT_SIZE)
                .finish(),
        )
        .with_border(Border::right(1.0).with_border_fill(theme.outline()))
        .finish(),
    )
    .with_width(SIDEBAR_WIDTH)
    .finish();

    // --- Content header (居中 "Settings" 标题) ---
    let settings_header = Container::new(
        Align::new(
            warpui::elements::Text::new_inline(
                rust_i18n::t!("settings_title"),
                appearance.ui_font_family(),
                CONTENT_FONT_SIZE,
            )
            .with_color(theme.nonactive_ui_text_color().into())
            .finish(),
        )
        .top_center()
        .finish(),
    )
    .with_padding_top(HEADER_PADDING)
    .finish();

    // --- Content ---
    let page = match state.current_page {
        NexSettingsSection::Appearance => render_appearance_page(
            &state.appearance_state,
            current_theme,
            current_font_size,
            line_height_ratio,
            window_opacity,
            cursor_style,
            current_font_weight,
            current_font_name,
            available_fonts,
            monospace_font,
            font_family_dropdown,
            font_weight_dropdown,
            open_file_editor_dropdown,
            current_language,
            reuse_view_tab,
            appearance,
            app,
        ),
        NexSettingsSection::Keybindings => render_keybindings_page(appearance, monospace_font),
    };

    let content_col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(settings_header)
        .with_child(page)
        .finish();

    let content_scrollable = ClippedScrollable::vertical(
        state.content_scroll_state.clone(),
        content_col,
        ScrollbarWidth::Auto,
        Fill::from(theme.nonactive_ui_detail()),
        Fill::from(theme.active_ui_detail()),
        Fill::None,
    )
    .finish();

    // --- Compose ---
    // warp settings_view/mod.rs:2427-2432
    let mut main_row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Max)
        .with_child(Shrinkable::new(1.0, sidebar).finish())
        .with_child(Shrinkable::new(1.0, content_scrollable).finish());

    // warp: theme_chooser 侧面板
    if state.theme_chooser_open {
        let panel = render_theme_chooser_panel(state, current_theme, monospace_font, appearance);
        main_row.add_child(panel);
    }

    Container::new(main_row.finish())
        .with_background(theme.surface_1())
        .finish()
}

// warp: theme_chooser.rs:855-869 — 主题选择器侧面板
fn render_theme_chooser_panel(
    state: &NexSettingsViewState,
    current_theme: ThemeChoice,
    monospace_font: FamilyId,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui_font = appearance.ui_font_family();
    let text_color: ColorU = theme.active_ui_text_color().into();

    // --- Header: "Themes" 标题 + 关闭按钮 ---
    // warp: theme_chooser.rs:637-688
    let title = Text::new_inline(theme_chooser_title(), ui_font, THEME_TITLE_FONT_SIZE)
        .with_style(Properties::default().weight(Weight::Semibold))
        .with_color(text_color)
        .finish();

    let close_state = state.theme_chooser_close_state.clone();
    let outline = theme.outline();
    let close_btn = EventHandler::new(
        Hoverable::new(close_state, move |mouse| {
            let mut c = Container::new(
                ConstrainedBox::new(Icon::new(CLOSE_ICON, text_color).finish())
                    .with_width(16.0)
                    .with_height(16.0)
                    .finish(),
            )
            .with_uniform_padding(4.0)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)));
            if mouse.is_hovered() {
                c = c.with_background(outline);
            }
            c.finish()
        })
        .finish(),
    )
    .on_left_mouse_down(|ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::CloseThemeChooser);
        DispatchEventResult::StopPropagation
    })
    .finish();

    let header = Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(title)
            .with_child(close_btn)
            .finish(),
    )
    .with_horizontal_padding(THEME_TITLE_MARGIN)
    .with_vertical_padding(THEME_TITLE_MARGIN)
    .finish();

    // --- 主题卡片列表 ---
    // warp: theme_chooser.rs:898-1001
    let mut list_col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    for (i, choice) in ThemeChoice::ALL.iter().enumerate() {
        let is_selected = *choice == current_theme;
        let card = render_theme_chooser_card(
            *choice,
            is_selected,
            &state.theme_chooser_card_states[i],
            &state.theme_chooser_cached_themes[i],
            monospace_font,
            ui_font,
            appearance,
        );
        list_col.add_child(card);
    }

    let scrollable = Expanded::new(
        1.0,
        ClippedScrollable::vertical(
            state.theme_chooser_scroll_state.clone(),
            list_col.finish(),
            ScrollbarWidth::Auto,
            Fill::from(theme.nonactive_ui_detail()),
            Fill::from(theme.active_ui_detail()),
            Fill::None,
        )
        .finish(),
    )
    .finish();

    // --- 面板容器 ---
    ConstrainedBox::new(
        Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(header)
                .with_child(scrollable)
                .finish(),
        )
        .with_background(theme.surface_1())
        .with_border(Border::left(1.0).with_border_fill(theme.outline()))
        .finish(),
    )
    .with_width(THEME_CHOOSER_WIDTH)
    .finish()
}

// warp: theme_chooser.rs:898-1001 — 单个主题卡片
fn render_theme_chooser_card(
    choice: ThemeChoice,
    is_selected: bool,
    mouse_state: &MouseStateHandle,
    cached_theme: &warp_core::ui::theme::WarpTheme,
    monospace_font: FamilyId,
    ui_font: FamilyId,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let text_color: ColorU = theme.active_ui_text_color().into();
    let selected_bg: ColorU = theme.surface_2().into();
    let mouse_state = mouse_state.clone();
    let card_theme = cached_theme.clone();
    let label = choice.label().to_string();

    EventHandler::new(
        Hoverable::new(mouse_state, move |mouse| {
            let preview =
                nexshell::themes::theme::render_preview(&card_theme, monospace_font, None);

            let name = Text::new_inline(label.clone(), ui_font, THEME_NAME_FONT_SIZE)
                .with_color(text_color)
                .finish();

            let mut container = Container::new(
                Flex::column()
                    .with_child(preview)
                    .with_child(
                        Container::new(name)
                            .with_margin_top(8.0)
                            .with_margin_left(THEME_NAME_MARGIN_LEFT)
                            .finish(),
                    )
                    .finish(),
            )
            .with_padding_top(THEME_ITEM_PADDING)
            .with_padding_bottom(THEME_ITEM_PADDING);

            if is_selected {
                container = container.with_background_color(selected_bg);
            } else if mouse.is_hovered() {
                container = container.with_background(theme.outline());
            }
            container.finish()
        })
        .finish(),
    )
    .on_left_mouse_down(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::SetTheme(choice));
        DispatchEventResult::StopPropagation
    })
    .finish()
}
