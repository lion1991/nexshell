// git_panel section — RootView 的 git_panel 相关方法集合。
//
// 约定（ADR 0001 「面板模块边界约束」）：
// - 本文件只写 `impl super::super::RootView { ... }`，无独立 struct / 顶层 fn。
// - 禁止新增无 `&self` 的自由函数；纯函数 helper 放到 git_panel_view_helpers.rs。
// - 被 mod.rs handle_action / View::render 调用的方法用 `pub(in crate::root_view)`
//   （section 已嵌套成目录，pub(super) 够不到 mod.rs）；section 内同级调用用 pub(super)，内部辅助保持 private。
//
// Warp 参考：app/src/code_review/git_dialog/ ── 按业务垂直切片拆。
//   mod.rs（共享 chrome + dispatch）/ commit.rs / push.rs / pr.rs。
//   本目录类比：mod.rs（入口 + 共享 helper）/ status_section / diff_section /
//   history_section / footer。

mod diff_section;
mod footer;
mod history_section;
mod status_section;

use super::super::{
    RootView, TerminalSessionKind, TerminalSessionTab, GIT_PANEL_DIVIDER_WIDTH,
    GIT_PANEL_WIDTH_MAX, GIT_PANEL_WIDTH_MIN, ICON_PATH_REFRESH,
};
use crate::file_panel_view_helpers::{file_panel_message, render_file_panel_icon_button};
use crate::git_panel_view_helpers::{git_ssh_host_key_prompt_info, git_ssh_host_key_prompt_title};
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use nexshell::git_ops::{GitDiffSelection, SshHostKeyPrompt};
use nexshell::git_panel::{apply_git_event, clear_stale_diff_selection, GitEvent, GitRequest};
use warpui::elements::{
    Border, ConstrainedBox, Container, CrossAxisAlignment, DragAxis, Draggable, Empty, Expanded,
    Flex, Hoverable, MainAxisSize, ParentElement, Text,
};
use warpui::fonts;
use warpui::modals::{AlertDialogWithCallbacks, ModalButton};
use warpui::{AppContext, Element, ViewContext};

impl RootView {
    // ===== A 类：从 handle_action match arm body 抽出的 handler =====

