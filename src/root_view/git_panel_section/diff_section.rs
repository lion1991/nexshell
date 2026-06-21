// diff section — RootView 的 git diff 视图：select_diff handler + GitDiff tab 打开 + diff preview 渲染。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。
// 跨 section 调用：open_git_diff_tab 同时被 status_section::handle_git_panel_select_entry 使用。

use std::path::PathBuf;

use crate::file_panel_view_helpers::file_panel_message;
use crate::git_panel_view_helpers::{
    git_diff_line_number_text, git_diff_tab_id, git_diff_tab_label,
};
use crate::ui_colors::HostOverviewColors;
use crate::{RootView, TerminalSessionKind, TerminalSessionTab};
use nexshell::git_ops::{GitDiffLine, GitDiffLineType, GitDiffSelection};
use nexshell::host_overview::format_bytes_short;
use nexshell::terminal_runtime::LocalTerminalRuntime;
use warp_core::ui::color::coloru_with_opacity;
use warpui::elements::{
    Align, Clipped, ClippedScrollable, ConstrainedBox, Container, CrossAxisAlignment, Expanded,
    Fill, Flex, MainAxisSize, ParentElement, ScrollbarWidth, Text,
};
use warpui::fonts;
use warpui::{AppContext, Element, ViewContext};

impl RootView {
    pub(super) fn open_git_diff_tab(
        &mut self,
        repo_root: PathBuf,
        selection: GitDiffSelection,
        source_tab_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        // 复用开启（默认）：同源终端 + 同 repo 下共用一个 diff 标签，忽略具体文件，
        // 命中即换内容重载；关闭：按 (源tab, repo, selection) 精确匹配，维持每文件一标签。
        let reuse = self.reuse_view_tab;
        if let Some(idx) = self.terminal_tabs.iter().position(|tab| {
            matches!(tab.kind, TerminalSessionKind::GitDiff)
                && tab.host_id.as_deref() == Some(source_tab_id.as_str())
                && tab.git_panel_state.repo_root.as_ref() == Some(&repo_root)
                && (reuse || tab.git_panel_state.selected_diff.as_ref() == Some(&selection))
        }) {
            if reuse {
                // 切换展示文件：更新 selection + 重置加载态，等 LoadDiff 事件回填
                // （调用方已先发 GitRequest::LoadDiff，apply_git_event_to_diff_tabs 按 selection 匹配）。
                let label = git_diff_tab_label(&selection.path);
                if let Some(tab) = self.terminal_tabs.get_mut(idx) {
                    tab.git_panel_state.selected_diff = Some(selection);
                    tab.git_panel_state.diff_preview = None;
                    tab.git_panel_state.diff_loading = true;
                    tab.git_panel_state.diff_error = None;
                    tab.fallback_label = label;
                    tab.custom_label = None;
                }
            }
            self.activate_terminal_tab(idx, ctx);
            return;
        }

        let session_id = git_diff_tab_id(&repo_root, &selection);
        let terminal = LocalTerminalRuntime::failed(&session_id, "git diff view");
        let label = git_diff_tab_label(&selection.path);
        self.push_terminal_tab(
            terminal,
            &session_id,
            label,
            TerminalSessionKind::GitDiff,
            Some(source_tab_id),
            None,
            ctx,
        );
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            tab.git_panel_state.repo_root = Some(repo_root);
            tab.git_panel_state.selected_diff = Some(selection);
            tab.git_panel_state.diff_preview = None;
            tab.git_panel_state.diff_loading = true;
            tab.git_panel_state.diff_error = None;
        }
    }

    pub(in crate::root_view) fn render_git_diff_page(&self, _app: &AppContext) -> Box<dyn Element> {
        let colors = HostOverviewColors::from_theme(&self.cached_warp_theme);
        let Some(tab) = self.terminal_tabs.get(self.active_tab_index) else {
            return file_panel_message(
                &rust_i18n::t!("git_panel_diff_select_file"),
                self.ui_font,
                colors.text_muted,
            );
        };
        let content = self.render_git_panel_diff_preview(tab, &colors);
        Container::new(content)
            .with_horizontal_padding(2.0)
            .with_vertical_padding(2.0)
            .with_background_color(colors.panel_bg)
            .finish()
    }

    fn render_git_panel_diff_preview(
        &self,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let state = &tab.git_panel_state;
        let title = state
            .selected_diff
            .as_ref()
            .map(|selection| {
                format!(
                    "{} · {}",
                    rust_i18n::t!("git_panel_section_diff"),
                    selection.path
                )
            })
            .unwrap_or_else(|| rust_i18n::t!("git_panel_section_diff").to_string());
        let title = Clipped::new(
            Text::new_inline(title, self.ui_font, 11.0)
                .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
                .with_color(colors.text_muted)
                .finish(),
        )
        .finish();

        let stats = state
            .diff_preview
            .as_ref()
            .map(|diff| format!("+{} -{}", diff.additions, diff.deletions));
        let mut header = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(1.0, title).finish());
        if let Some(stats) = stats {
            header = header.with_child(
                Text::new_inline(stats, self.monospace_font, 11.0)
                    .with_color(colors.text_muted)
                    .finish(),
            );
        }

        let content = if state.diff_loading {
            file_panel_message(
                &rust_i18n::t!("git_panel_diff_loading"),
                self.ui_font,
                colors.text_muted,
            )
        } else if let Some(error) = state.diff_error.as_ref() {
            file_panel_message(error, self.ui_font, colors.warning)
        } else if let Some(diff) = state.diff_preview.as_ref() {
            self.render_git_file_diff(diff, tab, colors)
        } else {
            file_panel_message(
                &rust_i18n::t!("git_panel_diff_select_file"),
                self.ui_font,
                colors.text_muted,
            )
        };

        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                Container::new(header.finish())
                    .with_padding_top(8.0)
                    .with_padding_bottom(6.0)
                    .finish(),
            )
            .with_child(Expanded::new(1.0, content).finish())
            .finish()
    }

    fn render_git_file_diff(
        &self,
        diff: &nexshell::git_ops::GitFileDiff,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        if diff.is_too_large {
            let message = rust_i18n::t!(
                "git_panel_diff_too_large",
                size = format_bytes_short(diff.raw_size as u64).as_str()
            )
            .to_string();
            return file_panel_message(&message, self.ui_font, colors.text_muted);
        }
        if diff.is_binary {
            return file_panel_message(
                diff.binary_message
                    .as_deref()
                    .unwrap_or(rust_i18n::t!("git_panel_diff_binary").as_ref()),
                self.ui_font,
                colors.text_muted,
            );
        }
        if diff.hunks.is_empty() {
            return file_panel_message(
                &rust_i18n::t!("git_panel_diff_empty"),
                self.ui_font,
                colors.text_muted,
            );
        }

        let mut rows = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for hunk in &diff.hunks {
            rows.add_child(self.render_git_diff_hunk_header(&hunk.header, colors));
            for line in &hunk.lines {
                rows.add_child(self.render_git_diff_line(line, colors));
            }
        }

        ClippedScrollable::vertical(
            tab.git_panel_diff_scroll_state.clone(),
            rows.finish(),
            ScrollbarWidth::Custom(4.0),
            Fill::Solid(colors.text_muted),
            Fill::Solid(colors.text_primary),
            Fill::None,
        )
        .with_overlayed_scrollbar()
        .finish()
    }

    fn render_git_diff_hunk_header(
        &self,
        header: &str,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        Container::new(
            Text::new_inline(header.to_string(), self.monospace_font, 11.0)
                .with_color(colors.cpu_accent)
                .finish(),
        )
        .with_padding_left(6.0)
        .with_padding_right(6.0)
        .with_padding_top(3.0)
        .with_padding_bottom(3.0)
        .with_background_color(coloru_with_opacity(colors.cpu_accent, 12))
        .finish()
    }

    fn render_git_diff_line(
        &self,
        line: &GitDiffLine,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let (prefix, text_color, bg_color) = match line.line_type {
            GitDiffLineType::Add => (
                "+",
                colors.download,
                Some(coloru_with_opacity(colors.download, 12)),
            ),
            GitDiffLineType::Delete => (
                "-",
                colors.upload,
                Some(coloru_with_opacity(colors.upload, 12)),
            ),
            GitDiffLineType::Context => (" ", colors.text_primary, None),
        };
        let mut text = line.text.clone();
        if line.no_trailing_newline {
            text.push_str("  \\ No newline at end of file");
        }

        let old_no = ConstrainedBox::new(
            Align::new(
                Text::new_inline(
                    git_diff_line_number_text(line.old_line_number),
                    self.monospace_font,
                    11.0,
                )
                .with_color(colors.text_muted)
                .finish(),
            )
            .right()
            .finish(),
        )
        .with_width(34.0)
        .finish();
        let new_no = ConstrainedBox::new(
            Align::new(
                Text::new_inline(
                    git_diff_line_number_text(line.new_line_number),
                    self.monospace_font,
                    11.0,
                )
                .with_color(colors.text_muted)
                .finish(),
            )
            .right()
            .finish(),
        )
        .with_width(34.0)
        .finish();
        let prefix = Text::new_inline(prefix.to_string(), self.monospace_font, 11.0)
            .with_color(text_color)
            .finish();
        let content = Clipped::new(
            Text::new_inline(text, self.monospace_font, 11.0)
                .with_color(text_color)
                .finish(),
        )
        .finish();

        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(old_no)
            .with_child(Container::new(new_no).with_padding_left(4.0).finish())
            .with_child(Container::new(prefix).with_padding_left(8.0).finish())
            .with_child(
                Container::new(Expanded::new(1.0, content).finish())
                    .with_padding_left(6.0)
                    .finish(),
            )
            .finish();
        let mut container = Container::new(row)
            .with_padding_left(2.0)
            .with_padding_right(4.0)
            .with_padding_top(1.0)
            .with_padding_bottom(1.0);
        if let Some(bg_color) = bg_color {
            container = container.with_background_color(bg_color);
        }
        container.finish()
    }
}
