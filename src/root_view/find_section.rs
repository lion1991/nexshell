// find_section — RootView 的终端查找栏：查找编辑器、查找 action、查找栏 render。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。
// create_find_editor 由 RootView::new() 调用；handle_* / close_find_bar 由 mod.rs handle_action
// 分发；render_find_bar 由 mod.rs impl View::render 调用——均用 pub(in crate::root_view)。
// 仅本文件内调用的 handle_find_editor_event 保持私有。find_match_label 已于 step 10 归 terminal_view_helpers。

use nexshell::text_editor::{
    EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions,
};
use warp_core::ui::theme::color::internal_colors::{neutral_2, neutral_3, neutral_4};
use warpui::color::ColorU;
use warpui::elements::{
    Align, Border, Clipped, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Fill,
    Flex, Hoverable, Icon, ParentElement, Radius, Shrinkable, Text,
};
use warpui::{Element, ViewContext};

use crate::terminal_grid_element::TerminalGridAction;
use crate::terminal_view_helpers::find_match_label;
use crate::{RootView, ICON_PATH_ARROW_DOWN, ICON_PATH_ARROW_UP, ICON_PATH_CLOSE};

impl RootView {
    // warp: view_components/find.rs:153-191
    pub(crate) fn create_find_editor(
        ctx: &mut ViewContext<Self>,
    ) -> warpui::ViewHandle<EditorView> {
        let editor = ctx.add_typed_action_view(|ctx| {
            let options = SingleLineEditorOptions {
                text: TextOptions {
                    font_size_override: Some(13.0),
                    ..Default::default()
                },
                select_all_on_focus: true,
                clear_selections_on_blur: true,
                ..Default::default()
            };
            let mut editor = EditorView::single_line(options, ctx);
            editor.set_placeholder_text("regex…", ctx);
            editor
        });
        ctx.subscribe_to_view(&editor, |me, _, event: &EditorEvent, ctx| {
            me.handle_find_editor_event(event, ctx);
        });
        editor
    }

