// warp/app/src/settings_view/settings_page.rs 渲染辅助函数
use warp_core::ui::appearance::Appearance;
use warpui::{
    elements::{
        Align, Border, Container, CrossAxisAlignment, Element, Empty, Flex, MainAxisSize,
        ParentElement, Shrinkable, Text,
    },
    fonts::{Properties, Weight},
};

pub const HEADER_FONT_SIZE: f32 = 23.0;
pub const SUBHEADER_FONT_SIZE: f32 = 16.0;
pub const CONTENT_FONT_SIZE: f32 = 12.0;
pub const HEADER_PADDING: f32 = 15.0;
pub const PAGE_PADDING: f32 = 28.0;
pub const SIDEBAR_WIDTH: f32 = 200.0;
pub const MAX_PAGE_WIDTH: f32 = 800.0;
pub const TOGGLE_BUTTON_RIGHT_PADDING: f32 = 5.0;
const PAGE_TITLE_MARGIN_BOTTOM: f32 = 4.0;
const SUBHEADER_MARGIN_BOTTOM: f32 = 4.0;

// warp settings_page.rs:738-751
pub fn render_page_title(text: &str, appearance: &Appearance) -> Box<dyn Element> {
    Container::new(
        Align::new(
            Text::new_inline(
                text.to_string(),
                appearance.ui_font_family(),
                HEADER_FONT_SIZE,
            )
            .with_style(Properties::default().weight(Weight::Bold))
            .with_color(appearance.theme().active_ui_text_color().into())
            .finish(),
        )
        .left()
        .finish(),
    )
    .with_margin_bottom(PAGE_TITLE_MARGIN_BOTTOM)
    .finish()
}

// warp settings_page.rs:279-296
pub fn render_category_header(text: &str, appearance: &Appearance) -> Box<dyn Element> {
    Container::new(
        Align::new(
            Text::new_inline(
                text.to_string(),
                appearance.ui_font_family(),
                SUBHEADER_FONT_SIZE,
            )
            .with_style(Properties::default().weight(Weight::Bold))
            .with_color(appearance.theme().active_ui_text_color().into())
            .finish(),
        )
        .left()
        .finish(),
    )
    .with_margin_bottom(SUBHEADER_MARGIN_BOTTOM)
    .with_padding_bottom(HEADER_PADDING)
    .finish()
}

// warp settings_page.rs:780-835 (simplified)
pub fn render_setting_row(label: Box<dyn Element>, control: Box<dyn Element>) -> Box<dyn Element> {
    let header = Shrinkable::new(
        1.0,
        Container::new(Align::new(label).left().finish()).finish(),
    )
    .finish();
    let toggle = Container::new(control)
        .with_padding_right(TOGGLE_BUTTON_RIGHT_PADDING)
        .finish();

    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_child(header)
            .with_child(toggle)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .finish(),
    )
    .with_padding_bottom(HEADER_PADDING)
    .finish()
}

// warp settings_page.rs:380-385 — 分隔线
pub fn render_separator(appearance: &Appearance) -> Box<dyn Element> {
    Container::new(Empty::new().finish())
        .with_border(Border::bottom(2.).with_border_fill(appearance.theme().outline()))
        .with_margin_bottom(HEADER_PADDING)
        .finish()
}
