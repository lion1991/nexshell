//! 玻璃 backdrop 包装 Element：paint 时给当前 scene 层声明 BackdropBlur，
//! 渲染器先把该层之下的画面模糊+调色回贴，child 全部内容画在玻璃之上。
//! child 自身背景需透明/半透明，玻璃才可见（tint 已提供底色）。

use pathfinder_geometry::vector::Vector2F;
use warpui::color::ColorU;
use warpui::elements::{AfterLayoutContext, Element, LayoutContext, Point, SizeConstraint, ZIndex};
use warpui::event::DispatchedEvent;
use warpui::geometry::rect::RectF;
use warpui::scene::ClipBounds;
use warpui::{AppContext, EventContext, PaintContext};

use crate::design_tokens::Glass;

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
        if self.own_layer {
            ctx.scene.start_layer(ClipBounds::ActiveLayer);
        }
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        if let Some(size) = self.size {
            ctx.scene.set_backdrop_blur(self.glass.backdrop(
                RectF::new(origin, size),
                self.corner_radius,
                self.tint_base,
            ));
        }
        self.child.paint(origin, ctx, app);
        if self.own_layer {
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
        self.child.dispatch_event(event, ctx, app)
    }
}
