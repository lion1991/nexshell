//! Git 提交详情卡渲染（ADR 0004：从 git_panel_view_helpers 抽出）。
//!
//! 详情卡 = 作者/时间 + 标题 + 正文(滚动) + 统计 + 文件列表(滚动) + SHA/复制。
//! 本文件只放无 &self 自由函数；入口 render_git_commit_detail_card 被
//! root_view/git_panel_section/history_section.rs 调用。文件行布局参照 warp
//! app/src/code_review/git_dialog（render_file_changes_box / render_file_list）。

use chrono::{Datelike, Timelike};
use warpui::elements::{
    Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, DispatchEventResult, DropShadow, Empty, EventHandler, Expanded, Fill, Flex,
    Hoverable, Icon, MainAxisSize, MouseStateHandle, ParentElement, Radius, ScrollbarWidth, Text,
};
use warpui::fonts;
use warpui::Element;

use nexshell::git_ops::{CommitFileChange, CommitRow};

use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{
    GIT_COMMIT_DETAIL_BODY_MAX_HEIGHT, GIT_COMMIT_DETAIL_CARD_WIDTH,
    GIT_COMMIT_DETAIL_FILES_MAX_HEIGHT, ICON_PATH_COPY,
};

fn format_git_commit_authored_at_for_detail(authored_at: &str) -> String {
    format_git_commit_authored_at_for_detail_at(authored_at, chrono::Local::now().fixed_offset())
}

fn format_git_commit_authored_at_for_detail_at(
    authored_at: &str,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> String {
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(authored_at) else {
        return String::new();
    };
    let authored = parsed.with_timezone(now.offset());
    let elapsed = now.signed_duration_since(authored);
    let relative = if elapsed.num_seconds() < 60 {
        "just now".to_string()
    } else if elapsed.num_minutes() < 60 {
        let minutes = elapsed.num_minutes();
        format!(
            "{minutes} minute{} ago",
            if minutes == 1 { "" } else { "s" }
        )
    } else if elapsed.num_hours() < 24 {
        let hours = elapsed.num_hours();
        format!("{hours} hour{} ago", if hours == 1 { "" } else { "s" })
    } else {
        let days = elapsed.num_days();
        format!("{days} day{} ago", if days == 1 { "" } else { "s" })
    };
    let month = match authored.month() {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        _ => "Dec",
    };
    let hour = authored.hour();
    let hour12 = match hour % 12 {
        0 => 12,
        h => h,
    };
    let am_pm = if hour < 12 { "AM" } else { "PM" };
    format!(
        "{relative} ({month} {}, {} at {hour12}:{:02} {am_pm})",
        authored.day(),
        authored.year(),
        authored.minute()
    )
}

pub(crate) fn git_commit_stat_label(files_changed: u32, insertions: u32, deletions: u32) -> String {
    let mut parts = vec![format!(
        "{files_changed} file{} changed",
        if files_changed == 1 { "" } else { "s" }
    )];
    if insertions > 0 {
        parts.push(format!(
            "{insertions} insertion{}(+)",
            if insertions == 1 { "" } else { "s" }
        ));
    }
    if deletions > 0 {
        parts.push(format!(
            "{deletions} deletion{}(-)",
            if deletions == 1 { "" } else { "s" }
        ));
    }
    parts.join(", ")
}

fn git_commit_copy_payload(short_sha: &str, full_sha: &str) -> String {
    let full_sha = full_sha.trim();
    if full_sha.is_empty() {
        short_sha.trim().to_string()
    } else {
        full_sha.to_string()
    }
}

// warp git_dialog::split_file_path：按最后一个 '/' 切分，目录段保留末尾斜杠。
fn split_file_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(idx) => (&path[idx + 1..], &path[..idx + 1]),
        None => (path, ""),
    }
}

// 文件行两段着色：返回 (文件名, 次要信息=目录[+改名来源])。
fn git_commit_file_change_display(file: &CommitFileChange) -> (String, String) {
    let (filename, directory) = split_file_path(&file.path);
    let secondary = match file.original_path.as_deref() {
        Some(original) if directory.is_empty() => format!("← {original}"),
        Some(original) => format!("{directory}  ← {original}"),
        None => directory.to_string(),
    };
    (filename.to_string(), secondary)
}

// 把某区段套 ClippedScrollable + max_height，避免详情卡无限拉高超窗被裁。warp render_file_changes_box。
fn scroll_capped(
    content: Box<dyn Element>,
    scroll_state: ClippedScrollStateHandle,
    max_height: f32,
    colors: HostOverviewColors,
) -> Box<dyn Element> {
    ConstrainedBox::new(
        ClippedScrollable::vertical(
            scroll_state,
            content,
            ScrollbarWidth::Custom(4.0),
            Fill::Solid(colors.text_muted),
            Fill::Solid(colors.text_primary),
            Fill::None,
        )
        .with_overlayed_scrollbar()
        .finish(),
    )
    .with_max_height(max_height)
    .finish()
}

