use warpui::{
    elements::{
        DispatchEventResult, EventHandler, Hoverable, MouseInBehavior, MouseStateHandle, ZIndex,
    },
    platform::Cursor,
    Element,
};

pub(crate) fn reassert_ibeam_cursor_on_mouse_move(child: Box<dyn Element>) -> Box<dyn Element> {
    EventHandler::new(child)
        .with_always_handle()
        .on_mouse_in(
            |ctx, _, _| {
                ctx.set_cursor(Cursor::IBeam, ZIndex::Overlay(usize::MAX));
                DispatchEventResult::PropagateToParent
            },
            Some(MouseInBehavior {
                fire_on_synthetic_events: true,
                fire_when_covered: false,
            }),
        )
        .finish()
}

pub(crate) fn text_input_ibeam_cursor_shell(
    state: MouseStateHandle,
    child: Box<dyn Element>,
) -> Box<dyn Element> {
    let child = Hoverable::new(state, move |_| child)
        .with_cursor(Cursor::IBeam)
        .with_defer_events_to_children()
        .finish();
    reassert_ibeam_cursor_on_mouse_move(child)
}