    pub(in crate::root_view) fn handle_toggle_git_panel(&mut self, ctx: &mut ViewContext<Self>) {
        // 防御：远程/数据 tab 不允许切换 git 面板（按钮已隐藏，这里只是兜底）
        if !self.active_tab_supports_git_panel() {
            return;
        }
        let Some(idx) = self.active_git_panel_tab_index() else {
            return;
        };
        let Some(tab) = self.terminal_tabs.get_mut(idx) else {
            return;
        };
        tab.git_panel_open = !tab.git_panel_open;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_git_panel_refresh(&mut self) {
        if let Some(tab) = self
            .active_git_panel_tab_index()
            .and_then(|index| self.terminal_tabs.get(index))
        {
            if let Some(w) = tab.git_worker.as_ref() {
                w.send(GitRequest::Refresh);
            }
        }
    }

    pub(in crate::root_view) fn handle_git_panel_stage(&mut self, path: String) {
        if let Some(tab) = self
            .active_git_panel_tab_index()
            .and_then(|index| self.terminal_tabs.get(index))
        {
            if let Some(w) = tab.git_worker.as_ref() {
                w.send(GitRequest::Stage(vec![path]));
            }
        }
    }

    pub(in crate::root_view) fn handle_git_panel_stage_all(&mut self, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        if let Some(tab) = self
            .active_git_panel_tab_index()
            .and_then(|index| self.terminal_tabs.get(index))
        {
            if let Some(w) = tab.git_worker.as_ref() {
                w.send(GitRequest::Stage(paths));
            }
        }
    }

    pub(in crate::root_view) fn handle_git_panel_unstage(&mut self, path: String) {
        if let Some(tab) = self
            .active_git_panel_tab_index()
            .and_then(|index| self.terminal_tabs.get(index))
        {
            if let Some(w) = tab.git_worker.as_ref() {
                w.send(GitRequest::Unstage(vec![path]));
            }
        }
    }

    pub(in crate::root_view) fn handle_git_panel_resize_start(&mut self, start_x: f32) {
        self.git_panel_resize_anchor = Some((start_x, self.git_panel_width));
    }

    pub(in crate::root_view) fn handle_git_panel_resize_move(
        &mut self,
        current_x: f32,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some((anchor_x, anchor_w)) = self.git_panel_resize_anchor {
            // 面板贴右边：mouse 往左移 → 宽度变大（同 file_panel 模式）
            let new_w =
                (anchor_w + (anchor_x - current_x)).clamp(GIT_PANEL_WIDTH_MIN, GIT_PANEL_WIDTH_MAX);
            if (self.git_panel_width - new_w).abs() > f32::EPSILON {
                self.git_panel_width = new_w;
                ctx.notify();
            }
        }
    }

    pub(in crate::root_view) fn handle_git_panel_resize_end(&mut self) {
        self.git_panel_resize_anchor = None;
    }

    // ===== C 类：从 main.rs 第 3 块 impl RootView 整体搬入的 git_panel fn =====

    pub(crate) fn apply_git_event_to_diff_tabs(&mut self, owner: &str, event: GitEvent) -> bool {
        let source_repo = self
            .terminal_tabs
            .iter()
            .find(|tab| tab.id == owner)
            .and_then(|tab| tab.git_panel_state.repo_root.clone());
        let Some(source_repo) = source_repo else {
            return false;
        };

        let matches_diff_tab = |tab: &TerminalSessionTab, selection: &GitDiffSelection| {
            matches!(tab.kind, TerminalSessionKind::GitDiff)
                && tab.host_id.as_deref() == Some(owner)
                && tab.git_panel_state.repo_root.as_ref() == Some(&source_repo)
                && tab.git_panel_state.selected_diff.as_ref() == Some(selection)
        };

        let mut changed = false;
        match &event {
            GitEvent::DiffLoading { selection }
            | GitEvent::DiffFailed { selection, .. }
            | GitEvent::DiffLoaded { selection, .. } => {
                for tab in self
                    .terminal_tabs
                    .iter_mut()
                    .filter(|tab| matches_diff_tab(tab, selection))
                {
                    apply_git_event(&mut tab.git_panel_state, event.clone());
                    changed = true;
                }
            }
            GitEvent::Snapshot { status, .. } => {
                for tab in self.terminal_tabs.iter_mut().filter(|tab| {
                    matches!(tab.kind, TerminalSessionKind::GitDiff)
                        && tab.host_id.as_deref() == Some(owner)
                        && tab.git_panel_state.repo_root.as_ref() == Some(&source_repo)
                }) {
                    tab.git_panel_state.status = status.clone();
                    // 文件被删除/改动消失后清掉陈旧 diff，避免标签停留显示已不存在文件的旧内容。
                    let had_diff = tab.git_panel_state.selected_diff.is_some();
                    clear_stale_diff_selection(&mut tab.git_panel_state);
                    if had_diff && tab.git_panel_state.selected_diff.is_none() {
                        changed = true;
                    }
                }
            }
            GitEvent::NotARepo { .. } => {
                for tab in self.terminal_tabs.iter_mut().filter(|tab| {
                    matches!(tab.kind, TerminalSessionKind::GitDiff)
                        && tab.host_id.as_deref() == Some(owner)
                        && tab.git_panel_state.repo_root.as_ref() == Some(&source_repo)
                }) {
                    tab.git_panel_state.diff_loading = false;
                    tab.git_panel_state.diff_error =
                        Some(rust_i18n::t!("git_panel_not_in_repo").to_string());
                    changed = true;
                }
            }
            _ => {}
        }
        changed
    }

    pub(crate) fn send_git_request_to_tab(&self, tab_id: &str, request: GitRequest) -> bool {
        self.terminal_tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .and_then(|tab| tab.git_worker.as_ref())
            .map(|worker| worker.send(request))
            .unwrap_or(false)
    }

    pub(crate) fn show_git_ssh_host_key_prompt(
        &mut self,
        tab_id: String,
        prompt: SshHostKeyPrompt,
        ctx: &mut ViewContext<Self>,
    ) {
        let title = git_ssh_host_key_prompt_title(&prompt);
        let info = git_ssh_host_key_prompt_info(&prompt);
        ctx.show_native_platform_modal(AlertDialogWithCallbacks::for_view(
            title,
            info,
            vec![
                ModalButton::for_view(rust_i18n::t!("git_panel_ssh_host_key_confirm"), {
                    let tab_id = tab_id.clone();
                    move |view: &mut Self, ctx: &mut ViewContext<Self>| {
                        view.queue_git_push_for_tab(&tab_id, true, ctx);
                    }
                }),
                ModalButton::for_view(
                    rust_i18n::t!("git_panel_ssh_host_key_cancel"),
                    |_: &mut Self, _: &mut ViewContext<Self>| {},
                ),
            ],
            |_: &mut Self, _: &mut ViewContext<Self>| {},
        ));
    }

    pub(crate) fn confirm_git_discard_worktree_change(
        &mut self,
        tab_id: String,
        path: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if path.trim().is_empty() {
            return;
        }
        ctx.show_native_platform_modal(AlertDialogWithCallbacks::for_view(
            rust_i18n::t!("git_panel_discard_title"),
            rust_i18n::t!("git_panel_discard_message", path = path.as_str()).to_string(),
            vec![
                ModalButton::for_view(rust_i18n::t!("git_panel_discard_confirm"), {
                    let tab_id = tab_id.clone();
                    let path = path.clone();
                    move |view: &mut Self, ctx: &mut ViewContext<Self>| {
                        view.queue_git_discard_worktree_change_for_tab(&tab_id, path.clone(), ctx);
                    }
                }),
                ModalButton::for_view(
                    rust_i18n::t!("dialog_cancel"),
                    |_: &mut Self, _: &mut ViewContext<Self>| {},
                ),
            ],
            |_: &mut Self, _: &mut ViewContext<Self>| {},
        ));
    }

    pub(crate) fn confirm_git_delete_untracked(
        &mut self,
        tab_id: String,
        path: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if path.trim().is_empty() {
            return;
        }
        ctx.show_native_platform_modal(AlertDialogWithCallbacks::for_view(
            rust_i18n::t!("git_panel_delete_untracked_title"),
            rust_i18n::t!("git_panel_delete_untracked_message", path = path.as_str()).to_string(),
            vec![
                ModalButton::for_view(rust_i18n::t!("git_panel_delete_untracked_confirm"), {
                    let tab_id = tab_id.clone();
                    let path = path.clone();
                    move |view: &mut Self, ctx: &mut ViewContext<Self>| {
                        view.queue_git_delete_untracked_for_tab(&tab_id, path.clone(), ctx);
                    }
                }),
                ModalButton::for_view(
                    rust_i18n::t!("dialog_cancel"),
                    |_: &mut Self, _: &mut ViewContext<Self>| {},
                ),
            ],
            |_: &mut Self, _: &mut ViewContext<Self>| {},
        ));
    }

    // ===== render fn：从 main.rs 第 2 块 impl RootView 整体搬入 =====

    pub(in crate::root_view) fn render_git_panel(&self, _app: &AppContext) -> Box<dyn Element> {
        let colors = self.design_tokens.overview;
        let width = self
            .git_panel_width
            .clamp(GIT_PANEL_WIDTH_MIN, GIT_PANEL_WIDTH_MAX);

        let Some(panel_index) = self.active_git_panel_tab_index() else {
            return self.render_git_panel_shell(
                file_panel_message(
                    &rust_i18n::t!("git_panel_empty"),
                    self.ui_font,
                    colors.text_muted,
                ),
                width,
                &colors,
            );
        };
        let Some(tab) = self.terminal_tabs.get(panel_index) else {
            return self.render_git_panel_shell(
                file_panel_message(
                    &rust_i18n::t!("git_panel_empty"),
                    self.ui_font,
                    colors.text_muted,
                ),
                width,
                &colors,
            );
        };

        if !matches!(tab.kind, TerminalSessionKind::Local) {
            return self.render_git_panel_shell(
                file_panel_message(
                    &rust_i18n::t!("git_panel_remote_unsupported"),
                    self.ui_font,
                    colors.text_muted,
                ),
                width,
                &colors,
            );
        }

        let header = self.render_git_panel_header(tab, &colors);
        let body = self.render_git_panel_body(tab, &colors);
        let footer = self.render_git_panel_footer(tab, &colors);

        let content = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(header)
            .with_child(Expanded::new(1.0, body).finish())
            .with_child(footer)
            .finish();
        self.render_git_panel_shell(content, width, &colors)
    }

    fn render_git_panel_shell(
        &self,
        inner: Box<dyn Element>,
        width: f32,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let padded = Container::new(inner)
            .with_padding_left(10.0)
            .with_padding_right(10.0)
            .with_padding_top(10.0)
            .with_padding_bottom(10.0)
            .finish();
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_git_panel_divider())
            .with_child(Expanded::new(1.0, padded).finish());
        Container::new(ConstrainedBox::new(row.finish()).with_width(width).finish())
            .with_background_color(colors.panel_bg)
            .with_border(Border::left(1.0).with_border_color(colors.panel_border))
            .finish()
    }

