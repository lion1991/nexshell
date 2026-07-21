//! git 面板文件列表虚拟化：行模型（算术定位，O(分组数)）+ 行构建自由函数。
//! 被 root_view/git_panel_section/status_section.rs 的 UniformList 调用；
//! build_items 闭包要求 'static，本文件函数只收 owned/Copy/Arc/Rc 参数，不借用 &self/&tab。

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pathfinder_geometry::vector::vec2f;

use crate::file_panel_view_helpers::render_file_panel_icon_button;
use crate::git_panel_view_helpers::{
    git_panel_diff_kind_for_entry, git_panel_entry_action_state_key, git_panel_entry_state_key,
    git_panel_entry_tooltip_position_id, git_panel_entry_tooltip_text,
};
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::{HostOverviewColors, UiColors};
use crate::{ICON_PATH_ARROW_DOWN, ICON_PATH_PLUS};
use nexshell::design_tokens::SemanticColors;
use nexshell::git_ops::{GitDiffSelection, GitFileEntry, GitStatusSnapshot};
use nexshell::git_panel::GitPanelSelectMode;
use warpui::color::ColorU;
use warpui::elements::{
    ChildAnchor, Clipped, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    DispatchEventResult, Empty, EventHandler, Expanded, Flex, Hoverable, MouseState,
    MouseStateHandle, OffsetPositioning, Padding, ParentElement, PositionedElementAnchor,
    PositionedElementOffsetBounds, Radius, SavePosition, Stack, Text,
};
use warpui::fonts;
use warpui::Element;

/// UniformList 要求所有行等高；每行外层钉死这个高度。
pub(crate) const GIT_PANEL_ROW_HEIGHT: f32 = 22.0;

/// 四个分组固定顺序：staged → unstaged → untracked → unmerged，与旧实现一致。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitPanelSectionKind {
    Staged,
    Unstaged,
    Untracked,
    Unmerged,
}

impl GitPanelSectionKind {
    fn entries(self, status: &GitStatusSnapshot) -> &[GitFileEntry] {
        match self {
            Self::Staged => &status.staged,
            Self::Unstaged => &status.unstaged,
            Self::Untracked => &status.untracked,
            Self::Unmerged => &status.unmerged,
        }
    }

    pub(crate) fn is_staged(self) -> bool {
        matches!(self, Self::Staged)
    }
}

