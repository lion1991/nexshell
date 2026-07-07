//! 三档 elevation 阴影预设：主题无关（纯黑不同 alpha，明暗通用）。
//! overlay 最高浮层（右键菜单/下拉/命令面板）、popover 中层浮窗（查找条/goto/commit 详情）、
//! card 贴面卡片。每档 = key（紧深）+ ambient（大软淡）双层叠加。
//! 注意 spread_radius 是模糊衰减留的 quad 扩边（≈3×blur），非 CSS spread。

use warpui_core::color::ColorU;
use warpui_core::elements::Container;
use warpui_core::geometry::rect::RectF;
use warpui_core::geometry::vector::vec2f;
use warpui_core::scene::{self, BackdropBlur, DropShadow};

const fn black(a: u8) -> ColorU {
    ColorU {
        r: 0,
        g: 0,
        b: 0,
        a,
    }
}

fn shadow(offset_y: f32, blur: f32, spread: f32, alpha: u8) -> DropShadow {
    DropShadow {
        color: black(alpha),
        offset: vec2f(0.0, offset_y),
        blur_radius: blur,
        spread_radius: spread,
    }
}

pub struct Elevation {
    pub key: DropShadow,
    pub ambient: DropShadow,
}

impl Elevation {
    pub fn overlay() -> Self {
        Self {
            key: shadow(6.0, 8.0, 24.0, 0x55),
            ambient: shadow(16.0, 24.0, 72.0, 0x2e),
        }
    }

    pub fn popover() -> Self {
        Self {
            key: shadow(3.0, 5.0, 15.0, 0x48),
            ambient: shadow(8.0, 14.0, 42.0, 0x26),
        }
    }

    pub fn card() -> Self {
        Self {
            key: shadow(1.0, 2.0, 6.0, 0x38),
            ambient: shadow(4.0, 8.0, 24.0, 0x1f),
        }
    }

    /// scene::Rect（&mut 链式）挂双层。
    pub fn apply_scene(&self, rect: &mut scene::Rect) {
        rect.with_drop_shadow(self.key)
            .with_drop_shadow_ambient(self.ambient);
    }

    /// elements::Container（消费型 builder）挂双层。
    pub fn apply_container(&self, c: Container) -> Container {
        c.with_drop_shadow(self.key)
            .with_drop_shadow_ambient(self.ambient)
    }
}

/// 玻璃系 backdrop blur 预设：只定模糊半径/饱和度/tint alpha，
/// tint 色相由调用方传浮层主题色，保证各浮层玻璃质感一致。
pub struct Glass {
    pub radius: f32,
    pub saturation: f32,
    pub tint_alpha: u8,
}

impl Glass {
    /// 最高浮层（右键菜单/下拉/命令面板）。
    pub fn overlay() -> Self {
        Self {
            radius: 24.0,
            saturation: 1.4,
            tint_alpha: 0xc0,
        }
    }

    /// 组玻璃 scene 原语；tint_base 取浮层背景主题色（alpha 被预设覆盖）。
    pub fn backdrop(&self, rect: RectF, corner_radius: f32, tint_base: ColorU) -> BackdropBlur {
        BackdropBlur {
            rect,
            corner_radius,
            radius: self.radius,
            tint: ColorU {
                a: self.tint_alpha,
                ..tint_base
            },
            saturation: self.saturation,
        }
    }
}
