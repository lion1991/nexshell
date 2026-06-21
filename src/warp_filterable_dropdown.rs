// warp: view_components/filterable_dropdown.rs — 完整复制，修改导入路径
use super::warp_dropdown_view::{
    DropdownAction, DropdownItem, MenuHeaderTextFormatter, DROPDOWN_PADDING, TOP_MENU_BAR_HEIGHT,
    TOP_MENU_BAR_MAX_WIDTH,
};
use warp::{
    appearance::Appearance,
    editor::{
        EditorView, Event as EditorEvent, PropagateAndNoOpNavigationKeys, SingleLineEditorOptions,
        TextOptions,
    },
    menu::{Event as MenuEvent, Menu, MenuItem, MenuVariant},
};
use warp_core::ui::icons;
use warp_editor::editor::NavigationKey;
use warpui::{
    elements::{
        Align, Border, ChildAnchor, ChildView, Clipped, ConstrainedBox, Container, CornerRadius,
        CrossAxisAlignment, Dismiss, Element, EventHandler, Flex, MainAxisAlignment, MainAxisSize,
        MouseStateHandle, OffsetPositioning, ParentElement, PositionedElementAnchor,
        PositionedElementOffsetBounds, Radius, SavePosition, Shrinkable, Stack,
    },
    geometry::vector::vec2f,
    ui_components::{
        button::{ButtonVariant, TextAndIcon, TextAndIconAlignment},
        components::{Coords, UiComponent, UiComponentStyles},
    },
    Action, AppContext, BlurContext, Entity, FocusContext, SingletonEntity, TypedActionView, View,
    ViewContext, ViewHandle,
};

const EMPTY_DROPDOWN_HEIGHT: f32 = 50.0;

pub enum FilterableDropdownEvent {
    ToggleExpanded,
    Close,
}

#[derive(Default, Debug, PartialEq)]
pub enum FilterableDropdownOrientation {
    #[allow(dead_code)]
    Up,
    #[default]
    Down,
}

pub struct FilterableDropdown<A: Action + Clone> {
    is_expanded: bool,
    disabled: bool,
    top_bar_mouse_state: MouseStateHandle,
    top_bar_max_width: f32,
    main_axis_size: MainAxisSize,
    dropdown: ViewHandle<Menu<DropdownAction<A>>>,
    filter_editor: ViewHandle<EditorView>,
    selected_item: Option<MenuItem<DropdownAction<A>>>,
    items: Vec<DropdownItem<A>>,
    orientation: FilterableDropdownOrientation,
    static_menu_header: Option<&'static str>,
    button_variant: ButtonVariant,
    style_override: Option<UiComponentStyles>,
    hovered_style_override: Option<UiComponentStyles>,
    menu_header_text_override: Option<MenuHeaderTextFormatter>,
    has_pinned_footer: bool,
    menu_width: Option<f32>,
    vertical_margin: f32,
    top_bar_height: f32,
}

