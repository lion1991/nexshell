use crate::ShellModel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellAction {
    ActivateActivity(&'static str),
    ActivateTab(&'static str),
    FocusTerminalPane(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellEffect {
    Handled,
    Noop,
}

pub fn reduce(shell: &mut ShellModel, action: ShellAction) -> ShellEffect {
    match action {
        ShellAction::ActivateActivity(id) => {
            if let Some(index) = shell.activity_items.iter().position(|item| item.id == id) {
                shell.active_activity_index = index;
                ShellEffect::Handled
            } else {
                ShellEffect::Noop
            }
        }
        ShellAction::ActivateTab(id) => {
            if let Some(index) = shell.tabs.iter().position(|tab| tab.id == id) {
                shell.active_tab_index = index;
                if shell.tabs[index].kind == crate::TabKind::Terminal {
                    shell.active_terminal_tab_index = index;
                }
                ShellEffect::Handled
            } else {
                ShellEffect::Noop
            }
        }
        ShellAction::FocusTerminalPane(index) => {
            if index < shell.terminal_pane_count {
                shell.focused_terminal_pane_index = index;
                ShellEffect::Handled
            } else {
                ShellEffect::Noop
            }
        }
    }
}
