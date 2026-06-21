//! UI 与主机概览的派生颜色。Warp color.rs 的 neutral_*/fg_overlay_* 在此组合成具体语义。

use warp_core::ui::theme::color::internal_colors::{
    fg_overlay_3, neutral_1, neutral_2, neutral_3, neutral_4, neutral_6,
};
use warp_core::ui::theme::WarpTheme;
use warpui::color::ColorU;

pub(crate) struct UiColors {
    pub title_bar_bg: ColorU,
    pub title_bar_border: ColorU,
    pub tab_bg_active: ColorU,
    pub tab_bg_hover: ColorU,
    pub tab_border_active: ColorU,
    pub tab_border_inactive: ColorU,
    pub tab_text_active: ColorU,
    pub tab_text_inactive: ColorU,
    pub icon_color_active: ColorU,
    pub icon_color_inactive: ColorU,
    pub icon_button_hover_bg: ColorU,
    pub tab_close_bg_default: ColorU,
    pub tab_close_bg_hover: ColorU,
    pub tooltip_bg: ColorU,
    pub tooltip_text: ColorU,
    pub combo_outer_hover_bg: ColorU,
    pub combo_inner_hover_bg: ColorU,
    pub combo_chevron_active_bg: ColorU,
}

impl UiColors {
    pub fn from_theme(theme: &WarpTheme) -> Self {
        let n1 = neutral_1(theme);
        let n2 = neutral_2(theme);
        let n3 = neutral_3(theme);
        let n4 = neutral_4(theme);
        let n6 = neutral_6(theme);
        let bg_solid = theme.background().into_solid();
        let active_text = theme.active_ui_text_color().into_solid();
        let inactive_text = theme.nonactive_ui_text_color().into_solid();
        let close_default = {
            let c = n3;
            ColorU::new(c.r, c.g, c.b, 0x99)
        };
        Self {
            title_bar_bg: n1,
            title_bar_border: n2,
            tab_bg_active: n3,
            tab_bg_hover: n2,
            tab_border_active: n2,
            tab_border_inactive: n1,
            tab_text_active: active_text,
            tab_text_inactive: inactive_text,
            icon_color_active: active_text,
            icon_color_inactive: inactive_text,
            icon_button_hover_bg: n2,
            tab_close_bg_default: close_default,
            tab_close_bg_hover: n4,
            tooltip_bg: n6,
            tooltip_text: bg_solid,
            combo_outer_hover_bg: n1,
            combo_inner_hover_bg: n2,
            combo_chevron_active_bg: fg_overlay_3(theme).into_solid(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct HostOverviewColors {
    pub panel_bg: ColorU,
    pub panel_border: ColorU,
    pub card_bg: ColorU,
    pub text_primary: ColorU,
    pub text_muted: ColorU,
    pub section_title: ColorU,
    pub metric_track: ColorU,
    pub cpu_accent: ColorU,
    pub memory_accent: ColorU,
    pub swap_accent: ColorU,
    pub upload: ColorU,
    pub download: ColorU,
    pub chart_grid: ColorU,
    pub warning: ColorU,
    pub ok: ColorU,
}

impl HostOverviewColors {
    pub fn from_theme(theme: &WarpTheme) -> Self {
        let n1 = neutral_1(theme);
        let n2 = neutral_2(theme);
        let n3 = neutral_3(theme);
        let n4 = neutral_4(theme);
        let tc = theme.terminal_colors();
        let accent = theme.accent().into_solid();
        let active = theme.active_ui_text_color().into_solid();
        let muted = theme.nonactive_ui_text_color().into_solid();
        let yellow = tc.normal.yellow;
        let magenta = tc.normal.magenta;
        let red = tc.normal.red;
        let green = tc.normal.green;

        Self {
            panel_bg: n1,
            panel_border: n3,
            card_bg: ColorU::new(n2.r, n2.g, n2.b, 0xd8),
            text_primary: active,
            text_muted: muted,
            section_title: ColorU::new(active.r, active.g, active.b, 0xdd),
            metric_track: ColorU::new(n4.r, n4.g, n4.b, 0x8f),
            cpu_accent: accent,
            memory_accent: ColorU::new(yellow.r, yellow.g, yellow.b, 0xff),
            swap_accent: ColorU::new(magenta.r, magenta.g, magenta.b, 0xdd),
            upload: ColorU::new(red.r, red.g, red.b, 0xdd),
            download: ColorU::new(green.r, green.g, green.b, 0xdd),
            chart_grid: ColorU::new(muted.r, muted.g, muted.b, 0x33),
            warning: ColorU::new(red.r, red.g, red.b, 0xff),
            ok: ColorU::new(green.r, green.g, green.b, 0xff),
        }
    }
}
