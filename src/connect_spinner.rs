//! 连接过渡层的进度环：外圈雷达式扩散波纹 + 轨道 + 旋转呼吸弧，中心可嵌图标。走 GPU Ring 原语。
//! 只画一帧；时间 `t`（秒）由调用方给，动画靠 UI tick 每帧重画推进。

use pathfinder_geometry::vector::{vec2f, Vector2F};
use warpui::color::ColorU;
use warpui::elements::{AfterLayoutContext, Element, LayoutContext, Point, SizeConstraint};
use warpui::event::DispatchedEvent;
use warpui::scene::Ring;
use warpui::{AppContext, EventContext, PaintContext};

const TWO_PI: f32 = std::f32::consts::TAU;
/// 弧每秒转的圈数。
const SPIN_REVS_PER_SEC: f32 = 0.85;
/// 弧长呼吸周期（弧度/秒）。
const BREATHE_RATE: f32 = 2.6;
/// 波纹周期（秒）；两道波纹错开半个周期。
const PING_PERIOD: f32 = 1.8;

pub struct ConnectSpinner {
    diameter: f32,
    thickness: f32,
    t: f32,
    track_color: ColorU,
    arc_color: ColorU,
    /// 0..=1，整体淡入淡出。
    opacity: f32,
    center: Option<Box<dyn Element>>,
    center_size: Vector2F,
    origin: Option<Point>,
}

/// 旋转弧这一帧的 (起始角, 扫掠角)：匀速旋转，弧长在 0.15~0.55 圈之间呼吸。
pub fn spinner_arc(t: f32) -> (f32, f32) {
    let start = (t * SPIN_REVS_PER_SEC * TWO_PI).rem_euclid(TWO_PI);
    let breathe = 0.5 + 0.5 * (t * BREATHE_RATE).sin();
    (start, TWO_PI * (0.15 + 0.4 * breathe))
}

/// 波纹进度 0..1（`phase` 为相位偏移，0..1）。
pub fn ping_progress(t: f32, phase: f32) -> f32 {
    (t / PING_PERIOD + phase).rem_euclid(1.0)
}

fn with_alpha(color: ColorU, factor: f32) -> ColorU {
    ColorU::new(
        color.r,
        color.g,
        color.b,
        (f32::from(color.a) * factor.clamp(0.0, 1.0)).round() as u8,
    )
}

impl ConnectSpinner {
    pub fn new(
        diameter: f32,
        thickness: f32,
        t: f32,
        track_color: ColorU,
        arc_color: ColorU,
    ) -> Self {
        Self {
            diameter,
            thickness,
            t,
            track_color,
            arc_color,
            opacity: 1.0,
            center: None,
            center_size: Vector2F::zero(),
            origin: None,
        }
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    pub fn with_center(mut self, center: Box<dyn Element>) -> Self {
        self.center = Some(center);
        self
    }

    pub fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }

    /// 元件占位边长：给波纹留出外扩空间。
    fn extent(&self) -> f32 {
        self.diameter * 1.8
    }
}

impl Element for ConnectSpinner {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(self.extent(), self.extent());
        if let Some(center) = &mut self.center {
            let inner = SizeConstraint::new(Vector2F::zero(), vec2f(self.diameter, self.diameter));
            self.center_size = center.layout(inner, ctx, app);
        }
        size
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        if let Some(center) = &mut self.center {
            center.after_layout(ctx, app);
        }
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let extent = self.extent();
        let center = origin + vec2f(extent, extent) * 0.5;
        let radius = (self.diameter - self.thickness) * 0.5;
        let max_radius = extent * 0.5 - 1.0;

        // 波纹：从环外沿扩散到占位边缘，边扩边淡。
        for phase in [0.0, 0.5] {
            let p = ping_progress(self.t, phase);
            let eased = 1.0 - (1.0 - p).powi(2);
            ctx.scene.draw_ring(Ring {
                center,
                radius: radius + (max_radius - radius) * eased,
                thickness: 1.5,
                start_angle: 0.0,
                sweep_angle: TWO_PI,
                color: with_alpha(self.arc_color, 0.35 * (1.0 - p) * self.opacity),
            });
        }

        ctx.scene.draw_ring(Ring {
            center,
            radius,
            thickness: self.thickness,
            start_angle: 0.0,
            sweep_angle: TWO_PI,
            color: with_alpha(self.track_color, self.opacity),
        });
        let (start, sweep) = spinner_arc(self.t);
        ctx.scene.draw_ring(Ring {
            center,
            radius,
            thickness: self.thickness,
            start_angle: start,
            sweep_angle: sweep,
            color: with_alpha(self.arc_color, self.opacity),
        });

        if let Some(label) = &mut self.center {
            label.paint(center - self.center_size * 0.5, ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        Some(vec2f(self.extent(), self.extent()))
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        match &mut self.center {
            Some(center) => center.dispatch_event(event, ctx, app),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_start_wraps_and_sweep_stays_in_range() {
        for i in 0..200 {
            let (start, sweep) = spinner_arc(i as f32 * 0.37);
            assert!((0.0..TWO_PI).contains(&start));
            assert!(sweep >= TWO_PI * 0.15 - 1e-4 && sweep <= TWO_PI * 0.55 + 1e-4);
        }
    }

    #[test]
    fn ping_progress_wraps_with_phase() {
        assert!((ping_progress(0.0, 0.5) - 0.5).abs() < 1e-6);
        assert!(ping_progress(PING_PERIOD * 0.75, 0.5) < 0.3);
    }
}
