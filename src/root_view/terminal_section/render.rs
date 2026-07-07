// terminal_section::render — 终端 body render 链 + 光标投影 + 键盘/overlay 输入判定。
// 本文件只含 impl RootView，无自由函数；render 入口由 mod.rs 的 View::render 派发，用 pub(in crate::root_view)，仅本文件内用的保持私有。

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use warp_core::ui::theme::color::internal_colors::neutral_3;
use warpui::color::ColorU;
use warpui::elements::{
    Align, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, DispatchEventResult, Empty,
    EventHandler, Expanded, Flex, Icon, MainAxisSize, ParentElement, Radius, SavePosition,
    Shrinkable, Stack, Text,
};
use warpui::{fonts, AppContext, CursorInfo, Element, EventContext};

use crate::terminal_grid_element::{
    CellMetrics, FindPanelState, TerminalGridAction, TerminalGridElement, TerminalShapedLineCache,
};
use crate::terminal_view_helpers::{
    split_pane_header_background_color, split_pane_header_badge_icon,
    split_pane_header_badge_title, terminal_disconnected_notice_text,
    terminal_keyboard_input_enabled, terminal_palette_ansi_color,
    terminal_tab_kind_uses_side_panel_layout,
};
use crate::{RootView, TerminalSessionKind, SPLIT_PANE_HEADER_HEIGHT, UI_FONT_SIZE};
use nexshell::pane_state::NexPaneId;
use nexshell::terminal_runtime::{
    terminal_snapshot_with_input_editor, LocalTerminalRuntime, TerminalInputEditor, TerminalPalette,
};

impl RootView {
    pub(in crate::root_view) fn render_active_tab_body_with_side_panels(
        &self,
        main_body: Box<dyn Element>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let Some(active_kind) = self
            .terminal_tabs
            .get(self.active_tab_index)
            .map(|tab| tab.kind)
        else {
            return main_body;
        };
        if !terminal_tab_kind_uses_side_panel_layout(active_kind) {
            return main_body;
        }

        let show_left = self.should_render_host_overview_sidebar();
        let show_file_panel = self.should_render_file_panel();
        let show_git_panel = self.should_render_git_panel();
        if show_left || show_file_panel || show_git_panel {
            let mut row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            if show_left {
                row.add_child(self.render_sidebar_panel(app));
            }
            row.add_child(Expanded::new(1.0, main_body).finish());
            if show_file_panel {
                row.add_child(self.render_file_panel(app));
            }
            if show_git_panel {
                row.add_child(self.render_git_panel(app));
            }
            row.finish()
        } else {
            main_body
        }
    }

    pub(in crate::root_view) fn render_single_terminal_body(
        &self,
        terminal: &Arc<Mutex<LocalTerminalRuntime>>,
        kind: TerminalSessionKind,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let runtime_snapshot = match terminal.lock() {
            Ok(rt) => rt.snapshot_for_render(),
            Err(_) => {
                return Align::new(
                    Text::new_inline(
                        "terminal runtime mutex poisoned",
                        self.monospace_font,
                        self.terminal_font_size,
                    )
                    .finish(),
                )
                .finish();
            }
        };
        let palette = TerminalPalette::from_theme(&self.cached_warp_theme);
        // 离线（远程/串口断开）不再整屏换占位：保留 grid 内容，底部加横幅提示。
        let disconnect_notice = terminal_disconnected_notice_text(
            kind,
            runtime_snapshot.connected,
            &runtime_snapshot.status,
        );
        let disconnect_red = terminal_palette_ansi_color(&palette, 1);
        if runtime_snapshot.connected && !runtime_snapshot.bootstrapped {
            return self.render_terminal_bootstrap_placeholder(
                runtime_snapshot.shell_display_name.as_deref(),
                &palette,
            );
        }
        let terminal_keyboard_enabled = self.terminal_keyboard_input_enabled(app);
        let snapshot = if terminal_keyboard_enabled {
            self.project_input_editor_snapshot(runtime_snapshot)
        } else {
            runtime_snapshot
        };
        let snapshot = {
            let mut s = (*snapshot).clone();
            s.grid.cursor_shape = self.cursor_style.to_terminal_shape();
            Arc::new(s)
        };
        let cell_metrics = CellMetrics::from_font_cache(
            app.font_cache(),
            self.monospace_font,
            self.terminal_font_size,
            self.line_height_ratio,
        );
        let grid = Align::new(
            TerminalGridElement::new(
                Arc::clone(&snapshot),
                cell_metrics,
                self.monospace_font,
                self.terminal_font_size,
                Arc::clone(terminal),
                Arc::clone(&self.input_editor),
                Arc::clone(&self.selection_drag),
                Arc::clone(&self.last_resize_cells),
                Arc::clone(&self.scrollbar_drag),
                Arc::clone(&self.cursor_over_terminal),
                Arc::clone(&self.scrollbar_thumb_hovered),
                Arc::clone(&self.find_state),
                Arc::clone(&self.smooth_scroll_px),
                Arc::clone(&self.cursor_smear),
                Arc::clone(&self.shaped_line_cache),
                Arc::clone(&self.terminal_ime_layout),
                terminal
                    .lock()
                    .map(|rt| rt.shell_is_foreground_handle())
                    .unwrap_or_else(|_| Arc::new(AtomicBool::new(true))),
                None,
                terminal_keyboard_enabled,
                palette,
            )
            .finish(),
        )
        .top_left()
        .finish();
        match disconnect_notice {
            Some(notice) => self.wrap_with_disconnect_banner(grid, &notice, disconnect_red),
            None => grid,
        }
    }

