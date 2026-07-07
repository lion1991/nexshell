// 多主机状态总览卡片网格（ServerCat 风格）。本文件只放纯渲染自由函数。
// 数据来自 RootView 的 HostOverviewFleet；点击卡片 = 快速连接该主机。
// 指标用部分填充水平进度条（warpui 无 arc，弧形环做不了，进度条更直观）。

use warpui::{
    color::ColorU,
    elements::{
        Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Empty, Expanded, Flex,
        Hoverable, Icon, MainAxisSize, ParentElement, Radius, Text,
    },
    fonts, Element,
};

use nexshell::host_management::HostCardSnapshot;
use nexshell::host_overview::{HostOverviewSnapshot, HostOverviewStatus, HostOverviewUiState};
use nexshell::host_overview_fleet::HostOverviewFleet;

use crate::host_management_view::constants::*;
use crate::host_management_view::host_card::HostCardStates;
use crate::terminal_grid_element::TerminalGridAction;

const GRID_COLUMNS: usize = 3;
const BAR_HEIGHT: f32 = 9.0;

/// 指标百分比 → 语义色档位（ok / warn / danger）。
fn level_color(percent: f32, hc: &HostUiColors) -> ColorU {
    if percent >= 85.0 {
        hc.semantic.danger
    } else if percent >= 60.0 {
        hc.semantic.warn
    } else {
        hc.semantic.ok
    }
}

/// 速率拆成 (数值, 单位)，如 72704 → ("71","K")。
fn split_rate(bytes_per_sec: u64) -> (String, String) {
    let b = bytes_per_sec as f64;
    if b >= 1024.0 * 1024.0 {
        (format!("{:.0}", b / 1024.0 / 1024.0), "M".to_string())
    } else if b >= 1024.0 {
        (format!("{:.0}", b / 1024.0), "K".to_string())
    } else {
        (format!("{bytes_per_sec}"), "B".to_string())
    }
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    if days > 0 {
        format!("{days}d {hours}h")
    } else {
        let mins = (seconds % 3_600) / 60;
        format!("{hours}h {mins}m")
    }
}

pub fn render_status_view(
    hosts: &[HostCardSnapshot],
    fleet: &HostOverviewFleet,
    states: &HostCardStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    // 仅 SSH 主机有 host overview 采集，串口等其他类型不在状态总览出现
    let ssh_hosts: Vec<&HostCardSnapshot> = hosts
        .iter()
        .filter(|host| host.protocol.eq_ignore_ascii_case("SSH"))
        .collect();
    if ssh_hosts.is_empty() {
        return Container::new(Empty::new().finish())
            .with_background_color(hc.panel_bg)
            .finish();
    }

    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    for row_start in (0..ssh_hosts.len()).step_by(GRID_COLUMNS) {
        if row_start > 0 {
            col.add_child(spacer_v(CARD_SPACING));
        }
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start);

        let row_end = (row_start + GRID_COLUMNS).min(ssh_hosts.len());
        for index in row_start..row_end {
            if index > row_start {
                row.add_child(spacer_h(CARD_SPACING));
            }
            let host = ssh_hosts[index];
            let card = render_status_card(host, index, fleet.state(&host.id), states, ui_font, hc);
            row.add_child(Expanded::new(1.0, card).finish());
        }
        for _ in 0..(GRID_COLUMNS - (row_end - row_start)) {
            row.add_child(spacer_h(CARD_SPACING));
            row.add_child(Expanded::new(1.0, Empty::new().finish()).finish());
        }
        col.add_child(row.finish());
    }

    Container::new(col.finish())
        .with_horizontal_padding(24.0)
        .with_vertical_padding(CARD_SPACING)
        .with_background_color(hc.panel_bg)
        .finish()
}

