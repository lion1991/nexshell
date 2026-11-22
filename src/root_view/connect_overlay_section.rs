// connect_overlay_section — SSH 连接中盖在终端上的过渡层：进度环、主机、三个阶段、进度条和计时。
// 本文件只含 impl RootView，无自由函数；淡入淡出状态机在 connect_overlay_helpers.rs，
// 进度环元件在 nexshell::connect_spinner。连接阶段由 SSH runtime 写进快照（ssh_connect_progress）。

use std::time::Instant;

use nexshell::click_through::ClickThrough;
use nexshell::connect_spinner::ConnectSpinner;
use nexshell::ssh_connect_progress::{ssh_connect_fraction, SshConnectProgress, SshConnectStage};
use nexshell::terminal_runtime::TerminalPalette;
use warpui::color::ColorU;
use warpui::elements::{
    Align, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Expanded, Flex, Icon,
    MainAxisSize, ParentElement, Radius, Stack, Text,
};
use warpui::{fonts, Element};

use crate::connect_overlay_helpers::{
    connect_overlays_animating, format_connect_elapsed, step_connect_overlay,
};
use crate::host_management_view::constants::ICON_TERMINAL;
use crate::host_management_view::host_card::{mask_endpoint, system_icon_path};
use crate::terminal_view_helpers::terminal_palette_ansi_color;
use crate::RootView;

const SPINNER_DIAMETER: f32 = 64.0;
const SPINNER_THICKNESS: f32 = 3.0;
const BAR_WIDTH: f32 = 280.0;
const BAR_HEIGHT: f32 = 3.0;

fn faded(color: ColorU, opacity: f32) -> ColorU {
    ColorU::new(
        color.r,
        color.g,
        color.b,
        (f32::from(color.a) * opacity.clamp(0.0, 1.0)).round() as u8,
    )
}

impl RootView {
    /// 连接中（或刚连完正在淡出）时在终端上叠过渡层；否则原样返回。
    pub(in crate::root_view) fn wrap_with_connect_overlay(
        &self,
        body: Box<dyn Element>,
        session_id: &str,
        progress: Option<&SshConnectProgress>,
        host_id: Option<&str>,
        host_label: &str,
        palette: &TerminalPalette,
    ) -> Box<dyn Element> {
        let now = Instant::now();
        let frame = {
            let mut anims = self.connect_overlays.borrow_mut();
            step_connect_overlay(&mut anims, session_id, progress, now)
        };
        let Some(frame) = frame else {
            return body;
        };
        let overlay = self.render_connect_overlay(
            &frame.progress,
            frame.opacity,
            host_id,
            host_label,
            palette,
            now,
        );
        // 过渡层只画不挡：点击、滚轮仍交给下面的终端（分屏时点它要能聚焦这个窗格）。
        Stack::new()
            .with_child(body)
            .with_child(ClickThrough::new(overlay).finish())
            .finish()
    }

    pub(in crate::root_view) fn connect_overlay_animating(&self, now: Instant) -> bool {
        connect_overlays_animating(&self.connect_overlays.borrow(), now)
    }

