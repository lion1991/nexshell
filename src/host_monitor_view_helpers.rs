//! 主机监控视图纯函数 helper：CPU/内存度量格式化、用量配色、运行时长。
//! 无 &self；按 ADR step 10（附录 B）从 main.rs 抽出。详见 docs/adr/0001-root-view-multi-file-impl.md。

use crate::ui_colors::HostOverviewColors;
use nexshell::host_overview::{format_bytes_short, HostOverviewStatus, UsageMetric};
use warpui::color::ColorU;
use warpui::elements::{
    Clipped, ConstrainedBox, CrossAxisAlignment, Flex, ParentElement, Shrinkable, Text,
};
use warpui::{fonts, Element};

/// 侧栏固定 248px，左右各 10px 内边距后的内容宽度。
pub(crate) const OVERVIEW_CONTENT_WIDTH: f32 = 228.0;

pub(crate) fn format_usage_metric(metric: &UsageMetric) -> String {
    format!(
        "{}/{}",
        format_bytes_short(metric.used_bytes),
        format_bytes_short(metric.total_bytes)
    )
}

/// 用量条填充色：≥90% 切换警告色。
pub(crate) fn usage_fill_color(percent: f32, base: ColorU, colors: &HostOverviewColors) -> ColorU {
    if percent >= 90.0 {
        colors.warning
    } else {
        base
    }
}

/// 连接状态点：错误红 / 未连接灰 / 已连接绿；仅延迟严重劣化（≥400ms）降级黄。
pub(crate) fn overview_status_dot_color(
    status: &HostOverviewStatus,
    latency_ms: Option<u64>,
    colors: &HostOverviewColors,
) -> ColorU {
    match status {
        HostOverviewStatus::Error(_) => colors.warning,
        HostOverviewStatus::Waiting => colors.text_muted,
        _ => match latency_ms {
            Some(ms) if ms >= 400 => colors.memory_accent,
            Some(_) => colors.ok,
            None => colors.text_muted,
        },
    }
}

pub(crate) fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        rust_i18n::t!("host_overview_uptime_days", n = days).to_string()
    } else if hours > 0 {
        rust_i18n::t!("host_overview_uptime_hours", n = hours).to_string()
    } else {
        rust_i18n::t!("host_overview_uptime_minutes", n = minutes).to_string()
    }
}

/// 概览进程行三列单元格（内存/CPU/命令）；纯函数以便在 Hoverable 闭包内重建。
pub(crate) fn overview_process_cells(
    memory: &str,
    cpu: &str,
    command: &str,
    font: fonts::FamilyId,
    color: ColorU,
) -> Box<dyn Element> {
    let make_text = |text: &str| {
        Text::new_inline(text.to_string(), font, 12.0)
            .with_color(color)
            .finish()
    };
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            ConstrainedBox::new(make_text(memory))
                .with_width(58.0)
                .finish(),
        )
        .with_child(
            ConstrainedBox::new(make_text(cpu))
                .with_width(46.0)
                .finish(),
        )
        .with_child(Shrinkable::new(1.0, Clipped::new(make_text(command)).finish()).finish())
        .finish()
}

/// 内核 meta 短串：取 `uname -srmo` 的前两段（名称 + 版本）。
pub(crate) fn format_kernel_short(kernel: &str) -> Option<String> {
    let mut parts = kernel.split_whitespace();
    let name = parts.next()?;
    match parts.next() {
        Some(release) => Some(format!("{name} {release}")),
        None => Some(name.to_string()),
    }
}