fn render_status_card(
    host: &HostCardSnapshot,
    index: usize,
    state: Option<&HostOverviewUiState>,
    states: &HostCardStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    let card_state = states.card_states[index].clone();
    let host_id = host.id.clone();
    let connect_state = states.connect_states[index].clone();
    let name = host.name.clone();
    let snapshot = state.map(|s| s.snapshot.clone());

    Hoverable::new(card_state, move |mouse| {
        let bg = if mouse.is_hovered() {
            hc.card_bg_hover
        } else {
            hc.card_bg
        };
        let border_color = if mouse.is_hovered() {
            hc.card_border_hover
        } else {
            hc.card_border
        };

        let cpu = snapshot.as_ref().and_then(|s| s.cpu_percent);
        let mem = snapshot
            .as_ref()
            .and_then(|s| s.memory.as_ref())
            .map(|m| m.percent);
        let load = snapshot.as_ref().and_then(|s| s.load_average).map(|l| l[0]);
        let (up, down) = snapshot
            .as_ref()
            .and_then(|s| s.network.as_ref())
            .map(|n| (Some(n.tx_bytes_per_sec), Some(n.rx_bytes_per_sec)))
            .unwrap_or((None, None));
        let disk_read = snapshot.as_ref().and_then(|s| s.disk_read_bytes_per_sec);
        let disk_write = snapshot.as_ref().and_then(|s| s.disk_write_bytes_per_sec);
        let uptime = snapshot.as_ref().and_then(|s| s.uptime_seconds);
        let (status_text, status_color) = status_indicator(snapshot.as_ref(), &hc);

        // 顶部：主机名 + 连接状态 + 终端快连图标
        let header = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new_inline(name.clone(), ui_font, 14.0)
                    .with_color(hc.text_primary)
                    .finish(),
            )
            .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
            .with_child(
                Text::new_inline(status_text.clone(), ui_font, UI_FONT_SIZE + 1.0)
                    .with_color(status_color)
                    .finish(),
            )
            .with_child(spacer_h(10.0))
            .with_child(render_terminal_button(
                connect_state.clone(),
                host_id.clone(),
                &hc,
            ))
            .finish();

        let content = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(header)
            .with_child(spacer_v(14.0))
            .with_child(render_metric_bar("CPU", cpu, ui_font, &hc))
            .with_child(spacer_v(9.0))
            .with_child(render_metric_bar("Mem", mem, ui_font, &hc))
            .with_child(spacer_v(16.0))
            .with_child(divider(&hc))
            .with_child(spacer_v(14.0))
            .with_child(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Start)
                    .with_child(
                        Expanded::new(
                            1.0,
                            render_io_group("网络", "↑ 上行", up, "↓ 下行", down, ui_font, &hc),
                        )
                        .finish(),
                    )
                    .with_child(spacer_h(16.0))
                    .with_child(
                        Expanded::new(
                            1.0,
                            render_io_group(
                                "磁盘", "Read", disk_read, "Write", disk_write, ui_font, &hc,
                            ),
                        )
                        .finish(),
                    )
                    .finish(),
            )
            .with_child(spacer_v(14.0))
            .with_child(divider(&hc))
            .with_child(spacer_v(12.0))
            .with_child(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(render_labeled_value("Load", opt_num(load, 2), ui_font, &hc))
                    .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
                    .with_child(render_labeled_value(
                        "Up",
                        uptime.map(format_uptime).unwrap_or_else(dash),
                        ui_font,
                        &hc,
                    ))
                    .finish(),
            )
            .finish();

        Container::new(content)
            .with_horizontal_padding(CARD_PADDING)
            .with_vertical_padding(CARD_PADDING)
            .with_background_color(bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(CARD_CORNER_RADIUS)))
            .with_border(Border::all(1.0).with_border_color(border_color))
            .finish()
    })
    .finish()
}

/// 右上角终端快连按钮：仅点它才连接（卡片其他区域不响应）。
fn render_terminal_button(
    state: warpui::elements::MouseStateHandle,
    host_id: String,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let hc = *hc;
    Hoverable::new(state, move |mouse| {
        let color = if mouse.is_hovered() {
            hc.text_primary
        } else {
            hc.text_secondary
        };
        Container::new(
            ConstrainedBox::new(Icon::new(ICON_TERMINAL, color).finish())
                .with_width(ICON_SIZE_SM)
                .with_height(ICON_SIZE_SM)
                .finish(),
        )
        .with_horizontal_padding(4.0)
        .with_vertical_padding(2.0)
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(TerminalGridAction::HostQuickConnect(host_id.clone()));
    })
    .finish()
}

/// 一行指标进度条：「标签  ▓▓▓░░░░░  值%」。进度条占满中间，标签/数值分居两端。
fn render_metric_bar(
    label: &str,
    value: Option<f32>,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let value_text = value.map(|v| format!("{:.0}%", v)).unwrap_or_else(dash);
    Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            ConstrainedBox::new(
                Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE_SMALL)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .with_width(40.0)
            .finish(),
        )
        .with_child(spacer_h(10.0))
        .with_child(Expanded::new(1.0, render_bar(value, hc)).finish())
        .with_child(spacer_h(12.0))
        .with_child(bold_text(
            value_text,
            ui_font,
            UI_FONT_SIZE + 1.0,
            hc.text_primary,
        ))
        .finish()
}

