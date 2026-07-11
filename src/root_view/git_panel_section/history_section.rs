// history section — RootView 的 git 提交历史视图：commit row + commit detail card + hover/scroll/resize。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。
// render_git_panel_history_divider / render_git_panel_history_section 被 status_section::render_git_panel_body 调用。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::git_commit_detail_helpers::render_git_commit_detail_card;
use crate::git_panel_view_helpers::{
    git_commit_detail_target, git_commit_hover_target_after_event,
    git_commit_hover_target_after_motion, git_commit_row_position_id,
    git_commit_row_visual_hovered, git_history_scroll_should_load_more,
    render_git_panel_commit_row_content, GIT_COMMIT_DETAIL_CLEAR_DELAY,
    GIT_HISTORY_SCROLLABLE_HEADER_PX,
};
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{RootView, TerminalSessionTab, GIT_HISTORY_DIVIDER_HEIGHT};
use nexshell::git_panel::{clamp_git_history_height, GitRequest};
use warpui::clipboard::ClipboardContent;
use warpui::elements::{
    Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
    CrossAxisAlignment, DispatchEventResult, DragAxis, Draggable, Empty, EventHandler, Expanded,
    Fill, Flex, Hoverable, MainAxisSize, MouseInBehavior, MouseState, ParentElement, SavePosition,
    ScrollbarWidth, Text,
};
use warpui::fonts;
use warpui::{Element, ViewContext};

impl RootView {
    pub(in crate::root_view) fn handle_git_history_resize_start(&mut self, start_y: f32) {
        // git_panel 在 GitDiff tab active 时仍渲染来源 tab；resize anchor 必须绑同一 tab，
        // 否则 resize_move 改的是 GitDiff tab 的字段，视觉无变化。
        if let Some(tab) = self
            .active_git_panel_tab_index()
            .and_then(|idx| self.terminal_tabs.get(idx))
        {
            self.git_history_resize_anchor =
                Some((tab.id.clone(), start_y, tab.git_panel_history_height));
        }
    }

