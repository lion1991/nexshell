// 多主机状态总览卡片网格（ServerCat 风格）。本文件只放纯渲染自由函数。
// 数据来自 RootView 的 HostOverviewFleet；点击卡片 = 快速连接该主机。
// CPU/Mem 用 RingGauge 环形仪表（GPU Ring 原语），sweep 走 FloatTransition 过渡。

use warpui::{
    color::ColorU,
    elements::{
        Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Empty, Expanded, Flex,
        Hoverable, Icon, MainAxisSize, ParentElement, Radius, Text,
    },
    fonts, Element,
};

use nexshell::host_management::HostCardSnapshot;
use nexshell::host_overview::{
    split_rate, HostOverviewSnapshot, HostOverviewStatus, HostOverviewUiState,
};
use nexshell::host_overview_fleet::HostOverviewFleet;
use nexshell::stat_widgets::{ConcentricRings, RingGauge, RingSpec};
use std::time::Instant;

use crate::host_management_view::constants::*;
use crate::host_management_view::host_card::HostCardStates;
use crate::terminal_grid_element::TerminalGridAction;

const GRID_COLUMNS: usize = 3;

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

    // 清理已删除主机的 sweep 过渡（key = "{host_id}:mem|load0..2"）。
    let now = Instant::now();
    states.gauge_anim.borrow_mut().retain(|key| {
        key.rsplit_once(':')
            .is_some_and(|(id, _)| ssh_hosts.iter().any(|host| host.id == id))
    });

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
            let card =
                render_status_card(host, index, fleet.state(&host.id), states, ui_font, hc, now);
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

