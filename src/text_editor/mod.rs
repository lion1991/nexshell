pub mod accept_autosuggestion_keybinding_view;
pub mod autosuggestion_ignore_view;
pub mod editor_support;
pub mod flags;
pub mod settings;
mod soft_wrap;
mod view;
pub mod vim_registers;

use std::cmp;
use std::ops::Range;

/// Consumers of the editor should only interface with the view.
/// They should _not_ be able to interface with the internal
/// details of the editor (e.g. the [`Buffer`]).
pub use view::*;
pub use warpui::text::point::Point;
use warpui::AppContext;

pub fn init(app: &mut AppContext) {
    view::init(app);
}

trait RangeExt<T> {
    fn sorted(&self) -> (T, T);
}

impl<T: Ord + Clone> RangeExt<T> for Range<T> {
    fn sorted(&self) -> (T, T) {
        (
            cmp::min(&self.start, &self.end).clone(),
            cmp::max(&self.start, &self.end).clone(),
        )
    }
}