    pub(in crate::root_view) fn handle_git_history_resize_move(
        &mut self,
        current_y: f32,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some((tab_id, anchor_y, anchor_h)) = self.git_history_resize_anchor.clone() {
            let new_h = clamp_git_history_height(anchor_h + (anchor_y - current_y));
            if let Some(tab) = self.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
                if (tab.git_panel_history_height - new_h).abs() > f32::EPSILON {
                    tab.git_panel_history_height = new_h;
                    self.git_history_height = new_h;
                    ctx.notify();
                }
            }
        }
    }

    pub(in crate::root_view) fn handle_git_history_resize_end(&mut self) {
        self.git_history_resize_anchor = None;
        self.save_ui_settings();
    }

    pub(in crate::root_view) fn handle_git_commit_copy_sha(
        &mut self,
        sha: String,
        ctx: &mut ViewContext<Self>,
    ) {
        let sha = sha.trim();
        if !sha.is_empty() {
            ctx.clipboard()
                .write(ClipboardContent::plain_text(sha.to_string()));
        }
    }

    pub(in crate::root_view) fn handle_git_commit_select(
        &mut self,
        tab_id: String,
        sha: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(tab) = self.terminal_tabs.iter_mut().find(|tab| tab.id == tab_id) {
            if tab
                .git_panel_state
                .recent_commits
                .iter()
                .any(|commit| commit.sha == sha)
            {
                tab.git_panel_selected_commit = Some(sha.clone());
                tab.git_panel_hovered_commit = Some(sha);
                tab.git_panel_hover_clear_after = None;
                ctx.notify();
            }
        }
    }

    pub(in crate::root_view) fn handle_git_history_scrolled(
        &mut self,
        tab_id: &str,
        scroll_start: f32,
        delta_y: f32,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(tab) = self.terminal_tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let previous_scroll_start = tab.git_panel_history_last_scroll_start;
        tab.git_panel_history_last_scroll_start = scroll_start;
        let state = &mut tab.git_panel_state;
        if state.history_loading_more || !state.history_has_more || !state.in_repo() {
            return;
        }
        let visible_height =
            (tab.git_panel_history_height - GIT_HISTORY_SCROLLABLE_HEADER_PX).max(1.0);
        if !git_history_scroll_should_load_more(
            previous_scroll_start,
            scroll_start,
            delta_y,
            state.recent_commits.len(),
            visible_height,
        ) {
            return;
        }
        let Some(worker) = tab.git_worker.as_ref() else {
            return;
        };
        if worker.send(GitRequest::LoadMoreHistory {
            offset: state.recent_commits.len(),
        }) {
            state.history_loading_more = true;
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn handle_git_commit_row_hover(
        &mut self,
        tab_id: &str,
        sha: &str,
        hovered: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(tab) = self.terminal_tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let detail_hovered = tab
            .git_panel_commit_detail_states
            .borrow()
            .get(sha)
            .and_then(|state| state.lock().ok().map(|s| s.is_mouse_over_element()))
            .unwrap_or(false);
        let (next, clear_after) = if hovered {
            (
                git_commit_hover_target_after_event(
                    tab.git_panel_hovered_commit.as_deref(),
                    sha,
                    true,
                ),
                None,
            )
        } else if tab.git_panel_hovered_commit.as_deref() == Some(sha) {
            git_commit_hover_target_after_motion(
                tab.git_panel_hovered_commit.as_deref(),
                false,
                detail_hovered,
                tab.git_panel_hover_clear_after,
                Instant::now(),
            )
        } else {
            (
                tab.git_panel_hovered_commit.clone(),
                tab.git_panel_hover_clear_after,
            )
        };
        if tab.git_panel_hovered_commit != next {
            tab.git_panel_hovered_commit = next;
            tab.git_panel_hover_clear_after = clear_after;
            ctx.notify();
        } else if tab.git_panel_hover_clear_after != clear_after {
            tab.git_panel_hover_clear_after = clear_after;
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn handle_git_commit_detail_hover(
        &mut self,
        tab_id: &str,
        sha: &str,
        hovered: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(tab) = self.terminal_tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let row_hovered = tab
            .git_panel_commit_states
            .borrow()
            .get(sha)
            .and_then(|state| state.lock().ok().map(|s| s.is_mouse_over_element()))
            .unwrap_or(false);
        let (next, clear_after) = if hovered {
            (
                git_commit_hover_target_after_event(
                    tab.git_panel_hovered_commit.as_deref(),
                    sha,
                    true,
                ),
                None,
            )
        } else if tab.git_panel_hovered_commit.as_deref() == Some(sha) {
            git_commit_hover_target_after_motion(
                tab.git_panel_hovered_commit.as_deref(),
                row_hovered,
                false,
                tab.git_panel_hover_clear_after,
                Instant::now(),
            )
        } else {
            (
                tab.git_panel_hovered_commit.clone(),
                tab.git_panel_hover_clear_after,
            )
        };
        if tab.git_panel_hovered_commit != next {
            tab.git_panel_hovered_commit = next;
            tab.git_panel_hover_clear_after = clear_after;
            ctx.notify();
        } else if tab.git_panel_hover_clear_after != clear_after {
            tab.git_panel_hover_clear_after = clear_after;
            ctx.notify();
        }
    }

    pub(crate) fn sweep_git_commit_hover(&mut self, ctx: &mut ViewContext<Self>) {
        // 同 handle_git_history_resize_start：GitDiff tab active 时仍要 sweep source tab 的 hover。
        let Some(panel_index) = self.active_git_panel_tab_index() else {
            return;
        };
        let Some(tab) = self.terminal_tabs.get_mut(panel_index) else {
            return;
        };
        let Some(sha) = tab.git_panel_hovered_commit.clone() else {
            return;
        };
        let row_hovered = tab
            .git_panel_commit_states
            .borrow()
            .get(&sha)
            .and_then(|state| state.lock().ok().map(|s| s.is_mouse_over_element()))
            .unwrap_or(false);
        let detail_hovered = tab
            .git_panel_commit_detail_states
            .borrow()
            .get(&sha)
            .and_then(|state| state.lock().ok().map(|s| s.is_mouse_over_element()))
            .unwrap_or(false);
        let (next, clear_after) = git_commit_hover_target_after_motion(
            tab.git_panel_hovered_commit.as_deref(),
            row_hovered,
            detail_hovered,
            tab.git_panel_hover_clear_after,
            Instant::now(),
        );
        if tab.git_panel_hovered_commit != next {
            tab.git_panel_hovered_commit = next;
            tab.git_panel_hover_clear_after = clear_after;
            ctx.notify();
        } else if tab.git_panel_hover_clear_after != clear_after {
            tab.git_panel_hover_clear_after = clear_after;
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn render_git_panel_history_divider(
        &self,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let drag_state = tab.git_panel_history_divider_drag_state.clone();
        let border = colors.panel_border;
        let hover = colors.metric_track;
        Hoverable::new(tab.git_panel_history_divider_state.clone(), move |mouse| {
            let mut handle = Container::new(
                ConstrainedBox::new(Empty::new().finish())
                    .with_height(GIT_HISTORY_DIVIDER_HEIGHT)
                    .finish(),
            )
            .with_border(Border::top(1.0).with_border_color(border));
            if mouse.is_hovered() {
                handle = handle.with_background_color(hover);
            }
            Draggable::new(drag_state.clone(), handle.finish())
                .with_drag_axis(DragAxis::VerticalOnly)
                .with_keep_original_visible(true)
                .on_drag_start(|ctx, _, rect| {
                    ctx.set_cursor(
                        warpui::platform::Cursor::ResizeUpDown,
                        warpui::elements::ZIndex::Overlay(usize::MAX),
                    );
                    ctx.dispatch_typed_action(TerminalGridAction::GitHistoryResizeStart(
                        rect.origin_y(),
                    ));
                })
                .on_drag(|ctx, _, rect, _| {
                    ctx.set_cursor(
                        warpui::platform::Cursor::ResizeUpDown,
                        warpui::elements::ZIndex::Overlay(usize::MAX),
                    );
                    ctx.dispatch_typed_action(TerminalGridAction::GitHistoryResizeMove(
                        rect.origin_y(),
                    ));
                })
                .on_drop(|ctx, _, _rect, _| {
                    ctx.dispatch_typed_action(TerminalGridAction::GitHistoryResizeEnd);
                })
                .finish()
        })
        .with_cursor(warpui::platform::Cursor::ResizeUpDown)
        .finish()
    }

    pub(in crate::root_view) fn render_git_panel_history_section(
        &self,
        tab: &TerminalSessionTab,
        commits: &[nexshell::git_ops::CommitRow],
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        // 淘汰已不在 history 列表里的 commit 行/详情卡/滚动状态（key 均为 commit.sha）。
        {
            let valid: std::collections::HashSet<&str> =
                commits.iter().map(|c| c.sha.as_str()).collect();
            tab.git_panel_commit_states
                .borrow_mut()
                .retain(|k, _| valid.contains(k.as_str()));
            tab.git_panel_commit_detail_states
                .borrow_mut()
                .retain(|k, _| valid.contains(k.as_str()));
            tab.git_panel_commit_copy_states
                .borrow_mut()
                .retain(|k, _| valid.contains(k.as_str()));
            tab.git_panel_commit_detail_files_scroll_states
                .borrow_mut()
                .retain(|k, _| valid.contains(k.as_str()));
            tab.git_panel_commit_detail_body_scroll_states
                .borrow_mut()
                .retain(|k, _| valid.contains(k.as_str()));
        }
        let title = rust_i18n::t!("git_panel_section_history").to_string();
        let header = Text::new_inline(format!("{title} ({})", commits.len()), self.ui_font, 11.0)
            .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
            .with_color(colors.text_muted)
            .finish();

        let mut rows = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        if commits.is_empty() {
            let empty = Text::new_inline(
                rust_i18n::t!("git_panel_no_commits").to_string(),
                self.ui_font,
                11.0,
            )
            .with_color(colors.text_muted)
            .finish();
            rows.add_child(
                Container::new(empty)
                    .with_padding_left(6.0)
                    .with_padding_top(4.0)
                    .finish(),
            );
        } else {
            for commit in commits {
                rows.add_child(self.render_git_panel_commit_row(tab, commit, colors));
            }
            if tab.git_panel_state.history_loading_more {
                rows.add_child(
                    Container::new(
                        Text::new_inline(
                            rust_i18n::t!("git_panel_history_loading_more").to_string(),
                            self.ui_font,
                            10.0,
                        )
                        .with_color(colors.text_muted)
                        .finish(),
                    )
                    .with_padding_left(24.0)
                    .with_padding_top(6.0)
                    .with_padding_bottom(6.0)
                    .finish(),
                );
            }
        }
        let scroll_state = tab.git_panel_history_scroll_state.clone();
        let rows = ClippedScrollable::vertical(
            scroll_state.clone(),
            rows.finish(),
            ScrollbarWidth::Custom(4.0),
            Fill::Solid(colors.text_muted),
            Fill::Solid(colors.text_primary),
            Fill::None,
        )
        .with_overlayed_scrollbar()
        .finish();
        let tab_id_for_scroll = tab.id.clone();
        let rows = EventHandler::new(rows)
            .with_always_handle()
            .on_scroll_wheel(move |ctx, _, delta, _| {
                ctx.dispatch_typed_action(TerminalGridAction::GitHistoryScrolled {
                    tab_id: tab_id_for_scroll.clone(),
                    scroll_start: scroll_state.scroll_start().as_f32(),
                    delta_y: delta.y(),
                });
                DispatchEventResult::PropagateToParent
            })
            .finish();
        // 详情卡浮层已上移到 root Stack（见 render_git_commit_detail_overlay）：
        // 必须与右键菜单同级 waterfall 才能遮挡下层终端，局部 Stack 会被事件穿透。
        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Container::new(header).with_padding_bottom(4.0).finish())
            .with_child(Expanded::new(1.0, rows).finish())
            .finish()
    }

    // 详情卡浮层：构造好返回，由 mod.rs 挂到 root Stack（与右键菜单同级 waterfall），
    // 这样才能遮挡下层终端、避免滚动/拖动穿透。无悬停或选中目标时返回 None。
    pub(in crate::root_view) fn render_git_commit_detail_overlay(
        &self,
    ) -> Option<(Box<dyn Element>, String)> {
        let idx = self.active_git_panel_tab_index()?;
        let tab = self.terminal_tabs.get(idx)?;
        if !tab.git_panel_open {
            return None;
        }
        let active_sha = git_commit_detail_target(
            tab.git_panel_selected_commit.as_deref(),
            tab.git_panel_hovered_commit.as_deref(),
        )?
        .to_string();
        let commit = tab
            .git_panel_state
            .recent_commits
            .iter()
            .find(|commit| commit.sha == active_sha)?
            .clone();
        let detail_state = tab
            .git_panel_commit_detail_states
            .borrow_mut()
            .entry(active_sha.clone())
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone();
        let copy_state = tab
            .git_panel_commit_copy_states
            .borrow_mut()
            .entry(active_sha.clone())
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone();
        let files_scroll_state = tab
            .git_panel_commit_detail_files_scroll_states
            .borrow_mut()
            .entry(active_sha.clone())
            .or_insert_with(ClippedScrollStateHandle::new)
            .clone();
        let body_scroll_state = tab
            .git_panel_commit_detail_body_scroll_states
            .borrow_mut()
            .entry(active_sha.clone())
            .or_insert_with(ClippedScrollStateHandle::new)
            .clone();
        let colors = self.design_tokens.overview;
        let ui_font = self.ui_font;
        let position_id = git_commit_row_position_id(&tab.id, &active_sha);
        let tab_id_for_action = tab.id.clone();
        let sha_for_action = active_sha.clone();
        let detail = Hoverable::new(detail_state, move |_mouse| {
            render_git_commit_detail_card(
                &commit,
                colors,
                ui_font,
                copy_state.clone(),
                files_scroll_state.clone(),
                body_scroll_state.clone(),
            )
        })
        .with_hover_out_delay(Duration::from_millis(250))
        .on_hover(move |hovered, ctx, _, _| {
            if !hovered {
                ctx.notify_after(GIT_COMMIT_DETAIL_CLEAR_DELAY);
            }
            ctx.dispatch_typed_action(TerminalGridAction::GitCommitDetailHover {
                tab_id: tab_id_for_action.clone(),
                sha: sha_for_action.clone(),
                hovered,
            });
        })
        .finish();
        Some((detail, position_id))
    }

    fn render_git_panel_commit_row(
        &self,
        tab: &TerminalSessionTab,
        commit: &nexshell::git_ops::CommitRow,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let state = tab
            .git_panel_commit_states
            .borrow_mut()
            .entry(commit.sha.clone())
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone();
        let commit = commit.clone();
        let colors = *colors;
        let ui_font = self.ui_font;
        let row_position_id = git_commit_row_position_id(&tab.id, &commit.sha);
        let action_tab_id = tab.id.clone();
        let action_sha = commit.sha.clone();
        let click_tab_id = tab.id.clone();
        let click_sha = commit.sha.clone();
        let mouse_in_tab_id = tab.id.clone();
        let mouse_in_sha = commit.sha.clone();
        let selected = tab.git_panel_selected_commit.as_deref() == Some(commit.sha.as_str());

        let row = Hoverable::new(state, move |mouse| {
            let row = render_git_panel_commit_row_content(
                &commit,
                colors,
                ui_font,
                selected
                    || git_commit_row_visual_hovered(
                        mouse.is_hovered(),
                        mouse.is_mouse_over_element(),
                    ),
            );
            SavePosition::new(row, &row_position_id).finish()
        })
        .with_hover_out_delay(Duration::from_millis(400))
        .on_hover(move |hovered, ctx, _, _| {
            if !hovered {
                ctx.notify_after(GIT_COMMIT_DETAIL_CLEAR_DELAY);
            }
            ctx.dispatch_typed_action(TerminalGridAction::GitCommitRowHover {
                tab_id: action_tab_id.clone(),
                sha: action_sha.clone(),
                hovered,
            });
        })
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::GitCommitSelect {
                tab_id: click_tab_id.clone(),
                sha: click_sha.clone(),
            });
        })
        .finish();
        EventHandler::new(row)
            .with_always_handle()
            .on_mouse_in(
                move |ctx, _, _| {
                    ctx.dispatch_typed_action(TerminalGridAction::GitCommitRowHover {
                        tab_id: mouse_in_tab_id.clone(),
                        sha: mouse_in_sha.clone(),
                        hovered: true,
                    });
                    DispatchEventResult::PropagateToParent
                },
                Some(MouseInBehavior {
                    fire_on_synthetic_events: false,
                    fire_when_covered: false,
                }),
            )
            .finish()
    }
}
