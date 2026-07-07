//! ThemePalette：对一个 WarpTheme 一次性算好共享底料（neutral 阶 / overlay / accent / 文字）。
//! 三套派生色（chrome/overview/host）都从这里取，避免各自重复调 internal_colors。

use warp_core::ui::theme::color::internal_colors::{
    fg_overlay_2, fg_overlay_3, neutral_1, neutral_2, neutral_3, neutral_4, neutral_6,
};
use warp_core::ui::theme::WarpTheme;
use warpui_core::color::ColorU;

#[derive(Clone, Copy)]
pub struct ThemePalette {
    pub neutral_1: ColorU,
    pub neutral_2: ColorU,
    pub neutral_3: ColorU,
    pub neutral_4: ColorU,
    pub neutral_6: ColorU,
    pub fg_overlay_2: ColorU,
    pub fg_overlay_3: ColorU,
    pub bg: ColorU,
    pub fg: ColorU,
    pub accent: ColorU,
    pub active_text: ColorU,
    pub inactive_text: ColorU,
    /// 背景亮度判定：暗色主题走 dark 语义档，亮色走 light。
    pub is_dark: bool,
}

impl ThemePalette {
    pub fn from_theme(theme: &WarpTheme) -> Self {
        let bg = theme.background().into_solid();
        Self {
            neutral_1: neutral_1(theme),
            neutral_2: neutral_2(theme),
            neutral_3: neutral_3(theme),
            neutral_4: neutral_4(theme),
            neutral_6: neutral_6(theme),
            fg_overlay_2: fg_overlay_2(theme).into_solid(),
            fg_overlay_3: fg_overlay_3(theme).into_solid(),
            bg,
            fg: theme.foreground().into_solid(),
            accent: theme.accent().into_solid(),
            active_text: theme.active_ui_text_color().into_solid(),
            inactive_text: theme.nonactive_ui_text_color().into_solid(),
            is_dark: luminance(bg) < 128.0,
        }
    }

    /// 徽章底色公式：前景色与背景 30:226 混合（原三套共用的 tint）。
    pub fn tint(&self, color: ColorU) -> ColorU {
        let mix = |c: u8, b: u8| ((c as u32 * 30 + b as u32 * 226) / 256) as u8;
        ColorU::new(
            mix(color.r, self.bg.r),
            mix(color.g, self.bg.g),
            mix(color.b, self.bg.b),
            255,
        )
    }

    /// 同色改 alpha 的便捷构造。
    pub fn with_alpha(color: ColorU, a: u8) -> ColorU {
        ColorU::new(color.r, color.g, color.b, a)
    }
}

fn luminance(c: ColorU) -> f32 {
    0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32
}