    // 离线横幅：保留终端内容，底部钉一条细横幅显示断开原因（远程/串口共用）。
    fn wrap_with_disconnect_banner(
        &self,
        body: Box<dyn Element>,
        notice: &str,
        red: ColorU,
    ) -> Box<dyn Element> {
        let text = notice
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("  ·  ");
        let banner = Container::new(
            Text::new_inline(text, self.monospace_font, 12.0)
                .with_color(red)
                .finish(),
        )
        .with_padding_top(3.0)
        .with_padding_bottom(3.0)
        .with_padding_left(16.0)
        .with_padding_right(8.0)
        .with_background_color(split_pane_header_background_color(&self.cached_warp_theme))
        .finish();
        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        column.add_child(Shrinkable::new(1.0, body).finish());
        column.add_child(banner);
        column.finish()
    }

    /// Bootstrap 占位（参考 Warp `bootstrapping_shell_text`）：shell integration
    /// marker 到达前，用紫色粗体 "Starting {shell}..." 替代空 grid，避免光标跳跃。
    fn render_terminal_bootstrap_placeholder(
        &self,
        shell_display: Option<&str>,
        palette: &TerminalPalette,
    ) -> Box<dyn Element> {
        let shell_name = shell_display.unwrap_or("shell");
        let magenta = terminal_palette_ansi_color(palette, 5);
        let text = Text::new_inline(
            format!("Starting {}...", shell_name),
            self.monospace_font,
            self.terminal_font_size,
        )
        .with_color(magenta)
        .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
        .finish();
        Align::new(
            Container::new(text)
                .with_padding_left(16.0)
                .with_padding_top(4.0)
                .finish(),
        )
        .top_left()
        .finish()
    }

