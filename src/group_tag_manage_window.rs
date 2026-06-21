use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use warp::appearance::Appearance;
use warp::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions};
use warpui::{
    elements::{
        Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
        CornerRadius, CrossAxisAlignment, Expanded, Fill, Flex, Hoverable, Icon, MainAxisAlignment,
        MainAxisSize, MouseState, MouseStateHandle, ParentElement, Radius, ScrollbarWidth, Text,
    },
    fonts, Element, Entity, FocusContext, ModelHandle, SingletonEntity, View, ViewContext,
    ViewHandle,
};

use crate::host_management_view::constants::*;
use nexshell::host_management::{
    create_group_in_db, create_tag_in_db, delete_group_from_db, delete_tag_from_all_hosts_in_db,
};
use warpui::ui_components::components::UiComponent;
use warpui::{CursorInfo, ModelAsRef, ReadModel, UpdateModel};

// ── Model ──

#[derive(Clone, Debug)]
pub enum GroupTagManageEvent {
    Closed { changed: bool },
}

pub struct GroupTagManageModel {
    pub groups: Vec<(String, String)>,
    pub tags: Vec<String>,
    pub db_path: PathBuf,
    pub changed: bool,
}

impl Entity for GroupTagManageModel {
    type Event = GroupTagManageEvent;
}

// ── Action ──

#[derive(Clone, Debug)]
pub enum GroupTagManageAction {
    AddGroup,
    DeleteGroup(String),
    AddTag,
    DeleteTag(String),
    Close,
}

// ── View ──

struct ManageStates {
    add_group_state: MouseStateHandle,
    add_tag_state: MouseStateHandle,
    done_state: MouseStateHandle,
    close_state: MouseStateHandle,
    group_delete_states: Vec<MouseStateHandle>,
    tag_delete_states: Vec<MouseStateHandle>,
    scroll_state: ClippedScrollStateHandle,
}

impl ManageStates {
    fn new() -> Self {
        let ms = || Arc::new(Mutex::new(MouseState::default()));
        Self {
            add_group_state: ms(),
            add_tag_state: ms(),
            done_state: ms(),
            close_state: ms(),
            group_delete_states: Vec::new(),
            tag_delete_states: Vec::new(),
            scroll_state: Default::default(),
        }
    }

    fn ensure_group_delete_count(&mut self, count: usize) {
        while self.group_delete_states.len() < count {
            self.group_delete_states
                .push(Arc::new(Mutex::new(MouseState::default())));
        }
    }

    fn ensure_tag_delete_count(&mut self, count: usize) {
        while self.tag_delete_states.len() < count {
            self.tag_delete_states
                .push(Arc::new(Mutex::new(MouseState::default())));
        }
    }
}

pub struct GroupTagManageView {
    model: ModelHandle<GroupTagManageModel>,
    ui_font: fonts::FamilyId,
    states: std::cell::RefCell<ManageStates>,
    group_input_editor: ViewHandle<EditorView>,
    tag_input_editor: ViewHandle<EditorView>,
}

impl Entity for GroupTagManageView {
    type Event = ();
}

impl GroupTagManageView {
    pub fn new(model: ModelHandle<GroupTagManageModel>, ctx: &mut ViewContext<Self>) -> Self {
        let ui_font = fonts::Cache::handle(ctx).update(ctx, |cache, _| {
            cache
                .load_system_font("Helvetica Neue")
                .or_else(|_| cache.load_system_font("Helvetica"))
                .or_else(|_| cache.load_system_font("Arial"))
                .expect("ui font")
        });

        let group_input = Self::create_editor(&rust_i18n::t!("manage_group_placeholder"), ctx);
        let tag_input = Self::create_editor(&rust_i18n::t!("manage_tag_placeholder"), ctx);

        ctx.subscribe_to_view(&group_input, |me, _, event: &EditorEvent, ctx| {
            if matches!(event, EditorEvent::Enter) {
                me.add_group(ctx);
            }
        });
        ctx.subscribe_to_view(&tag_input, |me, _, event: &EditorEvent, ctx| {
            if matches!(event, EditorEvent::Enter) {
                me.add_tag(ctx);
            }
        });

        Self {
            model,
            ui_font,
            states: std::cell::RefCell::new(ManageStates::new()),
            group_input_editor: group_input,
            tag_input_editor: tag_input,
        }
    }

