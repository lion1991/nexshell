use nexshell::design_tokens::scale;

// HostUiColors 现为 design_tokens::HostColors 的门面，call-site（含 from_theme）零改动。
pub use nexshell::design_tokens::HostColors as HostUiColors;

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
pub const CARD_PADDING: f32 = scale::SPACE_5;
pub const CARD_SPACING: f32 = scale::SPACE_4;
pub const CARD_CORNER_RADIUS: f32 = scale::RADIUS_LG;
pub const CARD_ICON_SIZE: f32 = 36.0;

// --- 分组导航 ---
pub const GROUP_ITEM_HEIGHT: f32 = 32.0;

// --- 通用 UI ---
pub const UI_FONT_SIZE: f32 = scale::FONT_MD;
pub const UI_FONT_SIZE_SMALL: f32 = scale::FONT_SM;
pub const ICON_SIZE_SM: f32 = 16.0;
pub const ICON_SIZE_MD: f32 = 20.0;
pub const BUTTON_HEIGHT: f32 = 32.0;
pub const BUTTON_CORNER_RADIUS: f32 = scale::RADIUS_MD;