pub(crate) fn render_git_commit_detail_card(
    commit: &CommitRow,
    colors: HostOverviewColors,
    ui_font: fonts::FamilyId,
    copy_state: MouseStateHandle,
    files_scroll_state: ClippedScrollStateHandle,
    body_scroll_state: ClippedScrollStateHandle,
) -> Box<dyn Element> {
    let author = if commit.author.trim().is_empty() {
        "unknown".to_string()
    } else {
        commit.author.clone()
    };
    let time_label = format_git_commit_authored_at_for_detail(&commit.authored_at);
    let mut header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Text::new_inline(author, ui_font, 11.0)
                .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
                .with_color(colors.cpu_accent)
                .finish(),
        );
    if !time_label.is_empty() {
        header.add_child(
            Text::new_inline(format!(", {time_label}"), ui_font, 11.0)
                .with_color(colors.text_muted)
                .finish(),
        );
    }

    let mut content = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header.finish())
        .with_child(
            Container::new(
                Text::new(commit.summary.clone(), ui_font, 12.0)
                    .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
                    .with_color(colors.text_primary)
                    .finish(),
            )
            .with_padding_top(10.0)
            .finish(),
        );
    if !commit.body.trim().is_empty() {
        // 长正文同样套滚动，避免整卡超窗被裁。
        let body = Text::new(commit.body.clone(), ui_font, 12.0)
            .with_color(colors.text_primary)
            .with_line_height_ratio(1.25)
            .finish();
        content.add_child(
            Container::new(scroll_capped(
                body,
                body_scroll_state,
                GIT_COMMIT_DETAIL_BODY_MAX_HEIGHT,
                colors,
            ))
            .with_padding_top(10.0)
            .finish(),
        );
    }

    let divider = ConstrainedBox::new(
        Container::new(Empty::new().finish())
            .with_background_color(colors.panel_border)
            .finish(),
    )
    .with_height(1.0)
    .finish();
    let stat_text =
        git_commit_stat_label(commit.files_changed, commit.insertions, commit.deletions);
    let stats = Text::new_inline(stat_text, ui_font, 11.0)
        .with_color(colors.text_primary)
        .finish();
    let sha = Text::new_inline(commit.sha.clone(), ui_font, 11.0)
        .with_color(colors.cpu_accent)
        .finish();
    let copy_payload = git_commit_copy_payload(&commit.sha, &commit.full_sha);
    let copy_button = Hoverable::new(copy_state, move |mouse| {
        let icon_color = if mouse.is_hovered() {
            colors.text_primary
        } else {
            colors.cpu_accent
        };
        let bg = if mouse.is_hovered() {
            colors.metric_track
        } else {
            nexshell::design_tokens::TRANSPARENT
        };
        ConstrainedBox::new(
            Container::new(
                ConstrainedBox::new(Icon::new(ICON_PATH_COPY, icon_color).finish())
                    .with_width(12.0)
                    .with_height(12.0)
                    .finish(),
            )
            .with_uniform_padding(3.0)
            .with_background_color(bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .finish(),
        )
        .with_width(18.0)
        .with_height(18.0)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::GitCommitCopySha(copy_payload.clone()));
    })
    .finish();
    let footer = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(sha)
        .with_child(Container::new(copy_button).with_padding_left(4.0).finish())
        .finish();

    content.add_child(Container::new(divider).with_padding_top(12.0).finish());
    content.add_child(Container::new(stats).with_padding_top(8.0).finish());
    if !commit.file_changes.is_empty() {
        let files_title = Text::new_inline(
            format!(
                "{} ({})",
                rust_i18n::t!("git_panel_commit_files"),
                commit.file_changes.len()
            ),
            ui_font,
            11.0,
        )
        .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
        .with_color(colors.text_muted)
        .finish();
        let mut files = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for file in &commit.file_changes {
            files.add_child(render_git_commit_file_change_row(file, colors, ui_font));
        }
        let list = Container::new(files.finish())
            .with_padding_bottom(4.0)
            .finish();
        content.add_child(Container::new(files_title).with_padding_top(10.0).finish());
        content.add_child(
            Container::new(scroll_capped(
                list,
                files_scroll_state,
                GIT_COMMIT_DETAIL_FILES_MAX_HEIGHT,
                colors,
            ))
            .with_padding_top(2.0)
            .finish(),
        );
    }
    content.add_child(Container::new(footer).with_padding_top(10.0).finish());

    let card = ConstrainedBox::new(
        Container::new(content.finish())
            .with_uniform_padding(10.0)
            .with_background_color(colors.panel_bg)
            .with_border(Border::all(1.0).with_border_color(colors.panel_border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.0)))
            .with_drop_shadow(DropShadow::default())
            .finish(),
    )
    .with_width(GIT_COMMIT_DETAIL_CARD_WIDTH)
    .finish();
    // 浮层吃掉自身范围内的滚轮/拖动，防穿透到下层终端。warp overlay 同款。
    EventHandler::new(card)
        .with_always_handle()
        .on_scroll_wheel(|_, _, _, _| DispatchEventResult::StopPropagation)
        .on_left_mouse_down(|_, _, _| DispatchEventResult::StopPropagation)
        .on_mouse_dragged(|_, _, _| DispatchEventResult::StopPropagation)
        .finish()
}