impl<A> FilterableDropdown<A>
where
    A: Action + Clone,
{
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let theme = Appearance::as_ref(ctx).theme();
        let border = Border::all(1.).with_border_fill(theme.outline());
        let dropdown = ctx.add_typed_action_view(|_ctx| {
            Menu::new()
                .with_menu_variant(MenuVariant::scrollable())
                .with_border(border)
                .prevent_interaction_with_other_elements()
        });
        ctx.subscribe_to_view(&dropdown, move |me, _, event, ctx| {
            me.handle_menu_event(event, ctx);
        });

        let filter_editor = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let mut editor = EditorView::single_line(
                SingleLineEditorOptions {
                    text: TextOptions::ui_text(Some(appearance.ui_font_size()), appearance),
                    propagate_and_no_op_vertical_navigation_keys:
                        PropagateAndNoOpNavigationKeys::Always,
                    ..Default::default()
                },
                ctx,
            );
            editor.set_placeholder_text("Search", ctx);
            editor
        });
        ctx.subscribe_to_view(&filter_editor, |me, _, event, ctx| {
            me.handle_filter_editor_event(event, ctx);
        });

        FilterableDropdown {
            is_expanded: false,
            disabled: false,
            dropdown,
            filter_editor,
            top_bar_mouse_state: Default::default(),
            top_bar_max_width: TOP_MENU_BAR_MAX_WIDTH,
            main_axis_size: MainAxisSize::Max,
            selected_item: None,
            items: Default::default(),
            orientation: Default::default(),
            static_menu_header: None,
            button_variant: ButtonVariant::Outlined,
            style_override: None,
            hovered_style_override: None,
            menu_header_text_override: None,
            has_pinned_footer: false,
            menu_width: None,
            vertical_margin: DROPDOWN_PADDING,
            top_bar_height: TOP_MENU_BAR_HEIGHT,
        }
    }

    #[allow(dead_code)]
    pub fn set_menu_header_text_override<F>(&mut self, formatter: F)
    where
        F: Fn(&str) -> String + 'static,
    {
        self.menu_header_text_override = Some(Box::new(formatter));
    }

    #[allow(dead_code)]
    pub fn set_footer<F>(&mut self, builder: F, ctx: &mut ViewContext<Self>)
    where
        F: Fn(&AppContext) -> Box<dyn Element> + 'static,
    {
        self.has_pinned_footer = true;
        self.dropdown.update(ctx, |menu, _| {
            menu.set_pinned_footer_builder(builder);
        });
    }

    #[allow(dead_code)]
    pub fn clear_footer(&mut self, ctx: &mut ViewContext<Self>) {
        self.has_pinned_footer = false;
        self.dropdown.update(ctx, |menu, _| {
            menu.clear_pinned_footer_builder();
        });
    }

    #[allow(dead_code)]
    pub fn set_main_axis_size(
        &mut self,
        main_axis_size: MainAxisSize,
        ctx: &mut ViewContext<Self>,
    ) {
        self.main_axis_size = main_axis_size;
        ctx.notify();
    }

    #[allow(dead_code)]
    pub fn set_style(&mut self, style: UiComponentStyles) {
        self.style_override = Some(style);
    }

    #[allow(dead_code)]
    pub fn set_button_variant(&mut self, button_variant: ButtonVariant) {
        self.button_variant = button_variant;
    }

    #[allow(dead_code)]
    pub fn set_orientation(&mut self, orientation: FilterableDropdownOrientation) {
        self.orientation = orientation;
    }

    pub fn add_items(&mut self, items: Vec<DropdownItem<A>>, ctx: &mut ViewContext<Self>) {
        self.items.extend(items.iter().cloned());
        self.set_filtered_items(ctx);
    }

    pub fn set_items(&mut self, items: Vec<DropdownItem<A>>, ctx: &mut ViewContext<Self>) {
        self.items = items;
        self.set_filtered_items(ctx);

        let label = self.current_selected_item_label();
        if !label.is_empty() && !self.items.iter().any(|item| item.display_text == label) {
            self.selected_item = None;
            ctx.notify();
        }
    }

    #[allow(dead_code)]
    pub fn set_rich_items(
        &mut self,
        items: Vec<MenuItem<DropdownAction<A>>>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.items = items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Item(fields) => {
                    let label = fields.label().to_string();
                    fields.on_select_action().and_then(|action| {
                        if let DropdownAction::SelectActionAndClose(a) = action {
                            Some(DropdownItem::new(label, a.clone()))
                        } else {
                            None
                        }
                    })
                }
                _ => None,
            })
            .collect();

        self.dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_items(items, ctx);
        });
        ctx.notify();
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)]
    pub fn reset_selection(&mut self, ctx: &mut ViewContext<Self>) {
        self.dropdown.update(ctx, |dropdown, ctx| {
            dropdown.reset_selection(ctx);
            ctx.notify();
        });
    }

    pub fn set_selected_by_name(
        &mut self,
        selected_item: impl AsRef<str>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_selected_by_name(selected_item.as_ref(), ctx);
            ctx.notify();
        });

        let selected_item_in_dropdown = self.selected_item_in_dropdown(ctx);
        if selected_item_in_dropdown.is_some()
            || self.current_selected_item_label() != selected_item.as_ref()
        {
            self.selected_item = selected_item_in_dropdown;
        }
        ctx.notify();
    }

    #[allow(dead_code)]
    pub fn set_selected_by_index(&mut self, selected_index: usize, ctx: &mut ViewContext<Self>) {
        self.dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_selected_by_index(selected_index, ctx);
            ctx.notify();
        });
        self.selected_item = self.selected_item_in_dropdown(ctx);
        ctx.notify();
    }

    #[allow(dead_code)]
    pub fn set_selected_by_action(&mut self, action: A, ctx: &mut ViewContext<Self>)
    where
        A: PartialEq,
    {
        self.dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_selected_by_action(&DropdownAction::SelectActionAndClose(action), ctx);
        });
        self.selected_item = self.selected_item_in_dropdown(ctx);
        ctx.notify();
    }

    pub fn set_top_bar_max_width(&mut self, max_width: f32) {
        self.top_bar_max_width = max_width;
    }

    pub fn set_menu_width(&mut self, width: f32, ctx: &mut ViewContext<Self>) {
        self.menu_width = Some(width);
        self.dropdown.update(ctx, |menu, ctx| {
            menu.set_width(width);
            ctx.notify();
        })
    }

    #[allow(dead_code)]
    pub fn set_disabled(&mut self, ctx: &mut ViewContext<Self>) {
        self.disabled = true;
        ctx.notify();
    }

    #[allow(dead_code)]
    pub fn set_enabled(&mut self, ctx: &mut ViewContext<Self>) {
        self.disabled = false;
        ctx.notify();
    }

    fn selected_item_in_dropdown(
        &self,
        ctx: &mut ViewContext<Self>,
    ) -> Option<MenuItem<DropdownAction<A>>> {
        self.dropdown
            .read(ctx, |dropdown, _| dropdown.selected_item())
    }

    fn current_selected_item_label(&self) -> &str {
        if let Some(MenuItem::Item(fields)) = self.selected_item.as_ref() {
            fields.label()
        } else {
            ""
        }
    }

    #[allow(dead_code)]
    pub fn selected_item_label(&self) -> Option<String> {
        match self.selected_item.as_ref() {
            Some(MenuItem::Item(fields)) => Some(fields.label().to_string()),
            _ => None,
        }
    }

    fn focus(&mut self, _delta: usize, ctx: &mut ViewContext<Self>) {
        ctx.focus(&self.filter_editor);
        ctx.notify();
    }

    fn select_action_and_close(&mut self, action: &A, ctx: &mut ViewContext<Self>) {
        if self.dropdown_items_len(ctx) > 0 {
            ctx.dispatch_typed_action_deferred(action.clone());
        } else {
            self.selected_item = None;
        }
        self.close(ctx);
    }

    pub fn close(&mut self, ctx: &mut ViewContext<Self>) {
        self.is_expanded = false;
        ctx.emit(FilterableDropdownEvent::Close);
        ctx.notify();
    }

    pub fn toggle_expanded(&mut self, ctx: &mut ViewContext<Self>) {
        self.is_expanded = !self.is_expanded;
        if self.is_expanded {
            ctx.focus(&self.filter_editor);
            ctx.emit(FilterableDropdownEvent::ToggleExpanded);
        }
        ctx.notify();
    }

    #[allow(dead_code)]
    pub fn is_expanded(&self) -> bool {
        self.is_expanded
    }

    fn render_closed_top_bar(&self, appearance: &Appearance) -> Box<dyn Element> {
        let (selected_item_text, font_family_id) = match self.static_menu_header {
            Some(header) => (header.to_string(), None),
            None => match self.selected_item.clone() {
                Some(MenuItem::Item(fields)) => {
                    let label = fields.label();
                    let text = if let Some(formatter) = &self.menu_header_text_override {
                        formatter(label)
                    } else {
                        label.to_string()
                    };
                    (text, fields.override_font_family())
                }
                _ => (String::new(), None),
            },
        };

        let mut top_bar = appearance
            .ui_builder()
            .button(self.button_variant, self.top_bar_mouse_state.clone())
            .with_text_and_icon_label(
                TextAndIcon::new(
                    TextAndIconAlignment::TextFirst,
                    selected_item_text,
                    icons::Icon::ChevronDown
                        .to_warpui_icon(appearance.theme().active_ui_text_color()),
                    self.main_axis_size,
                    MainAxisAlignment::SpaceBetween,
                    vec2f(15., 15.),
                )
                .with_inner_padding(10.),
            )
            .with_style(self.style_override.unwrap_or(UiComponentStyles {
                padding: Some(Coords {
                    top: 5.,
                    bottom: 5.,
                    left: 8.,
                    right: 8.,
                }),
                ..Default::default()
            }))
            .set_clicked_styles(None);

        if let Some(hovered_style) = self.hovered_style_override {
            top_bar = top_bar.with_hovered_styles(hovered_style);
        }

        if self.disabled {
            top_bar = top_bar.disabled();
        }

        if let Some(font_family_id) = font_family_id {
            top_bar =
                top_bar.with_style(UiComponentStyles::default().set_font_family_id(font_family_id))
        }

        top_bar
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(DropdownAction::<A>::ToggleExpanded);
            })
            .finish()
    }

    fn render_filter_input(&self, appearance: &Appearance) -> Box<dyn Element> {
        let h_padding = self
            .style_override
            .and_then(|s| s.padding)
            .map(|p| (p.left, p.right))
            .unwrap_or((8., 8.));

        let search_icon = ConstrainedBox::new(
            icons::Icon::SearchSmall
                .to_warpui_icon(appearance.theme().active_ui_text_color())
                .finish(),
        )
        .with_width(12.)
        .with_height(12.)
        .finish();
        let filter_editor =
            Container::new(Clipped::new(ChildView::new(&self.filter_editor).finish()).finish())
                .with_margin_left(4.)
                .finish();
        let filter_bar = Flex::row()
            .with_child(search_icon)
            .with_child(Shrinkable::new(1., filter_editor).finish())
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Max)
            .finish();

        let centered_content = Flex::column()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Max)
            .with_child(filter_bar)
            .finish();

        Container::new(centered_content)
            .with_padding_left(h_padding.0)
            .with_padding_right(h_padding.1)
            .with_border(
                Border::all(1.).with_border_fill(appearance.theme().foreground().with_opacity(20)),
            )
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
            .finish()
    }

    fn render_top_bar(&self, appearance: &Appearance) -> Box<dyn Element> {
        let top_bar_element = if self.is_expanded {
            self.render_filter_input(appearance)
        } else {
            self.render_closed_top_bar(appearance)
        };

        SavePosition::new(
            Container::new(
                ConstrainedBox::new(top_bar_element)
                    .with_max_width(self.top_bar_max_width)
                    .with_height(self.top_bar_height)
                    .finish(),
            )
            .finish(),
            &self.top_bar_label(),
        )
        .finish()
    }

    fn render_empty_menu(&self, appearance: &Appearance) -> Box<dyn Element> {
        let background_fill = appearance.theme().surface_2();
        let empty_text = appearance
            .ui_builder()
            .span("No matches found.")
            .with_style(UiComponentStyles {
                font_color: Some(appearance.theme().sub_text_color(background_fill).into()),
                ..Default::default()
            })
            .build()
            .finish();
        let empty_menu = ConstrainedBox::new(
            Container::new(Align::new(empty_text).finish())
                .with_background(background_fill)
                .with_border(
                    Border::all(1.)
                        .with_border_fill(appearance.theme().foreground().with_opacity(20)),
                )
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                .finish(),
        )
        .with_max_width(self.top_bar_max_width)
        .with_height(EMPTY_DROPDOWN_HEIGHT)
        .finish();

        Dismiss::new(EventHandler::new(empty_menu).finish())
            .on_dismiss(|ctx, _app| {
                ctx.dispatch_typed_action(DropdownAction::<A>::Close);
            })
            .prevent_interaction_with_other_elements()
            .finish()
    }

    fn top_bar_label(&self) -> String {
        format!("dropdown_top_bar_{}", self.dropdown.id())
    }

    fn handle_menu_event(&mut self, event: &MenuEvent, ctx: &mut ViewContext<Self>) {
        match event {
            MenuEvent::Close { via_select_item: _ } => self.close(ctx),
            MenuEvent::ItemSelected => {
                self.selected_item = self.selected_item_in_dropdown(ctx);
                ctx.notify();
            }
            MenuEvent::ItemHovered => {}
        }
    }

    fn handle_filter_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        match event {
            EditorEvent::Edited(_) => self.set_filtered_items(ctx),
            EditorEvent::Escape => self.close(ctx),
            EditorEvent::Enter => {
                let selected_action = match self.selected_item.as_ref() {
                    Some(MenuItem::Item(fields)) => Some(fields.on_select_action().cloned()),
                    _ => None,
                };

                if let Some(Some(action)) = selected_action {
                    self.handle_action(&action, ctx);
                }
                ctx.notify();
            }
            EditorEvent::Navigate(NavigationKey::Up) => {
                if self.dropdown_items_len(ctx) == 0 {
                    return;
                }

                self.dropdown.update(ctx, |dropdown, ctx| {
                    dropdown.select_previous(ctx);
                });

                self.selected_item = self.selected_item_in_dropdown(ctx);
                ctx.notify();
            }
            EditorEvent::Navigate(NavigationKey::Down) => {
                if self.dropdown_items_len(ctx) == 0 {
                    return;
                }

                self.dropdown.update(ctx, |dropdown, ctx| {
                    dropdown.select_next(ctx);
                });

                self.selected_item = self.selected_item_in_dropdown(ctx);
                ctx.notify();
            }
            _ => (),
        }
    }

    fn filter_query(&self, ctx: &AppContext) -> String {
        self.filter_editor.as_ref(ctx).buffer_text(ctx)
    }

    fn set_filtered_items(&mut self, ctx: &mut ViewContext<Self>) {
        let filter_query = self.filter_query(ctx).to_lowercase();

        let current_label = self.current_selected_item_label();
        let mut current_label_not_visible = true;
        self.dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_items(
                self.items
                    .iter()
                    .filter(|item| {
                        let item_matches_filter =
                            item.display_text.to_lowercase().contains(&filter_query);
                        if item.display_text == current_label && item_matches_filter {
                            current_label_not_visible = false;
                        };
                        item_matches_filter
                    })
                    .map(|item| item.into()),
                ctx,
            );

            if current_label_not_visible && !dropdown.is_empty() {
                dropdown.set_selected_by_index(0, ctx);
            } else {
                dropdown.set_selected_by_name(current_label, ctx);
            }
            ctx.notify();
        });
        ctx.notify();
    }

    fn dropdown_items_len(&self, ctx: &AppContext) -> usize {
        self.dropdown.as_ref(ctx).items_len()
    }

    #[allow(dead_code)]
    pub fn clear_filter(&mut self, ctx: &mut ViewContext<Self>) {
        self.filter_editor.update(ctx, |editor, ctx| {
            editor.clear_buffer(ctx);
            ctx.notify();
        });
    }

    #[allow(dead_code)]
    pub fn set_menu_header_to_static(&mut self, header: &'static str) {
        self.static_menu_header = Some(header);
    }
}

