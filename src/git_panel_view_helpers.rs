//! Git 面板视图相关的纯函数 helper、装饰徽章、上下文菜单与提交行渲染。
//! 提交详情卡渲染已抽到 git_commit_detail_helpers.rs（ADR 0004）。
//!
//! 这里集中放与 git 面板视图相关的逻辑，方便测试与 main.rs 解耦。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use nexshell::text_editor::{EditorOptions, TextOptions};
use warpui::color::ColorU;
use warpui::elements::{
    Align, Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Expanded, Flex,
    Icon, ParentElement, Radius, Text,
};
use warpui::fonts;
use warpui::Element;

use nexshell::git_ops::{
    CommitRow, GitDiffKind, GitDiffSelection, GitFileEntry, GitStatusSnapshot, SshHostKeyPrompt,
};
use nexshell::git_panel::{GitPanelState, GIT_HISTORY_PAGE_SIZE};

use super::terminal_grid_element::TerminalGridAction;
use super::ui_colors::HostOverviewColors;
use super::{
    GIT_REF_BADGE_MAX_WIDTH, ICON_PATH_CLOUD, ICON_PATH_GIT_BRANCH, ICON_PATH_GIT_LOCAL_REF,
};

pub(crate) const GIT_COMMIT_DETAIL_CLEAR_DELAY: Duration = Duration::from_millis(300);
const GIT_HISTORY_ROW_ESTIMATE_PX: f32 = 36.0;
const GIT_HISTORY_LOAD_MORE_THRESHOLD_PX: f32 = 96.0;
pub(crate) const GIT_HISTORY_SCROLLABLE_HEADER_PX: f32 = 24.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitCommitDecorationKind {
    LocalHead,
    Remote,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitCommitDecorationBadge {
    pub label: String,
    pub kind: GitCommitDecorationKind,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn git_commit_decoration_badge(decorations: &str) -> Option<GitCommitDecorationBadge> {
    git_commit_decoration_badges(decorations).into_iter().next()
}

pub(crate) fn git_commit_decoration_badges(decorations: &str) -> Vec<GitCommitDecorationBadge> {
    let mut badges = Vec::new();
    let mut remote = None;
    let mut fallback = None;
    for item in decorations
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some(branch) = item.strip_prefix("HEAD -> ") {
            if !badges.iter().any(|badge: &GitCommitDecorationBadge| {
                badge.kind == GitCommitDecorationKind::LocalHead
            }) {
                badges.push(GitCommitDecorationBadge {
                    label: branch.to_string(),
                    kind: GitCommitDecorationKind::LocalHead,
                });
            }
            continue;
        }
        if item == "HEAD" {
            continue;
        }
        let badge = GitCommitDecorationBadge {
            label: if item.starts_with("origin/") {
                String::new()
            } else {
                item.to_string()
            },
            kind: if item.starts_with("origin/") {
                GitCommitDecorationKind::Remote
            } else {
                GitCommitDecorationKind::Other
            },
        };
        match badge.kind {
            GitCommitDecorationKind::Remote if remote.is_none() => remote = Some(badge),
            GitCommitDecorationKind::Other if fallback.is_none() => fallback = Some(badge),
            _ => {}
        }
    }
    if let Some(remote) = remote {
        badges.push(remote);
    } else if badges.is_empty() {
        if let Some(fallback) = fallback {
            badges.push(fallback);
        }
    }
    badges
}

pub(crate) fn git_ref_badge_text_color(bg: ColorU) -> ColorU {
    let luminance = (bg.r as u32 * 299 + bg.g as u32 * 587 + bg.b as u32 * 114) / 1000;
    if luminance >= 140 {
        ColorU::new(0x14, 0x18, 0x1f, 0xff)
    } else {
        ColorU::new(0xf8, 0xfa, 0xfc, 0xff)
    }
}

pub(crate) fn git_commit_hover_target_after_event(
    current: Option<&str>,
    sha: &str,
    hovered: bool,
) -> Option<String> {
    if hovered {
        return Some(sha.to_string());
    }
    current.map(str::to_string)
}

pub(crate) fn git_commit_hover_target_after_motion(
    current: Option<&str>,
    row_hovered: bool,
    detail_hovered: bool,
    clear_after: Option<Instant>,
    now: Instant,
) -> (Option<String>, Option<Instant>) {
    if current.is_none() {
        return (None, None);
    }
    if row_hovered || detail_hovered {
        return (current.map(str::to_string), None);
    }
    let deadline = clear_after.unwrap_or(now + GIT_COMMIT_DETAIL_CLEAR_DELAY);
    if now >= deadline {
        (None, None)
    } else {
        (current.map(str::to_string), Some(deadline))
    }
}

pub(crate) fn git_commit_detail_target<'a>(
    _selected: Option<&'a str>,
    hovered: Option<&'a str>,
) -> Option<&'a str> {
    hovered
}