// warp render_file_list 同款单行：文件名(主色,始终可见) + 目录(灰,溢出截断) + 右侧 绿+/红-。
fn render_git_commit_file_change_row(
    file: &CommitFileChange,
    colors: HostOverviewColors,
    ui_font: fonts::FamilyId,
) -> Box<dyn Element> {
    let (filename, secondary) = git_commit_file_change_display(file);
    let mut name_row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Text::new_inline(filename, ui_font, 11.0)
                .with_color(colors.text_primary)
                .finish(),
        );
    if !secondary.is_empty() {
        name_row.add_child(
            Container::new(
                Text::new_inline(secondary, ui_font, 11.0)
                    .with_color(colors.text_muted)
                    .finish(),
            )
            .with_padding_left(4.0)
            .finish(),
        );
    }

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(Expanded::new(1.0, name_row.finish()).finish());
    if let Some(stats) = git_commit_file_change_stats(file, colors, ui_font) {
        row.add_child(Container::new(stats).with_padding_left(8.0).finish());
    }
    Container::new(row.finish())
        .with_padding_top(3.0)
        .with_padding_bottom(3.0)
        .finish()
}

// +N(绿) -N(红)；二进制/无 numstat 返回 None 不显示。
fn git_commit_file_change_stats(
    file: &CommitFileChange,
    colors: HostOverviewColors,
    ui_font: fonts::FamilyId,
) -> Option<Box<dyn Element>> {
    let (insertions, deletions) = match (file.insertions, file.deletions) {
        (None, None) => return None,
        (insertions, deletions) => (insertions.unwrap_or(0), deletions.unwrap_or(0)),
    };
    let stats = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Text::new_inline(format!("+{insertions}"), ui_font, 11.0)
                .with_color(colors.download)
                .finish(),
        )
        .with_child(
            Container::new(
                Text::new_inline(format!("-{deletions}"), ui_font, 11.0)
                    .with_color(colors.upload)
                    .finish(),
            )
            .with_padding_left(4.0)
            .finish(),
        );
    Some(stats.finish())
}

#[cfg(test)]
mod tests {
    use super::{
        format_git_commit_authored_at_for_detail_at, git_commit_copy_payload,
        git_commit_file_change_display,
    };
    use nexshell::git_ops::CommitFileChange;

    fn file_change(path: &str, original: Option<&str>) -> CommitFileChange {
        CommitFileChange {
            path: path.to_string(),
            original_path: original.map(str::to_string),
            insertions: Some(1),
            deletions: Some(0),
        }
    }

    #[test]
    fn file_change_display_splits_filename_and_dir() {
        let (name, secondary) =
            git_commit_file_change_display(&file_change("src/root_view/mod.rs", None));
        assert_eq!(name, "mod.rs");
        assert_eq!(secondary, "src/root_view/");
        let (name, secondary) = git_commit_file_change_display(&file_change("README.md", None));
        assert_eq!(name, "README.md");
        assert_eq!(secondary, "");
    }

    #[test]
    fn file_change_display_marks_rename_source() {
        let (name, secondary) =
            git_commit_file_change_display(&file_change("src/new.rs", Some("src/old.rs")));
        assert_eq!(name, "new.rs");
        assert_eq!(secondary, "src/  ← src/old.rs");
        // 根目录文件改名：无目录段，secondary 仅来源箭头。
        let (name, secondary) =
            git_commit_file_change_display(&file_change("new.md", Some("old.md")));
        assert_eq!(name, "new.md");
        assert_eq!(secondary, "← old.md");
    }

    #[test]
    fn git_commit_copy_payload_prefers_full_sha() {
        assert_eq!(
            git_commit_copy_payload("746fe7e", "746fe7e9f2c4f2c0de0c0ffee123456789abcdef"),
            "746fe7e9f2c4f2c0de0c0ffee123456789abcdef"
        );
        assert_eq!(git_commit_copy_payload("746fe7e", ""), "746fe7e");
    }

    #[test]
    fn git_commit_detail_time_formats_relative_and_absolute_time() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-05-20T09:18:00-07:00").unwrap();
        assert_eq!(
            format_git_commit_authored_at_for_detail_at("2026-05-16T09:18:00-07:00", now),
            "4 days ago (May 16, 2026 at 9:18 AM)"
        );
    }
}