/// 自适应宽进度条：底槽 + 按比例填充段（档位色），圆角；宽度由外层 Expanded 决定。
fn render_bar(value: Option<f32>, hc: &HostUiColors) -> Box<dyn Element> {
    let percent = value.unwrap_or(0.0).clamp(0.0, 100.0);
    let fill_weight = if value.is_some() {
        percent.max(0.001)
    } else {
        0.001
    };
    let rest_weight = (100.0 - percent).max(0.001);
    let fill_color = level_color(percent, hc);

    let track = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Expanded::new(
                fill_weight,
                Container::new(Empty::new().finish())
                    .with_background_color(fill_color)
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BAR_HEIGHT / 2.0)))
                    .finish(),
            )
            .finish(),
        )
        .with_child(Expanded::new(rest_weight, Empty::new().finish()).finish())
        .finish();

    ConstrainedBox::new(
        Container::new(track)
            .with_background_color(hc.card_border)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BAR_HEIGHT / 2.0)))
            .finish(),
    )
    .with_height(BAR_HEIGHT)
    .finish()
}

/// 「标签 值」一组：标签灰常规 + 值加粗。
fn render_labeled_value(
    label: &str,
    value: String,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_child(spacer_h(6.0))
        .with_child(bold_text(value, ui_font, UI_FONT_SIZE, hc.text_primary))
        .finish()
}

/// 加粗文字。
fn bold_text(
    content: String,
    ui_font: fonts::FamilyId,
    size: f32,
    color: ColorU,
) -> Box<dyn Element> {
    Text::new_inline(content, ui_font, size)
        .with_color(color)
        .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
        .finish()
}

/// IO 分组：分区标题 + 两行「标签 …… 数值」。
fn render_io_group(
    title: &str,
    label1: &str,
    value1: Option<u64>,
    label2: &str,
    value2: Option<u64>,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Text::new_inline(title.to_string(), ui_font, UI_FONT_SIZE_SMALL)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_child(spacer_v(10.0))
        .with_child(render_io_row(label1, value1, ui_font, hc))
        .with_child(spacer_v(8.0))
        .with_child(render_io_row(label2, value2, ui_font, hc))
        .finish()
}

/// 一行 IO：左标签（灰）+ 右数值（大）。
fn render_io_row(
    label: &str,
    value: Option<u64>,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
        .with_child(render_rate_value(value, ui_font, hc))
        .finish()
}

/// 速率数值：大号数字 + 小灰单位（K/s）。零显 0，无数据显 —。
fn render_rate_value(
    value: Option<u64>,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let (num, unit, num_color) = match value {
        Some(v) if v > 0 => {
            let (n, u) = split_rate(v);
            (n, format!("{u}/s"), hc.semantic.ok)
        }
        Some(_) => ("0".to_string(), "K/s".to_string(), hc.text_secondary),
        None => ("—".to_string(), String::new(), hc.text_secondary),
    };

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::End)
        .with_child(bold_text(num, ui_font, 16.0, num_color));
    if !unit.is_empty() {
        row.add_child(spacer_h(1.0));
        row.add_child(
            Container::new(
                Text::new_inline(unit, ui_font, 10.0)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .with_margin_bottom(1.0)
            .finish(),
        );
    }
    row.finish()
}

/// 横向分隔线（1px）。
fn divider(hc: &HostUiColors) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(Empty::new().finish())
            .with_background_color(hc.card_border)
            .finish(),
    )
    .with_height(1.0)
    .finish()
}

/// 连接状态 → 文字 + 颜色。
fn status_indicator(
    snapshot: Option<&HostOverviewSnapshot>,
    hc: &HostUiColors,
) -> (String, ColorU) {
    match snapshot.map(|s| &s.status) {
        Some(HostOverviewStatus::Error(_)) => ("连接失败".to_string(), hc.semantic.danger),
        Some(HostOverviewStatus::Ready) | Some(HostOverviewStatus::Collecting) => {
            ("● 在线".to_string(), hc.semantic.ok)
        }
        _ => ("连接中…".to_string(), hc.text_secondary),
    }
}

fn opt_num(value: Option<f32>, decimals: usize) -> String {
    value
        .map(|v| format!("{v:.decimals$}"))
        .unwrap_or_else(dash)
}

fn dash() -> String {
    "—".to_string()
}

fn spacer_h(width: f32) -> Box<dyn Element> {
    ConstrainedBox::new(Empty::new().finish())
        .with_width(width)
        .finish()
}

fn spacer_v(height: f32) -> Box<dyn Element> {
    ConstrainedBox::new(Empty::new().finish())
        .with_height(height)
        .finish()
}