    /// 左缘拖拽条；语义跟 file_panel_divider 完全一致，只是 action 换成 GitPanel*。
    fn render_git_panel_divider(&self) -> Box<dyn Element> {
        let drag_state = self.git_panel_divider_drag_state.clone();
        Hoverable::new(self.git_panel_divider_state.clone(), move |_mouse| {
            let inner = ConstrainedBox::new(Empty::new().finish())
                .with_width(GIT_PANEL_DIVIDER_WIDTH)
                .finish();
            Draggable::new(drag_state.clone(), inner)
                .with_drag_axis(DragAxis::HorizontalOnly)
                .with_keep_original_visible(true)
                .on_drag_start(|ctx, _, rect| {
                    ctx.set_cursor(
                        warpui::platform::Cursor::ResizeLeftRight,
                        warpui::elements::ZIndex::Overlay(usize::MAX),
                    );
                    ctx.dispatch_typed_action(TerminalGridAction::GitPanelResizeStart(
                        rect.origin_x(),
                    ));
                })
                .on_drag(|ctx, _, rect, _| {
                    ctx.set_cursor(
                        warpui::platform::Cursor::ResizeLeftRight,
                        warpui::elements::ZIndex::Overlay(usize::MAX),
                    );
                    ctx.dispatch_typed_action(TerminalGridAction::GitPanelResizeMove(
                        rect.origin_x(),
                    ));
                })
                .on_drop(|ctx, _, _rect, _| {
                    ctx.dispatch_typed_action(TerminalGridAction::GitPanelResizeEnd);
                })
                .finish()
        })
        .with_cursor(warpui::platform::Cursor::ResizeLeftRight)
        .finish()
    }

