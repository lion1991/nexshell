//! 玻璃 backdrop 包装 Element：paint 时给当前 scene 层声明 BackdropBlur，
//! 渲染器先把该层之下的画面模糊+调色回贴，child 全部内容画在玻璃之上。
//! child 自身背景需透明/半透明，玻璃才可见（tint 已提供底色）。

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex as StdMutex, OnceLock};

use pathfinder_geometry::vector::Vector2F;
use warpui::color::ColorU;
use warpui::elements::{
    AfterLayoutContext, CornerRadius, Element, LayoutContext, Point, Radius, SizeConstraint, ZIndex,
};
use warpui::event::{DispatchedEvent, Event};
use warpui::geometry::rect::RectF;
use warpui::scene::ClipBounds;
use warpui::{AppContext, EventContext, PaintContext};

use crate::design_tokens::Glass;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlassQuality {
    Off,
    Frosted,
    Liquid,
}

impl Default for GlassQuality {
    fn default() -> Self {
        Self::Frosted
    }
}

impl GlassQuality {
    pub const ALL: [Self; 3] = [Self::Off, Self::Frosted, Self::Liquid];

    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Frosted => "frosted",
            Self::Liquid => "liquid",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "off" => Some(Self::Off),
            "frosted" => Some(Self::Frosted),
            "liquid" => Some(Self::Liquid),
            _ => None,
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Off => rust_i18n::t!("appearance_glass_quality_off").to_string(),
            Self::Frosted => rust_i18n::t!("appearance_glass_quality_frosted").to_string(),
            Self::Liquid => rust_i18n::t!("appearance_glass_quality_liquid").to_string(),
        }
    }

    pub fn to_index(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Frosted => 1,
            Self::Liquid => 2,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Off),
            1 => Some(Self::Frosted),
            2 => Some(Self::Liquid),
            _ => None,
        }
    }

    fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Off,
            2 => Self::Liquid,
            _ => Self::Frosted,
        }
    }

    fn as_raw(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Frosted => 1,
            Self::Liquid => 2,
        }
    }
}

static GLASS_QUALITY: AtomicU8 = AtomicU8::new(1);
static REDUCE_TRANSPARENCY: AtomicBool = AtomicBool::new(false);
static CURRENT_POINTER_POSITION: OnceLock<StdMutex<Option<Vector2F>>> = OnceLock::new();
const POINTER_LIGHT_DIR_EPSILON: f32 = 1e-3;

pub fn set_glass_quality(quality: GlassQuality) {
    GLASS_QUALITY.store(quality.as_raw(), Ordering::Relaxed);
}

pub fn current_glass_quality() -> GlassQuality {
    GlassQuality::from_raw(GLASS_QUALITY.load(Ordering::Relaxed))
}

pub fn set_reduce_transparency_enabled(enabled: bool) {
    REDUCE_TRANSPARENCY.store(enabled, Ordering::Relaxed);
}

pub fn reduce_transparency_enabled() -> bool {
    REDUCE_TRANSPARENCY.load(Ordering::Relaxed)
}

pub fn resolve_glass_quality(
    user_quality: GlassQuality,
    reduce_transparency: bool,
) -> GlassQuality {
    if reduce_transparency {
        GlassQuality::Off
    } else {
        user_quality
    }
}

pub fn current_effective_glass_quality() -> GlassQuality {
    resolve_glass_quality(current_glass_quality(), reduce_transparency_enabled())
}

fn solid_backing_color(tint_base: ColorU) -> ColorU {
    ColorU {
        a: 0xff,
        ..tint_base
    }
}

fn default_glass_light_dir() -> Vector2F {
    warpui::scene::GlassOptical::default().light_dir
}

fn pointer_position_cell() -> &'static StdMutex<Option<Vector2F>> {
    CURRENT_POINTER_POSITION.get_or_init(|| StdMutex::new(None))
}

fn set_current_pointer_position(position: Option<Vector2F>) -> bool {
    let mut current = pointer_position_cell()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if *current == position {
        false
    } else {
        *current = position;
        true
    }
}

fn current_pointer_position() -> Option<Vector2F> {
    let current = pointer_position_cell()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *current
}

#[cfg(test)]
fn clear_current_pointer_position_for_test() {
    set_current_pointer_position(None);
}

