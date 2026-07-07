use warp_core::ui::appearance::Appearance;
use warpui::{
    elements::{
        Align, Border, ChildAnchor, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox,
        Container, CornerRadius, CrossAxisAlignment, DropShadow, Element, Expanded, Fill, Flex,
        Hoverable, Icon, MainAxisAlignment, MainAxisSize, MouseStateHandle, OffsetPositioning,
        ParentElement, PositionedElementAnchor, PositionedElementOffsetBounds, Radius,
        SavePosition, ScrollbarWidth, Shrinkable, Stack, Text,
    },
    geometry::vector::vec2f,
    ui_components::{
        button::{ButtonVariant, TextAndIcon, TextAndIconAlignment},
        components::{Coords, UiComponent, UiComponentStyles},
    },
    Action,
};

const TOP_MENU_BAR_HEIGHT: f32 = 30.0;
const MENU_VERTICAL_PADDING: f32 = 9.0;
const MENU_ITEM_VERTICAL_PADDING: f32 = 5.0;
const MENU_ITEM_HORIZONTAL_PADDING: f32 = 14.0;
const DEFAULT_MENU_MAX_HEIGHT: f32 = 300.0;
use nexshell::design_tokens::DROP_SHADOW_COLOR;

#[derive(Clone)]
pub struct WarpDropdownOption<A: Action + Clone> {
    pub label: String,
    pub action: A,
    pub selected: bool,
    pub state: MouseStateHandle,
    #[allow(dead_code)]
    pub icon_path: Option<&'static str>,
    #[allow(dead_code)]
    pub shortcut: Option<String>,
}

#[allow(dead_code)]
impl<A: Action + Clone> WarpDropdownOption<A> {
    pub fn new(
        label: impl Into<String>,
        action: A,
        selected: bool,
        state: MouseStateHandle,
    ) -> Self {
        Self {
            label: label.into(),
            action,
            selected,
            state,
            icon_path: None,
            shortcut: None,
        }
    }

    pub fn with_icon(mut self, path: &'static str) -> Self {
        self.icon_path = Some(path);
        self
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }
}

pub struct WarpDropdownProps<'a, A: Action + Clone> {
    pub position_id: &'static str,
    pub label: String,
    pub state: &'a MouseStateHandle,
    pub is_open: bool,
    pub options: Vec<WarpDropdownOption<A>>,
    pub toggle_action: A,
    pub appearance: &'a Appearance,
    pub menu_width: f32,
    pub top_bar_height: f32,
}

pub fn render_warp_dropdown<A>(props: WarpDropdownProps<'_, A>) -> Box<dyn Element>
where
    A: Action + Clone,
{
    let top_bar = render_dropdown_top_bar(
        props.label,
        props.state,
        props.toggle_action,
        props.appearance,
        props.top_bar_height,
    );
    render_warp_dropdown_with_top_bar(WarpDropdownCustomProps {
        position_id: props.position_id,
        top_bar,
        is_open: props.is_open,
        options: props.options,
        appearance: props.appearance,
        menu_width: props.menu_width,
        top_bar_height: props.top_bar_height,
    })
}

pub struct WarpDropdownCustomProps<'a, A: Action + Clone> {
    pub position_id: &'static str,
    pub top_bar: Box<dyn Element>,
    pub is_open: bool,
    pub options: Vec<WarpDropdownOption<A>>,
    pub appearance: &'a Appearance,
    pub menu_width: f32,
    pub top_bar_height: f32,
}

pub fn render_warp_dropdown_with_top_bar<A>(
    props: WarpDropdownCustomProps<'_, A>,
) -> Box<dyn Element>
where
    A: Action + Clone,
{
    let mut dropdown_stack = Stack::new().with_child(
        SavePosition::new(
            ConstrainedBox::new(props.top_bar)
                .with_height(props.top_bar_height.max(TOP_MENU_BAR_HEIGHT))
                .with_width(props.menu_width)
                .finish(),
            props.position_id,
        )
        .finish(),
    );

    if props.is_open {
        dropdown_stack.add_positioned_overlay_child(
            render_dropdown_menu(props.options, props.appearance, props.menu_width),
            OffsetPositioning::offset_from_save_position_element(
                props.position_id,
                vec2f(0.0, 2.0),
                PositionedElementOffsetBounds::WindowByPosition,
                PositionedElementAnchor::BottomLeft,
                ChildAnchor::TopLeft,
            ),
        );
    }

    dropdown_stack.finish()
}

