// body section — RootView 的文件面板内容渲染：header / 列表 / 远程目录 / 传输任务区。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。
// 入口由 mod.rs::render_file_panel 调用（render_file_panel_header / render_local_file_panel_header
// / render_local_file_panel_body / render_file_panel_body / render_file_panel_transfers）。

use std::sync::{Arc, Mutex};

use crate::file_panel_view_helpers::{
    file_panel_message, file_panel_name_tooltip, file_panel_reconnect_message, format_remote_mtime,
    render_file_panel_icon_button,
};
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::{HostOverviewColors, UiColors};
use crate::{
    RootView, TerminalSessionKind, TerminalSessionTab, ICON_PATH_ARROW_UP, ICON_PATH_CLOSE,
    ICON_PATH_FOLDER, ICON_PATH_LINK, ICON_PATH_LIST_VIEW, ICON_PATH_REFRESH, ICON_PATH_UPLOAD,
};
use nexshell::file_panel::{
    flatten_file_panel_tree, FilePanelSelectMode, FilePanelTreeRow, TransferRow, TransferStatus,
};
use nexshell::host_overview::format_bytes_short;
use nexshell::sftp_ops::EntryKind;
use warpui::elements::{
    Align, Border, Clipped, ClippedScrollable, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, DispatchEventResult, Empty, EventHandler, Expanded, Fill, Flex, Hoverable,
    Icon, MainAxisSize, MouseState, ParentElement, Radius, SavePosition, ScrollbarWidth,
    Shrinkable, Text,
};
use warpui::fonts;
use warpui::Element;

impl RootView {
    pub(in crate::root_view) fn render_file_panel_header(
        &self,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let uc = self.ui_colors();
        let cwd = tab.file_panel_state.cwd.clone();
        let up = render_file_panel_icon_button(
            tab.file_panel_up_state.clone(),
            ICON_PATH_ARROW_UP,
            uc.icon_color_inactive,
            uc.icon_button_hover_bg,
            TerminalGridAction::FilePanelGoUp,
        );
        let upload = render_file_panel_icon_button(
            tab.file_panel_upload_state.clone(),
            ICON_PATH_UPLOAD,
            uc.icon_color_inactive,
            uc.icon_button_hover_bg,
            TerminalGridAction::FilePanelOpenUploadDialog,
        );
        let refresh = render_file_panel_icon_button(
            tab.file_panel_refresh_state.clone(),
            ICON_PATH_REFRESH,
            uc.icon_color_inactive,
            uc.icon_button_hover_bg,
            TerminalGridAction::FilePanelRefresh,
        );

        let title = Text::new_inline("文件", self.ui_font, 12.0)
            .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
            .with_color(colors.text_primary)
            .finish();
        let path_text = Clipped::new(
            Text::new_inline(cwd, self.ui_font, 11.0)
                .with_color(colors.text_muted)
                .finish(),
        )
        .finish();

        let path_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(up)
            .with_child(
                Expanded::new(
                    1.0,
                    Container::new(path_text)
                        .with_padding_left(6.0)
                        .with_padding_right(6.0)
                        .finish(),
                )
                .finish(),
            )
            .with_child(upload)
            .with_child(refresh)
            .finish();

        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(title)
            .with_child(Container::new(path_row).with_padding_top(8.0).finish())
            .finish()
    }

