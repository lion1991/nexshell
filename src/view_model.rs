use crate::layout::{Rect, ShellLayout};
use crate::{ShellModel, TabKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityView {
    pub id: &'static str,
    pub label: &'static str,
    pub active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabView {
    pub id: &'static str,
    pub title: &'static str,
    pub kind: TabKind,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalHostView {
    pub index: usize,
    pub session_id: Option<&'static str>,
    pub rect: Rect,
    pub visible: bool,
    pub focused: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolView {
    pub label: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellViewSnapshot {
    pub window_title: &'static str,
    pub layout: ShellLayout,
    pub activities: Vec<ActivityView>,
    pub tabs: Vec<TabView>,
    pub terminal_hosts: Vec<TerminalHostView>,
    pub bottom_tools: Vec<ToolView>,
    pub monitor_panel: Option<Rect>,
}

pub fn project(shell: &ShellModel, layout: ShellLayout) -> ShellViewSnapshot {
    let active_activity_id = shell.active_activity().id;
    let active_tab_id = shell.active_tab().id;
    let terminal_session_id = shell.active_terminal_tab().id;
    let terminal_visible =
        active_activity_id == "terminal" && shell.active_tab().kind == TabKind::Terminal;

    ShellViewSnapshot {
        window_title: shell.window_title(),
        layout,
        activities: shell
            .activity_items()
            .iter()
            .map(|item| ActivityView {
                id: item.id,
                label: item.label,
                active: item.id == active_activity_id,
            })
            .collect(),
        tabs: shell
            .tabs()
            .iter()
            .map(|tab| TabView {
                id: tab.id,
                title: tab.title,
                kind: tab.kind.clone(),
                active: tab.id == active_tab_id,
            })
            .collect(),
        terminal_hosts: (0..shell.terminal_pane_count())
            .map(|index| TerminalHostView {
                index,
                session_id: Some(terminal_session_id),
                rect: layout.terminal_host,
                visible: terminal_visible,
                focused: index == shell.focused_terminal_pane_index(),
            })
            .collect(),
        bottom_tools: shell
            .bottom_tools()
            .iter()
            .map(|label| ToolView { label })
            .collect(),
        monitor_panel: layout.monitor_panel,
    }
}
