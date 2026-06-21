use crate::renderer_ipc::{self, RendererIpcCommand};
use crate::terminal_lifecycle::{
    self, CellMetrics, FontSpec, SurfaceSize, TerminalLifecycleCommand, TerminalLifecycleConfig,
};
use crate::terminal_mount::{self, TerminalMount};
use crate::view_model::ShellViewSnapshot;

#[derive(Clone, Debug, PartialEq)]
pub struct NativeAdapterConfig {
    pub surface: SurfaceSize,
    pub font: FontSpec,
    pub cell: CellMetrics,
    pub scrollback_lines: usize,
    pub is_local: bool,
    pub cursor_style: &'static str,
    pub sync_highlight_rules: bool,
}

impl NativeAdapterConfig {
    pub fn default_for_surface(surface: SurfaceSize) -> Self {
        Self {
            surface,
            font: FontSpec {
                family: "JetBrains Mono",
                size: 10.0,
                letter_spacing: 0.0,
                line_height: 2.0,
                dpr: 2.0,
            },
            cell: CellMetrics {
                width: 7.0,
                height: 22.0,
            },
            scrollback_lines: 10_000,
            is_local: true,
            cursor_style: "block",
            sync_highlight_rules: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeAdapterState {
    pub mounts: Vec<TerminalMount>,
    pub initialized_sessions: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeAdapterPlan {
    pub lifecycle: Vec<TerminalLifecycleCommand>,
    pub renderer: Vec<RendererIpcCommand>,
    pub next_state: NativeAdapterState,
}

pub fn plan_transition(
    previous: &NativeAdapterState,
    view: &ShellViewSnapshot,
    config: &NativeAdapterConfig,
) -> NativeAdapterPlan {
    let mounts = terminal_mount::mounts_for(view);
    let lifecycle = lifecycle_plan(previous, &mounts, config);
    let renderer = renderer_ipc::diff_plan(&previous.mounts, &mounts);

    NativeAdapterPlan {
        lifecycle,
        renderer,
        next_state: next_state(previous, mounts),
    }
}

fn lifecycle_plan(
    previous: &NativeAdapterState,
    mounts: &[TerminalMount],
    config: &NativeAdapterConfig,
) -> Vec<TerminalLifecycleCommand> {
    let mut commands = Vec::new();

    for mount in mounts {
        let Some(session_id) = mount.session_id else {
            continue;
        };

        if !previous.initialized_sessions.contains(&session_id) {
            commands.extend(terminal_lifecycle::initial_plan(&TerminalLifecycleConfig {
                session_id,
                rect: mount.rect,
                surface: config.surface,
                font: config.font,
                scrollback_lines: config.scrollback_lines,
                is_local: config.is_local,
                cursor_style: config.cursor_style,
                sync_highlight_rules: config.sync_highlight_rules,
            }));
            continue;
        }

        let old = previous.mounts.iter().find(|candidate| {
            candidate.session_id == mount.session_id && candidate.pane_index == mount.pane_index
        });
        if old.is_some_and(|old| old.rect != mount.rect) {
            commands.extend(terminal_lifecycle::resize_plan(
                session_id,
                mount.rect,
                config.surface,
                config.cell,
            ));
        }
    }

    commands
}

fn next_state(previous: &NativeAdapterState, mounts: Vec<TerminalMount>) -> NativeAdapterState {
    let mut initialized_sessions = previous.initialized_sessions.clone();
    for mount in &mounts {
        let Some(session_id) = mount.session_id else {
            continue;
        };
        if !initialized_sessions.contains(&session_id) {
            initialized_sessions.push(session_id);
        }
    }

    NativeAdapterState {
        mounts,
        initialized_sessions,
    }
}