fn pointer_light_dir_for_rect(rect: RectF, pointer_position: Option<Vector2F>) -> Vector2F {
    let Some(pointer_position) = pointer_position else {
        return default_glass_light_dir();
    };

    let delta = pointer_position - rect.center();
    let length = delta.length();
    if length <= POINTER_LIGHT_DIR_EPSILON {
        default_glass_light_dir()
    } else {
        delta * (1.0 / length)
    }
}

fn pointer_position_from_event(event: &Event) -> Option<Vector2F> {
    match event {
        Event::MouseMoved {
            position,
            is_synthetic: false,
            ..
        }
        | Event::ScrollWheel { position, .. }
        | Event::LeftMouseDown { position, .. }
        | Event::LeftMouseUp { position, .. }
        | Event::LeftMouseDragged { position, .. }
        | Event::MiddleMouseDown { position, .. }
        | Event::RightMouseDown { position, .. }
        | Event::BackMouseDown { position, .. }
        | Event::ForwardMouseDown { position, .. } => Some(*position),
        Event::ModifierStateChanged { mouse_position, .. } => Some(*mouse_position),
        _ => None,
    }
}

fn record_pointer_position_from_event(event: &Event) -> bool {
    let Some(position) = pointer_position_from_event(event) else {
        return false;
    };
    set_current_pointer_position(Some(position))
}

fn backdrop_for_quality(
    glass: &Glass,
    quality: GlassQuality,
    rect: RectF,
    corner_radius: f32,
    tint_base: ColorU,
    pointer_position: Option<Vector2F>,
) -> Option<warpui::scene::BackdropBlur> {
    match quality {
        GlassQuality::Off => None,
        GlassQuality::Frosted => Some(glass.backdrop(rect, corner_radius, tint_base)),
        GlassQuality::Liquid => {
            let mut blur = glass.liquid_backdrop(rect, corner_radius, tint_base);
            blur.optical.light_dir = pointer_light_dir_for_rect(rect, pointer_position);
            Some(blur)
        }
    }
}