pub(crate) fn git_history_scroll_should_load_more(
    previous_scroll_start: f32,
    current_scroll_start: f32,
    delta_y: f32,
    loaded_count: usize,
    visible_height: f32,
) -> bool {
    if delta_y >= -0.1 || loaded_count < GIT_HISTORY_PAGE_SIZE || current_scroll_start <= 0.0 {
        return false;
    }
    if (current_scroll_start - previous_scroll_start).abs() < 0.5 {
        return true;
    }
    let estimated_total_height = loaded_count as f32 * GIT_HISTORY_ROW_ESTIMATE_PX;
    current_scroll_start + visible_height + GIT_HISTORY_LOAD_MORE_THRESHOLD_PX
        >= estimated_total_height
}

pub(crate) fn git_commit_row_position_id(tab_id: &str, sha: &str) -> String {
    format!("nexshell_git_commit_row_{tab_id}_{sha}")
}

pub(crate) fn git_commit_row_visual_hovered(
    _delayed_hovered: bool,
    mouse_over_element: bool,
) -> bool {
    mouse_over_element
}

pub(crate) fn git_panel_entry_state_key(path: &str, is_staged: bool) -> String {
    let scope = if is_staged { "staged" } else { "worktree" };
    format!("{scope}:{path}")
}

pub(crate) fn git_panel_entry_action_state_key(path: &str, is_staged: bool) -> String {
    format!("action:{}", git_panel_entry_state_key(path, is_staged))
}

pub(crate) fn git_panel_entry_tooltip_position_id(path: &str, is_staged: bool) -> String {
    format!("tooltip:{}", git_panel_entry_state_key(path, is_staged))
}

pub(crate) fn git_panel_entry_tooltip_text(entry: &GitFileEntry, xy: &str) -> String {
    if let Some(orig) = entry.original_path.as_deref() {
        format!("{xy}  {} ← {orig}", entry.path)
    } else {
        format!("{xy}  {}", entry.path)
    }
}

pub(crate) fn git_diff_tab_id(repo_root: &PathBuf, selection: &GitDiffSelection) -> String {
    let mut hasher = DefaultHasher::new();
    repo_root.hash(&mut hasher);
    selection.path.hash(&mut hasher);
    format!("{:?}", selection.kind).hash(&mut hasher);
    format!("git-diff-{:016x}", hasher.finish())
}

pub(crate) fn git_diff_tab_label(path: &str) -> String {
    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path);
    format!("Diff · {file_name}")
}

pub(crate) fn git_panel_diff_kind_for_entry(entry: &GitFileEntry, is_staged: bool) -> GitDiffKind {
    if is_staged {
        GitDiffKind::Staged
    } else if entry.index_status == '?' {
        GitDiffKind::Untracked
    } else {
        GitDiffKind::Unstaged
    }
}

pub(crate) fn git_diff_line_number_text(line_number: Option<usize>) -> String {
    line_number.map(|n| n.to_string()).unwrap_or_default()
}

