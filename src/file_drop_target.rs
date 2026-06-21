//! 拦截 `Event::DragAndDropFiles` 的轻量 Element 包装器。
//! WarpUI 的 EventHandler 不暴露 file-drop 钩子（参考
//! warp/app/src/terminal/alt_screen/alt_screen_element.rs 自己实现 dispatch_event 的做法），
//! 所以我们在 file panel 区域写一层包装：先把事件递给 child，child 没消费时再判定是否落在
//! 自身 bounds 内，落在内就调用 callback。

use std::sync::Arc;

use pathfinder_geometry::{rect::RectF, vector::Vector2F};
use warpui::elements::{AfterLayoutContext, Element, LayoutContext, Point, SizeConstraint, ZIndex};
use warpui::event::{DispatchedEvent, Event, InBoundsExt};
use warpui::{AppContext, EventContext, PaintContext};

pub type DropCallback = Arc<dyn Fn(&mut EventContext, Vec<String>) + Send + Sync + 'static>;

/// 把 child 包一层，OS file-drop 事件落入 bounds 时调用 callback。
pub struct FileDropTarget {
    child: Box<dyn Element>,
    callback: DropCallback,
    size: Option<Vector2F>,
    origin: Option<Point>,
    // true：file-drop 落在 bounds 内时优先自处理，不下发给 child
    // （内层若是编辑器，会抢先把路径插入光标并追加，需拦截）。
    intercept: bool,
}

impl FileDropTarget {
    pub fn new(child: Box<dyn Element>, callback: DropCallback) -> Self {
        Self {
            child,
            callback,
            size: None,
            origin: None,
            intercept: false,
        }
    }

    /// 开启拦截模式：file-drop 不再下发给 child，整框 bounds 都由 callback 处理。
    pub fn intercept(mut self) -> Self {
        self.intercept = true;
        self
    }

    fn handle_file_drop(&self, event: &DispatchedEvent, ctx: &mut EventContext) -> bool {
        let Event::DragAndDropFiles { paths, .. } = event.raw_event() else {
            return false;
        };
        let bounds = match (self.origin, self.size) {
            (Some(p), Some(s)) => RectF::new(p.xy(), s),
            _ => return false,
        };
        if event.raw_event().in_bounds(bounds) {
            (self.callback)(ctx, paths.clone());
            return true;
        }
        false
    }
}

impl Element for FileDropTarget {
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
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        self.child.paint(origin, ctx, app);
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
        // 拦截模式：file-drop 优先自处理，不让内层编辑器抢先追加。
        if self.intercept && self.handle_file_drop(event, ctx) {
            return true;
        }
        if self.child.dispatch_event(event, ctx, app) {
            return true;
        }
        // 默认 child-first：child 没消费再判定 bounds。
        if !self.intercept && self.handle_file_drop(event, ctx) {
            return true;
        }
        false
    }
}
