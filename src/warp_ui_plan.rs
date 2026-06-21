#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WarpUiSource {
    pub path: &'static str,
    pub use_for: &'static str,
}

pub fn first_pass_sources() -> &'static [WarpUiSource] {
    &[
        WarpUiSource {
            path: "../warp/app/src/root_view.rs",
            use_for: "top-level native view composition and app chrome boundaries",
        },
        WarpUiSource {
            path: "../warp/app/src/window_settings.rs",
            use_for: "native window behavior and chrome configuration",
        },
        WarpUiSource {
            path: "../warp/app/src/tab.rs",
            use_for: "tab state, activation, lifecycle, and persistence boundaries",
        },
        WarpUiSource {
            path: "../warp/app/src/workspace/view/vertical_tabs.rs",
            use_for: "polished tab navigation, overflow behavior, and focus treatment",
        },
        WarpUiSource {
            path: "../warp/app/src/workspace/header_toolbar_item.rs",
            use_for: "toolbar item model and compact header actions",
        },
        WarpUiSource {
            path: "../warp/app/src/workspace/header_toolbar_editor.rs",
            use_for: "toolbar editing and item ordering concepts",
        },
        WarpUiSource {
            path: "../warp/app/src/ui_components/tab_selector.rs",
            use_for: "keyboard-driven tab switching and compact tab selection UX",
        },
        WarpUiSource {
            path: "../warp/app/src/pane_group/tree.rs",
            use_for: "split-pane tree shape and focus-preserving pane mutations",
        },
        WarpUiSource {
            path: "../warp/app/src/pane_group/focus_state.rs",
            use_for: "pane focus state transitions without coupling to block terminals",
        },
        WarpUiSource {
            path: "../warp/app/src/integration_testing/tab/assertion.rs",
            use_for: "tab-level integration assertions for future native-shell tests",
        },
    ]
}
