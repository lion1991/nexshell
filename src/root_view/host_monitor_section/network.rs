// host_monitor_section::network — 网络概览段 + 网络选择/图表 + 网络列表整页。
// 本文件只含 impl RootView，无自由函数。

use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::warp_dropdown::{
    render_warp_dropdown_with_top_bar, WarpDropdownCustomProps, WarpDropdownOption,
};
use crate::{RootView, TerminalSessionTab, ICON_PATH_CHEVRON_DOWN};
use nexshell::host_overview::{
    format_bytes_short, HostOverviewSnapshot, HostOverviewStatus, NetworkMetric, NetworkRatePoint,
    NetworkRow, NetworkRowKind, NetworkSortKey, SortDirection,
};
use std::sync::{Arc, Mutex};
use warp_core::ui::appearance::Appearance;
use warpui::color::ColorU;
use warpui::elements::{
    Border, Clipped, ClippedScrollable, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, Empty, Expanded, Flex, Hoverable, Icon, MainAxisSize, MouseState,
    MouseStateHandle, ParentElement, Radius, Shrinkable, Stack, Text,
};
use warpui::{fonts, AppContext, Element, SingletonEntity as _};

impl RootView {
    pub(in crate::root_view) fn render_overview_network_section(
        &self,
        tab: Option<&TerminalSessionTab>,
        snapshot: &HostOverviewSnapshot,
        colors: &HostOverviewColors,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_overview_network_title(tab, colors));

        if let Some(network) = self.selected_overview_network(tab, snapshot) {
            let mut top = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(self.render_network_selector(tab, network, snapshot, colors, app));
            top.add_child(Shrinkable::new(1.0, Empty::new().finish()).finish());
            let rate_text = |text: String, color: ColorU| {
                Text::new_inline(text, self.monospace_font, 12.0)
                    .with_color(color)
                    .finish()
            };
            let mut rates = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
            rates.add_child(rate_text(
                format!("↑ {}", format_bytes_short(network.tx_bytes_per_sec)),
                colors.upload,
            ));
            rates.add_child(
                Container::new(rate_text(
                    format!("↓ {}", format_bytes_short(network.rx_bytes_per_sec)),
                    colors.download,
                ))
                .with_margin_left(8.0)
                .finish(),
            );
            top.add_child(Clipped::new(rates.finish()).finish());
            column.add_child(
                Container::new(top.finish())
                    .with_padding_bottom(8.0)
                    .finish(),
            );
            column.add_child(self.render_network_chart(network, colors));
        } else {
            column.add_child(self.render_overview_muted_line(
                rust_i18n::t!("host_overview_no_network").as_ref(),
                colors,
            ));
        }

        let latency = snapshot
            .latency_ms
            .map(|ms| format!("{ms}ms"))
            .unwrap_or_else(|| "--".to_string());
        column.add_child(
            Container::new(self.render_overview_key_value(
                rust_i18n::t!("host_overview_latency").as_ref(),
                &latency,
                colors,
            ))
            .with_padding_top(10.0)
            .finish(),
        );