    fn create_editor(placeholder: &str, ctx: &mut ViewContext<Self>) -> ViewHandle<EditorView> {
        let placeholder = placeholder.to_string();
        ctx.add_typed_action_view(move |ctx| {
            let font_size = Appearance::as_ref(ctx).ui_font_size();
            let mut editor = EditorView::single_line(
                SingleLineEditorOptions {
                    text: TextOptions {
                        font_size_override: Some(font_size),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ctx,
            );
            editor.set_placeholder_text(&placeholder, ctx);
            editor
        })
    }

    fn add_group(&self, ctx: &mut ViewContext<Self>) {
        let name = self.group_input_editor.as_ref(ctx).buffer_text(ctx);
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let db_path = ctx.read_model(&self.model, |m, _| m.db_path.clone());
        if let Ok(id) = create_group_in_db(&db_path, &name) {
            ctx.update_model(&self.model, |m, ctx| {
                m.groups.push((id, name));
                m.changed = true;
                ctx.notify();
            });
            self.group_input_editor.update(ctx, |editor, ctx| {
                editor.system_reset_buffer_text("", ctx);
            });
            ctx.notify();
        }
    }

    fn add_tag(&self, ctx: &mut ViewContext<Self>) {
        let name = self.tag_input_editor.as_ref(ctx).buffer_text(ctx);
        let name = name.trim().to_string();
        if name.is_empty() || ctx.read_model(&self.model, |m, _| m.tags.contains(&name)) {
            return;
        }
        let db_path = ctx.read_model(&self.model, |m, _| m.db_path.clone());
        if create_tag_in_db(&db_path, &name).is_err() {
            return;
        }
        ctx.update_model(&self.model, |m, ctx| {
            m.tags.push(name);
            m.changed = true;
            ctx.notify();
        });
        self.tag_input_editor.update(ctx, |editor, ctx| {
            editor.system_reset_buffer_text("", ctx);
        });
        ctx.notify();
    }

    fn close(&self, ctx: &mut ViewContext<Self>) {
        ctx.update_model(&self.model, |m, ctx| {
            ctx.emit(GroupTagManageEvent::Closed { changed: m.changed });
        });
    }
}

impl View for GroupTagManageView {
    fn ui_name() -> &'static str {
        "GroupTagManageView"
    }

    fn render(&self, ctx: &warpui::AppContext) -> Box<dyn Element> {
        let model = ctx.model(&self.model);
        let ui_font = self.ui_font;
        let appearance = Appearance::as_ref(ctx);
        let hc = HostUiColors::from_theme(appearance.theme());
        let mut states = self.states.borrow_mut();
        states.ensure_group_delete_count(model.groups.len());
        states.ensure_tag_delete_count(model.tags.len());

        let mut root = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        root.add_child(render_header(&states.close_state, ui_font, &hc));

        // scrollable content
        let mut content = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        // groups section
        content.add_child(render_section_title(
            &rust_i18n::t!("manage_groups"),
            ui_font,
            &hc,
        ));
        let can_delete_group = model.groups.len() > 1;
        for (i, (id, name)) in model.groups.iter().enumerate() {
            let delete_action = GroupTagManageAction::DeleteGroup(id.clone());
            content.add_child(render_item_row(
                name,
                if can_delete_group {
                    Some(delete_action)
                } else {
                    None
                },
                &states.group_delete_states[i],
                ui_font,
                &hc,
            ));
        }
        content.add_child(render_add_row(
            &self.group_input_editor,
            &states.add_group_state,
            &rust_i18n::t!("manage_add_group"),
            GroupTagManageAction::AddGroup,
            appearance,
            ui_font,
            &hc,
        ));

        // tags section
        content.add_child(render_section_title(
            &rust_i18n::t!("manage_tags"),
            ui_font,
            &hc,
        ));
        for (i, tag) in model.tags.iter().enumerate() {
            let delete_action = GroupTagManageAction::DeleteTag(tag.clone());
            content.add_child(render_item_row(
                tag,
                Some(delete_action),
                &states.tag_delete_states[i],
                ui_font,
                &hc,
            ));
        }
        content.add_child(render_add_row(
            &self.tag_input_editor,
            &states.add_tag_state,
            &rust_i18n::t!("manage_add_tag"),
            GroupTagManageAction::AddTag,
            appearance,
            ui_font,
            &hc,
        ));

        let scrollable = ClippedScrollable::vertical(
            states.scroll_state.clone(),
            content.finish(),
            ScrollbarWidth::Auto,
            Fill::None,
            Fill::None,
            Fill::None,
        )
        .finish();
        root.add_child(Expanded::new(1.0, scrollable).finish());
        root.add_child(render_footer(&states.done_state, ui_font, &hc));

        Container::new(root.finish())
            .with_background_color(hc.panel_bg)
            .finish()
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        if focus_ctx.is_self_focused() {
            ctx.focus(&self.group_input_editor);
            ctx.notify();
        }
    }

    fn active_cursor_position(&self, ctx: &ViewContext<Self>) -> Option<CursorInfo> {
        let focused = ctx.focused_view_id(ctx.window_id())?;
        let editor = if focused == self.group_input_editor.id() {
            &self.group_input_editor
        } else {
            &self.tag_input_editor
        };
        let cursor_id = warp::editor::position_id_for_cursor(editor.id());
        let font_size = Appearance::as_ref(ctx).ui_font_size();
        ctx.element_position_by_id(cursor_id)
            .map(|position| CursorInfo {
                position,
                font_size,
            })
    }
}

impl warpui::TypedActionView for GroupTagManageView {
    type Action = GroupTagManageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            GroupTagManageAction::AddGroup => self.add_group(ctx),
            GroupTagManageAction::DeleteGroup(id) => {
                let db_path = ctx.read_model(&self.model, |m, _| m.db_path.clone());
                if delete_group_from_db(&db_path, id).is_ok() {
                    let id = id.clone();
                    ctx.update_model(&self.model, |m, ctx| {
                        m.groups.retain(|(gid, _)| *gid != id);
                        m.changed = true;
                        ctx.notify();
                    });
                    ctx.notify();
                }
            }
            GroupTagManageAction::AddTag => self.add_tag(ctx),
            GroupTagManageAction::DeleteTag(tag) => {
                let db_path = ctx.read_model(&self.model, |m, _| m.db_path.clone());
                if delete_tag_from_all_hosts_in_db(&db_path, tag).is_ok() {
                    let tag = tag.clone();
                    ctx.update_model(&self.model, |m, ctx| {
                        m.tags.retain(|t| *t != tag);
                        m.changed = true;
                        ctx.notify();
                    });
                    ctx.notify();
                }
            }
            GroupTagManageAction::Close => self.close(ctx),
        }
    }
}

