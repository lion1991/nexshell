// footer section — RootView 的 commit editor + commit/push 按钮 + 弃改流程。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。
// queue_git_push_for_tab 由 mod.rs::show_git_ssh_host_key_prompt 调用；
// queue_git_discard_worktree_change_for_tab 由 mod.rs::confirm_git_discard_worktree_change 调用。

use crate::git_panel_view_helpers::{animated_push_busy_label, git_panel_footer_kind, GitPanelFooterKind};
use crate::host_management_view::constants::HostUiColors;
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{
    RootView, TerminalSessionTab, GIT_COMMIT_EDITOR_MAX_HEIGHT, GIT_COMMIT_EDITOR_MIN_HEIGHT,
    ICON_PATH_REFRESH, ICON_PATH_UPLOAD,
};
use nexshell::git_panel::GitRequest;
use warp::editor::Event as EditorEvent;
use warpui::elements::{
    Align, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Empty, Fill, Flex,
    Hoverable, Icon, ParentElement, Radius, Text,
};
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::ui_components::text_input::TextInput;
use warpui::{Element, ViewContext};

impl RootView {
    pub(in crate::root_view) fn handle_git_commit_editor_focus(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(editor) = self
            .active_git_panel_tab_index()
            .and_then(|index| self.terminal_tabs.get(index))
            .map(|tab| tab.git_commit_editor.clone())
        {
            ctx.focus(&editor);
        }
    }