        // 延迟行自带 pb 7，补 5 与其他节的分隔线留白对齐
        Container::new(column.finish())
            .with_padding_bottom(5.0)
            .finish()
    }

    fn render_overview_network_title(
        &self,
        tab: Option<&TerminalSessionTab>,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let expand_state = tab.map(|t| t.host_overview_network_expand_state.clone());
        self.render_overview_expandable_section_title(
            rust_i18n::t!("host_overview_section_network").as_ref(),
            tab,
            expand_state,
            TerminalGridAction::OpenNetworkList,
            colors,
        )
    }

    fn render_network_selector(
        &self,
        tab: Option<&TerminalSessionTab>,
        selected: &NetworkMetric,
        snapshot: &HostOverviewSnapshot,
        colors: &HostOverviewColors,
        app: &AppContext,
    ) -> Box<dyn Element> {
        const SELECTOR_WIDTH: f32 = 96.0;
        const SELECTOR_HEIGHT: f32 = 22.0;

        let Some(tab) = tab else {
            return ConstrainedBox::new(
                Clipped::new(
                    Text::new_inline(selected.interface.clone(), self.monospace_font, 12.0)
                        .with_color(colors.text_primary)
                        .finish(),
                )
                .finish(),
            )
            .with_width(SELECTOR_WIDTH)
            .finish();
        };

        if snapshot.networks.len() <= 1 {
            return ConstrainedBox::new(
                Clipped::new(
                    Text::new_inline(selected.interface.clone(), self.monospace_font, 12.0)
                        .with_color(colors.text_primary)
                        .finish(),
                )
                .finish(),
            )
            .with_width(SELECTOR_WIDTH)
            .finish();
        }

        let state = tab.host_overview_network_dropdown_state.clone();
        let label = selected.interface.clone();
        let font = self.monospace_font;
        let text_color = colors.text_primary;
        let icon_color = colors.text_muted;
        let bg = colors.card_bg;
        let hover_bg = colors.metric_track;
        let border = colors.panel_border;
        let top_bar = Hoverable::new(state, move |mouse| {
            let mut row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Shrinkable::new(
                        1.0,
                        Clipped::new(
                            Text::new_inline(label.clone(), font, 12.0)
                                .with_color(text_color)
                                .finish(),
                        )
                        .finish(),
                    )
                    .finish(),
                );
            row.add_child(
                Container::new(
                    ConstrainedBox::new(Icon::new(ICON_PATH_CHEVRON_DOWN, icon_color).finish())
                        .with_width(12.0)
                        .with_height(12.0)
                        .finish(),
                )
                .with_margin_left(6.0)
                .finish(),
            );

            let background = if mouse.is_hovered() { hover_bg } else { bg };
            Container::new(row.finish())
                .with_padding_left(8.0)
                .with_padding_right(7.0)
                .with_background_color(background)
                .with_border(Border::all(1.0).with_border_color(border))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                .finish()
        })
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::ToggleHostNetworkDropdown);
        })
        .finish();

        let options = snapshot
            .networks
            .iter()
            .map(|network| {
                let label = network.interface.clone();
                WarpDropdownOption {
                    label: label.clone(),
                    action: TerminalGridAction::SelectHostNetwork(label.clone()),
                    selected: network.interface == selected.interface,
                    state: self.host_overview_network_item_state(tab, &label),
                    icon_path: None,
                    shortcut: None,
                }
            })
            .collect();

        ConstrainedBox::new(render_warp_dropdown_with_top_bar(WarpDropdownCustomProps {
            position_id: "host_overview_network_dropdown_top_bar",
            top_bar,
            is_open: tab.host_overview.network_dropdown_open,
            options,
            appearance: Appearance::as_ref(app),
            menu_width: SELECTOR_WIDTH,
            top_bar_height: SELECTOR_HEIGHT,
        }))
        .with_width(SELECTOR_WIDTH)
        .with_height(SELECTOR_HEIGHT)
        .finish()
    }

    fn host_overview_network_item_state(
        &self,
        tab: &TerminalSessionTab,
        interface: &str,
    ) -> MouseStateHandle {
        tab.host_overview_network_item_states
            .borrow_mut()
            .entry(interface.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone()
    }

    fn render_network_chart(
        &self,
        network: &NetworkMetric,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        const CHART_WIDTH: f32 = 228.0;
        const CHART_INNER_WIDTH: f32 = 216.0;
        const CHART_HEIGHT: f32 = 36.0;
        const BAR_WIDTH: f32 = 2.5;
        const BAR_MARGIN_RIGHT: f32 = 2.0;

        // 借用 history 切片渲染，避免每帧 clone
        let fallback = [NetworkRatePoint {
            rx_bytes_per_sec: network.rx_bytes_per_sec,
            tx_bytes_per_sec: network.tx_bytes_per_sec,
        }];
        let points: &[NetworkRatePoint] = if network.history.is_empty() {
            &fallback
        } else {
            &network.history
        };
        let max_points =
            ((CHART_INNER_WIDTH / (BAR_WIDTH + BAR_MARGIN_RIGHT)).floor() as usize).max(1);
        let visible = &points[points.len().saturating_sub(max_points)..];
        let raw_max = visible
            .iter()
            .map(|point| point.rx_bytes_per_sec.max(point.tx_bytes_per_sec))
            .max()
            .unwrap_or(0);
        let max_value = raw_max.max(1) as f32;

        // 最新样本贴右边、向左生长；历史不足时左侧留白（Expanded 撑开）
        let mut chart = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Expanded::new(1.0, Empty::new().finish()).finish());
        for point in visible {
            let tx_height = ((point.tx_bytes_per_sec as f32 / max_value) * CHART_HEIGHT)
                .clamp(1.0, CHART_HEIGHT);
            let rx_height = ((point.rx_bytes_per_sec as f32 / max_value) * CHART_HEIGHT)
                .clamp(1.0, CHART_HEIGHT);
            // 高的画底层、矮的画前层，避免一方被完全遮住
            let mut layers = [(tx_height, colors.upload), (rx_height, colors.download)];
            layers.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut bar_stack = Stack::new();
            bar_stack.add_child(
                ConstrainedBox::new(
                    Container::new(Empty::new().finish())
                        .with_background_color(colors.chart_grid)
                        .finish(),
                )
                .with_width(BAR_WIDTH)
                .with_height(CHART_HEIGHT)
                .finish(),
            );
            for (height, color) in layers {
                bar_stack.add_child(
                    Container::new(
                        ConstrainedBox::new(
                            Container::new(Empty::new().finish())
                                .with_background_color(color)
                                .finish(),
                        )
                        .with_width(BAR_WIDTH)
                        .with_height(height)
                        .finish(),
                    )
                    .with_padding_top(CHART_HEIGHT - height)
                    .finish(),
                );
            }
            chart.add_child(
                Container::new(
                    ConstrainedBox::new(bar_stack.finish())
                        .with_width(BAR_WIDTH)
                        .with_height(CHART_HEIGHT)
                        .finish(),
                )
                .with_margin_right(BAR_MARGIN_RIGHT)
                .finish(),
            );
        }

        let mut card_stack = Stack::new();
        card_stack.add_child(
            ConstrainedBox::new(Clipped::new(chart.finish()).finish())
                .with_width(CHART_INNER_WIDTH)
                .with_height(CHART_HEIGHT)
                .finish(),
        );
        // 右上角峰值刻度，给柱高一个参照
        if raw_max > 0 {
            let peak_text = rust_i18n::t!(
                "host_overview_peak",
                rate = format!("{}/s", format_bytes_short(raw_max))
            )
            .to_string();
            let scrim = ColorU::new(
                colors.panel_bg.r,
                colors.panel_bg.g,
                colors.panel_bg.b,
                0xaa,
            );
            card_stack.add_child(
                ConstrainedBox::new(
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
                        .with_child(
                            Container::new(
                                Text::new_inline(peak_text, self.ui_font, 10.0)
                                    .with_color(colors.text_muted)
                                    .finish(),
                            )
                            .with_horizontal_padding(4.0)
                            .with_vertical_padding(1.0)
                            .with_background_color(scrim)
                            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.0)))
                            .finish(),
                        )
                        .finish(),
                )
                .with_width(CHART_INNER_WIDTH)
                .finish(),
            );
        }

        ConstrainedBox::new(
            Container::new(card_stack.finish())
                .with_background_color(colors.card_bg)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(5.0)))
                .with_uniform_padding(6.0)
                .finish(),
        )
        .with_width(CHART_WIDTH)
        .finish()
    }

    fn selected_overview_network<'a>(
        &self,
        tab: Option<&TerminalSessionTab>,
        snapshot: &'a HostOverviewSnapshot,
    ) -> Option<&'a NetworkMetric> {
        if let Some(selected) = tab.and_then(|tab| tab.host_overview.selected_network.as_deref()) {
            if let Some(network) = snapshot
                .networks
                .iter()
                .find(|network| network.interface == selected)
            {
                return Some(network);
            }
        }

        if let Some(default_network) = snapshot.network.as_ref() {
            if let Some(network) = snapshot
                .networks
                .iter()
                .find(|network| network.interface == default_network.interface)
            {
                return Some(network);
            }
            return Some(default_network);
        }

        snapshot.networks.first()
    }

    pub(in crate::root_view) fn render_network_list_page(
        &self,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = &self.cached_warp_theme;
        let colors = HostOverviewColors::from_theme(theme);
        let active_tab = match self.terminal_tabs.get(self.active_tab_index) {
            Some(tab) => tab,
            None => return Container::new(Empty::new().finish()).finish(),
        };
        let sort_key = active_tab.host_overview.network_sort_key;
        let sort_dir = active_tab.host_overview.network_sort_direction;
        let mut sorted: Vec<NetworkRow> = active_tab.host_overview.snapshot.sockets.clone();
        sorted.sort_by(|a, b| match sort_key {
            NetworkSortKey::Pid => a.pid.cmp(&b.pid),
            NetworkSortKey::Process => a.process.cmp(&b.process),
            NetworkSortKey::LocalAddr => a.local_addr.cmp(&b.local_addr),
            NetworkSortKey::LocalPort => a.local_port.cmp(&b.local_port),
            NetworkSortKey::UniqueIps => a.unique_ips.cmp(&b.unique_ips),
            NetworkSortKey::Connections => a.connections.cmp(&b.connections),
            NetworkSortKey::RxBytes => a.rx_bytes.cmp(&b.rx_bytes),
            NetworkSortKey::TxBytes => a.tx_bytes.cmp(&b.tx_bytes),
        });
        if matches!(sort_dir, SortDirection::Desc) {
            sorted.reverse();
        }

        let title_label = active_tab.label();
        let count_text = format!("{} 个连接", sorted.len());

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

        let header = self.render_network_list_header(Some(active_tab), sort_key, &colors);

        let mut body_col = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        if sorted.is_empty() {
            let status_text = match &active_tab.host_overview.snapshot.status {
                HostOverviewStatus::Waiting | HostOverviewStatus::Collecting => {
                    rust_i18n::t!("network_list_placeholder").to_string()
                }
                HostOverviewStatus::Error(msg) => {
                    format!("{}: {}", rust_i18n::t!("network_list_error"), msg)
                }
                HostOverviewStatus::Ready => rust_i18n::t!("network_list_empty").to_string(),
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
            for (index, row) in sorted.iter().enumerate() {
                body_col.add_child(self.render_network_list_row(row, index, &colors));
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
            active_tab.network_list_scroll_state.clone(),
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

    fn render_network_header_cell(
        &self,
        tab: Option<&TerminalSessionTab>,
        key: NetworkSortKey,
        label: &str,
        active_key: NetworkSortKey,
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
            NetworkSortKey::Pid => 0,
            NetworkSortKey::Process => 1,
            NetworkSortKey::LocalAddr => 2,
            NetworkSortKey::LocalPort => 3,
            NetworkSortKey::UniqueIps => 4,
            NetworkSortKey::Connections => 5,
            NetworkSortKey::RxBytes => 6,
            NetworkSortKey::TxBytes => 7,
        };
        let state = tab.network_list_header_states[idx].clone();
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
            ctx.dispatch_typed_action(TerminalGridAction::SortHostNetwork(key));
        })
        .finish()
    }

    fn render_network_list_header(
        &self,
        tab: Option<&TerminalSessionTab>,
        active_key: NetworkSortKey,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let build_inner =
            |label: &str, key: NetworkSortKey, align_right: bool| -> Box<dyn Element> {
                let text = self.render_network_header_cell(tab, key, label, active_key, colors);
                let active = key == active_key;
                let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
                if align_right {
                    row.add_child(Expanded::new(1.0, Empty::new().finish()).finish());
                }
                row.add_child(text);
                if active {
                    let arrow = if matches!(
                        tab.map(|t| t.host_overview.network_sort_direction)
                            .unwrap_or(SortDirection::Asc),
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
            |label: &str, width: f32, key: NetworkSortKey, align_right: bool| -> Box<dyn Element> {
                ConstrainedBox::new(build_inner(label, key, align_right))
                    .with_width(width)
                    .finish()
            };
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(fixed("PID", 70.0, NetworkSortKey::Pid, false))
            .with_child(
                Expanded::new(2.0, build_inner("名称", NetworkSortKey::Process, false)).finish(),
            )
            .with_child(
                Expanded::new(2.0, build_inner("监听IP", NetworkSortKey::LocalAddr, false))
                    .finish(),
            )
            .with_child(fixed("端口", 70.0, NetworkSortKey::LocalPort, true))
            .with_child(fixed("IP数", 60.0, NetworkSortKey::UniqueIps, true))
            .with_child(fixed("连接数", 70.0, NetworkSortKey::Connections, true))
            .with_child(fixed("上传", 80.0, NetworkSortKey::TxBytes, true))
            .with_child(fixed("下载", 80.0, NetworkSortKey::RxBytes, true))
            .finish();
        Container::new(row)
            .with_horizontal_padding(16.0)
            .with_vertical_padding(8.0)
            .with_background_color(colors.card_bg)
            .with_border(Border::bottom(1.0).with_border_color(colors.panel_border))
            .finish()
    }

    fn render_network_list_row(
        &self,
        row: &NetworkRow,
        index: usize,
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
        let pid_text = row
            .pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());
        let process_text = if row.process.is_empty() {
            "-".to_string()
        } else {
            row.process.clone()
        };
        let local_addr_text = match row.kind {
            NetworkRowKind::Listen => row.local_addr.clone(),
            // 出站行的"监听IP"列没有意义，留 "-" 让用户一眼看出
            NetworkRowKind::Outbound => "-".to_string(),
        };
        let port_text = match row.kind {
            NetworkRowKind::Listen => row.local_port.to_string(),
            NetworkRowKind::Outbound => row
                .remote_port
                .map(|p| p.to_string())
                .unwrap_or_else(|| row.local_port.to_string()),
        };
        let unique_ips_text = row.unique_ips.to_string();
        let conn_text = row.connections.to_string();
        let tx_text = if row.tx_bytes == 0 {
            "0".to_string()
        } else {
            format_bytes_short(row.tx_bytes)
        };
        let rx_text = if row.rx_bytes == 0 {
            "0".to_string()
        } else {
            format_bytes_short(row.rx_bytes)
        };

        let flex_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(fixed(&pid_text, 70.0, true, false))
            .with_child(Expanded::new(2.0, build_inner(&process_text, false, false)).finish())
            .with_child(Expanded::new(2.0, build_inner(&local_addr_text, true, false)).finish())
            .with_child(fixed(&port_text, 70.0, false, true))
            .with_child(fixed(&unique_ips_text, 60.0, true, true))
            .with_child(fixed(&conn_text, 70.0, true, true))
            .with_child(fixed(&tx_text, 80.0, false, true))
            .with_child(fixed(&rx_text, 80.0, false, true))
            .finish();
        let bg = if index % 2 == 0 {
            colors.panel_bg
        } else {
            colors.card_bg
        };
        Container::new(flex_row)
            .with_horizontal_padding(16.0)
            .with_vertical_padding(4.0)
            .with_background_color(bg)
            .finish()
    }
}
