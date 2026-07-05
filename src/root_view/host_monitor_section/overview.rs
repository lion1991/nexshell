// host_monitor_section::overview — 概览侧栏：assembler（render_sidebar_panel）+ 通用渲染件（头部/状态/chip/键值/分隔行等）。
// 本文件只含 impl RootView，无自由函数。

use crate::host_monitor_view_helpers::{
    format_kernel_short, format_uptime, overview_status_dot_color,
};
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{RootView, TerminalSessionTab, ICON_PATH_COPY, ICON_PATH_EXPAND};
use nexshell::host_overview::{
    should_show_empty_overview_status, HostOverviewSnapshot, HostOverviewStatus,
};
use warpui::color::ColorU;
use warpui::elements::{
    Border, Clipped, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Empty, Expanded,
    Flex, Hoverable, Icon, MainAxisSize, MouseStateHandle, ParentElement, Radius, Shrinkable, Text,
};
use warpui::{fonts, AppContext, Element};

impl RootView {
    // 终端 tab 内嵌的主机监控左侧栏 assembler；与 render_file_panel / render_git_panel 对称，
    // 由 terminal_section 的 render_active_tab_body_with_side_panels 跨 section 调用。
    pub(in crate::root_view) fn render_sidebar_panel(&self, app: &AppContext) -> Box<dyn Element> {
        const SIDEBAR_WIDTH: f32 = 248.0;
        let colors = HostOverviewColors::from_theme(&self.cached_warp_theme);
        let waiting_snapshot;
        let active_tab = self.terminal_tabs.get(self.active_tab_index);
        let snapshot = if let Some(tab) = active_tab {
            &tab.host_overview.snapshot
        } else {
            waiting_snapshot = HostOverviewSnapshot::waiting(
                rust_i18n::t!("host_overview_not_connected").to_string(),
            );
            &waiting_snapshot
        };

        let mut content = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_overview_header(active_tab, snapshot, &colors));

        if should_show_empty_overview_status(snapshot) {
            content.add_child(self.render_overview_status(snapshot, &colors));
        }
        content.add_child(self.render_overview_divider(&colors));
        content.add_child(self.render_overview_system_section(active_tab, snapshot, &colors));
        content.add_child(self.render_overview_divider(&colors));
        content.add_child(self.render_overview_process_section(active_tab, snapshot, &colors));
        content.add_child(self.render_overview_divider(&colors));
        content.add_child(self.render_overview_network_section(active_tab, snapshot, &colors, app));
        content.add_child(self.render_overview_divider(&colors));
        content.add_child(self.render_overview_disk_section(active_tab, snapshot, &colors));