// ── Render helpers ──

fn render_header(
    close_state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = close_state.clone();

    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(rust_i18n::t!("manage_title").to_string(), ui_font, 15.0)
                    .with_color(hc.text_primary)
                    .finish(),
            )
            .with_child(Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish())
            .with_child(
                Hoverable::new(state, move |mouse| {
                    let color = if mouse.is_hovered() {
                        hc.text_primary
                    } else {
                        hc.text_secondary
                    };
                    ConstrainedBox::new(Icon::new(ICON_X_CIRCLE, color).finish())
                        .with_width(ICON_SIZE_MD)
                        .with_height(ICON_SIZE_MD)
                        .finish()
                })
                .with_cursor(warpui::platform::Cursor::PointingHand)
                .with_cursor(warpui::platform::Cursor::PointingHand)
                .with_cursor(warpui::platform::Cursor::PointingHand)
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(GroupTagManageAction::Close);
                })
                .finish(),
            )
            .finish(),
    )
    .with_horizontal_padding(20.0)
    .with_vertical_padding(14.0)
    .with_background_color(hc.toolbar_bg)
    .with_border(Border::bottom(1.0).with_border_color(hc.toolbar_border))
    .finish()
}

fn render_section_title(
    title: &str,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Container::new(
        Text::new_inline(title.to_string(), ui_font, 13.0)
            .with_color(hc.text_secondary)
            .finish(),
    )
    .with_padding_left(20.0)
    .with_padding_top(16.0)
    .with_padding_bottom(8.0)
    .finish()
}

fn render_item_row(
    name: &str,
    delete_action: Option<GroupTagManageAction>,
    delete_state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = delete_state.clone();
    let name = name.to_string();

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center);

    row.add_child(
        Expanded::new(
            1.0,
            Text::new_inline(name, ui_font, 13.0)
                .with_color(hc.text_primary)
                .finish(),
        )
        .finish(),
    );

    if let Some(action) = delete_action {
        row.add_child(
            Hoverable::new(state, move |mouse| {
                let color = if mouse.is_hovered() {
                    hc.text_primary
                } else {
                    hc.text_secondary
                };
                Container::new(
                    ConstrainedBox::new(Icon::new(ICON_TRASH, color).finish())
                        .with_width(14.0)
                        .with_height(14.0)
                        .finish(),
                )
                .with_horizontal_padding(4.0)
                .finish()
            })
            .with_cursor(warpui::platform::Cursor::PointingHand)
            .with_cursor(warpui::platform::Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(action.clone());
            })
            .finish(),
        );
    }

    Container::new(row.finish())
        .with_horizontal_padding(20.0)
        .with_vertical_padding(6.0)
        .with_border(Border::bottom(1.0).with_border_color(hc.card_border))
        .finish()
}

