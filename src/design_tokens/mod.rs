//! 设计 token 地基：一个 WarpTheme → 一次算 palette/semantic，再派生三套具体色。
//! chrome/overview/host 三套字段名与旧 UiColors/HostOverviewColors/HostUiColors 一一对应，
//! 旧结构体现为本模块的 re-export 门面，消费方 call-site 零改动。

pub mod elevation;
pub mod palette;
pub mod scale;
pub mod semantic;

use warp_core::ui::theme::WarpTheme;
use warpui_core::color::ColorU;

pub use elevation::{Elevation, Glass};
pub use palette::ThemePalette;
pub use semantic::{SemanticColors, TRANSPARENT, WINDOWS_CLOSE_HOVER};

#[derive(Clone, Copy)]
pub struct DesignTokens {
    pub palette: ThemePalette,
    pub semantic: SemanticColors,
    pub chrome: ChromeColors,
    pub overview: OverviewColors,
    pub host: HostColors,
}

impl DesignTokens {
    pub fn from_theme(theme: &WarpTheme) -> Self {
        let palette = ThemePalette::from_theme(theme);
        let semantic = SemanticColors::new(palette.is_dark);
        Self {
            palette,
            semantic,
            chrome: ChromeColors::derive(&palette),
            overview: OverviewColors::derive(&palette, &semantic),
            host: HostColors::derive(&palette, &semantic),
        }
    }
}

// ── chrome（旧 UiColors）：标题栏 / 标签 / tooltip / 下拉 chrome ──
#[derive(Clone, Copy)]
pub struct ChromeColors {
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
    /// 活动 tab 底部 accent 焦点条。
    pub tab_accent_bar: ColorU,
    /// 选中行 accent tint 圆角 pill 底色。
    pub selection_pill_bg: ColorU,
}

impl ChromeColors {
    fn derive(p: &ThemePalette) -> Self {
        Self {
            title_bar_bg: p.neutral_1,
            title_bar_border: p.neutral_2,
            tab_bg_active: p.neutral_3,
            tab_bg_hover: p.neutral_2,
            tab_border_active: p.neutral_2,
            tab_border_inactive: p.neutral_1,
            tab_text_active: p.active_text,
            tab_text_inactive: p.inactive_text,
            icon_color_active: p.active_text,
            icon_color_inactive: p.inactive_text,
            icon_button_hover_bg: p.neutral_2,
            tab_close_bg_default: ThemePalette::with_alpha(p.neutral_3, 0x99),
            tab_close_bg_hover: p.neutral_4,
            tooltip_bg: p.neutral_6,
            tooltip_text: p.bg,
            combo_outer_hover_bg: p.neutral_1,
            combo_inner_hover_bg: p.neutral_2,
            combo_chevron_active_bg: p.fg_overlay_3,
            tab_accent_bar: p.accent,
            selection_pill_bg: p.tint(p.accent),
        }
    }

    pub fn from_theme(theme: &WarpTheme) -> Self {
        Self::derive(&ThemePalette::from_theme(theme))
    }
}

// ── overview（旧 HostOverviewColors）：主机概览面板 / 图表 ──
#[derive(Clone, Copy)]
pub struct OverviewColors {
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
    /// 语义色（负载阈值判定等直接引用 ok/warn/danger）。
    pub semantic: SemanticColors,
}

impl OverviewColors {
    fn derive(p: &ThemePalette, s: &SemanticColors) -> Self {
        let active = p.active_text;
        let muted = p.inactive_text;
        Self {
            panel_bg: p.neutral_1,
            panel_border: p.neutral_3,
            card_bg: ThemePalette::with_alpha(p.neutral_2, 0xd8),
            text_primary: active,
            text_muted: muted,
            section_title: ThemePalette::with_alpha(active, 0xdd),
            metric_track: ThemePalette::with_alpha(p.neutral_4, 0x8f),
            cpu_accent: p.accent,
            memory_accent: s.memory,
            swap_accent: s.swap,
            upload: s.upload,
            download: s.download,
            chart_grid: ThemePalette::with_alpha(muted, 0x33),
            warning: s.danger,
            ok: s.ok,
            semantic: *s,
        }
    }

    pub fn from_theme(theme: &WarpTheme) -> Self {
        let p = ThemePalette::from_theme(theme);
        Self::derive(&p, &SemanticColors::new(p.is_dark))
    }
}

