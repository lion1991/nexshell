use warp_core::ui::theme::color::internal_colors::{
    fg_overlay_2, neutral_1, neutral_2, neutral_3, neutral_4,
};
use warp_core::ui::theme::WarpTheme;
use warpui::color::ColorU;

// --- 图标路径 ---
pub const ICON_SEARCH: &str = "icons/search.svg";
pub const ICON_REFRESH: &str = "icons/refresh.svg";
pub const ICON_GRID_VIEW: &str = "icons/grid-view.svg";
pub const ICON_LIST_VIEW: &str = "icons/list-view.svg";
pub const ICON_EYE: &str = "icons/eye.svg";
pub const ICON_EYE_OFF: &str = "icons/eye-off.svg";
pub const ICON_DOWNLOAD: &str = "icons/download.svg";
pub const ICON_UPLOAD: &str = "icons/upload.svg";
pub const ICON_CLOUD: &str = "icons/cloud.svg";
pub const ICON_ACTIVITY: &str = "icons/activity.svg";
#[allow(dead_code)]
pub const ICON_GEAR: &str = "icons/gear.svg";
pub const ICON_PLUS: &str = "icons/plus.svg";
pub const ICON_TERMINAL: &str = "icons/terminal.svg";
pub const ICON_LINUX: &str = "icons/linux.svg";
pub const ICON_SERIAL: &str = "icons/serial.svg";
pub const ICON_LINK: &str = "icons/link.svg";
pub const ICON_KEY: &str = "icons/key.svg";
pub const ICON_COPY: &str = "icons/copy.svg";
pub const ICON_FOLDER: &str = "icons/folder.svg";
#[allow(dead_code)]
pub const ICON_CHEVRON_DOWN: &str = "icons/chevron-down.svg";
pub const ICON_CHEVRON_RIGHT: &str = "icons/chevron-right.svg";
pub const ICON_PLAY: &str = "icons/play.svg";
pub const ICON_SWAP: &str = "icons/swap.svg";
pub const ICON_TRASH: &str = "icons/trash.svg";
pub const ICON_X_CIRCLE: &str = "icons/x-circle.svg";
pub const ICON_PENCIL: &str = "icons/pencil.svg";

// --- 面板布局 ---
pub const SIDEBAR_WIDTH: f32 = 220.0;
// 密钥页：左侧密钥列表固定宽，右侧详情 Expanded 填充。
pub const KEY_LIST_WIDTH: f32 = 340.0;

// --- 工具栏 ---
#[allow(dead_code)]
pub const TOOLBAR_HEIGHT: f32 = 48.0;
pub const TOOLBAR_TITLE_SIZE: f32 = 15.0;

// --- 搜索栏 ---
#[allow(dead_code)]
pub const SEARCH_BAR_HEIGHT: f32 = 40.0;

// --- 卡片 ---
#[allow(dead_code)]
pub const CARD_MIN_WIDTH: f32 = 320.0;
pub const CARD_PADDING: f32 = 20.0;
pub const CARD_SPACING: f32 = 16.0;
pub const CARD_CORNER_RADIUS: f32 = 8.0;
pub const CARD_ICON_SIZE: f32 = 40.0;

// --- 分组导航 ---
pub const GROUP_ITEM_HEIGHT: f32 = 32.0;

// --- 通用 UI ---
pub const UI_FONT_SIZE: f32 = 12.0;
pub const UI_FONT_SIZE_SMALL: f32 = 11.0;
pub const ICON_SIZE_SM: f32 = 16.0;
pub const ICON_SIZE_MD: f32 = 20.0;
pub const BUTTON_HEIGHT: f32 = 32.0;
pub const BUTTON_CORNER_RADIUS: f32 = 6.0;

#[derive(Clone, Copy)]
pub struct HostUiColors {
    pub panel_bg: ColorU,
    pub sidebar_bg: ColorU,
    pub sidebar_border: ColorU,
    pub toolbar_bg: ColorU,
    pub toolbar_border: ColorU,
    pub search_bar_bg: ColorU,
    pub search_bar_border: ColorU,
    pub card_bg: ColorU,
    pub card_bg_hover: ColorU,
    pub card_border: ColorU,
    pub card_border_hover: ColorU,
    pub badge_ssh_bg: ColorU,
    pub badge_ssh_text: ColorU,
    pub badge_serial_bg: ColorU,
    pub badge_serial_text: ColorU,
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
}

impl HostUiColors {
    pub fn from_theme(theme: &WarpTheme) -> Self {
        let n1 = neutral_1(theme);
        let n2 = neutral_2(theme);
        let n3 = neutral_3(theme);
        let n4 = neutral_4(theme);
        let bg = theme.background().into_solid();
        let outline = fg_overlay_2(theme).into_solid();
        let accent = theme.accent().into_solid();
        let fg = theme.foreground().into_solid();
        let active_text = theme.active_ui_text_color().into_solid();
        let inactive_text = theme.nonactive_ui_text_color().into_solid();

        // SSH badge: accent-tinted bg
        let ssh_bg = ColorU::new(
            ((accent.r as u32 * 30 + bg.r as u32 * 226) / 256) as u8,
            ((accent.g as u32 * 30 + bg.g as u32 * 226) / 256) as u8,
            ((accent.b as u32 * 30 + bg.b as u32 * 226) / 256) as u8,
            255,
        );
        // Connect 按钮 hover：accent 30:226 tint，跟 SSH 徽章同档（呼应主色）
        let connect_bg_hover = ssh_bg;
        // Serial badge: yellow-tinted
        let serial_accent = ColorU::new(0xcc, 0x99, 0x19, 0xff);
        let serial_bg = ColorU::new(
            ((serial_accent.r as u32 * 30 + bg.r as u32 * 226) / 256) as u8,
            ((serial_accent.g as u32 * 30 + bg.g as u32 * 226) / 256) as u8,
            ((serial_accent.b as u32 * 30 + bg.b as u32 * 226) / 256) as u8,
            255,
        );

        Self {
            panel_bg: bg,
            sidebar_bg: n1,
            sidebar_border: outline,
            toolbar_bg: n1,
            toolbar_border: outline,
            search_bar_bg: n2,
            search_bar_border: n3,
            card_bg: n2,
            card_bg_hover: n3,
            card_border: n3,
            card_border_hover: n4,
            badge_ssh_bg: ssh_bg,
            badge_ssh_text: accent,
            badge_serial_bg: serial_bg,
            badge_serial_text: serial_accent,
            connect_btn_bg: n4,
            connect_btn_bg_hover: connect_bg_hover,
            connect_btn_border: outline,
            group_selected_bg: n2,
            group_hover_bg: n1,
            tag_bg: ssh_bg,
            tag_text: accent,
            text_primary: active_text,
            text_secondary: inactive_text,
            text_accent: accent,
            accent_bg: accent,
            accent_text: fg,
            action_bar_bg: n3,
            action_bar_border: n4,
            scrollbar_thumb: ColorU::new(n3.r, n3.g, n3.b, 0x99),
            scrollbar_thumb_active: n3,
        }
    }
}