fn render_add_row(
    editor: &ViewHandle<EditorView>,
    add_state: &MouseStateHandle,
    label: &str,
    action: GroupTagManageAction,
    appearance: &Appearance,
    _ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc_copy = *hc;
    let state = add_state.clone();
    let label = label.to_string();

    let input = appearance
        .ui_builder()
        .text_input(editor.clone())
        .with_style(warpui::ui_components::components::UiComponentStyles {
            background: Some(hc.search_bar_bg.into()),
            border_width: Some(0.0),
            font_color: Some(hc.text_primary),
            height: Some(30.0),
            padding: Some(warpui::ui_components::components::Coords {
                top: 6.0,
                bottom: 6.0,
                left: 0.0,
                right: 0.0,
            }),
            ..Default::default()
        })
        .build()
        .finish();

    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Expanded::new(
                    1.0,
                    Container::new(warpui::elements::Stack::new().with_child(input).finish())
                        .with_horizontal_padding(8.0)
                        .with_background_color(hc.search_bar_bg)
                        .with_border(Border::all(1.0).with_border_color(hc.search_bar_border))
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                        .finish(),
                )
                .finish(),
            )
            .with_child(
                Container::new(
                    Hoverable::new(state, move |mouse| {
                        let bg = if mouse.is_hovered() {
                            hc_copy.card_bg_hover
                        } else {
                            hc_copy.search_bar_bg
                        };
                        Container::new(
                            Flex::row()
                                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                .with_child(
                                    Container::new(
                                        ConstrainedBox::new(
                                            Icon::new(ICON_PLUS, hc_copy.text_accent).finish(),
                                        )
                                        .with_width(14.0)
                                        .with_height(14.0)
                                        .finish(),
                                    )
                                    .with_margin_right(4.0)
                                    .finish(),
                                )
                                .with_child(
                                    Text::new_inline(label.clone(), _ui_font, UI_FONT_SIZE)
                                        .with_color(hc_copy.text_accent)
                                        .finish(),
                                )
                                .finish(),
                        )
                        .with_horizontal_padding(10.0)
                        .with_vertical_padding(6.0)
                        .with_background_color(bg)
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                        .with_border(Border::all(1.0).with_border_color(hc_copy.search_bar_border))
                        .finish()
                    })
                    .with_cursor(warpui::platform::Cursor::PointingHand)
                    .with_cursor(warpui::platform::Cursor::PointingHand)
                    .with_cursor(warpui::platform::Cursor::PointingHand)
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(action.clone());
                    })
                    .finish(),
                )
                .with_margin_left(8.0)
                .finish(),
            )
            .finish(),
    )
    .with_horizontal_padding(20.0)
    .with_vertical_padding(10.0)
    .finish()
}

fn render_footer(
    done_state: &MouseStateHandle,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let state = done_state.clone();

    let done_btn = Hoverable::new(state, move |mouse| {
        let bg = if mouse.is_hovered() {
            warpui::color::ColorU::new(
                (hc.accent_bg.r as u16 + 20).min(255) as u8,
                (hc.accent_bg.g as u16 + 20).min(255) as u8,
                (hc.accent_bg.b as u16 + 20).min(255) as u8,
                255,
            )
        } else {
            hc.accent_bg
        };
        Container::new(
            Text::new_inline(rust_i18n::t!("manage_done").to_string(), ui_font, 13.0)
                .with_color(hc.accent_text)
                .finish(),
        )
        .with_horizontal_padding(24.0)
        .with_vertical_padding(8.0)
        .with_background_color(bg)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.0)))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(GroupTagManageAction::Close);
    })
    .finish();

    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(done_btn)
            .finish(),
    )
    .with_horizontal_padding(20.0)
    .with_vertical_padding(14.0)
    .with_background_color(hc.toolbar_bg)
    .with_border(Border::top(1.0).with_border_color(hc.toolbar_border))
    .finish()
}