pub(crate) fn git_panel_stage_all_paths(entries: &[GitFileEntry]) -> Vec<String> {
    entries.iter().map(|entry| entry.path.clone()).collect()
}

/// 右键菜单的批量路径集合。
pub(crate) struct GitPanelContextPaths {
    /// 与右键目标同区的选中路径（unstage / gitignore / discard 用）。
    pub same_kind: Vec<String>,
    /// 跨区可暂存路径（unstaged + untracked，对齐 VS Code 批量暂存）。
    pub stageable: Vec<String>,
}

pub(crate) fn git_panel_context_paths(
    state: &GitPanelState,
    target: &GitDiffSelection,
) -> GitPanelContextPaths {
    if state.selected_entries.contains(target) {
        let collect = |keep: &dyn Fn(GitDiffKind) -> bool| -> Vec<String> {
            state
                .selected_entries
                .iter()
                .filter(|selection| keep(selection.kind))
                .map(|selection| selection.path.clone())
                .collect()
        };
        let same_kind = collect(&|kind| kind == target.kind);
        let stageable =
            collect(&|kind| matches!(kind, GitDiffKind::Unstaged | GitDiffKind::Untracked));
        if !same_kind.is_empty() {
            return GitPanelContextPaths {
                same_kind,
                stageable,
            };
        }
    }
    GitPanelContextPaths {
        same_kind: vec![target.path.clone()],
        stageable: vec![target.path.clone()],
    }
}