#[allow(clippy::too_many_arguments)]
fn render_status_card(
    host: &HostCardSnapshot,
    index: usize,
    state: Option<&HostOverviewUiState>,
    states: &HostCardStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
    now: Instant,
) -> Box<dyn Element> {
    let hc = *hc;
    let card_state = states.card_states[index].clone();
    let host_id = host.id.clone();
    let connect_state = states.connect_states[index].clone();
    let name = host.name.clone();
    let snapshot = state.map(|s| s.snapshot.clone());
    // sweep 过渡在闭包外采样（借 states），闭包内只消费动画值。
    let cpu = snapshot.as_ref().and_then(|s| s.cpu_percent);
    let mem = snapshot
        .as_ref()
        .and_then(|s| s.memory.as_ref())
        .map(|m| m.percent);
    let mem_display = sample_gauge(states, format!("{host_id}:mem"), mem, now);
    // Load 同心环数据（外→内 = 1/5/15 分钟）：load/核数 归一 + 档位色；无核数只画轨道。
    let loads = snapshot.as_ref().and_then(|s| s.load_average);
    let cores = snapshot
        .as_ref()
        .and_then(|s| s.cpu_cores)
        .filter(|c| *c > 0)
        .map(|c| c as f32);
    let mut load_rings: [(Option<f32>, ColorU); 3] = [(None, hc.text_secondary); 3];
    let mut load_dot = hc.text_secondary;
    for (i, slot) in load_rings.iter_mut().enumerate() {
        if let (Some(loads), Some(cores)) = (loads, cores) {
            let ratio = loads[i] / cores;
            let color = load_color(ratio, &hc);
            let display = sample_gauge(
                states,
                format!("{host_id}:load{i}"),
                Some(ratio.clamp(0.0, 1.0)),
                now,
            );
            *slot = (display, color);
            if i == 0 {
                load_dot = color;
            }
        }
    }

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
            .with_child(
                // 参考卡四列：Load 同心环 / Mem 环 / 网络叠排 / 磁盘叠排。
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Expanded::new(1.0, render_load_rings(&load_rings, load_dot, ui_font, &hc))
                            .finish(),
                    )
                    .with_child(
                        Expanded::new(1.0, render_ring_stat("Mem", mem_display, ui_font, &hc))
                            .finish(),
                    )
                    .with_child(
                        Expanded::new(1.0, render_rate_pair(up, "↑/s", down, "↓/s", ui_font, &hc))
                            .finish(),
                    )
                    .with_child(
                        Expanded::new(
                            1.0,
                            render_rate_pair(
                                disk_read, "Read/s", disk_write, "Write/s", ui_font, &hc,
                            ),
                        )
                        .finish(),
                    )
                    .finish(),
            )
            .with_child(spacer_v(16.0))
            .with_child(divider(&hc))
            .with_child(spacer_v(12.0))
            .with_child(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(render_labeled_value(
                        "CPU",
                        cpu.map(|v| format!("{v:.0}%")).unwrap_or_else(dash),
                        ui_font,
                        &hc,
                    ))
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

/// 环 sweep 过渡：闭包外借 states 采样；target=None（无数据）不入表。
fn sample_gauge(
    states: &HostCardStates,
    key: String,
    target: Option<f32>,
    now: Instant,
) -> Option<f32> {
    let target = target?;
    let mut map = states.gauge_anim.borrow_mut();
    map.retarget(key.clone(), target, now);
    Some(map.sample(&key, now).unwrap_or(target))
}

/// 环形仪表列：中心百分比 + 底部标签；数值弧走档位色，无数据只画轨道。
fn render_ring_stat(
    label: &str,
    display: Option<f32>,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    const RING_DIAMETER: f32 = 64.0;
    const RING_THICKNESS: f32 = 7.0;
    let value = display.unwrap_or(0.0).clamp(0.0, 100.0);
    let text = display.map(|v| format!("{v:.0}%")).unwrap_or_else(dash);
    // 轨道比分隔线再淡一档，让数值弧成为主角（同监控侧栏配方）。
    let track = ColorU::new(
        hc.metric_track.r,
        hc.metric_track.g,
        hc.metric_track.b,
        0x4a,
    );
    let mut gauge = RingGauge::new(RING_DIAMETER, RING_THICKNESS, track, level_color(value, hc))
        .with_label(bold_text(text, ui_font, 13.0, hc.text_primary));
    if display.is_some() {
        gauge = gauge.with_fraction(value / 100.0);
    }

    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(gauge.finish())
        .with_child(spacer_v(6.0))
        .with_child(
            Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE_SMALL)
                .with_color(hc.text_secondary)
                .finish(),
        )
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

/// 负载档位色：ratio = load/核数。<0.7 ok，<1.0 warn，否则 danger。
fn load_color(ratio: f32, hc: &HostUiColors) -> ColorU {
    if ratio < 0.7 {
        hc.semantic.ok
    } else if ratio < 1.0 {
        hc.semantic.warn
    } else {
        hc.semantic.danger
    }
}

/// Load 同心三环列（外→内 = 1/5/15 分钟）+ 中心健康点 + 底部标签。
fn render_load_rings(
    rings_data: &[(Option<f32>, ColorU); 3],
    dot_color: ColorU,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    const DIAMETER: f32 = 64.0;
    const THICKNESS: f32 = 6.0;
    const GAP: f32 = 2.0;
    const DOT_DIAMETER: f32 = 9.0;
    let track = ColorU::new(
        hc.metric_track.r,
        hc.metric_track.g,
        hc.metric_track.b,
        0x4a,
    );
    let mut rings = ConcentricRings::new(DIAMETER, THICKNESS, GAP, track);
    for (fraction, color) in rings_data {
        rings = rings.with_ring(RingSpec {
            fraction: *fraction,
            color: *color,
        });
    }
    rings = rings.with_center_dot(DOT_DIAMETER, dot_color);

    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(rings.finish())
        .with_child(spacer_v(6.0))
        .with_child(
            Text::new_inline("Load".to_string(), ui_font, UI_FONT_SIZE_SMALL)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .finish()
}

/// 速率两行叠排（↑/↓ 或 Read/Write）。
fn render_rate_pair(
    value1: Option<u64>,
    label1: &str,
    value2: Option<u64>,
    label2: &str,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(render_rate_stack(value1, label1, ui_font, hc))
        .with_child(spacer_v(10.0))
        .with_child(render_rate_stack(value2, label2, ui_font, hc))
        .finish()
}

/// 速率叠排：大数字 + 小单位一行，下行方向标签；>0 语义绿，其余灰。
fn render_rate_stack(
    value: Option<u64>,
    label: &str,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let (num, unit, num_color) = match value {
        Some(v) => {
            let (n, u) = split_rate(v);
            let color = if v > 0 {
                hc.semantic.ok
            } else {
                hc.text_secondary
            };
            (n, u, color)
        }
        None => ("—".to_string(), String::new(), hc.text_secondary),
    };

    let mut num_row = Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::End)
        .with_child(bold_text(num, ui_font, 16.0, num_color));
    if !unit.is_empty() {
        num_row.add_child(spacer_h(3.0));
        num_row.add_child(
            Container::new(
                Text::new_inline(unit, ui_font, 10.0)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .with_margin_bottom(1.5)
            .finish(),
        );
    }

    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(num_row.finish())
        .with_child(spacer_v(2.0))
        .with_child(
            Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE_SMALL)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .finish()
}

/// 横向分隔线（1px）。
fn divider(hc: &HostUiColors) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(Empty::new().finish())
            .with_background_color(hc.metric_track)
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