impl<A> Entity for FilterableDropdown<A>
where
    A: Action + Clone,
{
    type Event = FilterableDropdownEvent;
}

impl<A> TypedActionView for FilterableDropdown<A>
where
    A: Action + Clone,
{
    type Action = DropdownAction<A>;

    fn handle_action(&mut self, action: &DropdownAction<A>, ctx: &mut ViewContext<Self>) {
        match action {
            DropdownAction::Focus(delta) => self.focus(*delta, ctx),
            DropdownAction::Close => self.close(ctx),
            DropdownAction::SelectActionAndClose(action) => {
                self.select_action_and_close(action, ctx)
            }
            DropdownAction::ToggleExpanded => self.toggle_expanded(ctx),
        }
    }
}

impl<A> View for FilterableDropdown<A>
where
    A: Action + Clone,
{
    fn ui_name() -> &'static str {
        "FilterableDropdown"
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        if focus_ctx.is_self_focused() {
            self.focus(0, ctx)
        }
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let dropdown_menu = if !self.has_pinned_footer && self.dropdown_items_len(app) == 0 {
            self.render_empty_menu(appearance)
        } else {
            ChildView::new(&self.dropdown).finish()
        };

        let mut dropdown_stack = Stack::new().with_child(self.render_top_bar(appearance));
        if self.is_expanded {
            dropdown_stack.add_positioned_overlay_child(
                dropdown_menu,
                if self.orientation == FilterableDropdownOrientation::Down {
                    OffsetPositioning::offset_from_save_position_element(
                        self.top_bar_label(),
                        vec2f(0., 0.),
                        PositionedElementOffsetBounds::WindowByPosition,
                        PositionedElementAnchor::BottomLeft,
                        ChildAnchor::TopLeft,
                    )
                } else {
                    OffsetPositioning::offset_from_save_position_element(
                        self.top_bar_label(),
                        vec2f(0., 0.),
                        PositionedElementOffsetBounds::WindowByPosition,
                        PositionedElementAnchor::TopLeft,
                        ChildAnchor::BottomLeft,
                    )
                },
            );
        }
        Container::new(dropdown_stack.finish())
            .with_margin_top(self.vertical_margin)
            .with_margin_bottom(self.vertical_margin)
            .finish()
    }

    fn on_blur(&mut self, blur_ctx: &BlurContext, ctx: &mut ViewContext<Self>) {
        if blur_ctx.is_self_blurred() {
            ctx.emit(FilterableDropdownEvent::Close);
        }
    }
}
