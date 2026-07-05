// host_monitor_section::system — 系统信息 + 磁盘概览 + 系统信息整页。
// 本文件只含 impl RootView，无自由函数。

use crate::host_monitor_view_helpers::{
    format_uptime, format_usage_metric, usage_fill_color, OVERVIEW_CONTENT_WIDTH,
};
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{RootView, TerminalSessionTab};
use nexshell::host_overview::{
    format_bytes_short, DiskMetric, HostOverviewSnapshot, HostOverviewStatus,
};
use warpui::color::ColorU;
use warpui::elements::{
    Border, Clipped, ClippedScrollable, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, Empty, Expanded, Fill, Flex, MainAxisSize, ParentElement, Radius,
    ScrollbarWidth, Shrinkable, Stack, Text,
};
use warpui::{fonts, AppContext, Element};

impl RootView {
    pub(in crate::root_view) fn render_overview_system_section(
        &self,
        tab: Option<&TerminalSessionTab>,
        snapshot: &HostOverviewSnapshot,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_overview_system_title(tab, colors));

        let load = snapshot
            .load_average
            .map(|values| format!("{:.2}, {:.2}, {:.2}", values[0], values[1], values[2]))
            .unwrap_or_else(|| "--".to_string());
        column.add_child(self.render_overview_key_value(
            rust_i18n::t!("host_overview_load").as_ref(),
            &load,
            colors,
        ));
        column.add_child(self.render_overview_usage_bar(
            "CPU",
            snapshot.cpu_percent,
            None,
            colors.cpu_accent,
            colors,
        ));
        column.add_child(self.render_overview_usage_bar(
            rust_i18n::t!("host_overview_memory").as_ref(),
            snapshot.memory.as_ref().map(|metric| metric.percent),
            snapshot.memory.as_ref().map(format_usage_metric),
            colors.memory_accent,
            colors,
        ));
        if let Some(swap) = snapshot
            .swap
            .as_ref()
            .filter(|metric| metric.total_bytes > 0)
        {
            column.add_child(self.render_overview_usage_bar(
                rust_i18n::t!("host_overview_swap").as_ref(),
                Some(swap.percent),
                Some(format_usage_metric(swap)),
                colors.swap_accent,
                colors,
            ));
        }