pub(crate) fn git_panel_context_menu_items(
    tab_id: &str,
    kind: GitDiffKind,
    paths: GitPanelContextPaths,
    discard_enabled: bool,
) -> Vec<nexshell::menu::MenuItem<TerminalGridAction>> {
    use nexshell::menu::{MenuItem, MenuItemFields};

    let mut items = Vec::new();
    match kind {
        GitDiffKind::Staged => {
            items.push(
                MenuItemFields::new(rust_i18n::t!("git_panel_ctx_unstage"))
                    .with_disabled(paths.same_kind.is_empty())
                    .with_on_select_action(TerminalGridAction::GitPanelUnstagePaths {
                        tab_id: tab_id.to_string(),
                        paths: paths.same_kind.clone(),
                    })
                    .into_item(),
            );
        }
        GitDiffKind::Unstaged => {
            items.push(
                MenuItemFields::new(rust_i18n::t!("git_panel_ctx_stage"))
                    .with_disabled(paths.stageable.is_empty())
                    .with_on_select_action(TerminalGridAction::GitPanelStagePaths {
                        tab_id: tab_id.to_string(),
                        paths: paths.stageable.clone(),
                    })
                    .into_item(),
            );
            items.push(MenuItem::Separator);
            items.push(
                MenuItemFields::new(rust_i18n::t!("git_panel_ctx_discard_changes"))
                    .with_disabled(!discard_enabled || paths.same_kind.len() != 1)
                    .with_on_select_action(TerminalGridAction::GitPanelDiscardWorktreeChanges {
                        tab_id: tab_id.to_string(),
                        path: paths.same_kind.first().cloned().unwrap_or_default(),
                    })
                    .into_item(),
            );
        }
        GitDiffKind::Untracked => {
            items.push(
                MenuItemFields::new(rust_i18n::t!("git_panel_ctx_stage"))
                    .with_disabled(paths.stageable.is_empty())
                    .with_on_select_action(TerminalGridAction::GitPanelStagePaths {
                        tab_id: tab_id.to_string(),
                        paths: paths.stageable.clone(),
                    })
                    .into_item(),
            );
            items.push(
                MenuItemFields::new(rust_i18n::t!("git_panel_ctx_add_gitignore"))
                    .with_disabled(paths.same_kind.is_empty())
                    .with_on_select_action(TerminalGridAction::GitPanelAddToGitignore {
                        tab_id: tab_id.to_string(),
                        paths: paths.same_kind.clone(),
                    })
                    .into_item(),
            );
            items.push(MenuItem::Separator);
            // VSCode 风格：菜单统一叫「丢弃改动」，untracked 实为删盘，靠确认弹窗强警告。
            items.push(
                MenuItemFields::new(rust_i18n::t!("git_panel_ctx_discard_changes"))
                    .with_disabled(paths.same_kind.len() != 1)
                    .with_on_select_action(TerminalGridAction::GitPanelDeleteUntracked {
                        tab_id: tab_id.to_string(),
                        path: paths.same_kind.first().cloned().unwrap_or_default(),
                    })
                    .into_item(),
            );
        }
    }
    items
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitPanelFooterKind {
    None,
    Commit { enabled: bool },
    Push { enabled: bool, synchronized: bool },
}

pub(crate) fn git_panel_status_has_changes(status: &GitStatusSnapshot) -> bool {
    !status.staged.is_empty()
        || !status.unstaged.is_empty()
        || !status.untracked.is_empty()
        || !status.unmerged.is_empty()
}

pub(crate) fn git_panel_footer_kind(
    state: &GitPanelState,
    commit_busy: bool,
    push_busy: bool,
) -> GitPanelFooterKind {
    if !state.in_repo() {
        return GitPanelFooterKind::None;
    }
    if git_panel_status_has_changes(&state.status) {
        return GitPanelFooterKind::Commit {
            enabled: !state.status.staged.is_empty() && !commit_busy,
        };
    }
    if state.status.ahead > 0 {
        return GitPanelFooterKind::Push {
            enabled: state.status.upstream.is_some() && !push_busy,
            synchronized: false,
        };
    }
    if state.status.upstream.is_some() {
        return GitPanelFooterKind::Push {
            enabled: false,
            synchronized: true,
        };
    }
    if state.status.branch.is_some() {
        return GitPanelFooterKind::Push {
            enabled: false,
            synchronized: false,
        };
    }
    GitPanelFooterKind::None
}

pub(crate) fn git_panel_body_should_show_loading(state: &GitPanelState) -> bool {
    state.loading && !git_panel_status_has_changes(&state.status) && state.recent_commits.is_empty()
}

pub(crate) fn git_commit_editor_options(font_size: f32) -> EditorOptions {
    EditorOptions {
        text: TextOptions {
            font_size_override: Some(font_size),
            ..Default::default()
        },
        autogrow: true,
        soft_wrap: true,
        placeholder_soft_wrap: true,
        single_line: false,
        ..Default::default()
    }
}

pub(crate) fn animated_push_busy_label(label: &str, tick: u64) -> String {
    let base = label.trim_end_matches('.').trim_end_matches('\u{2026}');
    format!("{base}{}", ".".repeat((tick as usize % 3) + 1))
}

pub(crate) fn git_ssh_host_key_prompt_title(prompt: &SshHostKeyPrompt) -> String {
    if let Some(host) = prompt.host.as_deref().filter(|host| !host.is_empty()) {
        rust_i18n::t!("git_panel_ssh_host_key_title_host", host = host).to_string()
    } else {
        rust_i18n::t!("git_panel_ssh_host_key_title").to_string()
    }
}

pub(crate) fn git_ssh_host_key_prompt_info(prompt: &SshHostKeyPrompt) -> String {
    let mut parts = vec![rust_i18n::t!("git_panel_ssh_host_key_message").to_string()];
    if let Some(fingerprint) = prompt
        .fingerprint
        .as_deref()
        .filter(|fingerprint| !fingerprint.is_empty())
    {
        parts.push(
            rust_i18n::t!(
                "git_panel_ssh_host_key_fingerprint",
                fingerprint = fingerprint
            )
            .to_string(),
        );
    }
    if !prompt.message.trim().is_empty() {
        parts.push(prompt.message.trim().to_string());
    }
    parts.push(rust_i18n::t!("git_panel_ssh_host_key_warning").to_string());
    parts.join("\n\n")
}

pub(crate) fn render_git_panel_commit_row_content(
    commit: &CommitRow,
    colors: HostOverviewColors,
    ui_font: fonts::FamilyId,
    hovered: bool,
) -> Box<dyn Element> {
    let dot = Text::new_inline("●".to_string(), ui_font, 12.0)
        .with_color(colors.cpu_accent)
        .finish();
    let graph = ConstrainedBox::new(
        Container::new(Align::new(dot).finish())
            .with_border(Border::left(1.0).with_border_color(colors.cpu_accent))
            .with_padding_left(3.0)
            .finish(),
    )
    .with_width(18.0)
    .finish();

    let summary = Text::new_inline(commit.summary.clone(), ui_font, 11.0)
        .with_color(colors.text_primary)
        .finish();
    let mut meta_parts = vec![commit.sha.clone()];
    if !commit.author.is_empty() {
        meta_parts.push(commit.author.clone());
    }
    let meta = Text::new_inline(meta_parts.join("  "), ui_font, 10.0)
        .with_color(colors.text_muted)
        .finish();

    let mut summary_row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(Expanded::new(1.0, summary).finish());
    for decoration in git_commit_decoration_badges(&commit.decorations) {
        let (icon_path, bg) = match decoration.kind {
            GitCommitDecorationKind::LocalHead => (ICON_PATH_GIT_LOCAL_REF, colors.cpu_accent),
            GitCommitDecorationKind::Remote => (ICON_PATH_CLOUD, colors.swap_accent),
            GitCommitDecorationKind::Other => (ICON_PATH_GIT_BRANCH, colors.metric_track),
        };
        let fg = git_ref_badge_text_color(bg);
        let icon = ConstrainedBox::new(Icon::new(icon_path, fg).finish())
            .with_width(12.0)
            .with_height(12.0)
            .finish();
        let icon_only = decoration.label.is_empty();
        let badge_content = if icon_only {
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(icon)
                .finish()
        } else {
            let badge_text = Text::new_inline(decoration.label, ui_font, 10.0)
                .with_color(fg)
                .finish();
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(icon)
                .with_child(Container::new(badge_text).with_padding_left(3.0).finish())
                .finish()
        };
        let badge = ConstrainedBox::new(
            Container::new(badge_content)
                .with_horizontal_padding(if icon_only { 3.0 } else { 5.0 })
                .with_vertical_padding(2.0)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(7.0)))
                .with_background_color(bg)
                .finish(),
        )
        .with_max_width(GIT_REF_BADGE_MAX_WIDTH)
        .finish();
        summary_row.add_child(Container::new(badge).with_padding_left(6.0).finish());
    }

    let body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(summary_row.finish())
        .with_child(Container::new(meta).with_padding_top(1.0).finish())
        .finish();

    let row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(graph)
        .with_child(Expanded::new(1.0, body).finish())
        .finish();
    let mut container = Container::new(row)
        .with_padding_top(3.0)
        .with_padding_bottom(3.0)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)));
    if hovered {
        container = container.with_background_color(colors.card_bg);
    }
    container.finish()
}