fn render_dropdown_top_bar<A>(
    label: String,
    state: &MouseStateHandle,
    toggle_action: A,
    appearance: &Appearance,
    top_bar_height: f32,
) -> Box<dyn Element>
where
    A: Action + Clone,
{
    let compact = top_bar_height <= 32.0;
    let font_size = 12.0;
    let icon_size = if compact {
        vec2f(12.0, 12.0)
    } else {
        vec2f(15.0, 15.0)
    };
    let inner_padding = if compact { 8.0 } else { 10.0 };
    appearance
        .ui_builder()
        .button(ButtonVariant::Secondary, state.clone())
        .with_text_and_icon_label(
            TextAndIcon::new(
                TextAndIconAlignment::TextFirst,
                label,
                Icon::new(
                    "icons/chevron-down.svg",
                    appearance.theme().active_ui_text_color().into_solid(),
                ),
                MainAxisSize::Max,
                MainAxisAlignment::SpaceBetween,
                icon_size,
            )
            .with_inner_padding(inner_padding),
        )
        .with_style(UiComponentStyles {
            padding: Some(Coords {
                top: 5.0,
                bottom: 5.0,
                left: 8.0,
                right: 8.0,
            }),
            font_size: Some(font_size),
            height: Some(top_bar_height),
            ..Default::default()
        })
        .set_clicked_styles(None)
        .build()
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_mouse_down(move |ctx, _, _| {
            ctx.dispatch_typed_action(toggle_action.clone());
        })
        .finish()
}

// warp: menu.rs:2002-2015 — 带 ClippedScrollable 的下拉菜单
fn render_dropdown_menu<A>(
    options: Vec<WarpDropdownOption<A>>,
    appearance: &Appearance,
    menu_width: f32,
) -> Box<dyn Element>
where
    A: Action + Clone,
{
    let theme = appearance.theme();
    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    for option in options {
        col.add_child(render_dropdown_menu_item(option, appearance, menu_width));
    }

    let scrollable = ClippedScrollable::vertical(
        ClippedScrollStateHandle::new(),
        col.finish(),
        ScrollbarWidth::Auto,
        Fill::from(theme.nonactive_ui_detail()),
        Fill::from(theme.active_ui_detail()),
        Fill::None,
    )
    .with_overlayed_scrollbar()
    .finish();

    Container::new(
        ConstrainedBox::new(scrollable)
            .with_width(menu_width)
            .with_max_height(DEFAULT_MENU_MAX_HEIGHT)
            .finish(),
    )
    .with_padding_top(MENU_VERTICAL_PADDING)
    .with_padding_bottom(MENU_VERTICAL_PADDING)
    .with_background(theme.surface_2())
    .with_border(Border::all(1.0).with_border_fill(theme.outline()))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(5.0)))
    .with_drop_shadow(DropShadow::new_with_standard_offset_and_spread(
        DROP_SHADOW_COLOR,
    ))
    .finish()
}

fn render_dropdown_menu_item<A>(
    option: WarpDropdownOption<A>,
    appearance: &Appearance,
    menu_width: f32,
) -> Box<dyn Element>
where
    A: Action + Clone,
{
    let state = option.state;
    let theme = appearance.theme();
    let label = option.label;
    let action = option.action;
    let selected = option.selected;
    let icon_path = option.icon_path;
    let shortcut = option.shortcut;
    let font_family = appearance.ui_builder().ui_font_family();
    let font_size = if menu_width <= 180.0 {
        12.0
    } else {
        appearance.ui_builder().ui_font_size()
    };

    Hoverable::new(state, move |mouse| {
        let is_hovered_or_selected = mouse.is_hovered() || selected;
        let background = if is_hovered_or_selected {
            Some(theme.accent_button_color())
        } else {
            None
        };
        let text_background = background.unwrap_or_else(|| theme.surface_2());
        let text_color = theme.main_text_color(text_background).into_solid();
        let secondary_color = theme.disabled_text_color(text_background).into_solid();

        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceEvenly)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);

        if let Some(path) = icon_path {
            row.add_child(
                Container::new(
                    ConstrainedBox::new(Icon::new(path, text_color).finish())
                        .with_width(font_size)
                        .with_height(font_size)
                        .finish(),
                )
                .with_margin_right(font_size / 2.0)
                .finish(),
            );
        }

        row.add_child(
            Text::new_inline(label.clone(), font_family, font_size)
                .with_color(text_color)
                .finish(),
        );

        row.add_child(Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish());

        if let Some(ref shortcut_text) = shortcut {
            row.add_child(
                Shrinkable::new(
                    1.0,
                    Align::new(
                        Text::new_inline(shortcut_text.clone(), font_family, font_size)
                            .with_color(secondary_color)
                            .finish(),
                    )
                    .right()
                    .finish(),
                )
                .finish(),
            );
        }

        let row = Container::new(row.finish())
            .with_padding_top(MENU_ITEM_VERTICAL_PADDING)
            .with_padding_bottom(MENU_ITEM_VERTICAL_PADDING)
            .with_padding_left(MENU_ITEM_HORIZONTAL_PADDING)
            .with_padding_right(MENU_ITEM_HORIZONTAL_PADDING);

        let row = if let Some(background) = background {
            row.with_background(background)
        } else {
            row
        };

        ConstrainedBox::new(Align::new(row.finish()).left().finish())
            .with_width(menu_width)
            .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_mouse_down(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish()
}
