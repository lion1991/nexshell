//! Warp horizontal tab-bar rules used by the native-shell spike.
//!
//! Source references:
//! - `warp/app/src/tab.rs`: `TAB_INDICATOR_HEIGHT`,
//!   `COMPACT_TAB_WIDTH_THRESHOLD`, tab `max_width = 200`, and hover
//!   fixed-width behavior.
//! - `warp/app/src/workspace/tab_settings.rs`: default
//!   `NewTabPlacement::AfterCurrentTab`.

pub const TAB_MAX_WIDTH: f32 = 200.0;
pub const TAB_INDICATOR_HEIGHT: f32 = 14.0;
pub const COMPACT_TAB_WIDTH_THRESHOLD: f32 = 42.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NewTabPlacement {
    AfterCurrentTab,
    AfterAllTabs,
}

impl Default for NewTabPlacement {
    fn default() -> Self {
        Self::AfterCurrentTab
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabWidthMode {
    Full,
    Compact,
}

impl TabWidthMode {
    pub fn for_width(width: f32) -> Self {
        if width < COMPACT_TAB_WIDTH_THRESHOLD {
            Self::Compact
        } else {
            Self::Full
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabWidthConstraint {
    Max(f32),
    Fixed(f32),
}

impl TabWidthConstraint {
    pub fn from_hover_width(hover_fixed_width: Option<f32>) -> Self {
        match hover_fixed_width {
            Some(width) => Self::Fixed(width),
            None => Self::Max(TAB_MAX_WIDTH),
        }
    }
}

pub fn new_tab_insert_index(
    tab_count: usize,
    active_tab_index: usize,
    placement: NewTabPlacement,
) -> usize {
    match placement {
        NewTabPlacement::AfterAllTabs => tab_count,
        NewTabPlacement::AfterCurrentTab => {
            if tab_count == 0 {
                0
            } else {
                active_tab_index.saturating_add(1).min(tab_count)
            }
        }
    }
}