#[cfg(test)]
mod tests {
    use super::{
        git_commit_decoration_badge, git_commit_decoration_badges, GitCommitDecorationBadge,
        GitCommitDecorationKind,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn git_commit_decoration_label_hides_remote_branch_name() {
        assert_eq!(
            git_commit_decoration_badge("HEAD -> windows-native-shell")
                .map(|badge| badge.label),
            Some("windows-native-shell".to_string())
        );
        assert_eq!(
            git_commit_decoration_badge("origin/windows-native-shell")
                .map(|badge| badge.label),
            Some(String::new())
        );
    }

    #[test]
    fn git_commit_decoration_badge_classifies_local_and_remote_refs() {
        assert_eq!(
            git_commit_decoration_badge("HEAD -> windows-native-shell"),
            Some(GitCommitDecorationBadge {
                label: "windows-native-shell".to_string(),
                kind: GitCommitDecorationKind::LocalHead,
            })
        );
        assert_eq!(
            git_commit_decoration_badge("origin/windows-native-shell"),
            Some(GitCommitDecorationBadge {
                label: String::new(),
                kind: GitCommitDecorationKind::Remote,
            })
        );
    }

    #[test]
    fn git_commit_decoration_badges_show_local_and_remote_refs_on_same_commit() {
        assert_eq!(
            git_commit_decoration_badges(
                "HEAD -> windows-native-shell, origin/windows-native-shell"
            ),
            vec![
                GitCommitDecorationBadge {
                    label: "windows-native-shell".to_string(),
                    kind: GitCommitDecorationKind::LocalHead,
                },
                GitCommitDecorationBadge {
                    label: String::new(),
                    kind: GitCommitDecorationKind::Remote,
                },
            ]
        );
    }

    #[test]
    fn git_panel_context_paths_merges_stageable_kinds_across_sections() {
        use nexshell::git_ops::{GitDiffKind, GitDiffSelection};

        let sel = |path: &str, kind| GitDiffSelection {
            path: path.into(),
            kind,
        };
        let mut state = super::GitPanelState::default();
        state.selected_entries = [
            sel("staged.rs", GitDiffKind::Staged),
            sel("changed.rs", GitDiffKind::Unstaged),
            sel("new.txt", GitDiffKind::Untracked),
        ]
        .into_iter()
        .collect();

        let target = sel("changed.rs", GitDiffKind::Unstaged);
        let paths = super::git_panel_context_paths(&state, &target);
        assert_eq!(paths.same_kind, vec!["changed.rs".to_string()]);
        assert_eq!(
            paths.stageable,
            vec!["changed.rs".to_string(), "new.txt".to_string()]
        );

        // 右键目标不在选择集：两组都退化为目标单文件。
        let outside = sel("other.rs", GitDiffKind::Unstaged);
        let paths = super::git_panel_context_paths(&state, &outside);
        assert_eq!(paths.same_kind, vec!["other.rs".to_string()]);
        assert_eq!(paths.stageable, vec!["other.rs".to_string()]);
    }

    #[test]
    fn git_panel_entry_state_key_separates_staged_and_worktree_rows() {
        assert_ne!(
            super::git_panel_entry_state_key("src/git_ops.rs", true),
            super::git_panel_entry_state_key("src/git_ops.rs", false)
        );
        assert_eq!(
            super::git_panel_entry_state_key("src/git_ops.rs", false),
            super::git_panel_entry_state_key("src/git_ops.rs", false)
        );
    }

    #[test]
    fn git_panel_entry_tooltip_uses_full_status_and_path() {
        let entry = nexshell::git_ops::GitFileEntry {
            path: "__pycache__/提取编码表局点.cpython-312.pyc".into(),
            original_path: None,
            index_status: '?',
            worktree_status: '?',
            stage: nexshell::git_ops::GitFileStage::Unstaged,
        };

        assert_eq!(
            super::git_panel_entry_tooltip_text(&entry, "??"),
            "??  __pycache__/提取编码表局点.cpython-312.pyc"
        );
        assert_ne!(
            super::git_panel_entry_tooltip_position_id(&entry.path, true),
            super::git_panel_entry_tooltip_position_id(&entry.path, false)
        );
    }

    #[test]
    fn git_commit_editor_uses_wrapping_autogrow_layout_bounds() {
        let options = super::git_commit_editor_options(13.0);

        assert_eq!(options.text.font_size_override, Some(13.0));
        assert!(options.autogrow);
        assert!(options.soft_wrap);
        assert!(options.placeholder_soft_wrap);
        assert!(!options.single_line);
        assert!(crate::GIT_COMMIT_EDITOR_MIN_HEIGHT > 32.0);
        assert!(crate::GIT_COMMIT_EDITOR_MAX_HEIGHT >= crate::GIT_COMMIT_EDITOR_MIN_HEIGHT * 2.0);
    }

    #[test]
    fn git_push_busy_label_animates_without_localized_ellipsis() {
        assert_eq!(
            super::animated_push_busy_label("Pushing\u{2026}", 0),
            "Pushing."
        );
        assert_eq!(
            super::animated_push_busy_label("Pushing\u{2026}", 1),
            "Pushing.."
        );
        assert_eq!(
            super::animated_push_busy_label("Pushing\u{2026}", 2),
            "Pushing..."
        );
        assert_eq!(
            super::animated_push_busy_label("正在推送\u{2026}", 5),
            "正在推送..."
        );
    }

    #[test]
    fn git_ssh_host_key_prompt_info_exposes_fingerprint_and_raw_prompt() {
        let prompt = nexshell::git_ops::SshHostKeyPrompt {
            message: "The authenticity of host 'example.com' can't be established.".into(),
            host: Some("example.com".into()),
            fingerprint: Some("SHA256:abc".into()),
        };

        let info = super::git_ssh_host_key_prompt_info(&prompt);

        assert!(info.contains("SHA256:abc"));
        assert!(info.contains("The authenticity of host"));
    }

    #[test]
    fn git_commit_row_visual_hover_tracks_actual_mouse_position() {
        assert!(super::git_commit_row_visual_hovered(false, true));
        assert!(!super::git_commit_row_visual_hovered(true, false));
    }

    #[test]
    fn git_commit_hover_target_survives_detail_hover_and_stale_row_exit() {
        assert_eq!(
            super::git_commit_hover_target_after_event(None, "0ebff5b", true),
            Some("0ebff5b".to_string())
        );
        assert_eq!(
            super::git_commit_hover_target_after_event(Some("0ebff5b"), "0ebff5b", false),
            Some("0ebff5b".to_string())
        );
        assert_eq!(
            super::git_commit_hover_target_after_event(Some("d77fb40"), "0ebff5b", false),
            Some("d77fb40".to_string())
        );
        assert_eq!(
            super::git_commit_hover_target_after_event(Some("0ebff5b"), "0ebff5b", false),
            Some("0ebff5b".to_string())
        );
    }

    #[test]
    fn git_commit_hover_target_waits_before_clearing_between_row_and_detail() {
        let now = Instant::now();
        let (target, clear_after) =
            super::git_commit_hover_target_after_motion(Some("0ebff5b"), false, false, None, now);
        assert_eq!(target, Some("0ebff5b".to_string()));
        assert!(clear_after.is_some_and(|deadline| deadline > now));
        assert_eq!(
            clear_after.unwrap().duration_since(now),
            Duration::from_millis(300)
        );

        let (target, clear_after) = super::git_commit_hover_target_after_motion(
            Some("0ebff5b"),
            false,
            false,
            Some(now - Duration::from_millis(1)),
            now,
        );
        assert_eq!(target, None);
        assert_eq!(clear_after, None);

        let (target, clear_after) = super::git_commit_hover_target_after_motion(
            Some("0ebff5b"),
            true,
            false,
            Some(now + Duration::from_millis(100)),
            now,
        );
        assert_eq!(target, Some("0ebff5b".to_string()));
        assert_eq!(clear_after, None);
    }

    #[test]
    fn git_commit_detail_target_does_not_pin_click_selection_after_hover_leaves() {
        assert_eq!(super::git_commit_detail_target(Some("clicked"), None), None);
        assert_eq!(
            super::git_commit_detail_target(Some("clicked"), Some("hovered")),
            Some("hovered")
        );
        assert_eq!(super::git_commit_detail_target(None, None), None);
    }

    #[test]
    fn git_history_scroll_load_more_triggers_only_near_bottom_while_scrolling_down() {
        assert!(!super::git_history_scroll_should_load_more(
            100.0, 120.0, 24.0, 20, 180.0,
        ));
        assert!(super::git_history_scroll_should_load_more(
            470.0, 470.0, -24.0, 20, 180.0,
        ));
        assert!(super::git_history_scroll_should_load_more(
            360.0, 455.0, -24.0, 20, 180.0,
        ));
    }
}
