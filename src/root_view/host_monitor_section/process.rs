// host_monitor_section::process — 进程概览段 + 进程列表整页。
// 本文件只含 impl RootView，无自由函数。

use crate::host_monitor_view_helpers::overview_process_cells;
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{RootView, TerminalSessionTab};
use nexshell::host_overview::{
    format_bytes_short, HostOverviewSnapshot, HostOverviewStatus, ProcessMetric, ProcessSortKey,
    SortDirection,
};
use std::sync::{Arc, Mutex};
use warpui::color::ColorU;
use warpui::elements::{
    Border, Clipped, ClippedScrollable, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, DispatchEventResult, Empty, EventHandler, Expanded, Flex, Hoverable,
    MainAxisSize, MouseState, ParentElement, Radius, Shrinkable, Text,
};
use warpui::{fonts, AppContext, Element};

impl RootView {
    pub(in crate::root_view) fn render_overview_process_section(
        &self,
        tab: Option<&TerminalSessionTab>,
        snapshot: &HostOverviewSnapshot,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let (sort_key, sort_direction) = tab
            .map(|t| {
                (
                    t.host_overview.process_sort_key,
                    t.host_overview.process_sort_direction,
                )
            })
            .unwrap_or((ProcessSortKey::Cpu, SortDirection::Desc));
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_overview_process_title(tab, colors))
            .with_child(self.render_process_header(tab, sort_key, sort_direction, colors));

        let sorted: Vec<&ProcessMetric> = if let Some(tab) = tab {
            tab.host_overview
                .sorted_processes()
                .into_iter()
                .filter(|p| !p.command.contains("<defunct>"))
                .take(5)
                .collect()
        } else {
            snapshot
                .processes
                .iter()
                .filter(|p| !p.command.contains("<defunct>"))
                .take(5)
                .collect()
        };
        if let Some(tab) = tab {
            // pid 会随排序/进程退出轮换，按当前可见行清理 hover 状态防积累
            let visible: Vec<u32> = sorted.iter().map(|p| p.pid).collect();
            tab.host_overview_process_row_states
                .borrow_mut()
                .retain(|pid, _| visible.contains(pid));
        }
        if sorted.is_empty() {
            column.add_child(self.render_overview_muted_line(
                rust_i18n::t!("host_overview_no_process").as_ref(),
                colors,
            ));
        } else {
            for process in sorted {
                column.add_child(self.render_process_row(tab, process, colors));
            }
        }