    pub(in crate::root_view) fn handle_git_commit_editor_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            EditorEvent::Enter => self.run_git_commit(ctx),
            EditorEvent::Escape => self.discard_git_commit_message(ctx),
            _ => {}
        }
    }

    pub(crate) fn run_git_commit(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(panel_index) = self.active_git_panel_tab_index() else {
            return;
        };
        let Some(tab) = self.terminal_tabs.get(panel_index) else {
            return;
        };
        if tab.git_commit_busy {
            return;
        }
        if !tab.git_panel_state.in_repo() || tab.git_panel_state.status.staged.is_empty() {
            return;
        }
        let message = tab.git_commit_editor.as_ref(ctx).buffer_text(ctx);
        let trimmed = message.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        let Some(worker) = tab.git_worker.as_ref() else {
            return;
        };
        let queued = worker.send(GitRequest::Commit {
            message: trimmed,
            amend: false,
        });
        if !queued {
            return;
        }
        let tab_mut = &mut self.terminal_tabs[panel_index];
        tab_mut.git_commit_busy = true;
        ctx.notify();
    }

    pub(super) fn queue_git_push_for_tab(
        &mut self,
        tab_id: &str,
        accept_new_ssh_host: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(index) = self.terminal_tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        self.queue_git_push_for_index(index, accept_new_ssh_host, ctx);
    }

    fn queue_git_push_for_index(
        &mut self,
        index: usize,
        accept_new_ssh_host: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(tab) = self.terminal_tabs.get(index) else {
            return;
        };
        if !matches!(
            git_panel_footer_kind(&tab.git_panel_state, tab.git_commit_busy, tab.git_push_busy),
            GitPanelFooterKind::Push {
                enabled: true,
                synchronized: false
            }
        ) {
            return;
        }
        let Some(worker) = tab.git_worker.as_ref() else {
            return;
        };
        if !worker.send(GitRequest::Push {
            accept_new_ssh_host,
        }) {
            return;
        }
        let tab_mut = &mut self.terminal_tabs[index];
        tab_mut.git_push_busy = true;
        self.git_push_animation_tick = 0;
        ctx.notify();
    }

    pub(crate) fn run_git_push(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(panel_index) = self.active_git_panel_tab_index() else {
            return;
        };
        self.queue_git_push_for_index(panel_index, false, ctx);
    }

    pub(super) fn queue_git_discard_worktree_change_for_tab(
        &mut self,
        tab_id: &str,
        path: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if path.is_empty() {
            return;
        }
        let Some(tab) = self.terminal_tabs.iter().find(|tab| tab.id == tab_id) else {
            return;
        };
        let Some(worker) = tab.git_worker.as_ref() else {
            return;
        };
        if worker.send(GitRequest::DiscardWorktreeChanges(vec![path])) {
            ctx.notify();
        }
    }

    pub(super) fn queue_git_delete_untracked_for_tab(
        &mut self,
        tab_id: &str,
        path: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if path.is_empty() {
            return;
        }
        let Some(tab) = self.terminal_tabs.iter().find(|tab| tab.id == tab_id) else {
            return;
        };
        let Some(worker) = tab.git_worker.as_ref() else {
            return;
        };
        if worker.send(GitRequest::DeleteUntracked(vec![path])) {
            ctx.notify();
        }
    }

    pub(crate) fn discard_git_commit_message(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(tab) = self
            .active_git_panel_tab_index()
            .and_then(|idx| self.terminal_tabs.get(idx))
        else {
            return;
        };
        let editor = tab.git_commit_editor.clone();
        editor.update(ctx, |editor, ctx| {
            editor.clear_buffer_and_reset_undo_stack(ctx);
        });
        ctx.notify();
    }

    pub(in crate::root_view) fn render_git_panel_footer(
        &self,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let state = &tab.git_panel_state;
        let footer_kind = git_panel_footer_kind(state, tab.git_commit_busy, tab.git_push_busy);
        if footer_kind == GitPanelFooterKind::None {
            return Empty::new().finish();
        }

        let hc = HostUiColors::from_theme(&self.cached_warp_theme);
        let ui_font = self.ui_font;

        let bg_enabled = hc.accent_bg;
        let bg_disabled = hc.card_bg_hover;
        let fg_enabled = hc.accent_text;
        let fg_disabled = colors.text_muted;

        if let GitPanelFooterKind::Push {
            enabled,
            synchronized,
        } = footer_kind
        {
            let upstream = state
                .status
                .upstream
                .as_deref()
                .unwrap_or(rust_i18n::t!("git_panel_push_no_upstream").as_ref())
                .to_string();
            let count = state.status.ahead;
            let status_text = if synchronized {
                rust_i18n::t!("git_panel_push_synced", upstream = upstream.as_str()).to_string()
            } else if state.status.upstream.is_none() {
                rust_i18n::t!("git_panel_push_no_upstream").to_string()
            } else if count == 1 {
                rust_i18n::t!("git_panel_push_ready_one", upstream = upstream.as_str()).to_string()
            } else {
                rust_i18n::t!(
                    "git_panel_push_ready_many",
                    count = count,
                    upstream = upstream.as_str()
                )
                .to_string()
            };
            let label_text = if tab.git_push_busy {
                animated_push_busy_label(
                    &rust_i18n::t!("git_panel_push_busy").to_string(),
                    self.git_push_animation_tick / 3,
                )
            } else {
                rust_i18n::t!("git_panel_push_button").to_string()
            };
            let push_busy = tab.git_push_busy;
            let message = Text::new_inline(status_text, ui_font, 11.0)
                .with_color(colors.text_muted)
                .finish();
            let button: Box<dyn Element> = if enabled {
                Hoverable::new(tab.git_panel_push_state.clone(), move |_mouse| {
                    let icon =
                        ConstrainedBox::new(Icon::new(ICON_PATH_UPLOAD, fg_enabled).finish())
                            .with_width(13.0)
                            .with_height(13.0)
                            .finish();
                    let label = Text::new_inline(label_text.clone(), ui_font, 12.0)
                        .with_color(fg_enabled)
                        .finish();
                    let content = Flex::row()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(icon)
                        .with_child(Container::new(label).with_padding_left(6.0).finish())
                        .finish();
                    Container::new(
                        Container::new(Align::new(content).finish())
                            .with_horizontal_padding(12.0)
                            .with_vertical_padding(6.0)
                            .finish(),
                    )
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                    .with_background_color(bg_enabled)
                    .finish()
                })
                .with_cursor(warpui::platform::Cursor::PointingHand)
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(TerminalGridAction::GitPushConfirm);
                })
                .finish()
            } else {
                let content = if push_busy {
                    let icon =
                        ConstrainedBox::new(Icon::new(ICON_PATH_REFRESH, fg_disabled).finish())
                            .with_width(13.0)
                            .with_height(13.0)
                            .finish();
                    let label = Text::new_inline(label_text, ui_font, 12.0)
                        .with_color(fg_disabled)
                        .finish();
                    Flex::row()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(icon)
                        .with_child(Container::new(label).with_padding_left(6.0).finish())
                        .finish()
                } else {
                    Text::new_inline(label_text, ui_font, 12.0)
                        .with_color(fg_disabled)
                        .finish()
                };
                Container::new(
                    Container::new(Align::new(content).finish())
                        .with_horizontal_padding(12.0)
                        .with_vertical_padding(6.0)
                        .finish(),
                )
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                .with_background_color(bg_disabled)
                .finish()
            };
            return Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(Container::new(message).with_padding_top(8.0).finish())
                .with_child(Container::new(button).with_padding_top(6.0).finish())
                .finish();
        }

        let editor_for_render = tab.git_commit_editor.clone();
        let panel_border = colors.panel_border;
        let editor_box = Hoverable::new(tab.git_commit_editor_shell_state.clone(), move |_| {
            TextInput::new(
                editor_for_render.clone(),
                UiComponentStyles::default()
                    .set_background(Fill::None)
                    .set_border_color(Fill::Solid(panel_border))
                    .set_border_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                    .set_border_width(1.0)
                    .set_padding(Coords::default().left(8.0).right(8.0).top(8.0).bottom(8.0)),
            )
            .build()
            .with_min_height(GIT_COMMIT_EDITOR_MIN_HEIGHT)
            .with_max_height(GIT_COMMIT_EDITOR_MAX_HEIGHT)
            .finish()
        })
        .with_cursor(warpui::platform::Cursor::IBeam)
        .with_defer_events_to_children()
        .on_mouse_down(move |ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::GitCommitEditorFocus);
        })
        .finish();
        let editor_box = crate::input_cursor::reassert_ibeam_cursor_on_mouse_move(editor_box);

        let enabled = matches!(footer_kind, GitPanelFooterKind::Commit { enabled: true });
        let label_text = if tab.git_commit_busy {
            rust_i18n::t!("git_panel_commit_busy").to_string()
        } else {
            rust_i18n::t!("git_panel_commit_button").to_string()
        };

        let button: Box<dyn Element> = if enabled {
            Hoverable::new(tab.git_panel_commit_state.clone(), move |mouse| {
                let bg = if mouse.is_hovered() {
                    bg_enabled
                } else {
                    // 未 hover 也用蓝色（VS Code 风格），hover 时略亮可以靠主题
                    bg_enabled
                };
                Container::new(
                    Container::new(
                        Align::new(
                            Text::new_inline(label_text.clone(), ui_font, 12.0)
                                .with_color(fg_enabled)
                                .finish(),
                        )
                        .finish(),
                    )
                    .with_horizontal_padding(12.0)
                    .with_vertical_padding(6.0)
                    .finish(),
                )
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                .with_background_color(bg)
                .finish()
            })
            .with_cursor(warpui::platform::Cursor::PointingHand)
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::GitCommitConfirm);
            })
            .finish()
        } else {
            Container::new(
                Container::new(
                    Align::new(
                        Text::new_inline(label_text, ui_font, 12.0)
                            .with_color(fg_disabled)
                            .finish(),
                    )
                    .finish(),
                )
                .with_horizontal_padding(12.0)
                .with_vertical_padding(6.0)
                .finish(),
            )
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .with_background_color(bg_disabled)
            .finish()
        };

        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Container::new(editor_box).with_padding_top(8.0).finish())
            .with_child(Container::new(button).with_padding_top(6.0).finish())
            .finish()
    }
}