    pub(in crate::root_view) fn render_split_terminal_body(
        &self,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let tab = match self.terminal_tabs.get(self.active_tab_index) {
            Some(t) => t,
            None => return Empty::new().finish(),
        };
        let focused_id = tab.focused_pane_id;
        let cell_metrics = CellMetrics::from_font_cache(
            app.font_cache(),
            self.monospace_font,
            self.terminal_font_size,
            self.line_height_ratio,
        );

        let theme = &self.cached_warp_theme;
        let divider_color = theme.outline().into_solid();
        let pane_header_bg = split_pane_header_background_color(theme);
        let badge_n3 = neutral_3(&theme);
        let badge_bg = ColorU::new(badge_n3.r, badge_n3.g, badge_n3.b, 0xe0);
        let inactive_badge_bg = ColorU::new(badge_n3.r, badge_n3.g, badge_n3.b, 0x80);
        let badge_text_color = theme.active_ui_text_color().into_solid();
        let inactive_badge_text_color = theme.nonactive_ui_text_color().into_solid();
        let badge_icon_color = theme.nonactive_ui_text_color().into_solid();
        let ui_font = self.ui_font;
        let badge_font_size = UI_FONT_SIZE - 1.0;
        let terminal_keyboard_enabled = self.terminal_keyboard_input_enabled(app);

        let render_pane = |pane_id: NexPaneId| -> Box<dyn Element> {
            let terminal = match tab.pane_terminals.get(&pane_id) {
                Some(t) => Arc::clone(t),
                None => return Empty::new().finish(),
            };
            let is_focused = pane_id == focused_id && terminal_keyboard_enabled;

            let runtime_snapshot = match terminal.lock() {
                Ok(rt) => rt.snapshot_for_render(),
                Err(_) => return Empty::new().finish(),
            };

            let palette = TerminalPalette::from_theme(&self.cached_warp_theme);
            let disconnect_notice = terminal_disconnected_notice_text(
                tab.kind,
                runtime_snapshot.connected,
                &runtime_snapshot.status,
            );
            let disconnect_red = terminal_palette_ansi_color(&palette, 1);
            if runtime_snapshot.connected && !runtime_snapshot.bootstrapped {
                return self.render_terminal_bootstrap_placeholder(
                    runtime_snapshot.shell_display_name.as_deref(),
                    &palette,
                );
            }

            let snapshot = if is_focused {
                self.project_input_editor_snapshot(runtime_snapshot)
            } else {
                runtime_snapshot
            };
            let snapshot = {
                let mut s = (*snapshot).clone();
                s.grid.cursor_shape = self.cursor_style.to_terminal_shape();
                Arc::new(s)
            };

            let (sel_drag, sb_drag, cursor_over, sb_hover, find_st, scroll_px, smear, shaped, ime) =
                if is_focused {
                    (
                        Arc::clone(&self.selection_drag),
                        Arc::clone(&self.scrollbar_drag),
                        Arc::clone(&self.cursor_over_terminal),
                        Arc::clone(&self.scrollbar_thumb_hovered),
                        Arc::clone(&self.find_state),
                        Arc::clone(&self.smooth_scroll_px),
                        Arc::clone(&self.cursor_smear),
                        Arc::clone(&self.shaped_line_cache),
                        Arc::clone(&self.terminal_ime_layout),
                    )
                } else {
                    (
                        Arc::new(Mutex::new(false)),
                        Arc::new(Mutex::new(None)),
                        Arc::new(Mutex::new(false)),
                        Arc::new(Mutex::new(false)),
                        Arc::new(Mutex::new(FindPanelState::default())),
                        Arc::new(Mutex::new(0.0)),
                        Arc::new(Mutex::new(crate::cursor_smear::CursorSmear::new())),
                        Arc::new(Mutex::new(TerminalShapedLineCache::default())),
                        Arc::new(Mutex::new(None)),
                    )
                };

            let fg_handle = terminal
                .lock()
                .map(|rt| rt.shell_is_foreground_handle())
                .unwrap_or_else(|_| Arc::new(AtomicBool::new(true)));

            let grid_element = TerminalGridElement::new(
                Arc::clone(&snapshot),
                cell_metrics,
                self.monospace_font,
                self.terminal_font_size,
                Arc::clone(&terminal),
                if is_focused {
                    Arc::clone(&self.input_editor)
                } else {
                    Arc::new(Mutex::new(TerminalInputEditor::default()))
                },
                sel_drag,
                if is_focused {
                    Arc::clone(&self.last_resize_cells)
                } else {
                    Arc::new(Mutex::new((0, 0)))
                },
                sb_drag,
                cursor_over,
                sb_hover,
                find_st,
                scroll_px,
                smear,
                shaped,
                ime,
                fg_handle,
                Some(pane_id),
                is_focused,
                palette,
            )
            .finish();

            let pos_id = pane_id.position_id();
            let pane_element = Align::new(grid_element).top_left().finish();
            let pane_element = match &disconnect_notice {
                Some(notice) => {
                    self.wrap_with_disconnect_banner(pane_element, notice, disconnect_red)
                }
                None => pane_element,
            };

            let title = terminal.lock().ok().and_then(|rt| rt.title());
            let fallback_label = tab.custom_label.as_deref().unwrap_or(&tab.fallback_label);
            let display_path =
                split_pane_header_badge_title(title.as_deref(), fallback_label, tab.kind);

            let icon = ConstrainedBox::new(
                Icon::new(split_pane_header_badge_icon(tab.kind), badge_icon_color).finish(),
            )
            .with_width(12.0)
            .with_height(12.0)
            .finish();

            let label = Text::new_inline(display_path, ui_font, badge_font_size)
                .with_color(if is_focused {
                    badge_text_color
                } else {
                    inactive_badge_text_color
                })
                .finish();

            let badge_row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(icon)
                .with_child(Container::new(label).with_padding_left(4.0).finish())
                .finish();

            let badge = Container::new(badge_row)
                .with_padding_top(3.0)
                .with_padding_bottom(3.0)
                .with_padding_left(6.0)
                .with_padding_right(8.0)
                .with_background_color(if is_focused {
                    badge_bg
                } else {
                    inactive_badge_bg
                })
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(5.0)))
                .finish();