        Container::new(
            ConstrainedBox::new(
                Container::new(content.finish())
                    .with_padding_left(10.0)
                    .with_padding_right(10.0)
                    .with_padding_top(12.0)
                    .with_padding_bottom(10.0)
                    .finish(),
            )
            .with_width(SIDEBAR_WIDTH)
            .finish(),
        )
        .with_background_color(colors.panel_bg)
        .with_border(Border::right(1.0).with_border_color(colors.panel_border))
        .finish()
    }

    pub(in crate::root_view) fn render_overview_header(
        &self,
        tab: Option<&TerminalSessionTab>,
        snapshot: &HostOverviewSnapshot,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let title = snapshot
            .hostname
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| rust_i18n::t!("host_overview_fallback_title").to_string());
        let host_text = snapshot.host.trim().to_string();
        let subtitle = if host_text.is_empty() {
            rust_i18n::t!("host_overview_subtitle_placeholder").to_string()
        } else {
            host_text.clone()
        };
        let mut meta = Vec::new();
        if let Some(secs) = snapshot.uptime_seconds {
            meta.push(format_uptime(secs));
        }
        if let Some(kernel) = snapshot.kernel.as_deref().and_then(format_kernel_short) {
            meta.push(kernel);
        }

        let mut subtitle_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Shrinkable::new(
                    1.0,
                    Clipped::new(
                        Text::new_inline(subtitle, self.ui_font, 12.0)
                            .with_color(colors.text_muted)
                            .finish(),
                    )
                    .finish(),
                )
                .finish(),
            );
        if !host_text.is_empty() {
            if let Some(tab) = tab {
                subtitle_row.add_child(
                    Container::new(self.render_copy_address_button(tab, &host_text, colors))
                        .with_margin_left(6.0)
                        .finish(),
                );
            }
        }

        // 标题前的连接状态点：错误红 / 未连接灰 / 已连接绿（延迟 ≥400ms 黄）
        let title_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Container::new(
                    ConstrainedBox::new(
                        Container::new(Empty::new().finish())
                            .with_background_color(overview_status_dot_color(
                                &snapshot.status,
                                snapshot.latency_ms,
                                colors,
                            ))
                            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                            .finish(),
                    )
                    .with_width(8.0)
                    .with_height(8.0)
                    .finish(),
                )
                .with_margin_right(7.0)
                .finish(),
            )
            .with_child(
                Shrinkable::new(
                    1.0,
                    Clipped::new(
                        Text::new_inline(title, self.ui_font, 16.0)
                            .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
                            .with_color(colors.text_primary)
                            .finish(),
                    )
                    .finish(),
                )
                .finish(),
            );

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(title_row.finish())
            .with_child(
                Container::new(subtitle_row.finish())
                    .with_padding_top(3.0)
                    .finish(),
            );
        if !meta.is_empty() {
            column.add_child(
                Container::new(
                    Clipped::new(
                        Text::new_inline(meta.join(" · "), self.ui_font, 11.0)
                            .with_color(colors.text_muted)
                            .finish(),
                    )
                    .finish(),
                )
                .with_padding_top(4.0)
                .finish(),
            );
        }

        Container::new(column.finish())
            .with_padding_bottom(10.0)
            .finish()
    }

    // 分节 1px 细线，统一节间节奏
    fn render_overview_divider(&self, colors: &HostOverviewColors) -> Box<dyn Element> {
        Container::new(
            ConstrainedBox::new(
                Container::new(Empty::new().finish())
                    .with_background_color(colors.panel_border)
                    .finish(),
            )
            .with_height(1.0)
            .finish(),
        )
        .with_margin_bottom(10.0)
        .finish()
    }

    fn render_copy_address_button(
        &self,
        tab: &TerminalSessionTab,
        text: &str,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        const BUTTON_SIZE: f32 = 22.0;
        const ICON_SIZE: f32 = 13.0;
        let state = tab.host_overview_copy_button_state.clone();
        let host_only = text.rsplit_once('@').map(|(_, rest)| rest).unwrap_or(text);
        let payload = host_only
            .rsplit_once(':')
            .map(|(host, _)| host.to_string())
            .unwrap_or_else(|| host_only.to_string());
        let muted = colors.text_muted;
        let active = colors.text_primary;
        let hover_bg = colors.metric_track;
        Hoverable::new(state, move |mouse| {
            let icon_color = if mouse.is_hovered() { active } else { muted };
            let bg = if mouse.is_hovered() {
                hover_bg
            } else {
                ColorU::new(0, 0, 0, 0)
            };
            ConstrainedBox::new(
                Container::new(
                    ConstrainedBox::new(Icon::new(ICON_PATH_COPY, icon_color).finish())
                        .with_width(ICON_SIZE)
                        .with_height(ICON_SIZE)
                        .finish(),
                )
                .with_background_color(bg)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                .with_uniform_padding((BUTTON_SIZE - ICON_SIZE) / 2.0)
                .finish(),
            )
            .with_width(BUTTON_SIZE)
            .with_height(BUTTON_SIZE)
            .finish()
        })
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::CopyHostAddress(payload.clone()));
        })
        .finish()
    }

    pub(in crate::root_view) fn render_overview_status(
        &self,
        snapshot: &HostOverviewSnapshot,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let (text, color) = match &snapshot.status {
            HostOverviewStatus::Waiting => (
                rust_i18n::t!("host_overview_status_waiting").to_string(),
                colors.text_muted,
            ),
            HostOverviewStatus::Collecting if snapshot.has_collected_data() => (
                rust_i18n::t!("host_overview_status_updating").to_string(),
                colors.text_muted,
            ),
            HostOverviewStatus::Collecting => (
                rust_i18n::t!("host_overview_status_collecting").to_string(),
                colors.text_muted,
            ),
            HostOverviewStatus::Ready if !snapshot.has_collected_data() => (
                rust_i18n::t!("host_overview_status_empty").to_string(),
                colors.text_muted,
            ),
            HostOverviewStatus::Ready => (
                rust_i18n::t!("host_overview_status_ready").to_string(),
                colors.text_muted,
            ),
            HostOverviewStatus::Error(error) => (
                rust_i18n::t!("host_overview_status_error", error = error).to_string(),
                colors.warning,
            ),
        };

        Container::new(
            Text::new_inline(text, self.ui_font, 11.0)
                .with_color(color)
                .finish(),
        )
        .with_padding_bottom(10.0)
        .finish()
    }

    pub(in crate::root_view) fn render_overview_key_value(
        &self,
        label: &str,
        value: &str,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let mut row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(label.to_string(), self.ui_font, 12.0)
                    .with_color(colors.text_muted)
                    .finish(),
            );
        row.add_child(Shrinkable::new(1.0, Empty::new().finish()).finish());
        row.add_child(
            Text::new_inline(value.to_string(), self.monospace_font, 12.0)
                .with_color(colors.text_primary)
                .finish(),
        );

        Container::new(row.finish())
            .with_padding_bottom(7.0)
            .finish()
    }

    pub(in crate::root_view) fn render_overview_muted_line(
        &self,
        text: &str,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        Container::new(
            Text::new_inline(text.to_string(), self.ui_font, 12.0)
                .with_color(colors.text_muted)
                .finish(),
        )
        .with_padding_bottom(8.0)
        .finish()
    }

    pub(in crate::root_view) fn render_overview_expandable_section_title(
        &self,
        title: &str,
        tab: Option<&TerminalSessionTab>,
        expand_state: Option<MouseStateHandle>,
        action: TerminalGridAction,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let title_text = Text::new_inline(title.to_string(), self.ui_font, 11.0)
            .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
            .with_color(colors.section_title)
            .finish();
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(title_text)
            .with_child(Expanded::new(1.0, Empty::new().finish()).finish());
        if let (Some(tab), Some(state)) = (tab, expand_state) {
            // 仅挂着 host_id 的 tab 显示展开按钮
            if tab.host_id.is_some() {
                let icon_color = colors.section_title;
                let icon_color_hover = colors.text_primary;
                let btn = Hoverable::new(state, move |mouse| {
                    let color = if mouse.is_hovered() {
                        icon_color_hover
                    } else {
                        icon_color
                    };
                    Container::new(
                        ConstrainedBox::new(Icon::new(ICON_PATH_EXPAND, color).finish())
                            .with_width(12.0)
                            .with_height(12.0)
                            .finish(),
                    )
                    .with_uniform_padding(2.0)
                    .finish()
                })
                .with_cursor(warpui::platform::Cursor::PointingHand)
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(action.clone());
                })
                .finish();
                row.add_child(btn);
            }
        }
        Container::new(row.finish())
            .with_padding_bottom(6.0)
            .finish()
    }
}