    fn render_git_panel_header(
        &self,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let uc = self.ui_colors();
        let state = &tab.git_panel_state;
        let branch_label = match state.repo_root.as_ref() {
            Some(_) => state
                .status
                .branch
                .clone()
                .unwrap_or_else(|| rust_i18n::t!("git_panel_detached").to_string()),
            None => rust_i18n::t!("git_panel_no_repo").to_string(),
        };
        let mut sub = String::new();
        if state.in_repo() {
            if state.status.ahead > 0 {
                sub.push_str(&format!(" ↑{}", state.status.ahead));
            }
            if state.status.behind > 0 {
                sub.push_str(&format!(" ↓{}", state.status.behind));
            }
        }
        let title_text = if sub.is_empty() {
            branch_label
        } else {
            format!("{branch_label}{sub}")
        };

        let title = Text::new_inline(title_text, self.ui_font, 12.0)
            .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
            .with_color(colors.text_primary)
            .finish();

        let refresh = render_file_panel_icon_button(
            tab.git_panel_refresh_state.clone(),
            ICON_PATH_REFRESH,
            uc.icon_color_inactive,
            uc.icon_button_hover_bg,
            TerminalGridAction::GitPanelRefresh,
        );

        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(1.0, title).finish())
            .with_child(refresh)
            .finish();

        Container::new(row).with_padding_bottom(8.0).finish()
    }
}