/// 单个分组的展示元数据——只存标题/开关/条目数，不拷贝条目本身（避免 O(n) 行 Vec）。
pub(crate) struct GitPanelRowSection {
    pub kind: GitPanelSectionKind,
    pub title: String,
    pub show_stage_all: bool,
    pub discard_enabled: bool,
    pub entry_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitPanelRowKind {
    SectionHeader(usize),
    Entry { section: usize, index: usize },
    Gap,
}

/// 从快照提取非空分组的元数据；顺序固定，空分组不占行。
pub(crate) fn git_panel_row_sections(status: &GitStatusSnapshot) -> Vec<GitPanelRowSection> {
    let mut sections = Vec::new();
    if !status.staged.is_empty() {
        sections.push(GitPanelRowSection {
            kind: GitPanelSectionKind::Staged,
            title: rust_i18n::t!("git_panel_section_staged").to_string(),
            show_stage_all: false,
            discard_enabled: false,
            entry_count: status.staged.len(),
        });
    }
    if !status.unstaged.is_empty() {
        sections.push(GitPanelRowSection {
            kind: GitPanelSectionKind::Unstaged,
            title: rust_i18n::t!("git_panel_section_changes").to_string(),
            show_stage_all: true,
            discard_enabled: true,
            entry_count: status.unstaged.len(),
        });
    }
    if !status.untracked.is_empty() {
        sections.push(GitPanelRowSection {
            kind: GitPanelSectionKind::Untracked,
            title: rust_i18n::t!("git_panel_section_untracked").to_string(),
            show_stage_all: false,
            discard_enabled: false,
            entry_count: status.untracked.len(),
        });
    }
    if !status.unmerged.is_empty() {
        sections.push(GitPanelRowSection {
            kind: GitPanelSectionKind::Unmerged,
            title: rust_i18n::t!("git_panel_section_unmerged").to_string(),
            show_stage_all: false,
            discard_enabled: false,
            entry_count: status.unmerged.len(),
        });
    }
    sections
}

/// 总行数 = 每个分组(1 header + entries) 之和 + 分组间 gap(末尾不加)。
pub(crate) fn git_panel_total_rows(sections: &[GitPanelRowSection]) -> usize {
    if sections.is_empty() {
        return 0;
    }
    let content_rows: usize = sections.iter().map(|s| 1 + s.entry_count).sum();
    content_rows + (sections.len() - 1)
}

/// 按算术定位第 index 行属于哪个分组/条目/gap，O(分组数)，不构建行 Vec。
pub(crate) fn git_panel_row_at(
    sections: &[GitPanelRowSection],
    index: usize,
) -> Option<GitPanelRowKind> {
    let mut cursor = 0usize;
    for (section_idx, section) in sections.iter().enumerate() {
        let section_rows = 1 + section.entry_count;
        if index < cursor + section_rows {
            let offset = index - cursor;
            return Some(if offset == 0 {
                GitPanelRowKind::SectionHeader(section_idx)
            } else {
                GitPanelRowKind::Entry {
                    section: section_idx,
                    index: offset - 1,
                }
            });
        }
        cursor += section_rows;
        if section_idx + 1 < sections.len() {
            if index == cursor {
                return Some(GitPanelRowKind::Gap);
            }
            cursor += 1;
        }
    }
    None
}

/// 取第 section 个分组第 index 条目（从 Arc 快照按位置索引，只在可见行构建时调用）。
pub(crate) fn git_panel_entry_at<'a>(
    sections: &[GitPanelRowSection],
    status: &'a GitStatusSnapshot,
    section: usize,
    index: usize,
) -> Option<&'a GitFileEntry> {
    let section = sections.get(section)?;
    section.kind.entries(status).get(index)
}

/// 状态字母语义色：M→warn A→ok D→danger R/C→info 其余→muted，取变更主字母。
pub(crate) fn git_status_letter_color(
    entry: &GitFileEntry,
    semantic: SemanticColors,
    muted: ColorU,
) -> ColorU {
    // porcelain v2 无变更位是 '.'（非空格）。
    let letter = if !matches!(entry.index_status, ' ' | '.' | '?') {
        entry.index_status
    } else {
        entry.worktree_status
    };
    match letter {
        'M' => semantic.warn,
        'A' => semantic.ok,
        'D' => semantic.danger,
        'R' | 'C' => semantic.info,
        _ => muted,
    }
}

/// 分组标题行：粗体标题 + "(count)"，unstaged 组带"全部暂存"按钮。
pub(crate) fn build_git_panel_header_row(
    section: &GitPanelRowSection,
    stage_all_state: MouseStateHandle,
    ui_font: fonts::FamilyId,
    text_muted: ColorU,
    accent_text: ColorU,
    hover_bg: ColorU,
) -> Box<dyn Element> {
    let header = Text::new_inline(
        format!("{} ({})", section.title, section.entry_count),
        ui_font,
        11.0,
    )
    .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
    .with_color(text_muted)
    .finish();
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(Expanded::new(1.0, header).finish());
    if section.show_stage_all {
        let label = rust_i18n::t!("git_panel_stage_all").to_string();
        let button = Hoverable::new(stage_all_state, move |mouse| {
            let label = Text::new_inline(label.clone(), ui_font, 10.0)
                .with_color(accent_text)
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
            ctx.dispatch_typed_action(TerminalGridAction::GitPanelStageAll);
        })
        .finish();
        row = row.with_child(button);
    }
    ConstrainedBox::new(row.finish())
        .with_height(GIT_PANEL_ROW_HEIGHT)
        .finish()
}

