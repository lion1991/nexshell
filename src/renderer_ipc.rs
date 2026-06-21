use crate::layout::Rect;
use crate::terminal_mount::{TerminalBackend, TerminalMount};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RendererIpcCommand {
    StartRender {
        session_id: &'static str,
    },
    StopRender {
        session_id: &'static str,
    },
    SetViewport {
        session_id: &'static str,
        rect: Rect,
    },
    SetFocused {
        session_id: &'static str,
        focused: bool,
    },
}

impl RendererIpcCommand {
    pub fn tauri_command_name(&self) -> &'static str {
        match self {
            Self::StartRender { .. } => "terminal_start_render",
            Self::StopRender { .. } => "terminal_stop_render",
            Self::SetViewport { .. } => "terminal_set_viewport",
            Self::SetFocused { .. } => "terminal_set_focused",
        }
    }
}

pub fn plan(mounts: &[TerminalMount]) -> Vec<RendererIpcCommand> {
    let mut commands = Vec::new();

    for mount in mounts {
        let Some(session_id) = mount.session_id else {
            continue;
        };

        match mount.backend {
            TerminalBackend::ExistingWindowWgpuSurface => {
                if mount.visible {
                    commands.push(RendererIpcCommand::StartRender { session_id });
                    commands.push(RendererIpcCommand::SetViewport {
                        session_id,
                        rect: mount.rect,
                    });
                    commands.push(RendererIpcCommand::SetFocused {
                        session_id,
                        focused: mount.focused,
                    });
                } else {
                    commands.push(RendererIpcCommand::StopRender { session_id });
                }
            }
        }
    }

    commands
}

pub fn diff_plan(previous: &[TerminalMount], current: &[TerminalMount]) -> Vec<RendererIpcCommand> {
    let mut commands = Vec::new();

    for mount in current {
        let Some(session_id) = mount.session_id else {
            continue;
        };
        let old = previous.iter().find(|candidate| {
            candidate.session_id == mount.session_id && candidate.pane_index == mount.pane_index
        });

        match old {
            None if mount.visible => commands.extend(plan(&[*mount])),
            None => {}
            Some(old) => commands.extend(diff_mount(*old, *mount, session_id)),
        }
    }

    for old in previous {
        let Some(session_id) = old.session_id else {
            continue;
        };
        let still_present = current
            .iter()
            .any(|mount| mount.session_id == old.session_id && mount.pane_index == old.pane_index);
        if !still_present && old.visible {
            commands.push(RendererIpcCommand::StopRender { session_id });
        }
    }

    commands
}

fn diff_mount(
    previous: TerminalMount,
    current: TerminalMount,
    session_id: &'static str,
) -> Vec<RendererIpcCommand> {
    if previous.backend != current.backend {
        return plan(&[current]);
    }

    match (previous.visible, current.visible) {
        (false, false) => Vec::new(),
        (false, true) => plan(&[current]),
        (true, false) => vec![RendererIpcCommand::StopRender { session_id }],
        (true, true) => {
            let mut commands = Vec::new();
            if previous.rect != current.rect {
                commands.push(RendererIpcCommand::SetViewport {
                    session_id,
                    rect: current.rect,
                });
            }
            if previous.focused != current.focused {
                commands.push(RendererIpcCommand::SetFocused {
                    session_id,
                    focused: current.focused,
                });
            }
            commands
        }
    }
}