    // warp: view_components/find.rs:212-238
    fn handle_find_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        let find_active = self.find_state.lock().map(|s| s.active).unwrap_or(false);
        if !find_active {
            return;
        }
        match event {
            EditorEvent::Edited(_) => {
                let query = self.find_editor.as_ref(ctx).buffer_text(ctx);
                if let Ok(mut state) = self.find_state.lock() {
                    state.query = query.clone();
                }
                if let Ok(rt) = self.terminal.lock() {
                    rt.set_find_query((!query.is_empty()).then(|| query));
                }
                ctx.notify();
            }
            EditorEvent::Enter => {
                if let Ok(rt) = self.terminal.lock() {
                    rt.step_find(1);
                }
                ctx.notify();
            }
            EditorEvent::ShiftEnter => {
                if let Ok(rt) = self.terminal.lock() {
                    rt.step_find(-1);
                }
                ctx.notify();
            }
            EditorEvent::Escape => {
                self.close_find_bar(ctx);
            }
            _ => {}
        }
    }

    // warp: view.rs:18177-18210 (show_find_bar)
    pub(in crate::root_view) fn handle_open_find_bar(&mut self, ctx: &mut ViewContext<Self>) {
        let query = self.find_state.lock().ok().map(|mut state| {
            state.active = true;
            state.query.clone()
        });
        let query = query.unwrap_or_default();
        self.find_editor.update(ctx, |editor, ctx| {
            editor.system_reset_buffer_text(&query, ctx);
        });
        if !query.is_empty() {
            if let Ok(rt) = self.terminal.lock() {
                rt.set_find_query(Some(query));
            }
        }
        ctx.focus(&self.find_editor);
        ctx.notify();
    }

    pub(crate) fn close_find_bar(&mut self, ctx: &mut ViewContext<Self>) {
        if let Ok(mut state) = self.find_state.lock() {
            state.active = false;
        }
        if let Ok(rt) = self.terminal.lock() {
            rt.set_find_query(None);
        }
        ctx.focus_self();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_find_step(
        &mut self,
        step: i32,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Ok(rt) = self.terminal.lock() {
            rt.step_find(step);
        }
        ctx.notify();
    }

    // 查找栏 overlay：active 时返回 top-right 浮层，否则 None。
    pub(in crate::root_view) fn render_find_bar(&self) -> Option<Box<dyn Element>> {
        let find_active = self.find_state.lock().map(|s| s.active).unwrap_or(false);
        if !find_active {
            return None;
        }

        // warp: view_components/find.rs:571-694
        // 13px 字行高 ~18，盒高贴齐行高避免编辑器内顶对齐的底部 slack。
        let editor_height = 18.0;
        let icon_size = 14.0;
        let find_bar_padding = 6.0;
        let editor_padding = 6.0;
        let icon_padding = 4.0;
        let icon_spacing = 4.0;
        let border_radius = 8.0;
        let find_bar_width = 380.0;
        let find_theme = &self.cached_warp_theme;
        let label_color = find_theme.nonactive_ui_text_color().into_solid();
        let hover_bg = neutral_3(&find_theme);
        let bar_n2 = neutral_2(&find_theme);
        let bar_bg = ColorU::new(bar_n2.r, bar_n2.g, bar_n2.b, 0xf5);
        let border_color = neutral_4(&find_theme);
        let find_icon_active = find_theme.active_ui_text_color().into_solid();
        let find_snapshot = self.terminal.lock().ok().map(|rt| rt.snapshot_for_render());
        let match_count = find_snapshot.as_ref().map_or(0, |s| s.find_match_count);

        let query_editor = Shrinkable::new(
            1.,
            ConstrainedBox::new(
                Clipped::new(warpui::elements::ChildView::new(&self.find_editor).finish()).finish(),
            )
            .with_height(editor_height)
            .finish(),
        )
        .finish();

        // 去框化输入区：搜索图标做左锚，输入直接融在玻璃条上。
        let editor_area = Shrinkable::new(
            1.,
            Container::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Container::new(
                            ConstrainedBox::new(
                                Icon::new(crate::ICON_PATH_SEARCH, label_color).finish(),
                            )
                            .with_width(icon_size)
                            .with_height(icon_size)
                            .finish(),
                        )
                        .with_margin_right(8.0)
                        .finish(),
                    )
                    .with_child(query_editor)
                    .finish(),
            )
            .with_padding_left(6.0)
            .with_padding_right(4.0)
            .with_padding_top(editor_padding)
            .with_padding_bottom(editor_padding)
            .with_margin_right(2.0 * icon_spacing)
            .finish(),
        )
        .finish();

        let find_current = find_snapshot.as_ref().and_then(|s| s.find_current_match);
        let match_label = find_match_label(match_count, find_current);
        // 不定高：交给 find_row 的 cross-axis 垂直居中，定高盒会把文字顶到上缘。
        let label_child = Container::new(
            Text::new_inline(match_label, self.monospace_font, 12.0)
                .with_color(label_color)
                .finish(),
        )
        .with_margin_right(icon_spacing)
        .finish();

        // warp: find.rs:618-631
        let down_btn = Container::new(
            Hoverable::new(self.find_btn_next.clone(), move |state| {
                let bg = if state.is_hovered() && match_count > 0 {
                    Fill::Solid(hover_bg)
                } else {
                    Fill::Solid(ColorU::transparent_black())
                };
                Container::new(
                    ConstrainedBox::new(
                        Icon::new(
                            ICON_PATH_ARROW_DOWN,
                            if match_count == 0 {
                                label_color
                            } else {
                                find_icon_active
                            },
                        )
                        .finish(),
                    )
                    .with_height(icon_size)
                    .with_width(icon_size)
                    .finish(),
                )
                .with_background(bg)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(icon_padding)))
                .with_uniform_padding(icon_padding)
                .finish()
            })
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::FindStep(1));
            })
            .finish(),
        )
        .with_margin_left(icon_spacing)
        .finish();

        // warp: find.rs:638-651
        let up_btn = Container::new(
            Hoverable::new(self.find_btn_prev.clone(), move |state| {
                let bg = if state.is_hovered() && match_count > 0 {
                    Fill::Solid(hover_bg)
                } else {
                    Fill::Solid(ColorU::transparent_black())
                };
                Container::new(
                    ConstrainedBox::new(
                        Icon::new(
                            ICON_PATH_ARROW_UP,
                            if match_count == 0 {
                                label_color
                            } else {
                                find_icon_active
                            },
                        )
                        .finish(),
                    )
                    .with_height(icon_size)
                    .with_width(icon_size)
                    .finish(),
                )
                .with_background(bg)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(icon_padding)))
                .with_uniform_padding(icon_padding)
                .finish()
            })
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::FindStep(-1));
            })
            .finish(),
        )
        .with_margin_right(icon_spacing)
        .finish();

        // warp: find.rs:658-665
        let close_btn = Container::new(
            Hoverable::new(self.find_btn_close.clone(), move |state| {
                let bg = if state.is_hovered() {
                    Fill::Solid(hover_bg)
                } else {
                    Fill::Solid(ColorU::transparent_black())
                };
                Container::new(
                    ConstrainedBox::new(Icon::new(ICON_PATH_CLOSE, find_icon_active).finish())
                        .with_height(icon_size)
                        .with_width(icon_size)
                        .finish(),
                )
                .with_background(bg)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(icon_padding)))
                .with_uniform_padding(icon_padding)
                .finish()
            })
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::CloseFindBar);
            })
            .finish(),
        )
        .finish();

        // 计数与导航按钮之间的发丝分隔，收整右侧簇（两侧等距留白）。
        let cluster_divider = Container::new(
            ConstrainedBox::new(
                Container::new(warpui::elements::Empty::new().finish())
                    .with_background_color(border_color)
                    .finish(),
            )
            .with_width(1.0)
            .with_height(14.0)
            .finish(),
        )
        .with_margin_left(6.0)
        .with_margin_right(6.0)
        .finish();

        let find_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(editor_area)
            .with_child(label_child)
            .with_child(cluster_divider)
            .with_child(down_btn)
            .with_child(up_btn)
            .with_child(close_btn)
            .finish();

        // warp: find.rs:669-684；玻璃背景：实色由 GlassBackdrop 模糊+tint 提供。
        // 内层定高只含编辑器行本体，bar padding 由外层 uniform_padding 提供，别双算。
        let find_bar = Container::new(
            ConstrainedBox::new(Container::new(find_row).finish())
                .with_height(editor_height + (2.0 * editor_padding))
                .with_width(find_bar_width)
                .finish(),
        )
        .with_uniform_padding(find_bar_padding)
        .with_border(Border::all(1.0).with_border_color(border_color))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(border_radius)));
        let find_bar = nexshell::design_tokens::Elevation::popover()
            .apply_container(find_bar)
            .finish();
        // 与终端同层挂载，需自开层保证模糊采样到终端文字。
        let find_bar =
            nexshell::glass_backdrop::GlassBackdrop::new(find_bar, border_radius, bar_bg)
                .with_glass(nexshell::design_tokens::Glass::popover())
                .with_own_layer()
                .finish();

        Some(
            Align::new(
                Container::new(find_bar)
                    .with_padding_top(10.0)
                    .with_padding_right(20.0)
                    .finish(),
            )
            .top_right()
            .finish(),
        )
    }
}
