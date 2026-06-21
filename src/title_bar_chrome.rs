//! 标题栏 chrome 布局与窗口控件图标渲染（macOS/Windows/其他平台差异）。

use pathfinder_geometry::vector::vec2f;
use warpui::color::ColorU;
use warpui::elements::{
    Border, ChildAnchor, ConstrainedBox, CornerRadius, Icon, OffsetPositioning, ParentAnchor,
    ParentOffsetBounds, Radius, Rect, Stack,
};
use warpui::Element;

use super::{
    ICON_PATH_CLOSE, TAB_BAR_PADDING_RIGHT, TRAFFIC_LIGHT_RESERVED_WIDTH,
    WINDOWS_WINDOW_CONTROL_BUTTON_WIDTH,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TitleBarChromePlatform {
    Macos,
    Windows,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TitleBarChromeLayout {
    pub left_padding: f32,
    pub right_padding: f32,
    pub windows_controls_width: f32,
}

pub(crate) fn current_title_bar_chrome_platform() -> TitleBarChromePlatform {
    if cfg!(target_os = "macos") {
        TitleBarChromePlatform::Macos
    } else if cfg!(target_os = "windows") {
        TitleBarChromePlatform::Windows
    } else {
        TitleBarChromePlatform::Other
    }
}

pub(crate) fn title_bar_chrome_layout(platform: TitleBarChromePlatform) -> TitleBarChromeLayout {
    match platform {
        TitleBarChromePlatform::Macos => TitleBarChromeLayout {
            left_padding: TRAFFIC_LIGHT_RESERVED_WIDTH,
            right_padding: TAB_BAR_PADDING_RIGHT,
            windows_controls_width: 0.0,
        },
        TitleBarChromePlatform::Windows => TitleBarChromeLayout {
            left_padding: 0.0,
            right_padding: 0.0,
            windows_controls_width: WINDOWS_WINDOW_CONTROL_BUTTON_WIDTH * 3.0,
        },
        TitleBarChromePlatform::Other => TitleBarChromeLayout {
            left_padding: 0.0,
            right_padding: TAB_BAR_PADDING_RIGHT,
            windows_controls_width: 0.0,
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowControlKind {
    Minimize,
    Maximize,
    Restore,
    Close,
}

pub(crate) fn render_windows_window_control_icon(
    kind: WindowControlKind,
    color: ColorU,
) -> Box<dyn Element> {
    match kind {
        WindowControlKind::Minimize => {
            ConstrainedBox::new(Rect::new().with_background_color(color).finish())
                .with_width(10.0)
                .with_height(1.0)
                .finish()
        }
        WindowControlKind::Maximize => render_windows_maximize_icon(color, false),
        WindowControlKind::Restore => render_windows_maximize_icon(color, true),
        WindowControlKind::Close => ConstrainedBox::new(Icon::new(ICON_PATH_CLOSE, color).finish())
            .with_width(12.0)
            .with_height(12.0)
            .finish(),
    }
}

pub(crate) fn render_windows_maximize_icon(color: ColorU, restore: bool) -> Box<dyn Element> {
    let rect = || {
        ConstrainedBox::new(
            Rect::new()
                .with_border(Border::all(1.0).with_border_color(color))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(1.0)))
                .finish(),
        )
        .with_width(10.0)
        .with_height(10.0)
        .finish()
    };

    if !restore {
        return rect();
    }

    let mut stack = Stack::new();
    stack.add_positioned_child(
        rect(),
        OffsetPositioning::offset_from_parent(
            vec2f(0.0, 0.0),
            ParentOffsetBounds::Unbounded,
            ParentAnchor::BottomLeft,
            ChildAnchor::BottomLeft,
        ),
    );
    stack.add_positioned_child(
        rect(),
        OffsetPositioning::offset_from_parent(
            vec2f(0.0, 0.0),
            ParentOffsetBounds::Unbounded,
            ParentAnchor::TopRight,
            ChildAnchor::TopRight,
        ),
    );
    ConstrainedBox::new(stack.finish())
        .with_width(12.0)
        .with_height(12.0)
        .finish()
}

#[cfg(test)]
mod tests {
    #[test]
    fn title_bar_layout_keeps_windows_controls_on_right() {
        let macos = super::title_bar_chrome_layout(super::TitleBarChromePlatform::Macos);
        assert_eq!(macos.left_padding, super::TRAFFIC_LIGHT_RESERVED_WIDTH);
        assert_eq!(macos.right_padding, super::TAB_BAR_PADDING_RIGHT);
        assert_eq!(macos.windows_controls_width, 0.0);

        let windows = super::title_bar_chrome_layout(super::TitleBarChromePlatform::Windows);
        assert_eq!(windows.left_padding, 0.0);
        assert_eq!(windows.right_padding, 0.0);
        assert_eq!(
            windows.windows_controls_width,
            super::WINDOWS_WINDOW_CONTROL_BUTTON_WIDTH * 3.0
        );

        let other = super::title_bar_chrome_layout(super::TitleBarChromePlatform::Other);
        assert_eq!(other.left_padding, 0.0);
        assert_eq!(other.right_padding, super::TAB_BAR_PADDING_RIGHT);
        assert_eq!(other.windows_controls_width, 0.0);
    }
}
