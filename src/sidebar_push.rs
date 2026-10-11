//! 左侧栏推入 / 推出：侧栏与正文当成一条带横向平移，正文直接按最终宽度布局
//! （终端只 resize 一次，不逐帧 SIGWINCH），整体裁剪到自身范围内。
//!
//! reveal = 侧栏露出比例。打开时正文最终宽度 = 总宽 - 侧栏，右侧暂留空隙；
//! 关闭时正文最终宽度 = 总宽，右侧暂被裁掉。两者都随 reveal 平移到位。

use pathfinder_geometry::vector::{vec2f, Vector2F};
use warpui::elements::{AfterLayoutContext, Element, LayoutContext, Point, SizeConstraint};
use warpui::event::{DispatchedEvent, Event};
use warpui::geometry::rect::RectF;
use warpui::scene::ClipBounds;
use warpui::{AppContext, EventContext, PaintContext};

pub(crate) struct SidebarPush {
    sidebar: Option<Box<dyn Element>>,
    body: Box<dyn Element>,
    reveal: f32,
    /// 最终布局是否给侧栏留位（目标是打开）。
    reserve_sidebar: bool,
    sidebar_width: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SidebarPush {
    pub(crate) fn new(
        sidebar: Option<Box<dyn Element>>,
        body: Box<dyn Element>,
        reveal: f32,
        reserve_sidebar: bool,
    ) -> Self {
        Self {
            sidebar,
            body,
            reveal: reveal.clamp(0.0, 1.0),
            reserve_sidebar,
            sidebar_width: 0.0,
            size: None,
            origin: None,
        }
    }
}

impl Element for SidebarPush {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = constraint.max;
        self.sidebar_width = self.sidebar.as_mut().map_or(0.0, |sidebar| {
            let loose = SizeConstraint::new(vec2f(0.0, size.y()), size);
            sidebar.layout(loose, ctx, app).x()
        });
        let reserved = if self.reserve_sidebar {
            self.sidebar_width
        } else {
            0.0
        };
        self.body.layout(
            SizeConstraint::strict(vec2f((size.x() - reserved).max(0.0), size.y())),
            ctx,
            app,
        );
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        if let Some(sidebar) = self.sidebar.as_mut() {
            sidebar.after_layout(ctx, app);
        }
        self.body.after_layout(ctx, app);
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let size = self.size.unwrap_or_default();
        // 取整：满屏文字的亚像素位移在收敛尾部会显成抖动。
        let visible = (self.reveal * self.sidebar_width).round();
        ctx.scene
            .start_layer(ClipBounds::BoundedByActiveLayerAnd(RectF::new(
                origin, size,
            )));
        self.body.paint(origin + vec2f(visible, 0.0), ctx, app);
        if let Some(sidebar) = self.sidebar.as_mut() {
            sidebar.paint(origin + vec2f(visible - self.sidebar_width, 0.0), ctx, app);
        }
        ctx.scene.stop_layer();
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        // 只在滑动途中用到本元素。点击 / 滚轮：侧栏不响应（同标题栏按钮槽，收起时画的
        // 可能是已切走标签的概览）；正文只收落在自身范围内的，被裁掉的溢出带仍记着命中区，
        // 不拦会和右侧面板左缘抢点击。移动 / 抬起照常派发，hover 与拖拽能正常收尾。
        if let Some(position) = press_position(event.raw_event()) {
            let inside = self.origin.zip(self.size).is_some_and(|(origin, size)| {
                RectF::new(origin.xy(), size).contains_point(position)
            });
            return inside && self.body.dispatch_event(event, ctx, app);
        }
        let mut handled = self.body.dispatch_event(event, ctx, app);
        if let Some(sidebar) = self.sidebar.as_mut() {
            handled |= sidebar.dispatch_event(event, ctx, app);
        }
        handled
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

fn press_position(event: &Event) -> Option<Vector2F> {
    match event {
        Event::LeftMouseDown { position, .. }
        | Event::RightMouseDown { position, .. }
        | Event::MiddleMouseDown { position, .. }
        | Event::BackMouseDown { position, .. }
        | Event::ForwardMouseDown { position, .. }
        | Event::ScrollWheel { position, .. } => Some(*position),
        _ => None,
    }
}