#[cfg(test)]
mod tests {
    use crate::git_panel_view_helpers::{
        git_panel_body_should_show_loading, git_panel_footer_kind, git_panel_stage_all_paths,
        GitPanelFooterKind,
    };
    use nexshell::git_ops::GitStatusSnapshot;
    use nexshell::git_panel::GitPanelState;
    use std::path::PathBuf;

    #[test]
    fn git_panel_stage_all_paths_use_current_section_entries() {
        let entries = vec![
            nexshell::git_ops::GitFileEntry {
                path: "src/main.rs".into(),
                original_path: None,
                index_status: '.',
                worktree_status: 'M',
                stage: nexshell::git_ops::GitFileStage::Unstaged,
            },
            nexshell::git_ops::GitFileEntry {
                path: "src/git_panel.rs".into(),
                original_path: None,
                index_status: '.',
                worktree_status: 'M',
                stage: nexshell::git_ops::GitFileStage::Unstaged,
            },
        ];
        assert_eq!(
            git_panel_stage_all_paths(&entries),
            vec!["src/main.rs".to_string(), "src/git_panel.rs".to_string()]
        );
    }

    #[test]
    fn git_panel_footer_switches_to_push_only_when_clean_and_ahead() {
        let clean_ahead = GitPanelState {
            repo_root: Some(PathBuf::from("/repo")),
            status: GitStatusSnapshot {
                branch: Some("main".into()),
                upstream: Some("origin/main".into()),
                ahead: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            git_panel_footer_kind(&clean_ahead, false, false),
            GitPanelFooterKind::Push {
                enabled: true,
                synchronized: false,
            }
        );

        let clean_ahead_loading = GitPanelState {
            loading: true,
            ..clean_ahead.clone()
        };
        assert_eq!(
            git_panel_footer_kind(&clean_ahead_loading, false, false),
            GitPanelFooterKind::Push {
                enabled: true,
                synchronized: false,
            }
        );

        let clean_synced = GitPanelState {
            repo_root: Some(PathBuf::from("/repo")),
            status: GitStatusSnapshot {
                branch: Some("main".into()),
                upstream: Some("origin/main".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            git_panel_footer_kind(&clean_synced, false, false),
            GitPanelFooterKind::Push {
                enabled: false,
                synchronized: true,
            }
        );
    }

    #[test]
    fn git_panel_footer_keeps_push_surface_for_clean_branch_without_upstream() {
        let clean_no_upstream = GitPanelState {
            repo_root: Some(PathBuf::from("/repo")),
            status: GitStatusSnapshot {
                branch: Some("main".into()),
                upstream: None,
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(
            git_panel_footer_kind(&clean_no_upstream, false, false),
            GitPanelFooterKind::Push {
                enabled: false,
                synchronized: false,
            }
        );
    }

    #[test]
    fn git_panel_footer_keeps_commit_mode_while_changes_exist() {
        let with_staged_changes = GitPanelState {
            repo_root: Some(PathBuf::from("/repo")),
            status: GitStatusSnapshot {
                branch: Some("main".into()),
                upstream: Some("origin/main".into()),
                ahead: 1,
                staged: vec![nexshell::git_ops::GitFileEntry {
                    path: "src/main.rs".into(),
                    original_path: None,
                    index_status: 'M',
                    worktree_status: '.',
                    stage: nexshell::git_ops::GitFileStage::Staged,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            git_panel_footer_kind(&with_staged_changes, false, false),
            GitPanelFooterKind::Commit { enabled: true }
        );
    }

    #[test]
    fn git_panel_body_loading_keeps_existing_clean_snapshot_visible() {
        let mut state = GitPanelState {
            repo_root: Some(PathBuf::from("/repo")),
            loading: true,
            recent_commits: vec![nexshell::git_ops::CommitRow {
                sha: "abc1234".into(),
                full_sha: "abc1234".into(),
                author: "matt".into(),
                authored_at: String::new(),
                decorations: String::new(),
                summary: "local".into(),
                body: String::new(),
                files_changed: 0,
                insertions: 0,
                deletions: 0,
                file_changes: Vec::new(),
            }],
            ..Default::default()
        };

        assert!(!git_panel_body_should_show_loading(&state));

        state.recent_commits.clear();
        assert!(git_panel_body_should_show_loading(&state));
    }
}
