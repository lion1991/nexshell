use crate::layout::Rect;
use crate::view_model::ShellViewSnapshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalBackend {
    ExistingWindowWgpuSurface,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalMount {
    pub pane_index: usize,
    pub session_id: Option<&'static str>,
    pub rect: Rect,
    pub visible: bool,
    pub focused: bool,
    pub backend: TerminalBackend,
}

pub fn mounts_for(view: &ShellViewSnapshot) -> Vec<TerminalMount> {
    view.terminal_hosts
        .iter()
        .map(|host| TerminalMount {
            pane_index: host.index,
            session_id: host.session_id,
            rect: host.rect,
            visible: host.visible,
            focused: host.focused,
            backend: TerminalBackend::ExistingWindowWgpuSurface,
        })
        .collect()
}
