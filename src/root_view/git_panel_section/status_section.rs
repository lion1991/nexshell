// status section — RootView 的 git 状态视图：staged / unstaged / untracked / unmerged 三栏 + entries。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。
// 文件区已虚拟化（UniformList，只构建可见行）：行模型/行构建自由函数在 git_panel_row_helpers.rs。
// 同 section 调用：handle_git_panel_select_entry → diff_section::open_git_diff_tab；
// render_git_panel_body → history_section::render_git_panel_history_divider / render_git_panel_history_section。

use std::sync::Arc;

use crate::file_panel_view_helpers::file_panel_message;
use crate::git_panel_row_helpers::{
    build_git_panel_entry_row, build_git_panel_gap_row, build_git_panel_header_row,
    git_panel_entry_at, git_panel_row_at, git_panel_row_sections, git_panel_total_rows,
    GitPanelRowKind, GitPanelRowSection,
};
use crate::git_panel_view_helpers::{
    git_panel_body_should_show_loading, git_panel_entry_action_state_key, git_panel_entry_state_key,
};
use crate::ui_colors::HostOverviewColors;
use crate::{RootView, TerminalSessionTab};
use nexshell::git_ops::{GitDiffKind, GitDiffSelection, GitStatusSnapshot};
use nexshell::git_panel::{
    apply_git_panel_selection, clamp_git_history_height, GitPanelSelectMode, GitRequest,
};
use warpui::color::ColorU;
use warpui::elements::{
    ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Expanded, Fill, Flex,
    MainAxisSize, ParentElement, Radius, Scrollable, ScrollableElement, ScrollbarWidth, Text,
    UniformList,
};
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

        let status = Arc::clone(&state.status);
        self.prune_git_panel_hover_states_if_snapshot_changed(tab, &status);

        let sections = git_panel_row_sections(&status);
        let total_rows = git_panel_total_rows(&sections);

        let changes: Box<dyn Element> = if total_rows == 0 {
            // 无变更：干净文案。Text 忽略 min 约束，须包 Max 列占满 Expanded 的紧约束，
            // 否则 changes 区坍缩、历史区被顶到面板上部（分隔条随之视觉冻结）。
            let clean = Text::new_inline(
                rust_i18n::t!("git_panel_clean").to_string(),
                self.ui_font,
                11.0,
            )
            .with_color(colors.text_muted)
            .finish();
            Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(Container::new(clean).with_padding_top(6.0).finish())
                .finish()
        } else {
            self.build_git_panel_changes_list(tab, &status, sections, total_rows, colors)
        };

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

    /// 淘汰已不在 status 快照里的行/按钮 hover 状态（key 构造与行构建处同源）。
    /// 用 Arc::ptr_eq 比较上次已处理的快照：快照没变的帧零开销，不再每帧全量 retain。
    fn prune_git_panel_hover_states_if_snapshot_changed(
        &self,
        tab: &TerminalSessionTab,
        status: &Arc<GitStatusSnapshot>,
    ) {
        let unchanged = tab
            .git_panel_pruned_status
            .borrow()
            .as_ref()
            .is_some_and(|prev| Arc::ptr_eq(prev, status));
        if unchanged {
            return;
        }
        let mut valid = std::collections::HashSet::new();
        let mut valid_action = std::collections::HashSet::new();
        for (entries, is_staged) in [
            (&status.staged, true),
            (&status.unstaged, false),
            (&status.untracked, false),
            (&status.unmerged, false),
        ] {
            for e in entries {
                valid.insert(git_panel_entry_state_key(&e.path, is_staged));
                valid_action.insert(git_panel_entry_action_state_key(&e.path, is_staged));
            }
        }
        tab.git_panel_entry_states
            .borrow_mut()
            .retain(|k, _| valid.contains(k));
        tab.git_panel_entry_action_states
            .borrow_mut()
            .retain(|k, _| valid_action.contains(k));
        *tab.git_panel_pruned_status.borrow_mut() = Some(Arc::clone(status));
    }

    /// 文件变更列表：UniformList 虚拟滚动，只构建可见 range 的行（header/entry/gap）。
    fn build_git_panel_changes_list(
        &self,
        tab: &TerminalSessionTab,
        status: &Arc<GitStatusSnapshot>,
        sections: Vec<GitPanelRowSection>,
        total_rows: usize,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let list_state = tab.git_panel_list_state.clone();
        let status_for_list = Arc::clone(status);
        let selected_entries = Arc::clone(&tab.git_panel_state.selected_entries);
        let entry_states = tab.git_panel_entry_states.clone();
        let entry_action_states = tab.git_panel_entry_action_states.clone();
        let tab_id = tab.id.clone();
        let ui_font = self.ui_font;
        let overview = *colors;
        let semantic = self.design_tokens.semantic;
        let chrome = self.ui_colors();
        let stage_all_state = tab.git_panel_stage_all_state.clone();
        let text_muted = overview.text_muted;
        let accent_text = overview.cpu_accent;
        let header_hover_bg = chrome.tab_bg_hover;
        let sections_for_list = sections;

        let list = UniformList::new(list_state, total_rows, move |range, _ctx| {
            range
                .map(|index| match git_panel_row_at(&sections_for_list, index) {
                    Some(GitPanelRowKind::SectionHeader(section_idx)) => {
                        build_git_panel_header_row(
                            &sections_for_list[section_idx],
                            stage_all_state.clone(),
                            ui_font,
                            text_muted,
                            accent_text,
                            header_hover_bg,
                        )
                    }
                    Some(GitPanelRowKind::Entry { section, index }) => {
                        match git_panel_entry_at(
                            &sections_for_list,
                            &status_for_list,
                            section,
                            index,
                        ) {
                            Some(entry) => build_git_panel_entry_row(
                                entry.clone(),
                                sections_for_list[section].kind.is_staged(),
                                sections_for_list[section].discard_enabled,
                                Arc::clone(&selected_entries),
                                entry_states.clone(),
                                entry_action_states.clone(),
                                tab_id.clone(),
                                ui_font,
                                overview,
                                semantic,
                                chrome,
                            ),
                            None => build_git_panel_gap_row(),
                        }
                    }
                    Some(GitPanelRowKind::Gap) | None => build_git_panel_gap_row(),
                })
                .collect::<Vec<_>>()
                .into_iter()
        })
        .finish_scrollable();

        Scrollable::vertical(
            tab.git_panel_scrollbar_state.clone(),
            list,
            ScrollbarWidth::Custom(4.0),
            Fill::Solid(colors.text_muted),
            Fill::Solid(colors.text_primary),
            Fill::None,
        )
        .with_overlayed_scrollbar()
        .finish()
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
}