pub struct GlassBackdrop {
    child: Box<dyn Element>,
    glass: Glass,
    corner_radius: f32,
    tint_base: ColorU,
    // 宿主与下层内容同层时置 true：自开一层，保证模糊采样到本层已画内容。
    // 已在独立 overlay 层的浮层（菜单等）保持 false。
    own_layer: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl GlassBackdrop {
    /// tint_base 取浮层背景主题色；corner_radius 与 child 圆角一致。
    pub fn new(child: Box<dyn Element>, corner_radius: f32, tint_base: ColorU) -> Self {
        Self {
            child,
            glass: Glass::overlay(),
            corner_radius,
            tint_base,
            own_layer: false,
            size: None,
            origin: None,
        }
    }

    pub fn with_glass(mut self, glass: Glass) -> Self {
        self.glass = glass;
        self
    }

    pub fn with_own_layer(mut self) -> Self {
        self.own_layer = true;
        self
    }
}

impl Element for GlassBackdrop {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        self.child.after_layout(ctx, app);
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let quality = current_effective_glass_quality();
        let needs_sampling_layer = self.own_layer && quality != GlassQuality::Off;
        if needs_sampling_layer {
            ctx.scene.start_layer(ClipBounds::ActiveLayer);
        }
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        if let Some(size) = self.size {
            let rect = RectF::new(origin, size);
            match quality {
                GlassQuality::Off => {
                    ctx.scene
                        .draw_rect_without_hit_recording(rect)
                        .with_background(solid_backing_color(self.tint_base))
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(
                            self.corner_radius,
                        )));
                }
                GlassQuality::Frosted | GlassQuality::Liquid => {
                    let pointer_position = if quality == GlassQuality::Liquid {
                        current_pointer_position()
                    } else {
                        None
                    };
                    if let Some(blur) = backdrop_for_quality(
                        &self.glass,
                        quality,
                        rect,
                        self.corner_radius,
                        self.tint_base,
                        pointer_position,
                    ) {
                        ctx.scene.set_backdrop_blur(blur);
                    }
                }
            }
        }
        self.child.paint(origin, ctx, app);
        if needs_sampling_layer {
            ctx.scene.stop_layer();
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn z_index(&self) -> Option<ZIndex> {
        self.origin.map(|p| p.z_index())
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if record_pointer_position_from_event(event.raw_event())
            && current_effective_glass_quality() == GlassQuality::Liquid
        {
            ctx.notify();
        }
        self.child.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn glass_quality_round_trips_by_id() {
        assert_eq!(GlassQuality::from_id("off"), Some(GlassQuality::Off));
        assert_eq!(
            GlassQuality::from_id("frosted"),
            Some(GlassQuality::Frosted)
        );
        assert_eq!(GlassQuality::from_id("liquid"), Some(GlassQuality::Liquid));
        assert_eq!(GlassQuality::from_id("unknown"), None);
    }

    #[test]
    fn glass_quality_global_setting_round_trips() {
        let _guard = TEST_LOCK.lock().unwrap();

        set_glass_quality(GlassQuality::Off);
        assert_eq!(current_glass_quality(), GlassQuality::Off);

        set_glass_quality(GlassQuality::Frosted);
        assert_eq!(current_glass_quality(), GlassQuality::Frosted);
    }

    #[test]
    fn reduce_transparency_forces_effective_quality_off() {
        assert_eq!(
            resolve_glass_quality(GlassQuality::Liquid, true),
            GlassQuality::Off
        );
        assert_eq!(
            resolve_glass_quality(GlassQuality::Frosted, true),
            GlassQuality::Off
        );
    }

    #[test]
    fn effective_quality_preserves_user_choice_without_reduce_transparency() {
        assert_eq!(
            resolve_glass_quality(GlassQuality::Liquid, false),
            GlassQuality::Liquid
        );
        assert_eq!(
            resolve_glass_quality(GlassQuality::Frosted, false),
            GlassQuality::Frosted
        );
    }

    #[test]
    fn effective_quality_reads_reduce_transparency_global_state() {
        let _guard = TEST_LOCK.lock().unwrap();

        set_glass_quality(GlassQuality::Liquid);
        set_reduce_transparency_enabled(false);
        assert_eq!(current_effective_glass_quality(), GlassQuality::Liquid);

        set_reduce_transparency_enabled(true);
        assert_eq!(current_effective_glass_quality(), GlassQuality::Off);

        set_reduce_transparency_enabled(false);
        set_glass_quality(GlassQuality::Frosted);
    }

    #[test]
    fn off_backing_color_is_opaque() {
        let color = solid_backing_color(ColorU::new(10, 20, 30, 40));

        assert_eq!(color, ColorU::new(10, 20, 30, 255));
    }

    fn assert_vec2_near(actual: Vector2F, expected: Vector2F) {
        assert!(
            (actual - expected).length() < 1e-3,
            "expected {:?}, got {:?}",
            expected,
            actual
        );
    }

    #[test]
    fn pointer_light_dir_points_from_glass_center_to_pointer() {
        let rect = RectF::new(Vector2F::new(10.0, 20.0), Vector2F::new(100.0, 50.0));
        let pointer = Vector2F::new(90.0, 5.0);

        let light_dir = pointer_light_dir_for_rect(rect, Some(pointer));

        assert_vec2_near(light_dir, Vector2F::new(0.6, -0.8));
    }

    #[test]
    fn pointer_light_dir_uses_default_without_a_useful_pointer() {
        let rect = RectF::new(Vector2F::zero(), Vector2F::new(100.0, 50.0));
        let default_light_dir = warpui::scene::GlassOptical::default().light_dir;

        assert_vec2_near(pointer_light_dir_for_rect(rect, None), default_light_dir);
        assert_vec2_near(
            pointer_light_dir_for_rect(rect, Some(rect.center())),
            default_light_dir,
        );
    }

    #[test]
    fn backdrop_for_quality_maps_liquid_to_active_optical_only() {
        let rect = RectF::new(Vector2F::zero(), Vector2F::new(100.0, 50.0));
        let tint = ColorU::new(10, 20, 30, 255);
        let glass = Glass::overlay();

        assert!(backdrop_for_quality(&glass, GlassQuality::Off, rect, 12.0, tint, None).is_none());

        let frosted =
            backdrop_for_quality(&glass, GlassQuality::Frosted, rect, 12.0, tint, None).unwrap();
        assert!(!frosted.optical.is_active());

        let liquid =
            backdrop_for_quality(&glass, GlassQuality::Liquid, rect, 12.0, tint, None).unwrap();
        assert!(liquid.optical.is_active());
        assert_eq!(liquid.optical.thickness, 0.85);
        assert_eq!(liquid.optical.ior_delta, 0.28);
        assert_eq!(liquid.optical.specular, 0.55);
        assert_eq!(liquid.optical.crisp_mix, 0.85);
        assert_eq!(liquid.tint.a, 0x73);
    }

    #[test]
    fn backdrop_for_quality_applies_pointer_light_dir_only_to_liquid() {
        let rect = RectF::new(Vector2F::zero(), Vector2F::new(100.0, 50.0));
        let tint = ColorU::new(10, 20, 30, 255);
        let glass = Glass::overlay();
        let pointer = Some(rect.center() + Vector2F::new(30.0, -40.0));
        let expected_light_dir = Vector2F::new(0.6, -0.8);

        let frosted =
            backdrop_for_quality(&glass, GlassQuality::Frosted, rect, 12.0, tint, pointer).unwrap();
        assert!(!frosted.optical.is_active());
        assert_vec2_near(frosted.optical.light_dir, default_glass_light_dir());

        let liquid =
            backdrop_for_quality(&glass, GlassQuality::Liquid, rect, 12.0, tint, pointer).unwrap();
        assert!(liquid.optical.is_active());
        assert_vec2_near(liquid.optical.light_dir, expected_light_dir);
    }

    #[test]
    fn pointer_position_from_event_reads_real_mouse_positions() {
        let position = Vector2F::new(12.0, 34.0);

        let user_move = warpui::event::Event::MouseMoved {
            position,
            cmd: false,
            shift: false,
            is_synthetic: false,
        };
        assert_eq!(pointer_position_from_event(&user_move), Some(position));

        let synthetic_move = warpui::event::Event::MouseMoved {
            position,
            cmd: false,
            shift: false,
            is_synthetic: true,
        };
        assert_eq!(pointer_position_from_event(&synthetic_move), None);

        let drag = warpui::event::Event::LeftMouseDragged {
            position,
            modifiers: warpui::event::ModifiersState::default(),
        };
        assert_eq!(pointer_position_from_event(&drag), Some(position));
    }

    #[test]
    fn record_pointer_position_from_event_updates_pointer_cache() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_current_pointer_position_for_test();

        let first_position = Vector2F::new(12.0, 34.0);
        let real_move = warpui::event::Event::MouseMoved {
            position: first_position,
            cmd: false,
            shift: false,
            is_synthetic: false,
        };
        assert!(record_pointer_position_from_event(&real_move));
        assert_eq!(current_pointer_position(), Some(first_position));

        let synthetic_move = warpui::event::Event::MouseMoved {
            position: Vector2F::new(90.0, 10.0),
            cmd: false,
            shift: false,
            is_synthetic: true,
        };
        assert!(!record_pointer_position_from_event(&synthetic_move));
        assert_eq!(current_pointer_position(), Some(first_position));

        clear_current_pointer_position_for_test();
    }

    #[test]
    fn backdrop_for_quality_uses_popover_preset_for_pr4_surfaces() {
        let rect = RectF::new(Vector2F::zero(), Vector2F::new(100.0, 50.0));
        let tint = ColorU::new(10, 20, 30, 255);
        let glass = Glass::popover();

        let frosted =
            backdrop_for_quality(&glass, GlassQuality::Frosted, rect, 12.0, tint, None).unwrap();
        assert!(!frosted.optical.is_active());
        assert_eq!(frosted.tint.a, 0xd2);

        let liquid =
            backdrop_for_quality(&glass, GlassQuality::Liquid, rect, 12.0, tint, None).unwrap();
        assert!(liquid.optical.is_active());
        assert_eq!(liquid.optical.thickness, 0.55);
        assert_eq!(liquid.optical.ior_delta, 0.18);
        assert_eq!(liquid.optical.specular, 0.35);
        assert_eq!(liquid.optical.crisp_mix, 0.6);
        assert_eq!(liquid.tint.a, 0x96);
    }
}
