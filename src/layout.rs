// 标题栏的实际渲染高度（main.rs `title_bar` 用 `.h(px(36.0))`）。
// 这个常量喂给 `ShellLayout`，进而决定 `terminal_host.y`，
// 鼠标坐标解算靠它把窗口坐标换算成 cell 行号。两边对不上时，
// 选择跨行时会出现“稍稍上移就跳到上一行”的偏移。
pub const TITLE_BAR_HEIGHT: u32 = 36;
pub const ACTIVITY_RAIL_WIDTH: u32 = 70;
pub const TAB_BAR_HEIGHT: u32 = 52;
pub const BOTTOM_TOOLBAR_HEIGHT: u32 = 52;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellLayout {
    pub title_bar: Rect,
    pub activity_rail: Rect,
    pub tab_bar: Rect,
    pub terminal_host: Rect,
    pub bottom_toolbar: Rect,
    pub monitor_panel: Option<Rect>,
}

impl ShellLayout {
    pub fn for_window(size: Size) -> Self {
        let title_height = TITLE_BAR_HEIGHT.min(size.height);
        let body_y = title_height;
        let body_height = size.height.saturating_sub(title_height);

        let rail_width = ACTIVITY_RAIL_WIDTH.min(size.width);
        let main_x = rail_width;
        let main_width = size.width.saturating_sub(rail_width);

        let tab_height = TAB_BAR_HEIGHT.min(body_height);
        let remaining_after_tabs = body_height.saturating_sub(tab_height);
        let bottom_height = BOTTOM_TOOLBAR_HEIGHT.min(remaining_after_tabs);
        let terminal_height = remaining_after_tabs.saturating_sub(bottom_height);
        let terminal_y = body_y + tab_height;
        let bottom_y = terminal_y + terminal_height;

        Self {
            title_bar: Rect::new(0, 0, size.width, title_height),
            activity_rail: Rect::new(0, body_y, rail_width, body_height),
            tab_bar: Rect::new(main_x, body_y, main_width, tab_height),
            terminal_host: Rect::new(main_x, terminal_y, main_width, terminal_height),
            bottom_toolbar: Rect::new(main_x, bottom_y, main_width, bottom_height),
            monitor_panel: None,
        }
    }
}
