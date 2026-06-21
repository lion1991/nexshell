// status section — RootView 的 git 状态视图：staged / unstaged / untracked / unmerged 三栏 + entries。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。
// 同 section 调用：handle_git_panel_select_entry → diff_section::open_git_diff_tab；
// render_git_panel_body → history_section::render_git_panel_history_divider / render_git_panel_history_section。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pathfinder_geometry::vector::vec2f;

use crate::file_panel_view_helpers::{file_panel_message, render_file_panel_icon_button};
use crate::git_panel_view_helpers::{
    git_panel_body_should_show_loading, git_panel_diff_kind_for_entry,
    git_panel_entry_action_state_key, git_panel_entry_state_key,
    git_panel_entry_tooltip_position_id, git_panel_entry_tooltip_text, git_panel_stage_all_paths,
};
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{RootView, TerminalSessionTab, ICON_PATH_ARROW_DOWN, ICON_PATH_PLUS};
use nexshell::git_ops::{GitDiffKind, GitDiffSelection};
use nexshell::git_panel::{
    apply_git_panel_selection, clamp_git_history_height, GitPanelSelectMode, GitRequest,
};
use warpui::color::ColorU;
use warpui::elements::{
    ChildAnchor, Clipped, ClippedScrollable, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, DispatchEventResult, EventHandler, Expanded, Fill, Flex, Hoverable,
    MainAxisSize, MouseState, OffsetPositioning, Padding, ParentElement, PositionedElementAnchor,
    PositionedElementOffsetBounds, Radius, SavePosition, ScrollbarWidth, Stack, Text,
};
use warpui::fonts;
use warpui::{Element, ViewContext};

