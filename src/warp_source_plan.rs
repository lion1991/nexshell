#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WarpSource {
    pub path: &'static str,
    pub use_for: &'static str,
}

pub fn first_pass_sources() -> &'static [WarpSource] {
    &[
        WarpSource {
            path: "../warp/crates/warp_terminal/src/local_tty/event_loop.rs",
            use_for: "PTY read/write batching, lock fairness, resize/input channel draining",
        },
        WarpSource {
            path: "../warp/crates/warp_terminal/src/model/ansi/mod.rs",
            use_for: "parser orchestration and ref-testable byte processing boundaries",
        },
        WarpSource {
            path: "../warp/crates/warp_terminal/src/model/grid/flat_storage/mod.rs",
            use_for: "scrollback storage model for large terminal histories",
        },
        WarpSource {
            path: "../warp/crates/warp_terminal/src/model/grid/cell.rs",
            use_for: "compact cell layout, zero-width grapheme limits, attribute flags",
        },
        WarpSource {
            path: "../warp/crates/warp_terminal/src/model/grid/resize.rs",
            use_for: "resize and reflow behavior using flat storage",
        },
        WarpSource {
            path: "../warp/app/src/terminal/grid_renderer.rs",
            use_for: "visible-row rendering, glyph cache interaction, selection/find overlays",
        },
        WarpSource {
            path: "../warp/app/src/terminal/ref_tests/mod.rs",
            use_for: "recording-based terminal regression tests",
        },
    ]
}
