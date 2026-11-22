//! 只画不挡鼠标的包装元件：子元素画在一个 click-through 层里，命中测试穿透到下面
//! （如盖在终端上的连接过渡层，点击、滚轮仍交给终端）。

use pathfinder_geometry::vector::Vector2F;
use warpui::elements::{AfterLayoutContext, Element, LayoutContext, Point, SizeConstraint};
use warpui::event::DispatchedEvent;
use warpui::scene::ClipBounds;
use warpui::{AppContext, EventContext, PaintContext};

pub struct ClickThrough {
    child: Box<dyn Element>,
}

impl ClickThrough {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self { child }
    }

    pub fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for ClickThrough {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.child.layout(constraint, ctx, app)
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        self.child.after_layout(ctx, app);
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        ctx.scene.start_layer(ClipBounds::ActiveLayer);
        ctx.scene.set_active_layer_click_through();
        self.child.paint(origin, ctx, app);
        ctx.scene.stop_layer();
    }

    fn size(&self) -> Option<Vector2F> {
        self.child.size()
    }

    fn origin(&self) -> Option<Point> {
        self.child.origin()
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