    pub(in crate::root_view) fn render_local_file_panel_header(
        &self,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let uc = self.ui_colors();
        // 本地面板：上级目录按钮（替代上传——上传是 SFTP 远程概念，本地无意义）。
        // 向上浏览会设 follow_cwd=false，配合右键「切换到终端目录」切回终端 cwd。
        let up = render_file_panel_icon_button(
            tab.file_panel_up_state.clone(),
            ICON_PATH_ARROW_UP,
            uc.icon_color_inactive,
            uc.icon_button_hover_bg,
            TerminalGridAction::FilePanelGoUp,
        );
        let refresh = render_file_panel_icon_button(
            tab.file_panel_refresh_state.clone(),
            ICON_PATH_REFRESH,
            uc.icon_color_inactive,
            uc.icon_button_hover_bg,
            TerminalGridAction::FilePanelRefresh,
        );

        let title = Text::new_inline("Project explorer", self.ui_font, 12.0)
            .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
            .with_color(colors.text_primary)
            .finish();
        let path_text = Clipped::new(
            Text::new_inline(tab.file_panel_state.cwd.clone(), self.ui_font, 10.0)
                .with_color(colors.text_muted)
                .finish(),
        )
        .finish();

        let top = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(1.0, title).finish())
            .with_child(up)
            .with_child(refresh)
            .finish();

        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(top)
            .with_child(Container::new(path_text).with_padding_top(4.0).finish())
            .finish()
    }

    pub(in crate::root_view) fn render_local_file_panel_body(
        &self,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let state = &tab.file_panel_state;
        if let Some(err) = state.error.as_ref() {
            return file_panel_message(err, self.ui_font, colors.warning);
        }
        let rows = flatten_file_panel_tree(state);
        if state.loading && rows.is_empty() {
            return file_panel_message("加载中...", self.ui_font, colors.text_muted);
        }
        if rows.is_empty() {
            return file_panel_message("（空目录）", self.ui_font, colors.text_muted);
        }

        let uc = self.ui_colors();
        let mut col = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for row in &rows {
            col.add_child(self.render_local_file_panel_tree_row(tab, row, colors, &uc));
        }

        let scrollable = ClippedScrollable::vertical(
            tab.file_panel_scroll_state.clone(),
            col.finish(),
            ScrollbarWidth::Custom(4.0),
            Fill::Solid(colors.text_muted),
            Fill::Solid(colors.text_primary),
            Fill::None,
        )
        .with_overlayed_scrollbar()
        .finish();
        let with_blank_ctx = EventHandler::new(scrollable)
            .on_right_mouse_down(|ctx, _app, position| {
                ctx.dispatch_typed_action(TerminalGridAction::FilePanelShowContextMenu {
                    name: None,
                    is_dir: false,
                    position,
                });
                DispatchEventResult::StopPropagation
            })
            .finish();
        Container::new(with_blank_ctx)
            .with_padding_top(8.0)
            .finish()
    }

    fn render_local_file_panel_tree_row(
        &self,
        tab: &TerminalSessionTab,
        row: &FilePanelTreeRow,
        colors: &HostOverviewColors,
        uc: &UiColors,
    ) -> Box<dyn Element> {
        let path = row.path.clone();
        let is_dir = row.is_dir();
        let is_error = row.is_error();
        let is_selected = !is_error && tab.file_panel_state.selected_names.contains(&path);
        let icon_path = match row.kind {
            EntryKind::Dir => ICON_PATH_FOLDER,
            EntryKind::Symlink => ICON_PATH_LINK,
            _ => ICON_PATH_LIST_VIEW,
        };
        let icon_color = if is_error {
            colors.warning
        } else if is_dir {
            uc.icon_color_active
        } else {
            uc.icon_color_inactive
        };
        let chevron = if is_dir && !is_error {
            if row.is_loading {
                "..."
            } else if row.is_expanded {
                "▾"
            } else {
                "▸"
            }
        } else {
            ""
        }
        .to_string();
        let name = row.name.clone();
        let depth = row.depth;
        let size_text = if is_dir || is_error {
            String::new()
        } else {
            format_bytes_short(row.size)
        };
        let mtime_text = if is_error {
            String::new()
        } else {
            format_remote_mtime(row.modified)
        };
        let hover_bg = uc.icon_button_hover_bg;
        let selected_bg = uc.selection_pill_bg;
        let text_color = if is_error {
            colors.warning
        } else {
            colors.text_primary
        };
        let muted = colors.text_muted;
        let font = self.ui_font;
        // 悬浮显示完整名（长名截断时，review 反馈）：SavePosition 锚点 + hover 叠 tooltip。
        let name_pos_id = format!("fp-name::{path}");
        let tooltip_bg = uc.tooltip_bg;
        let tooltip_text = uc.tooltip_text;

        let row_builder = move |mouse: &MouseState| -> Box<dyn Element> {
            let mut row_flex = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_size(MainAxisSize::Max);
            if depth > 0 {
                row_flex.add_child(
                    ConstrainedBox::new(Empty::new().finish())
                        .with_width(depth as f32 * 16.0)
                        .finish(),
                );
            }
            let chevron_text = Text::new_inline(chevron.clone(), font, 12.0)
                .with_color(muted)
                .finish();
            row_flex.add_child(
                ConstrainedBox::new(Align::new(chevron_text).finish())
                    .with_width(16.0)
                    .finish(),
            );
            let icon = ConstrainedBox::new(Icon::new(icon_path, icon_color).finish())
                .with_width(15.0)
                .with_height(15.0)
                .finish();
            row_flex.add_child(Container::new(icon).with_padding_right(8.0).finish());
            let name_text = SavePosition::new(
                Clipped::new(
                    Text::new_inline(name.clone(), font, 12.0)
                        .with_color(text_color)
                        .finish(),
                )
                .finish(),
                &name_pos_id,
            )
            .finish();
            row_flex.add_child(Expanded::new(1.0, name_text).finish());
            if !mtime_text.is_empty() {
                row_flex.add_child(
                    Container::new(
                        Text::new_inline(mtime_text.clone(), font, 10.0)
                            .with_color(muted)
                            .finish(),
                    )
                    .with_padding_left(8.0)
                    .finish(),
                );
            }
            if !size_text.is_empty() {
                row_flex.add_child(
                    Container::new(
                        Text::new_inline(size_text.clone(), font, 10.0)
                            .with_color(muted)
                            .finish(),
                    )
                    .with_padding_left(8.0)
                    .finish(),
                );
            }

            let mut container = Container::new(row_flex.finish())
                .with_padding_left(4.0)
                .with_padding_right(4.0)
                .with_padding_top(4.0)
                .with_padding_bottom(4.0)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)));
            if is_selected {
                container = container.with_background_color(selected_bg);
            } else if !is_error && mouse.is_hovered() {
                container = container.with_background_color(hover_bg);
            }
            let base = container.finish();
            if !mouse.is_hovered() {
                return base;
            }
            file_panel_name_tooltip(
                base,
                &name_pos_id,
                name.clone(),
                font,
                tooltip_bg,
                tooltip_text,
            )
        };

        if is_error {
            return row_builder(&MouseState::default());
        }

        let state = tab
            .file_panel_entry_states
            .borrow_mut()
            .entry(path.clone())
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone();
        let path_for_click = path.clone();
        let path_for_ctx = path.clone();
        let mut hover = Hoverable::new(state, row_builder)
            .with_cursor(warpui::platform::Cursor::PointingHand)
            .on_click_with_modifiers(move |ctx, _, _, modifiers| {
                let mode = if modifiers.shift {
                    FilePanelSelectMode::Range
                } else if modifiers.cmd || modifiers.ctrl {
                    FilePanelSelectMode::Toggle
                } else {
                    FilePanelSelectMode::Replace
                };
                ctx.dispatch_typed_action(TerminalGridAction::FilePanelTreeItemClicked {
                    path: path_for_click.clone(),
                    is_dir,
                    mode,
                });
            });
        // 文件双击 → 内置只读查看器（二进制/超大由 handler 回退「用外部程序打开」）；
        // 目录双击在树里靠单击展开，不处理。编辑仍走右键「编辑」。
        if !is_dir {
            let path_for_dbl = path.clone();
            hover = hover.on_double_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::FilePanelOpenInCodeViewer {
                    path: path_for_dbl.clone(),
                });
            });
        }
        EventHandler::new(hover.finish())
            .on_right_mouse_down(move |ctx, _app, position| {
                ctx.dispatch_typed_action(TerminalGridAction::FilePanelShowContextMenu {
                    name: Some(path_for_ctx.clone()),
                    is_dir,
                    position,
                });
                DispatchEventResult::StopPropagation
            })
            .finish()
    }

    pub(in crate::root_view) fn render_file_panel_body(
        &self,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let state = &tab.file_panel_state;
        if !matches!(tab.kind, TerminalSessionKind::Local) && tab.ssh_handle.is_none() {
            return file_panel_message("等待 SSH 连接...", self.ui_font, colors.text_muted);
        }
        if let Some(err) = state.error.as_ref() {
            // 断线 → 可点击重连；其它错误保持纯文本。
            if !Self::terminal_tab_is_connected(tab) {
                if let Some(index) = self.terminal_tabs.iter().position(|t| t.id == tab.id) {
                    return file_panel_reconnect_message(err, self.ui_font, colors.warning, index);
                }
            }
            return file_panel_message(err, self.ui_font, colors.warning);
        }
        if state.loading && state.entries.is_empty() {
            return file_panel_message("加载中...", self.ui_font, colors.text_muted);
        }
        if state.entries.is_empty() {
            return file_panel_message("（空目录）", self.ui_font, colors.text_muted);
        }

        let uc = self.ui_colors();
        let mut col = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for entry in &state.entries {
            let row = self.render_file_panel_entry(tab, entry, &state.selected_names, colors, &uc);
            col.add_child(row);
        }
        // warp: 列表外面套 ClippedScrollable 提供纵向滚动条
        let scrollable = ClippedScrollable::vertical(
            tab.file_panel_scroll_state.clone(),
            col.finish(),
            ScrollbarWidth::Custom(4.0),
            Fill::Solid(colors.text_muted),
            Fill::Solid(colors.text_primary),
            Fill::None,
        )
        .with_overlayed_scrollbar()
        .finish();
        // 空白区域右键 → 不带 name 的 context menu（entry 自己 StopPropagation，不会冒泡到这里）
        let with_blank_ctx = EventHandler::new(scrollable)
            .on_right_mouse_down(|ctx, _app, position| {
                ctx.dispatch_typed_action(TerminalGridAction::FilePanelShowContextMenu {
                    name: None,
                    is_dir: false,
                    position,
                });
                DispatchEventResult::StopPropagation
            })
            .finish();
        Container::new(with_blank_ctx)
            .with_padding_top(8.0)
            .finish()
    }

    /// 底部任务区：上传/下载进度。无任务时返回 None。
    pub(in crate::root_view) fn render_file_panel_transfers(
        &self,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Option<Box<dyn Element>> {
        let transfers = &tab.file_panel_state.transfers;
        if transfers.is_empty() {
            return None;
        }
        let mut col = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        let title = Text::new_inline("任务", self.ui_font, 11.0)
            .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
            .with_color(colors.text_muted)
            .finish();
        col.add_child(title);
        for row in transfers.iter().rev().take(5) {
            col.add_child(self.render_transfer_row(tab, row, colors));
        }
        Some(
            Container::new(col.finish())
                .with_padding_top(10.0)
                .with_border(Border::top(1.0).with_border_color(colors.panel_border))
                .finish(),
        )
    }

    fn render_transfer_row(
        &self,
        tab: &TerminalSessionTab,
        row: &TransferRow,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let status_text = match &row.status {
            TransferStatus::Active => match row.total {
                Some(total) if total > 0 => {
                    let pct =
                        ((row.transferred as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as u32;
                    format!("{pct}%")
                }
                _ => format_bytes_short(row.transferred),
            },
            TransferStatus::Done => "已完成".to_string(),
            TransferStatus::Failed(_) => "失败".to_string(),
            TransferStatus::Cancelled => "已取消".to_string(),
        };
        let status_color = match &row.status {
            TransferStatus::Active => colors.text_muted,
            TransferStatus::Done => colors.download,
            TransferStatus::Failed(_) => colors.warning,
            TransferStatus::Cancelled => colors.text_muted,
        };

        let arrow = if row.is_upload { "↑" } else { "↓" };
        let arrow_text = Text::new_inline(arrow.to_string(), self.ui_font, 11.0)
            .with_color(colors.text_muted)
            .finish();
        let name = Clipped::new(
            Text::new_inline(row.file_name.clone(), self.ui_font, 11.0)
                .with_color(colors.text_primary)
                .finish(),
        )
        .finish();
        let status = Text::new_inline(status_text, self.ui_font, 11.0)
            .with_color(status_color)
            .finish();

        let mut content = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Container::new(arrow_text).with_padding_right(6.0).finish())
            .with_child(Expanded::new(1.0, Shrinkable::new(1.0, name).finish()).finish())
            .with_child(Container::new(status).with_padding_left(8.0).finish());

        if matches!(row.status, TransferStatus::Active) {
            let uc = self.ui_colors();
            let state = tab
                .file_panel_transfer_cancel_states
                .borrow_mut()
                .entry(row.transfer_id)
                .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
                .clone();
            let cancel_btn = render_file_panel_icon_button(
                state,
                ICON_PATH_CLOSE,
                uc.icon_color_inactive,
                uc.icon_button_hover_bg,
                TerminalGridAction::FilePanelCancelTransfer(row.transfer_id),
            );
            content.add_child(Container::new(cancel_btn).with_padding_left(6.0).finish());
        }

        if let TransferStatus::Failed(message) = &row.status {
            content.add_child(
                Container::new(
                    Text::new_inline(message.clone(), self.ui_font, 10.0)
                        .with_color(colors.warning)
                        .finish(),
                )
                .with_padding_left(8.0)
                .finish(),
            );
        }

        Container::new(content.finish())
            .with_padding_top(4.0)
            .with_padding_bottom(2.0)
            .finish()
    }

    fn render_file_panel_entry(
        &self,
        tab: &TerminalSessionTab,
        entry: &nexshell::sftp_ops::RemoteEntry,
        selected: &std::collections::BTreeSet<String>,
        colors: &HostOverviewColors,
        uc: &UiColors,
    ) -> Box<dyn Element> {
        let name = entry.name.clone();
        let is_dir = matches!(entry.kind, EntryKind::Dir);
        let is_selected = selected.contains(&name);

        let icon_path = match entry.kind {
            EntryKind::Dir => ICON_PATH_FOLDER,
            EntryKind::Symlink => ICON_PATH_LINK,
            _ => ICON_PATH_LIST_VIEW,
        };
        let icon_color = if is_dir {
            uc.icon_color_active
        } else {
            uc.icon_color_inactive
        };
        let size_text = if is_dir {
            String::new()
        } else {
            format_bytes_short(entry.size)
        };
        let mtime_text = format_remote_mtime(entry.modified);

        let state = tab
            .file_panel_entry_states
            .borrow_mut()
            .entry(name.clone())
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone();
        let hover_bg = uc.icon_button_hover_bg;
        let selected_bg = uc.selection_pill_bg;
        let text_color = colors.text_primary;
        let muted = colors.text_muted;
        let font = self.ui_font;
        // 悬浮显示完整名（长名截断时，review 反馈）：SavePosition 锚点 + hover 叠 tooltip。
        let name_pos_id = format!("fp-name::{name}");
        let tooltip_bg = uc.tooltip_bg;
        let tooltip_text = uc.tooltip_text;

        let row_builder = move |mouse: &MouseState| -> Box<dyn Element> {
            let icon = ConstrainedBox::new(Icon::new(icon_path, icon_color).finish())
                .with_width(14.0)
                .with_height(14.0)
                .finish();
            let name_text = SavePosition::new(
                Clipped::new(
                    Text::new_inline(name.clone(), font, 12.0)
                        .with_color(text_color)
                        .finish(),
                )
                .finish(),
                &name_pos_id,
            )
            .finish();
            // 固定列宽 + 右对齐 → 跨行 mtime/size 视觉对齐；name 用 Expanded(1.0) 吃剩余空间
            let mtime_col = ConstrainedBox::new(
                Align::new(
                    Text::new_inline(mtime_text.clone(), font, 10.0)
                        .with_color(muted)
                        .finish(),
                )
                .right()
                .finish(),
            )
            .with_width(78.0)
            .finish();
            let size_col = ConstrainedBox::new(
                Align::new(
                    Text::new_inline(size_text.clone(), font, 10.0)
                        .with_color(muted)
                        .finish(),
                )
                .right()
                .finish(),
            )
            .with_width(44.0)
            .finish();
            let row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(Container::new(icon).with_padding_right(8.0).finish())
                .with_child(Expanded::new(1.0, name_text).finish())
                .with_child(Container::new(mtime_col).with_padding_left(8.0).finish())
                .with_child(Container::new(size_col).with_padding_left(8.0).finish());
            let mut container = Container::new(row.finish())
                .with_padding_left(6.0)
                .with_padding_right(6.0)
                .with_padding_top(4.0)
                .with_padding_bottom(4.0)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)));
            if is_selected {
                container = container.with_background_color(selected_bg);
            } else if mouse.is_hovered() {
                container = container.with_background_color(hover_bg);
            }
            let base = container.finish();
            if !mouse.is_hovered() {
                return base;
            }
            file_panel_name_tooltip(
                base,
                &name_pos_id,
                name.clone(),
                font,
                tooltip_bg,
                tooltip_text,
            )
        };

        let name_for_click = entry.name.clone();
        let name_for_dbl = entry.name.clone();
        // cmd / ctrl → Toggle，shift → Range，其余 → Replace。
        // ctrl 在 macOS 实际上多用于触发右键菜单，但为了跨平台行为一致仍归到 Toggle。
        let mut hover = Hoverable::new(state, row_builder)
            .with_cursor(warpui::platform::Cursor::PointingHand)
            .on_click_with_modifiers(move |ctx, _, _, modifiers| {
                let mode = if modifiers.shift {
                    FilePanelSelectMode::Range
                } else if modifiers.cmd || modifiers.ctrl {
                    FilePanelSelectMode::Toggle
                } else {
                    FilePanelSelectMode::Replace
                };
                ctx.dispatch_typed_action(TerminalGridAction::FilePanelSelect {
                    name: name_for_click.clone(),
                    mode,
                });
            });
        if is_dir {
            hover = hover.on_double_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::FilePanelEnterDir(
                    name_for_dbl.clone(),
                ));
            });
        } else {
            // 远程文件双击 → 内置编辑器（ADR 0005）；二进制/超大由 handler 提示下载。
            let full_path = nexshell::file_panel::join_path(&tab.file_panel_state.cwd, &entry.name);
            hover = hover.on_double_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::FilePanelOpenInCodeViewer {
                    path: full_path.clone(),
                });
            });
        }
        let name_for_ctx = entry.name.clone();
        EventHandler::new(hover.finish())
            .on_right_mouse_down(move |ctx, _app, position| {
                ctx.dispatch_typed_action(TerminalGridAction::FilePanelShowContextMenu {
                    name: Some(name_for_ctx.clone()),
                    is_dir,
                    position,
                });
                DispatchEventResult::StopPropagation
            })
            .finish()
    }
}
