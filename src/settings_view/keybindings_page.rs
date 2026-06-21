use warp_core::ui::appearance::Appearance;
use warpui::{
    elements::{
        Align, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Element, Expanded,
        Flex, MainAxisSize, ParentElement, Radius, Text,
    },
    fonts::FamilyId,
};

use super::render_helpers::{
    render_page_title, CONTENT_FONT_SIZE, HEADER_PADDING, MAX_PAGE_WIDTH, PAGE_PADDING,
};

const KEYBINDING_KEYS: &[(&str, &str)] = &[
    ("key_clear_screen", "⌘K"),
    ("key_find", "⌘F"),
    ("key_copy", "⌘C"),
    ("key_paste", "⌘V"),
    ("key_new_tab", "⌘T"),
    ("key_close_tab", "⌘W"),
    ("key_prev_tab", "⌃⇧Tab"),
    ("key_next_tab", "⌃Tab"),
    ("key_split_right", "⌘D"),
    ("key_split_down", "⇧⌘D"),
    ("key_close_pane", "⇧⌘W"),
    ("key_nav_left", "⌥⌘←"),
    ("key_nav_right", "⌥⌘→"),
    ("key_nav_up", "⌥⌘↑"),
    ("key_nav_down", "⌥⌘↓"),
    ("key_maximize_pane", "⇧⌘Enter"),
    ("key_increase_font", "⌘+"),
    ("key_decrease_font", "⌘−"),
    ("key_reset_font", "⌘0"),
];

pub fn render_keybindings_page(
    appearance: &Appearance,
    monospace_font: FamilyId,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui_font = appearance.ui_font_family();

    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    col.add_child(render_page_title(
        &rust_i18n::t!("settings_keybindings"),
        appearance,
    ));

    for &(i18n_key, keys) in KEYBINDING_KEYS {
        let label = Text::new_inline(rust_i18n::t!(i18n_key), ui_font, CONTENT_FONT_SIZE)
            .with_color(theme.active_ui_text_color().into())
            .finish();

        let pill = Container::new(
            Container::new(
                Text::new_inline(keys.to_string(), monospace_font, CONTENT_FONT_SIZE)
                    .with_color(theme.active_ui_text_color().into())
                    .finish(),
            )
            .with_padding_left(8.0)
            .with_padding_right(8.0)
            .with_padding_top(3.0)
            .with_padding_bottom(3.0)
            .with_background(theme.surface_2())
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .finish(),
        )
        .finish();

        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(1.0, Align::new(label).left().finish()).finish())
            .with_child(pill)
            .finish();

        col.add_child(
            Container::new(row)
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
        );
    }

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