        // 末条用量条自带 pb 9，补 4 与其他节的分隔线留白对齐
        Container::new(column.finish())
            .with_padding_bottom(4.0)
            .finish()
    }

    fn render_overview_system_title(
        &self,
        tab: Option<&TerminalSessionTab>,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let expand_state = tab.map(|t| t.host_overview_system_expand_state.clone());
        self.render_overview_expandable_section_title(
            rust_i18n::t!("host_overview_section_system").as_ref(),
            tab,
            expand_state,
            TerminalGridAction::OpenSystemInfo,
            colors,
        )
    }

    // 两行布局：标签/数值一行，全宽细条一行；≥90% 填充切警告色
    fn render_overview_usage_bar(
        &self,
        label: &str,
        percent: Option<f32>,
        detail: Option<String>,
        accent: ColorU,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        const BAR_HEIGHT: f32 = 6.0;
        const BAR_RADIUS: f32 = 3.0;
        let value = percent.unwrap_or(0.0).clamp(0.0, 100.0);
        // 显示为 0% 的不画填充；画则最小 = 直径，避免圆角被挤成异形
        let fill_width = if value >= 0.5 {
            (OVERVIEW_CONTENT_WIDTH * value / 100.0).max(BAR_HEIGHT)
        } else {
            0.0
        };
        let percent_text = percent
            .map(|value| format!("{value:.0}%"))
            .unwrap_or_else(|| "--".to_string());

        let mut label_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(label.to_string(), self.ui_font, 12.0)
                    .with_color(colors.text_muted)
                    .finish(),
            );
        label_row.add_child(Shrinkable::new(1.0, Empty::new().finish()).finish());
        if let Some(detail) = detail {
            label_row.add_child(
                Container::new(
                    Text::new_inline(detail, self.monospace_font, 11.0)
                        .with_color(colors.text_muted)
                        .finish(),
                )
                .with_margin_right(8.0)
                .finish(),
            );
        }
        label_row.add_child(
            Text::new_inline(percent_text, self.monospace_font, 12.0)
                .with_color(colors.text_primary)
                .finish(),
        );

        let mut bar = Stack::new();
        bar.add_child(
            ConstrainedBox::new(
                Container::new(Empty::new().finish())
                    .with_background_color(colors.metric_track)
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BAR_RADIUS)))
                    .finish(),
            )
            .with_width(OVERVIEW_CONTENT_WIDTH)
            .with_height(BAR_HEIGHT)
            .finish(),
        );
        if fill_width > 0.0 {
            bar.add_child(
                ConstrainedBox::new(
                    Container::new(Empty::new().finish())
                        .with_background_color(usage_fill_color(value, accent, colors))
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BAR_RADIUS)))
                        .finish(),
                )
                .with_width(fill_width)
                .with_height(BAR_HEIGHT)
                .finish(),
            );
        }

        Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(label_row.finish())
                .with_child(Container::new(bar.finish()).with_padding_top(4.0).finish())
                .finish(),
        )
        .with_padding_bottom(9.0)
        .finish()
    }

    pub(in crate::root_view) fn render_overview_disk_section(
        &self,
        tab: Option<&TerminalSessionTab>,
        snapshot: &HostOverviewSnapshot,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        const SCROLL_MAX_HEIGHT: f32 = 168.0;
        let mut header = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_overview_section_title(
                rust_i18n::t!("host_overview_section_disk").as_ref(),
                colors,
            ))
            .with_child(self.render_disk_header(colors));

        // 侧栏只看真实磁盘，tmpfs 等伪文件系统去整页看；全是伪文件系统时回退显示全部
        let mut disks: Vec<&DiskMetric> = snapshot
            .disks
            .iter()
            .filter(|disk| !disk.is_pseudo_filesystem())
            .collect();
        if disks.is_empty() {
            disks = snapshot.disks.iter().collect();
        }

        let body: Box<dyn Element> = if disks.is_empty() {
            self.render_overview_muted_line(rust_i18n::t!("host_overview_no_disk").as_ref(), colors)
        } else {
            let mut rows = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for disk in disks {
                rows.add_child(self.render_disk_row(disk, colors));
            }
            if let Some(tab) = tab {
                let scrollable = ClippedScrollable::vertical(
                    tab.host_overview_disk_scroll_state.clone(),
                    rows.finish(),
                    ScrollbarWidth::Custom(4.0),
                    Fill::Solid(colors.text_muted),
                    Fill::Solid(colors.text_primary),
                    Fill::None,
                )
                .with_overlayed_scrollbar()
                .finish();
                ConstrainedBox::new(scrollable)
                    .with_max_height(SCROLL_MAX_HEIGHT)
                    .finish()
            } else {
                rows.finish()
            }
        };
        header.add_child(body);

        Container::new(header.finish())
            .with_padding_bottom(10.0)
            .finish()
    }

    fn render_overview_section_title(
        &self,
        title: &str,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        Container::new(
            Text::new_inline(title.to_string(), self.ui_font, 11.0)
                .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
                .with_color(colors.section_title)
                .finish(),
        )
        .with_padding_bottom(6.0)
        .finish()
    }

    fn render_disk_header(&self, colors: &HostOverviewColors) -> Box<dyn Element> {
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(
                    rust_i18n::t!("host_overview_col_mount").to_string(),
                    self.ui_font,
                    11.0,
                )
                .with_color(colors.text_muted)
                .finish(),
            )
            .with_child(Shrinkable::new(1.0, Empty::new().finish()).finish())
            .with_child(
                Text::new_inline(
                    rust_i18n::t!("host_overview_col_disk_usage").to_string(),
                    self.ui_font,
                    11.0,
                )
                .with_color(colors.text_muted)
                .finish(),
            );
        Container::new(row.finish())
            .with_padding_bottom(5.0)
            .finish()
    }

    // 挂载点 + 可用/大小 + 占用百分比一行，3px 用量条一行
    fn render_disk_row(&self, disk: &DiskMetric, colors: &HostOverviewColors) -> Box<dyn Element> {
        const BAR_HEIGHT: f32 = 3.0;
        let usage = format!(
            "{}/{}",
            format_bytes_short(disk.available_bytes),
            format_bytes_short(disk.total_bytes)
        );
        let percent = disk.percent.clamp(0.0, 100.0);
        // 显示为 0% 的不画填充，避免「0% 却有个点」的矛盾
        let fill_width = if percent >= 0.5 {
            (OVERVIEW_CONTENT_WIDTH * percent / 100.0).max(BAR_HEIGHT)
        } else {
            0.0
        };

        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Shrinkable::new(
                    1.0,
                    Clipped::new(
                        Text::new_inline(disk.mount.clone(), self.monospace_font, 12.0)
                            .with_color(colors.text_primary)
                            .finish(),
                    )
                    .finish(),
                )
                .finish(),
            )
            .with_child(
                Container::new(
                    Text::new_inline(usage, self.monospace_font, 11.0)
                        .with_color(colors.text_muted)
                        .finish(),
                )
                .with_margin_left(8.0)
                .with_margin_right(8.0)
                .finish(),
            )
            .with_child(
                Text::new_inline(format!("{percent:.0}%"), self.monospace_font, 12.0)
                    .with_color(colors.text_primary)
                    .finish(),
            );

        let mut bar = Stack::new();
        bar.add_child(
            ConstrainedBox::new(
                Container::new(Empty::new().finish())
                    .with_background_color(colors.metric_track)
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(1.5)))
                    .finish(),
            )
            .with_width(OVERVIEW_CONTENT_WIDTH)
            .with_height(BAR_HEIGHT)
            .finish(),
        );
        if fill_width > 0.0 {
            bar.add_child(
                ConstrainedBox::new(
                    Container::new(Empty::new().finish())
                        .with_background_color(usage_fill_color(percent, colors.cpu_accent, colors))
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(1.5)))
                        .finish(),
                )
                .with_width(fill_width)
                .with_height(BAR_HEIGHT)
                .finish(),
            );
        }

        Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(row.finish())
                .with_child(Container::new(bar.finish()).with_padding_top(3.0).finish())
                .finish(),
        )
        .with_padding_bottom(7.0)
        .finish()
    }

    pub(in crate::root_view) fn render_system_info_page(
        &self,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = &self.cached_warp_theme;
        let colors = HostOverviewColors::from_theme(theme);
        let active_tab = match self.terminal_tabs.get(self.active_tab_index) {
            Some(tab) => tab,
            None => return Container::new(Empty::new().finish()).finish(),
        };
        let snapshot = &active_tab.host_overview.snapshot;

        let title_bar = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Text::new_inline(active_tab.label(), self.ui_font, 13.0)
                        .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
                        .with_color(colors.text_primary)
                        .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(16.0)
        .with_vertical_padding(10.0)
        .with_border(Border::bottom(1.0).with_border_color(colors.panel_border))
        .finish();

        let mut body_col = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        match &snapshot.status {
            HostOverviewStatus::Waiting | HostOverviewStatus::Collecting
                if !snapshot.has_collected_data() =>
            {
                body_col.add_child(
                    Container::new(
                        Text::new_inline(
                            rust_i18n::t!("system_info_placeholder").to_string(),
                            self.ui_font,
                            12.0,
                        )
                        .with_color(colors.text_muted)
                        .finish(),
                    )
                    .with_horizontal_padding(16.0)
                    .with_vertical_padding(20.0)
                    .finish(),
                );
            }
            HostOverviewStatus::Error(msg) => {
                body_col.add_child(
                    Container::new(
                        Text::new_inline(
                            format!("{}: {}", rust_i18n::t!("system_info_error"), msg),
                            self.ui_font,
                            12.0,
                        )
                        .with_color(colors.text_muted)
                        .finish(),
                    )
                    .with_horizontal_padding(16.0)
                    .with_vertical_padding(20.0)
                    .finish(),
                );
            }
            _ => {
                // uname -srmo 输出顺序：kernel-name / kernel-release / machine / operating-system
                let kernel_parts: Vec<&str> = snapshot
                    .kernel
                    .as_deref()
                    .map(|s| s.split_whitespace().collect())
                    .unwrap_or_default();
                let kernel_name = kernel_parts.first().copied();
                let kernel_release = kernel_parts.get(1).copied();
                let machine = kernel_parts.get(2).copied();
                let os_name = if kernel_parts.len() >= 4 {
                    Some(kernel_parts[3..].join(" "))
                } else {
                    None
                };

                let rows: Vec<(&str, Option<String>)> = vec![
                    ("操作系统", os_name),
                    ("内核", kernel_name.map(str::to_string)),
                    ("内核版本", kernel_release.map(str::to_string)),
                    ("硬件架构", machine.map(str::to_string)),
                    ("主机名", snapshot.hostname.clone()),
                    ("用户", snapshot.username.clone()),
                    ("运行时长", snapshot.uptime_seconds.map(format_uptime)),
                    (
                        "负载",
                        snapshot
                            .load_average
                            .map(|v| format!("{:.2}, {:.2}, {:.2}", v[0], v[1], v[2])),
                    ),
                    ("SSH 延迟", snapshot.latency_ms.map(|ms| format!("{ms}ms"))),
                ];
                for (label, value) in rows {
                    if let Some(value) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                        body_col.add_child(self.render_system_info_row(label, value, &colors));
                    }
                }

                if let Some(cpu_percent) = snapshot.cpu_percent {
                    body_col.add_child(self.render_system_info_row(
                        "CPU 占用",
                        &format!("{cpu_percent:.1}%"),
                        &colors,
                    ));
                }
                if let Some(mem) = snapshot.memory.as_ref() {
                    body_col.add_child(self.render_system_info_row(
                        "内存",
                        &format!(
                            "{}  已使用 {}  {:.0}%  剩余 {}",
                            format_bytes_short(mem.total_bytes),
                            format_bytes_short(mem.used_bytes),
                            mem.percent,
                            format_bytes_short(mem.total_bytes.saturating_sub(mem.used_bytes)),
                        ),
                        &colors,
                    ));
                }
                if let Some(swap) = snapshot.swap.as_ref().filter(|s| s.total_bytes > 0) {
                    body_col.add_child(self.render_system_info_row(
                        "交换",
                        &format!(
                            "{}  已使用 {}  {:.0}%  剩余 {}",
                            format_bytes_short(swap.total_bytes),
                            format_bytes_short(swap.used_bytes),
                            swap.percent,
                            format_bytes_short(swap.total_bytes.saturating_sub(swap.used_bytes)),
                        ),
                        &colors,
                    ));
                }

                if !snapshot.networks.is_empty() {
                    body_col.add_child(self.render_system_info_section(
                        "网络接口",
                        &colors,
                        |col| {
                            col.add_child(self.render_system_info_table_header(
                                &["名称", "发送", "接收", "发送速度", "接收速度"],
                                &[140.0, 110.0, 110.0, 110.0, 110.0],
                                &colors,
                            ));
                            let mut sorted = snapshot.networks.clone();
                            sorted.sort_by(|a, b| a.interface.cmp(&b.interface));
                            for net in sorted.iter() {
                                let tx_total =
                                    net.history.iter().map(|p| p.tx_bytes_per_sec).sum::<u64>();
                                let rx_total =
                                    net.history.iter().map(|p| p.rx_bytes_per_sec).sum::<u64>();
                                col.add_child(self.render_system_info_table_row(
                                    &[
                                        net.interface.clone(),
                                        format_bytes_short(tx_total),
                                        format_bytes_short(rx_total),
                                        format!("{}/s", format_bytes_short(net.tx_bytes_per_sec)),
                                        format!("{}/s", format_bytes_short(net.rx_bytes_per_sec)),
                                    ],
                                    &[140.0, 110.0, 110.0, 110.0, 110.0],
                                    &colors,
                                ));
                            }
                        },
                    ));
                }

                if !snapshot.disks.is_empty() {
                    body_col.add_child(self.render_system_info_section(
                        "文件系统",
                        &colors,
                        |col| {
                            col.add_child(self.render_system_info_table_header(
                                &["挂载点", "大小", "已用", "百分比", "可用"],
                                &[260.0, 90.0, 90.0, 80.0, 90.0],
                                &colors,
                            ));
                            for disk in snapshot.disks.iter() {
                                col.add_child(self.render_system_info_table_row(
                                    &[
                                        disk.mount.clone(),
                                        format_bytes_short(disk.total_bytes),
                                        format_bytes_short(disk.used_bytes),
                                        format!("{:.1}%", disk.percent),
                                        format_bytes_short(disk.available_bytes),
                                    ],
                                    &[260.0, 90.0, 90.0, 80.0, 90.0],
                                    &colors,
                                ));
                            }
                        },
                    ));
                }
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
            active_tab.system_info_scroll_state.clone(),
            Container::new(body_col.finish())
                .with_horizontal_padding(16.0)
                .with_vertical_padding(14.0)
                .finish(),
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
                .with_child(Expanded::new(1.0, body).finish())
                .finish(),
        )
        .with_background_color(colors.panel_bg)
        .finish()
    }

    fn render_system_info_row(
        &self,
        label: &str,
        value: &str,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        const LABEL_WIDTH: f32 = 84.0;
        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(
                    ConstrainedBox::new(
                        Text::new_inline(label.to_string(), self.ui_font, 12.0)
                            .with_color(colors.text_muted)
                            .finish(),
                    )
                    .with_width(LABEL_WIDTH)
                    .finish(),
                )
                .with_child(
                    Text::new_inline(value.to_string(), self.monospace_font, 12.0)
                        .with_color(colors.text_primary)
                        .finish(),
                )
                .finish(),
        )
        .with_padding_bottom(8.0)
        .finish()
    }

    fn render_system_info_section(
        &self,
        label: &str,
        colors: &HostOverviewColors,
        build: impl FnOnce(&mut warpui::elements::Flex),
    ) -> Box<dyn Element> {
        const LABEL_WIDTH: f32 = 84.0;
        let mut content_col = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        build(&mut content_col);
        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(
                    ConstrainedBox::new(
                        Text::new_inline(label.to_string(), self.ui_font, 12.0)
                            .with_color(colors.text_muted)
                            .finish(),
                    )
                    .with_width(LABEL_WIDTH)
                    .finish(),
                )
                .with_child(Expanded::new(1.0, content_col.finish()).finish())
                .finish(),
        )
        .with_padding_bottom(12.0)
        .finish()
    }

    fn render_system_info_table_header(
        &self,
        labels: &[&str],
        widths: &[f32],
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        for (label, width) in labels.iter().zip(widths.iter()) {
            row.add_child(
                ConstrainedBox::new(
                    Text::new_inline(label.to_string(), self.ui_font, 12.0)
                        .with_color(colors.text_muted)
                        .finish(),
                )
                .with_width(*width)
                .finish(),
            );
        }
        Container::new(row.finish())
            .with_padding_bottom(4.0)
            .finish()
    }

    fn render_system_info_table_row(
        &self,
        cells: &[String],
        widths: &[f32],
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        for (cell, width) in cells.iter().zip(widths.iter()) {
            row.add_child(
                ConstrainedBox::new(
                    Text::new_inline(cell.clone(), self.monospace_font, 12.0)
                        .with_color(colors.text_primary)
                        .finish(),
                )
                .with_width(*width)
                .finish(),
            );
        }
        Container::new(row.finish())
            .with_padding_bottom(4.0)
            .finish()
    }
}