/// 空行占位（分组间隔），钉同一行高以满足 UniformList 等高约束。
pub(crate) fn build_git_panel_gap_row() -> Box<dyn Element> {
    ConstrainedBox::new(Empty::new().finish())
        .with_height(GIT_PANEL_ROW_HEIGHT)
        .finish()
}

/// 单个文件行：状态字母 + 路径（hover 500ms 弹 tooltip）+ 行尾 stage/unstage 图标按钮 + 右键菜单。
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_git_panel_entry_row(
    entry: GitFileEntry,
    is_staged: bool,
    discard_enabled: bool,
    selected_entries: Arc<BTreeSet<GitDiffSelection>>,
    entry_states: Rc<RefCell<HashMap<String, MouseStateHandle>>>,
    entry_action_states: Rc<RefCell<HashMap<String, MouseStateHandle>>>,
    tab_id: String,
    ui_font: fonts::FamilyId,
    overview: HostOverviewColors,
    semantic: SemanticColors,
    chrome: UiColors,
) -> Box<dyn Element> {
    let xy = format!("{}{}", entry.index_status, entry.worktree_status);
    let label_text = git_panel_entry_tooltip_text(&entry, &xy);
    let diff_kind = git_panel_diff_kind_for_entry(&entry, is_staged);
    let diff_selection = GitDiffSelection {
        path: entry.path.clone(),
        kind: diff_kind,
    };
    // 高亮单一来源：只认 selected_entries，不 OR 焦点 selected_diff，避免右键/取消选中后残留高亮（对齐 Warp block 选择）
    let is_selected = selected_entries.contains(&diff_selection);

    let path_for_action = entry.path.clone();
    let action = if is_staged {
        TerminalGridAction::GitPanelUnstage(path_for_action)
    } else {
        TerminalGridAction::GitPanelStage(path_for_action)
    };

    let state = entry_states
        .borrow_mut()
        .entry(git_panel_entry_state_key(&entry.path, is_staged))
        .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
        .clone();
    let action_state = entry_action_states
        .borrow_mut()
        .entry(git_panel_entry_action_state_key(&entry.path, is_staged))
        .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
        .clone();

    let text_color = overview.text_primary;
    let hover_bg = chrome.tab_bg_hover;
    let selected_bg = chrome.selection_bg;
    let tooltip_bg = chrome.tooltip_bg;
    let tooltip_text_color = chrome.tooltip_text;
    // 状态字母语义上色：xy 前缀单独染色，路径部分维持主文字色。
    let status_color = git_status_letter_color(&entry, semantic, overview.text_muted);
    let status_prefix = xy.clone();
    let path_rest = label_text[xy.len()..].to_string();
    let tooltip_label = label_text.clone();
    let tooltip_position_id = git_panel_entry_tooltip_position_id(&entry.path, is_staged);
    let select_path = entry.path.clone();

    let label = Hoverable::new(state, move |mouse| {
        let label_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(status_prefix.clone(), ui_font, 11.0)
                    .with_color(status_color)
                    .finish(),
            )
            .with_child(
                Text::new_inline(path_rest.clone(), ui_font, 11.0)
                    .with_color(text_color)
                    .finish(),
            )
            .finish();
        let text =
            SavePosition::new(Clipped::new(label_row).finish(), &tooltip_position_id).finish();
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

    let action_icon = if is_staged {
        ICON_PATH_ARROW_DOWN
    } else {
        ICON_PATH_PLUS
    };
    let action_button = render_file_panel_icon_button(
        action_state,
        action_icon,
        chrome.icon_color_inactive,
        chrome.icon_button_hover_bg,
        action,
    );

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

    let row = EventHandler::new(row)
        .on_right_mouse_down(move |ctx, _app, position| {
            ctx.dispatch_typed_action(TerminalGridAction::GitPanelShowContextMenu {
                tab_id: tab_id.clone(),
                path: path_for_ctx.clone(),
                kind: diff_kind,
                discard_enabled,
                position,
            });
            DispatchEventResult::StopPropagation
        })
        .finish();

    ConstrainedBox::new(row)
        .with_height(GIT_PANEL_ROW_HEIGHT)
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(kind: GitPanelSectionKind, entry_count: usize) -> GitPanelRowSection {
        GitPanelRowSection {
            kind,
            title: "T".into(),
            show_stage_all: false,
            discard_enabled: false,
            entry_count,
        }
    }

    #[test]
    fn total_rows_sums_headers_entries_and_inter_section_gaps() {
        // 空分组列表：0 行。
        assert_eq!(git_panel_total_rows(&[]), 0);
        // 单分组：1 header + 2 entries，无 gap。
        let single = vec![section(GitPanelSectionKind::Staged, 2)];
        assert_eq!(git_panel_total_rows(&single), 3);
        // 两分组：各 1 header + entries，中间 1 gap。
        let two = vec![
            section(GitPanelSectionKind::Staged, 2),
            section(GitPanelSectionKind::Unstaged, 3),
        ];
        // (1+2) + (1+3) + 1 gap = 8
        assert_eq!(git_panel_total_rows(&two), 8);
    }

    #[test]
    fn row_at_locates_header_entry_gap_and_out_of_range() {
        let sections = vec![
            section(GitPanelSectionKind::Staged, 2),
            section(GitPanelSectionKind::Unstaged, 1),
        ];
        // staged: header(0), entry0(1), entry1(2), gap(3)
        // unstaged: header(4), entry0(5)
        assert_eq!(
            git_panel_row_at(&sections, 0),
            Some(GitPanelRowKind::SectionHeader(0))
        );
        assert_eq!(
            git_panel_row_at(&sections, 1),
            Some(GitPanelRowKind::Entry {
                section: 0,
                index: 0
            })
        );
        assert_eq!(
            git_panel_row_at(&sections, 2),
            Some(GitPanelRowKind::Entry {
                section: 0,
                index: 1
            })
        );
        assert_eq!(git_panel_row_at(&sections, 3), Some(GitPanelRowKind::Gap));
        assert_eq!(
            git_panel_row_at(&sections, 4),
            Some(GitPanelRowKind::SectionHeader(1))
        );
        assert_eq!(
            git_panel_row_at(&sections, 5),
            Some(GitPanelRowKind::Entry {
                section: 1,
                index: 0
            })
        );
        assert_eq!(git_panel_row_at(&sections, 6), None);
        assert_eq!(git_panel_total_rows(&sections), 6);
    }

    #[test]
    fn row_sections_extract_only_non_empty_groups_in_fixed_order() {
        let status = GitStatusSnapshot {
            unstaged: vec![GitFileEntry {
                path: "a.rs".into(),
                original_path: None,
                index_status: '.',
                worktree_status: 'M',
                stage: nexshell::git_ops::GitFileStage::Unstaged,
            }],
            untracked: vec![GitFileEntry {
                path: "b.rs".into(),
                original_path: None,
                index_status: '?',
                worktree_status: '?',
                stage: nexshell::git_ops::GitFileStage::Unstaged,
            }],
            ..Default::default()
        };
        let sections = git_panel_row_sections(&status);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].kind, GitPanelSectionKind::Unstaged);
        assert!(sections[0].show_stage_all);
        assert_eq!(sections[1].kind, GitPanelSectionKind::Untracked);
        assert!(!sections[1].show_stage_all);

        let entry = git_panel_entry_at(&sections, &status, 0, 0).unwrap();
        assert_eq!(entry.path, "a.rs");
        assert!(git_panel_entry_at(&sections, &status, 0, 1).is_none());
    }
}