            let header = ConstrainedBox::new(
                Container::new(Align::new(badge).left().finish())
                    .with_padding_left(6.0)
                    .with_padding_top(4.0)
                    .with_background_color(pane_header_bg)
                    .finish(),
            )
            .with_height(SPLIT_PANE_HEADER_HEIGHT)
            .finish();

            let mut pane_column = Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            pane_column.add_child(header);
            pane_column.add_child(Shrinkable::new(1.0, pane_element).finish());
            SavePosition::new(pane_column.finish(), &pos_id).finish()
        };

        let mut body_stack = Stack::new();
        body_stack.add_child(tab.pane_tree.render(
            &render_pane,
            divider_color,
            |pane_id, ctx: &mut EventContext<'_>| {
                ctx.dispatch_typed_action(TerminalGridAction::FocusPane(pane_id));
            },
            |border, ctx: &mut EventContext<'_>| {
                ctx.dispatch_typed_action(TerminalGridAction::StartPaneResizing(border));
            },
        ));

        let body = EventHandler::new(body_stack.finish())
            .on_mouse_dragged(|ctx, _, position| {
                ctx.dispatch_typed_action(TerminalGridAction::PaneResizeMove(position));
                DispatchEventResult::StopPropagation
            })
            .on_left_mouse_up(|ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::EndPaneResizing);
                DispatchEventResult::PropagateToParent
            })
            .finish();

        body
    }

    fn project_input_editor_snapshot(
        &self,
        snapshot: Arc<nexshell::terminal_runtime::TerminalRuntimeSnapshot>,
    ) -> Arc<nexshell::terminal_runtime::TerminalRuntimeSnapshot> {
        let Ok(editor) = self.input_editor.lock() else {
            return snapshot;
        };
        if editor.is_empty() {
            return snapshot;
        }

        let projected_grid = terminal_snapshot_with_input_editor(&snapshot.grid, &editor);
        if projected_grid == snapshot.grid {
            return snapshot;
        }

        let mut projected = (*snapshot).clone();
        projected.lines = projected_grid
            .lines
            .iter()
            .map(|line| line.text.clone())
            .collect();
        projected.grid = projected_grid;
        Arc::new(projected)
    }

    pub(crate) fn live_terminal_cursor_position(&self) -> Option<CursorInfo> {
        let layout = self
            .terminal_ime_layout
            .lock()
            .ok()
            .and_then(|layout| *layout)?;
        let runtime_snapshot = self.terminal.lock().ok()?.snapshot_for_render();
        let snapshot = self.project_input_editor_snapshot(runtime_snapshot);
        let smooth_scroll_px = self
            .smooth_scroll_px
            .lock()
            .map(|value| *value as f32)
            .unwrap_or(0.0);
        layout
            .cursor_rect_for_snapshot(&snapshot.grid, smooth_scroll_px)
            .map(|position| CursorInfo {
                position,
                font_size: layout.font_size(),
            })
    }

    fn terminal_keyboard_input_enabled(&self, app: &AppContext) -> bool {
        terminal_keyboard_input_enabled(
            self.file_panel_input_active(),
            self.overlay_editor_focused(app),
        )
    }

    fn overlay_editor_focused(&self, app: &AppContext) -> bool {
        self.find_editor.is_focused(app)
            || self.tab_rename_editor.is_focused(app)
            || self
                .host_password_editor
                .as_ref()
                .is_some_and(|editor| editor.is_focused(app))
            || self
                .terminal_tabs
                .get(self.active_tab_index)
                .is_some_and(|tab| tab.git_commit_editor.is_focused(app))
    }
}