// ── host（旧 HostUiColors）：主机管理列表页 ──
#[derive(Clone, Copy)]
pub struct HostColors {
    pub panel_bg: ColorU,
    pub sidebar_bg: ColorU,
    pub sidebar_border: ColorU,
    pub toolbar_bg: ColorU,
    pub toolbar_border: ColorU,
    pub search_bar_bg: ColorU,
    pub search_bar_border: ColorU,
    /// 玻璃工具栏上的 inset 控件底（半透明，让玻璃透出）。
    pub toolbar_inset_bg: ColorU,
    pub card_bg: ColorU,
    pub card_bg_hover: ColorU,
    pub card_border: ColorU,
    pub card_border_hover: ColorU,
    /// 卡片内进度条底槽/分隔线：半透明，默认与 hover 底上都可见（勿借 card_border）。
    pub metric_track: ColorU,
    pub badge_ssh_bg: ColorU,
    pub badge_ssh_text: ColorU,
    pub badge_serial_bg: ColorU,
    pub badge_serial_text: ColorU,
    pub badge_rdp_bg: ColorU,
    pub badge_rdp_text: ColorU,
    pub connect_btn_bg: ColorU,
    pub connect_btn_bg_hover: ColorU,
    pub connect_btn_border: ColorU,
    pub group_selected_bg: ColorU,
    pub group_hover_bg: ColorU,
    pub tag_bg: ColorU,
    pub tag_text: ColorU,
    pub text_primary: ColorU,
    pub text_secondary: ColorU,
    pub text_accent: ColorU,
    pub accent_bg: ColorU,
    pub accent_text: ColorU,
    pub action_bar_bg: ColorU,
    pub action_bar_border: ColorU,
    pub scrollbar_thumb: ColorU,
    pub scrollbar_thumb_active: ColorU,
    /// 分组色点调色板（GitHub Primer label 色系），按分组 id 取模选色。
    pub group_dot_palette: [ColorU; 8],
    /// 语义色（供本页写死 hex 清理后引用，如删除红 / 在线绿）。
    pub semantic: SemanticColors,
}

impl HostColors {
    fn derive(p: &ThemePalette, s: &SemanticColors) -> Self {
        let outline = p.fg_overlay_2;
        let ssh_bg = p.tint(p.accent);
        Self {
            panel_bg: p.bg,
            sidebar_bg: p.neutral_1,
            sidebar_border: outline,
            toolbar_bg: p.neutral_1,
            toolbar_border: outline,
            search_bar_bg: p.neutral_2,
            search_bar_border: p.neutral_3,
            toolbar_inset_bg: ThemePalette::with_alpha(p.neutral_2, 0xb3),
            card_bg: p.neutral_2,
            card_bg_hover: p.neutral_3,
            card_border: p.neutral_3,
            card_border_hover: p.neutral_4,
            metric_track: ThemePalette::with_alpha(p.neutral_4, 0x8f),
            badge_ssh_bg: ssh_bg,
            badge_ssh_text: p.accent,
            badge_serial_bg: p.tint(s.warn),
            badge_serial_text: s.warn,
            badge_rdp_bg: p.tint(s.rdp),
            badge_rdp_text: s.rdp,
            connect_btn_bg: p.neutral_4,
            connect_btn_bg_hover: ssh_bg,
            connect_btn_border: outline,
            group_selected_bg: p.neutral_2,
            // 勿用 neutral_1：与 sidebar_bg 同色 = hover 隐身（同色隐身病，见 metric_track）。
            group_hover_bg: ThemePalette::with_alpha(p.neutral_3, 0x66),
            tag_bg: ssh_bg,
            tag_text: p.accent,
            text_primary: p.active_text,
            text_secondary: p.inactive_text,
            text_accent: p.accent,
            accent_bg: p.accent,
            accent_text: p.fg,
            action_bar_bg: p.neutral_3,
            action_bar_border: p.neutral_4,
            scrollbar_thumb: ThemePalette::with_alpha(p.neutral_3, 0x99),
            scrollbar_thumb_active: p.neutral_3,
            group_dot_palette: if p.is_dark {
                [
                    rgb(0x58a6ff),
                    rgb(0x3fb950),
                    rgb(0xd29922),
                    rgb(0xdb6d28),
                    rgb(0xf85149),
                    rgb(0xbc8cff),
                    rgb(0xff7eb6),
                    rgb(0x39c5cf),
                ]
            } else {
                [
                    rgb(0x0969da),
                    rgb(0x1a7f37),
                    rgb(0x9a6700),
                    rgb(0xbc4c00),
                    rgb(0xcf222e),
                    rgb(0x8250df),
                    rgb(0xbf3989),
                    rgb(0x1b7c83),
                ]
            },
            semantic: *s,
        }
    }

    pub fn from_theme(theme: &WarpTheme) -> Self {
        let p = ThemePalette::from_theme(theme);
        Self::derive(&p, &SemanticColors::new(p.is_dark))
    }
}

const fn rgb(hex: u32) -> ColorU {
    ColorU {
        r: ((hex >> 16) & 0xff) as u8,
        g: ((hex >> 8) & 0xff) as u8,
        b: (hex & 0xff) as u8,
        a: 0xff,
    }
}
