//! Warp ui_components 子集（menu 本地化所需：buttons + blended_colors + icons）。
pub mod avatar;
pub(crate) mod blended_colors;
pub mod buttons;
pub mod red_notification_dot;

pub use warp_core::ui::icons;

const BORDER_RADIUS: f32 = 4.;
