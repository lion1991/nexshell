pub mod editor;
pub mod font;
pub mod input_type;
pub mod select;

pub use editor::*;
pub use font::*;
pub use input_type::*;
pub use select::*;

use serde::{Deserialize, Serialize};

/// FG 颜色可自动调整以与 BG 对比时的策略。显式定义遮蔽 font::* 里宏生成的同名 marker。
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "When to adjust foreground color to ensure readability against the background.",
    rename_all = "snake_case"
)]
pub enum EnforceMinimumContrast {
    /// 从不改 FG 颜色
    Never,
    /// 仅当 FG 用默认色时可改
    #[default]
    OnlyNamedColors,
    /// 无论 FG 如何指定都改
    Always,
}
