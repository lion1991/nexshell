//! 监控统计小组件：RingGauge 环形仪表，走 GPU Ring 原语（SDF 圆弧）。
//! 轨道整圈 + 数值弧（12 点起顺时针、圆头端帽），中心可嵌 label 元素。

use pathfinder_geometry::vector::{vec2f, Vector2F};
use warpui::color::ColorU;
use warpui::elements::{AfterLayoutContext, Element, LayoutContext, Point, SizeConstraint};
use warpui::event::DispatchedEvent;
use warpui::scene::Ring;
use warpui::{AppContext, EventContext, PaintContext};

const TWO_PI: f32 = std::f32::consts::TAU;

pub struct RingGauge {
    diameter: f32,
    thickness: f32,
    track_color: ColorU,
    value_color: ColorU,
    /// 0..=1；None 只画轨道（数据未就绪）。
    fraction: Option<f32>,
    label: Option<Box<dyn Element>>,
    label_size: Vector2F,
    origin: Option<Point>,
}

impl RingGauge {
    pub fn new(diameter: f32, thickness: f32, track_color: ColorU, value_color: ColorU) -> Self {
        Self {
            diameter,
            thickness,
            track_color,
            value_color,
            fraction: None,
            label: None,
            label_size: Vector2F::zero(),
            origin: None,
        }
    }

    pub fn with_fraction(mut self, fraction: f32) -> Self {
        self.fraction = Some(fraction.clamp(0.0, 1.0));
        self
    }

    /// 中心 label（一般是百分比 Text），画在环之上、按环心居中。
    pub fn with_label(mut self, label: Box<dyn Element>) -> Self {
        self.label = Some(label);
        self
    }

    pub fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for RingGauge {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(self.diameter, self.diameter);
        if let Some(label) = &mut self.label {
            let inner = SizeConstraint::new(Vector2F::zero(), size);
            self.label_size = label.layout(inner, ctx, app);
        }
        size
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        if let Some(label) = &mut self.label {
            label.after_layout(ctx, app);
        }
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let center = origin + vec2f(self.diameter, self.diameter) * 0.5;
        // 中线半径内收半个环厚，外缘刚好贴住 diameter。
        let radius = (self.diameter - self.thickness) * 0.5;
        ctx.scene.draw_ring(Ring {
            center,
            radius,
            thickness: self.thickness,
            start_angle: 0.0,
            sweep_angle: TWO_PI,
            color: self.track_color,
        });
        if let Some(fraction) = self.fraction {
            // 过小的扫掠只剩端帽圆点，视觉误导，干脆不画。
            let sweep = fraction * TWO_PI;
            if sweep > 0.02 {
                ctx.scene.draw_ring(Ring {
                    center,
                    radius,
                    thickness: self.thickness,
                    start_angle: 0.0,
                    sweep_angle: sweep,
                    color: self.value_color,
                });
            }
        }
        if let Some(label) = &mut self.label {
            label.paint(center - self.label_size * 0.5, ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        Some(vec2f(self.diameter, self.diameter))
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
        if let Some(label) = &mut self.label {
            label.dispatch_event(event, ctx, app)
        } else {
            false
        }
    }
}

/// 单环规格：fraction=Some 才画数值弧，None 只画轨道。
pub struct RingSpec {
    pub fraction: Option<f32>,
    pub color: ColorU,
}

/// 同心多环仪表：外→内逐环内收，共享轨道色。无 label、无事件。
pub struct ConcentricRings {
    diameter: f32,
    thickness: f32,
    gap: f32,
    track_color: ColorU,
    rings: Vec<RingSpec>,
    /// 中心实心圆点 (直径, 颜色)：radius=0 的 Ring 即实心圆盘。
    center_dot: Option<(f32, ColorU)>,
    origin: Option<Point>,
}

impl ConcentricRings {
    pub fn new(diameter: f32, thickness: f32, gap: f32, track_color: ColorU) -> Self {
        Self {
            diameter,
            thickness,
            gap,
            track_color,
            rings: Vec::new(),
            center_dot: None,
            origin: None,
        }
    }

    /// 追加一环；rings[0] 为最外环。
    pub fn with_ring(mut self, ring: RingSpec) -> Self {
        self.rings.push(ring);
        self
    }

    pub fn with_center_dot(mut self, diameter: f32, color: ColorU) -> Self {
        self.center_dot = Some((diameter, color));
        self
    }

    pub fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for ConcentricRings {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        vec2f(self.diameter, self.diameter)
    }

    fn after_layout(&mut self, _ctx: &mut AfterLayoutContext, _app: &AppContext) {}

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let center = origin + vec2f(self.diameter, self.diameter) * 0.5;
        let outer_radius = (self.diameter - self.thickness) * 0.5;
        for (i, spec) in self.rings.iter().enumerate() {
            let radius = outer_radius - i as f32 * (self.thickness + self.gap);
            if radius <= 0.0 {
                break;
            }
            ctx.scene.draw_ring(Ring {
                center,
                radius,
                thickness: self.thickness,
                start_angle: 0.0,
                sweep_angle: TWO_PI,
                color: self.track_color,
            });
            if let Some(fraction) = spec.fraction {
                let sweep = fraction.clamp(0.0, 1.0) * TWO_PI;
                if sweep > 0.02 {
                    ctx.scene.draw_ring(Ring {
                        center,
                        radius,
                        thickness: self.thickness,
                        start_angle: 0.0,
                        sweep_angle: sweep,
                        color: spec.color,
                    });
                }
            }
        }
        if let Some((dot_diameter, color)) = self.center_dot {
            ctx.scene.draw_ring(Ring {
                center,
                radius: 0.0,
                thickness: dot_diameter,
                start_angle: 0.0,
                sweep_angle: TWO_PI,
                color,
            });
        }
    }

    fn size(&self) -> Option<Vector2F> {
        Some(vec2f(self.diameter, self.diameter))
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        _event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        false
    }
}