        Container::new(column.finish())
            .with_padding_bottom(10.0)
            .finish()
    }

    fn render_overview_process_title(
        &self,
        tab: Option<&TerminalSessionTab>,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let expand_state = tab.map(|t| t.host_overview_process_expand_state.clone());
        self.render_overview_expandable_section_title(
            rust_i18n::t!("host_overview_section_process").as_ref(),
            tab,
            expand_state,
            TerminalGridAction::OpenProcessList,
            colors,
        )
    }

    fn render_process_header(
        &self,
        tab: Option<&TerminalSessionTab>,
        sort_key: ProcessSortKey,
        direction: SortDirection,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let arrow = if matches!(direction, SortDirection::Desc) {
            " ↓"
        } else {
            " ↑"
        };
        let make_label = |label: &str, key: ProcessSortKey| -> String {
            if key == sort_key {
                format!("{label}{arrow}")
            } else {
                label.to_string()
            }
        };
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                ConstrainedBox::new(self.render_process_header_cell(
                    tab,
                    ProcessSortKey::Memory,
                    &make_label(
                        rust_i18n::t!("host_overview_col_memory").as_ref(),
                        ProcessSortKey::Memory,
                    ),
                    sort_key,
                    colors,
                ))
                .with_width(58.0)
                .finish(),
            )
            .with_child(
                ConstrainedBox::new(self.render_process_header_cell(
                    tab,
                    ProcessSortKey::Cpu,
                    &make_label(
                        rust_i18n::t!("host_overview_col_cpu").as_ref(),
                        ProcessSortKey::Cpu,
                    ),
                    sort_key,
                    colors,
                ))
                .with_width(46.0)
                .finish(),
            )
            .with_child(
                Shrinkable::new(
                    1.0,
                    self.render_process_header_cell(
                        tab,
                        ProcessSortKey::Command,
                        &make_label(
                            rust_i18n::t!("host_overview_col_command").as_ref(),
                            ProcessSortKey::Command,
                        ),
                        sort_key,
                        colors,
                    ),
                )
                .finish(),
            );

        Container::new(row.finish())
            .with_padding_bottom(5.0)
            .finish()
    }

    fn render_process_header_cell(
        &self,
        tab: Option<&TerminalSessionTab>,
        key: ProcessSortKey,
        label: &str,
        active_key: ProcessSortKey,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let font = self.monospace_font;
        let active = key == active_key;
        let active_color = colors.text_primary;
        let muted_color = colors.text_muted;
        let label_owned = label.to_string();
        let make_text = move |color: ColorU| {
            Text::new_inline(label_owned.clone(), font, 11.0)
                .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
                .with_color(color)
                .finish()
        };

        let Some(tab) = tab else {
            return make_text(if active { active_color } else { muted_color });
        };
        let idx = match key {
            ProcessSortKey::Memory => 0,
            ProcessSortKey::Cpu => 1,
            ProcessSortKey::Command => 2,
            ProcessSortKey::Pid => 3,
            ProcessSortKey::User => 4,
            ProcessSortKey::ExePath => 5,
        };
        let state = tab.host_overview_process_header_states[idx].clone();
        Hoverable::new(state, move |mouse| {
            let color = if active {
                active_color
            } else if mouse.is_hovered() {
                active_color
            } else {
                muted_color
            };
            make_text(color)
        })
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::SortHostProcesses(key));
        })
        .finish()
    }

    fn render_process_row(
        &self,
        tab: Option<&TerminalSessionTab>,
        process: &ProcessMetric,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let memory = format_bytes_short(process.rss_bytes);
        let cpu = format!("{:.1}", process.cpu_percent);
        let font = self.monospace_font;

        // 无 host_id 的 tab 不挂交互（菜单里的 kill 也依赖 host_id）
        let Some(tab) = tab.filter(|t| t.host_id.is_some()) else {
            return Container::new(overview_process_cells(
                &memory,
                &cpu,
                &process.command,
                font,
                colors.text_primary,
            ))
            .with_vertical_padding(2.5)
            .finish();
        };

        let state = tab
            .host_overview_process_row_states
            .borrow_mut()
            .entry(process.pid)
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone();
        let selected = tab.process_list_selected_pid == Some(process.pid);
        let text_color = colors.text_primary;
        let hover_bg = colors.metric_track;
        // 与进程整页一致：选中行用低 alpha 强调色
        let selected_bg = ColorU::new(
            colors.cpu_accent.r,
            colors.cpu_accent.g,
            colors.cpu_accent.b,
            0x55,
        );
        let command = process.command.clone();
        let row = Hoverable::new(state, move |mouse| {
            let bg = if selected {
                selected_bg
            } else if mouse.is_hovered() {
                hover_bg
            } else {
                nexshell::design_tokens::TRANSPARENT
            };
            Container::new(overview_process_cells(
                &memory, &cpu, &command, font, text_color,
            ))
            .with_vertical_padding(2.5)
            .with_background_color(bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.0)))
            .finish()
        })
        .finish();

        let pid = process.pid;
        let cmd_for_menu = process.command.clone();
        let args_for_menu = process.args.clone();
        let exe_for_menu = process.exe_path.clone().unwrap_or_default();
        EventHandler::new(row)
            .on_right_mouse_down(move |ctx, _, position, _modifiers| {
                ctx.dispatch_typed_action(TerminalGridAction::ProcessListShowContextMenu {
                    pid,
                    command: cmd_for_menu.clone(),
                    args: args_for_menu.clone(),
                    exe_path: exe_for_menu.clone(),
                    position,
                });
                DispatchEventResult::StopPropagation
            })
            .finish()
    }

    pub(in crate::root_view) fn render_process_list_page(
        &self,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let colors = self.design_tokens.overview;
        let active_tab = match self.terminal_tabs.get(self.active_tab_index) {
            Some(tab) => tab,
            None => return Container::new(Empty::new().finish()).finish(),
        };
        // ProcessList tab 自身的 monitor 已由 sync_host_overview_monitor 启动，
        // 直接读它自己 snapshot 的进程数据。
        let processes: Vec<ProcessMetric> = active_tab.host_overview.snapshot.processes.clone();
        // 排序用 ProcessList tab 自己的 ui state
        let sort_key = active_tab.host_overview.process_sort_key;
        let sort_dir = active_tab.host_overview.process_sort_direction;
        let mut sorted: Vec<ProcessMetric> = processes
            .into_iter()
            .filter(|p| !p.command.contains("<defunct>"))
            .collect();
        sorted.sort_by(|a, b| match sort_key {
            ProcessSortKey::Pid => a.pid.cmp(&b.pid),
            ProcessSortKey::User => a.user.cmp(&b.user),
            ProcessSortKey::Memory => a.rss_bytes.cmp(&b.rss_bytes),
            ProcessSortKey::Cpu => a
                .cpu_percent
                .partial_cmp(&b.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal),
            ProcessSortKey::Command => a.command.cmp(&b.command),
            ProcessSortKey::ExePath => a.exe_path.cmp(&b.exe_path),
        });
        if matches!(sort_dir, SortDirection::Desc) {
            sorted.reverse();
        }

        let title_label = active_tab.label();
        let count_text = format!("{} 个进程", sorted.len());

        let title_bar = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Text::new_inline(title_label, self.ui_font, 13.0)
                        .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
                        .with_color(colors.text_primary)
                        .finish(),
                )
                .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
                .with_child(
                    Text::new_inline(count_text, self.ui_font, 12.0)
                        .with_color(colors.text_muted)
                        .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(16.0)
        .with_vertical_padding(10.0)
        .with_border(Border::bottom(1.0).with_border_color(colors.panel_border))
        .finish();

        let header = self.render_process_list_header(Some(active_tab), sort_key, &colors);

        let mut body_col = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        if sorted.is_empty() {
            let status_text = match &active_tab.host_overview.snapshot.status {
                HostOverviewStatus::Waiting | HostOverviewStatus::Collecting => {
                    rust_i18n::t!("process_list_placeholder").to_string()
                }
                HostOverviewStatus::Error(msg) => {
                    format!("{}: {}", rust_i18n::t!("process_list_error"), msg)
                }
                HostOverviewStatus::Ready => rust_i18n::t!("process_list_empty").to_string(),
            };
            body_col.add_child(
                Container::new(
                    Text::new_inline(status_text, self.ui_font, 12.0)
                        .with_color(colors.text_muted)
                        .finish(),
                )
                .with_horizontal_padding(16.0)
                .with_vertical_padding(20.0)
                .finish(),
            );
        } else {
            let selected_pid = active_tab.process_list_selected_pid;
            for (index, process) in sorted.iter().enumerate() {
                let selected = selected_pid == Some(process.pid);
                body_col.add_child(self.render_process_list_row(process, index, selected, &colors));
            }
        }

        let scrollbar_thumb = warpui::elements::Fill::Solid(ColorU::new(
            colors.text_muted.r,
            colors.text_muted.g,
            colors.text_muted.b,
            0x66,
        ));
        let scrollbar_thumb_active = warpui::elements::Fill::Solid(colors.text_muted);
        let scrollbar_track = warpui::elements::Fill::None;
        let body = ClippedScrollable::vertical(
            active_tab.process_list_scroll_state.clone(),
            body_col.finish(),
            warpui::elements::ScrollbarWidth::Custom(6.0),
            scrollbar_thumb,
            scrollbar_thumb_active,
            scrollbar_track,
        )
        .with_overlayed_scrollbar()
        .finish();

        Container::new(
            Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(title_bar)
                .with_child(header)
                .with_child(Expanded::new(1.0, body).finish())
                .finish(),
        )
        .with_background_color(colors.panel_bg)
        .finish()
    }

    fn render_process_list_header(
        &self,
        tab: Option<&TerminalSessionTab>,
        active_key: ProcessSortKey,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let build_inner =
            |label: &str, key: ProcessSortKey, align_right: bool| -> Box<dyn Element> {
                let text = self.render_process_header_cell(tab, key, label, active_key, colors);
                let active = key == active_key;
                let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
                if align_right {
                    row.add_child(Expanded::new(1.0, Empty::new().finish()).finish());
                }
                row.add_child(text);
                if active {
                    let arrow = if matches!(
                        tab.map(|t| t.host_overview.process_sort_direction)
                            .unwrap_or(SortDirection::Desc),
                        SortDirection::Desc
                    ) {
                        "▼"
                    } else {
                        "▲"
                    };
                    row.add_child(
                        Container::new(
                            Text::new_inline(arrow.to_string(), self.ui_font, 9.0)
                                .with_color(colors.text_primary)
                                .finish(),
                        )
                        .with_margin_left(2.0)
                        .finish(),
                    );
                }
                if !align_right {
                    row.add_child(Expanded::new(1.0, Empty::new().finish()).finish());
                }
                Container::new(Clipped::new(row.finish()).finish())
                    .with_padding_right(12.0)
                    .finish()
            };
        let fixed =
            |label: &str, width: f32, key: ProcessSortKey, align_right: bool| -> Box<dyn Element> {
                ConstrainedBox::new(build_inner(label, key, align_right))
                    .with_width(width)
                    .finish()
            };
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(fixed("PID", 70.0, ProcessSortKey::Pid, false))
            .with_child(fixed("用户", 90.0, ProcessSortKey::User, false))
            .with_child(fixed("内存", 80.0, ProcessSortKey::Memory, true))
            .with_child(fixed("CPU", 70.0, ProcessSortKey::Cpu, true))
            .with_child(
                Expanded::new(3.0, build_inner("命令", ProcessSortKey::Command, false)).finish(),
            )
            .with_child(
                Expanded::new(2.0, build_inner("位置", ProcessSortKey::ExePath, false)).finish(),
            )
            .finish();
        Container::new(row)
            .with_horizontal_padding(16.0)
            .with_vertical_padding(8.0)
            .with_background_color(colors.card_bg)
            .with_border(Border::bottom(1.0).with_border_color(colors.panel_border))
            .finish()
    }

    fn render_process_list_row(
        &self,
        process: &ProcessMetric,
        index: usize,
        selected: bool,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let font = self.monospace_font;
        let build_inner = |text: &str, muted: bool, align_right: bool| -> Box<dyn Element> {
            let color = if muted {
                colors.text_muted
            } else {
                colors.text_primary
            };
            let label = Text::new_inline(text.to_string(), font, 12.0)
                .with_color(color)
                .finish();
            let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
            if align_right {
                row.add_child(Expanded::new(1.0, Empty::new().finish()).finish());
            }
            row.add_child(label);
            if !align_right {
                row.add_child(Expanded::new(1.0, Empty::new().finish()).finish());
            }
            Container::new(Clipped::new(row.finish()).finish())
                .with_padding_right(12.0)
                .finish()
        };
        let fixed = |text: &str, width: f32, muted: bool, align_right: bool| -> Box<dyn Element> {
            ConstrainedBox::new(build_inner(text, muted, align_right))
                .with_width(width)
                .finish()
        };
        let pid_text = process.pid.to_string();
        let user_text = process.user.clone();
        let mem_text = format_bytes_short(process.rss_bytes);
        let cpu_text = format!("{:.1}", process.cpu_percent);
        let cmd_text = if process.args.is_empty() {
            process.command.clone()
        } else {
            format!("{} ⋮ {}", process.command, process.args)
        };
        let exe_text = process.exe_path.clone().unwrap_or_default();

        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(fixed(&pid_text, 70.0, true, false))
            .with_child(fixed(&user_text, 90.0, false, false))
            .with_child(fixed(&mem_text, 80.0, false, true))
            .with_child(fixed(&cpu_text, 70.0, false, true))
            .with_child(Expanded::new(3.0, build_inner(&cmd_text, false, false)).finish())
            .with_child(Expanded::new(2.0, build_inner(&exe_text, true, false)).finish())
            .finish();
        let bg = if selected {
            // 主题强调色低 alpha 叠在斑马纹之上，跨主题都明显
            ColorU::new(
                colors.cpu_accent.r,
                colors.cpu_accent.g,
                colors.cpu_accent.b,
                0x55,
            )
        } else if index % 2 == 0 {
            colors.panel_bg
        } else {
            colors.card_bg
        };
        let pid = process.pid;
        let cmd_for_menu = process.command.clone();
        let args_for_menu = process.args.clone();
        let exe_for_menu = process.exe_path.clone().unwrap_or_default();
        let row_with_ctx = EventHandler::new(row)
            .on_right_mouse_down(move |ctx, _, position, _modifiers| {
                ctx.dispatch_typed_action(TerminalGridAction::ProcessListShowContextMenu {
                    pid,
                    command: cmd_for_menu.clone(),
                    args: args_for_menu.clone(),
                    exe_path: exe_for_menu.clone(),
                    position,
                });
                DispatchEventResult::StopPropagation
            })
            .finish();
        Container::new(row_with_ctx)
            .with_horizontal_padding(16.0)
            .with_vertical_padding(4.0)
            .with_background_color(bg)
            .finish()
    }
}
