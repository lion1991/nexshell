//! 本地化自 warp/app/src/view_components/find.rs —— code_editor 只用 FindDirection + FIND_BAR_PADDING。

use serde::Serialize;

pub const FIND_BAR_PADDING: f32 = 4.;

#[derive(Debug, Copy, Clone, PartialEq, Serialize)]
pub enum FindDirection {
    Up,
    Down,
}
