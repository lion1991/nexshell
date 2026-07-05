#![cfg_attr(target_family = "wasm", allow(dead_code, unused_imports))]

mod comment_types;
mod comments;
pub(super) mod diff;
mod element;
pub mod find;
pub mod goto_line;
pub mod line;
mod line_iterator;
pub mod model;
mod nav_bar;
pub mod scroll;
pub mod view;

pub use comments::{EditorCommentsModel, EditorReviewComment};
pub(crate) use diff::DiffResult;
pub use element::GutterHoverTarget;
pub use nav_bar::NavBarBehavior;
pub use view::{
    init as init_code_editor_view, CodeEditorEvent, CodeEditorRenderOptions, CodeEditorView,
};