    fn render_connect_overlay(
        &self,
        progress: &SshConnectProgress,
        opacity: f32,
        host_id: Option<&str>,
        host_label: &str,
        palette: &TerminalPalette,
        now: Instant,
    ) -> Box<dyn Element> {
        let theme = &self.cached_warp_theme;
        let accent = faded(terminal_palette_ansi_color(palette, 6), opacity);
        let done = faded(terminal_palette_ansi_color(palette, 2), opacity);
        let primary_base = theme.active_ui_text_color().into_solid();
        let secondary_base = theme.nonactive_ui_text_color().into_solid();
        let primary = faded(primary_base, opacity);
        let secondary = faded(secondary_base, opacity);
        let dim = faded(secondary_base, opacity * 0.55);
        let track = ColorU::new(secondary_base.r, secondary_base.g, secondary_base.b, 0x38);

        let host = host_id.and_then(|id| self.host_state.host_by_id(id));
        let icon_path = host.map_or(ICON_TERMINAL, |h| system_icon_path(h.system));
        let target = if self.host_state.privacy_mode {
            mask_endpoint(&progress.target)
        } else {
            progress.target.clone()
        };
        let t = now
            .saturating_duration_since(progress.started_at)
            .as_secs_f32();

        let spinner = ConnectSpinner::new(
            SPINNER_DIAMETER,
            SPINNER_THICKNESS,
            t,
            track,
            terminal_palette_ansi_color(palette, 6),
        )
        .with_opacity(opacity)
        .with_center(
            ConstrainedBox::new(Icon::new(icon_path, accent).finish())
                .with_width(24.0)
                .with_height(24.0)
                .finish(),
        )
        .finish();

        let name = Text::new_inline(host_label.to_string(), self.ui_font, 17.0)
            .with_color(primary)
            .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
            .finish();
        let address = Text::new_inline(target, self.monospace_font, 12.0)
            .with_color(secondary)
            .finish();

        let mut stages = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        for (index, stage) in SshConnectStage::ALL.into_iter().enumerate() {
            let (glyph, color, bold) = match stage.index().cmp(&progress.stage.index()) {
                std::cmp::Ordering::Less => ("✓", done, false),
                std::cmp::Ordering::Equal => ("●", accent, true),
                std::cmp::Ordering::Greater => ("○", dim, false),
            };
            let label = match stage {
                SshConnectStage::Tcp => rust_i18n::t!("ssh_connect_stage_tcp"),
                SshConnectStage::Auth => rust_i18n::t!("ssh_connect_stage_auth"),
                SshConnectStage::Shell => rust_i18n::t!("ssh_connect_stage_shell"),
            };
            let weight = if bold {
                fonts::Weight::Bold
            } else {
                fonts::Weight::Normal
            };
            let item = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Container::new(
                        Text::new_inline(glyph, self.ui_font, 11.0)
                            .with_color(color)
                            .finish(),
                    )
                    .with_margin_right(5.0)
                    .finish(),
                )
                .with_child(
                    Text::new_inline(label.to_string(), self.ui_font, 12.0)
                        .with_color(if bold { primary } else { color })
                        .with_style(fonts::Properties::default().weight(weight))
                        .finish(),
                )
                .finish();
            stages.add_child(
                Container::new(item)
                    .with_margin_left(if index == 0 { 0.0 } else { 18.0 })
                    .finish(),
            );
        }

        let fraction = ssh_connect_fraction(progress, now).clamp(0.0, 1.0);
        let bar = ConstrainedBox::new(
            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_child(
                        ConstrainedBox::new(
                            Container::new(warpui::elements::Empty::new().finish())
                                .with_background_color(accent)
                                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(
                                    BAR_HEIGHT / 2.0,
                                )))
                                .finish(),
                        )
                        .with_width(BAR_WIDTH * fraction)
                        .with_height(BAR_HEIGHT)
                        .finish(),
                    )
                    .with_child(
                        Expanded::new(1.0, warpui::elements::Empty::new().finish()).finish(),
                    )
                    .finish(),
            )
            .with_background_color(faded(track, opacity))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(BAR_HEIGHT / 2.0)))
            .finish(),
        )
        .with_width(BAR_WIDTH)
        .with_height(BAR_HEIGHT)
        .finish();
        let elapsed = Text::new_inline(
            format_connect_elapsed(now.saturating_duration_since(progress.started_at)),
            self.monospace_font,
            11.0,
        )
        .with_color(dim)
        .finish();
        let bar_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(bar)
            .with_child(
                ConstrainedBox::new(Container::new(elapsed).with_margin_left(10.0).finish())
                    .with_width(48.0)
                    .finish(),
            );

        let column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(spinner)
            .with_child(Container::new(name).with_margin_top(4.0).finish())
            .with_child(Container::new(address).with_margin_top(6.0).finish())
            .with_child(
                Container::new(stages.finish())
                    .with_margin_top(22.0)
                    .finish(),
            )
            .with_child(
                // 右侧计时占了 48，左边补同宽让进度条居中。
                Container::new(bar_row.finish())
                    .with_margin_top(14.0)
                    .with_margin_left(48.0)
                    .finish(),
            )
            .finish();
        Align::new(column).finish()
    }
}