impl RootView {
    pub(in crate::root_view) fn handle_git_panel_select_entry(
        &mut self,
        path: String,
        kind: GitDiffKind,
        mode: GitPanelSelectMode,
        ctx: &mut ViewContext<Self>,
    ) {
        let selection = GitDiffSelection {
            path: path.clone(),
            kind,
        };
        // 仅单击单选(Replace)打开 diff 标签；cmd/ctrl(Toggle) 与 shift(Range) 多选只更新选择集。
        let load_diff = matches!(mode, GitPanelSelectMode::Replace);
        let (repo_root, source_tab_id, sent) = self
            .active_git_panel_tab_index()
            .and_then(|index| self.terminal_tabs.get_mut(index))
            .map(|tab| {
                apply_git_panel_selection(&mut tab.git_panel_state, selection.clone(), mode);
                let sent = load_diff
                    && tab
                        .git_worker
                        .as_ref()
                        .map(|w| w.send(GitRequest::LoadDiff(selection.clone())))
                        .unwrap_or(false);
                (tab.git_panel_state.repo_root.clone(), tab.id.clone(), sent)
            })
            .unwrap_or((None, String::new(), false));
        if sent {
            if let Some(repo_root) = repo_root {
                self.open_git_diff_tab(repo_root, selection, source_tab_id, ctx);
            }
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn render_git_panel_body(
        &self,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let state = &tab.git_panel_state;
        let has_changes = !state.status.staged.is_empty()
            || !state.status.unstaged.is_empty()
            || !state.status.untracked.is_empty()
            || !state.status.unmerged.is_empty();
        // 不在仓库内：有错误信息(致命，无快照可显示)整页错误，否则提示不在仓库。
        // 操作级错误(push/commit/stage 失败)快照仍在，不短路，下方挂提示条照常渲染。
        if !state.in_repo() {
            if let Some(err) = state.error.as_ref() {
                return file_panel_message(err, self.ui_font, colors.warning);
            }
            return file_panel_message(
                &rust_i18n::t!("git_panel_not_in_repo"),
                self.ui_font,
                colors.text_muted,
            );
        }
        if git_panel_body_should_show_loading(state) {
            return file_panel_message(
                &rust_i18n::t!("git_panel_loading"),
                self.ui_font,
                colors.text_muted,
            );
        }

        let mut changes_column =
            Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        if !state.status.staged.is_empty() {
            changes_column.add_child(self.render_git_panel_section(
                tab,
                &rust_i18n::t!("git_panel_section_staged"),
                &state.status.staged,
                true,
                false,
                false,
                colors,
            ));
        }
        if !state.status.unstaged.is_empty() {
            changes_column.add_child(self.render_git_panel_section(
                tab,
                &rust_i18n::t!("git_panel_section_changes"),
                &state.status.unstaged,
                false,
                true,
                true,
                colors,
            ));
        }
        if !state.status.untracked.is_empty() {
            changes_column.add_child(self.render_git_panel_section(
                tab,
                &rust_i18n::t!("git_panel_section_untracked"),
                &state.status.untracked,
                false,
                false,
                false,
                colors,
            ));
        }
        if !state.status.unmerged.is_empty() {
            changes_column.add_child(self.render_git_panel_section(
                tab,
                &rust_i18n::t!("git_panel_section_unmerged"),
                &state.status.unmerged,
                false,
                false,
                false,
                colors,
            ));
        }

        if !has_changes {
            let clean = Text::new_inline(
                rust_i18n::t!("git_panel_clean").to_string(),
                self.ui_font,
                11.0,
            )
            .with_color(colors.text_muted)
            .finish();
            changes_column.add_child(Container::new(clean).with_padding_top(6.0).finish());
        }

        let changes = ClippedScrollable::vertical(
            tab.git_panel_scroll_state.clone(),
            changes_column.finish(),
            ScrollbarWidth::Custom(4.0),
            Fill::Solid(colors.text_muted),
            Fill::Solid(colors.text_primary),
            Fill::None,
        )
        .with_overlayed_scrollbar()
        .finish();
        let history = ConstrainedBox::new(self.render_git_panel_history_section(
            tab,
            &state.recent_commits,
            colors,
        ))
        .with_height(clamp_git_history_height(tab.git_panel_history_height))
        .finish();
        let mut body = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        if let Some(err) = state.error.as_ref() {
            body.add_child(self.render_git_panel_error_banner(err, colors));
        }
        body.add_child(Expanded::new(1.0, changes).finish());
        body.add_child(self.render_git_panel_history_divider(tab, colors));
        body.add_child(history);
        body.finish()
    }

    /// 操作级错误(push/commit/stage 失败)的非阻断提示条；文件区/历史区仍正常显示。
    /// 用 Text::new（soft_wrap）让长错误（如 push 的 fatal: unable to access ...）按面板宽换行显示完整。
    fn render_git_panel_error_banner(
        &self,
        message: &str,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let bg = ColorU::new(colors.warning.r, colors.warning.g, colors.warning.b, 0x22);
        let inner = Container::new(
            Text::new(message.to_string(), self.ui_font, 11.0)
                .with_line_height_ratio(1.3)
                .with_color(colors.warning)
                .finish(),
        )
        .with_horizontal_padding(8.0)
        .with_vertical_padding(6.0)
        .finish();
        let banner = Container::new(inner)
            .with_background_color(bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .finish();
        Container::new(banner).with_padding_bottom(8.0).finish()
    }

    /// 单个分组（staged / unstaged / untracked / unmerged）。
    fn render_git_panel_section(
        &self,
        tab: &TerminalSessionTab,
        title: &str,
        entries: &[nexshell::git_ops::GitFileEntry],
        is_staged: bool,
        show_stage_all: bool,
        discard_enabled: bool,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let header = Text::new_inline(format!("{title} ({})", entries.len()), self.ui_font, 11.0)
            .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
            .with_color(colors.text_muted)
            .finish();
        let mut header_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(1.0, header).finish());
        if show_stage_all {
            let paths = git_panel_stage_all_paths(entries);
            let label = rust_i18n::t!("git_panel_stage_all").to_string();
            let ui_font = self.ui_font;
            let text_color = colors.cpu_accent;
            let hover_bg = self.ui_colors().tab_bg_hover;
            let button = Hoverable::new(tab.git_panel_stage_all_state.clone(), move |mouse| {
                let label = Text::new_inline(label.clone(), ui_font, 10.0)
                    .with_color(text_color)
                    .finish();
                let mut container = Container::new(
                    Container::new(label)
                        .with_horizontal_padding(6.0)
                        .with_vertical_padding(2.0)
                        .finish(),
                )
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.0)));
                if mouse.is_hovered() {
                    container = container.with_background_color(hover_bg);
                }
                container.finish()
            })
            .with_cursor(warpui::platform::Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::GitPanelStageAll(paths.clone()));
            })
            .finish();
            header_row = header_row.with_child(button);
        }

        let mut col = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                Container::new(header_row.finish())
                    .with_padding_bottom(4.0)
                    .finish(),
            );
        for entry in entries {
            col.add_child(self.render_git_panel_entry(
                tab,
                entry,
                is_staged,
                discard_enabled,
                colors,
            ));
        }
        Container::new(col.finish())
            .with_padding_top(6.0)
            .with_padding_bottom(6.0)
            .finish()
    }

    fn render_git_panel_entry(
        &self,
        tab: &TerminalSessionTab,
        entry: &nexshell::git_ops::GitFileEntry,
        is_staged: bool,
        discard_enabled: bool,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let xy = format!("{}{}", entry.index_status, entry.worktree_status);
        let label_text = git_panel_entry_tooltip_text(entry, &xy);
        let diff_kind = git_panel_diff_kind_for_entry(entry, is_staged);
        let diff_selection = GitDiffSelection {
            path: entry.path.clone(),
            kind: diff_kind,
        };
        // 高亮单一来源：只认 selected_entries，不 OR 焦点 selected_diff，避免右键/取消选中后残留高亮（对齐 Warp block 选择）
        let is_selected = tab
            .git_panel_state
            .selected_entries
            .contains(&diff_selection);

        let path_for_action = entry.path.clone();
        let action = if is_staged {
            TerminalGridAction::GitPanelUnstage(path_for_action)
        } else {
            TerminalGridAction::GitPanelStage(path_for_action)
        };

        let state = tab
            .git_panel_entry_states
            .borrow_mut()
            .entry(git_panel_entry_state_key(&entry.path, is_staged))
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone();
        let action_state = tab
            .git_panel_entry_action_states
            .borrow_mut()
            .entry(git_panel_entry_action_state_key(&entry.path, is_staged))
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone();

        let ui_font = self.ui_font;
        let text_color = colors.text_primary;
        let hover_bg = self.ui_colors().tab_bg_hover;
        let selected_bg = colors.card_bg;
        let tooltip_bg = self.ui_colors().tooltip_bg;
        let tooltip_text_color = self.ui_colors().tooltip_text;
        let label_clone = label_text.clone();
        let tooltip_label = label_text.clone();
        let tooltip_position_id = git_panel_entry_tooltip_position_id(&entry.path, is_staged);
        let select_path = entry.path.clone();

        let label = Hoverable::new(state, move |mouse| {
            let text = SavePosition::new(
                Clipped::new(
                    Text::new_inline(label_clone.clone(), ui_font, 11.0)
                        .with_color(text_color)
                        .finish(),
                )
                .finish(),
                &tooltip_position_id,
            )
            .finish();
            let mut container = Container::new(
                Container::new(text)
                    .with_padding_left(6.0)
                    .with_padding_right(6.0)
                    .with_padding_top(3.0)
                    .with_padding_bottom(3.0)
                    .finish(),
            )
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.0)));
            if is_selected {
                container = container.with_background_color(selected_bg);
            } else if mouse.is_mouse_over_element() {
                container = container.with_background_color(hover_bg);
            }
            let base = container.finish();
            if !mouse.is_hovered() {
                return base;
            }

            let tooltip = Container::new(
                ConstrainedBox::new(
                    Text::new(tooltip_label.clone(), ui_font, 12.0)
                        .with_line_height_ratio(1.25)
                        .with_color(tooltip_text_color)
                        .finish(),
                )
                .with_max_width(520.0)
                .finish(),
            )
            .with_background_color(tooltip_bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .with_padding(Padding::uniform(8.0))
            .finish();

            let mut stack = Stack::new().with_child(base);
            stack.add_positioned_overlay_child(
                tooltip,
                OffsetPositioning::offset_from_save_position_element(
                    tooltip_position_id.clone(),
                    vec2f(0.0, 6.0),
                    PositionedElementOffsetBounds::WindowByPosition,
                    PositionedElementAnchor::BottomLeft,
                    ChildAnchor::TopLeft,
                ),
            );
            stack.finish()
        })
        .with_hover_in_delay(Duration::from_millis(500))
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_click_with_modifiers(move |ctx, _, _, modifiers| {
            let mode = if modifiers.shift {
                GitPanelSelectMode::Range
            } else if modifiers.cmd || modifiers.ctrl {
                GitPanelSelectMode::Toggle
            } else {
                GitPanelSelectMode::Replace
            };
            ctx.dispatch_typed_action(TerminalGridAction::GitPanelSelectEntry {
                path: select_path.clone(),
                kind: diff_kind,
                mode,
            });
        })
        .finish();

        let uc = self.ui_colors();
        let action_icon = if is_staged {
            ICON_PATH_ARROW_DOWN
        } else {
            ICON_PATH_PLUS
        };
        let action_button = render_file_panel_icon_button(
            action_state,
            action_icon,
            uc.icon_color_inactive,
            uc.icon_button_hover_bg,
            action,
        );

        let tab_id_for_ctx = tab.id.clone();
        let path_for_ctx = entry.path.clone();
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(1.0, label).finish())
            .with_child(
                Container::new(action_button)
                    .with_padding_left(4.0)
                    .finish(),
            )
            .finish();

        EventHandler::new(row)
            .on_right_mouse_down(move |ctx, _app, position| {
                ctx.dispatch_typed_action(TerminalGridAction::GitPanelShowContextMenu {
                    tab_id: tab_id_for_ctx.clone(),
                    path: path_for_ctx.clone(),
                    kind: diff_kind,
                    discard_enabled,
                    position,
                });
                DispatchEventResult::StopPropagation
            })
            .finish()
    }
}
