// terminal_section::split — 分屏 / pane 布局（附录 A #85-98）。
// 只含 impl RootView；split/close/navigate/focus/resize/maximize 由 mod.rs handle_action 分发，
// 用 pub(in crate::root_view)。sync_foreground_flag_for_tab 仅本文件内用，保持私有。

use std::sync::{Arc, Mutex};

use pathfinder_geometry::vector::Vector2F;
use warpui::ViewContext;

use crate::{RootView, DEFAULT_COLS, DEFAULT_ROWS};
use nexshell::host_management::HostConnectionPlan;
use nexshell::pane_state::NexPaneId;
use nexshell::pane_tree::{Direction, DraggedBorder, SplitDirection};
use nexshell::terminal_runtime::LocalTerminalRuntime;

impl RootView {
    fn sync_foreground_flag_for_tab(&self, tab_index: usize) {
        let fg = self
            .terminal
            .lock()
            .ok()
            .map(|rt| rt.shell_is_foreground_handle());
        if let Some(fg) = fg {
            if let Ok(mut flags) = self.foreground_flags.lock() {
                if tab_index < flags.len() {
                    flags[tab_index] = fg;
                }
            }
        }
    }

    pub(crate) fn split_active_pane(&mut self, direction: Direction, ctx: &mut ViewContext<Self>) {
        let tab = match self.terminal_tabs.get(self.active_tab_index) {
            Some(t) => t,
            None => return,
        };
        if tab.serial_port.is_some() {
            self.host_state.notice = Some("串口会话已独占设备，不能分屏复用同一串口".to_string());
            ctx.notify();
            return;
        }
        let current_focused = tab.focused_pane_id;
        let host_id = tab.host_id.clone();
        // 分屏继承源 pane 的 cwd（仅本地终端经 OSC7 上报，远程/串口恒为 None）
        let source_cwd = tab
            .terminal
            .lock()
            .ok()
            .and_then(|rt| rt.snapshot().local_cwd.clone())
            .filter(|p| p.is_dir());

        let (cols, rows) = self
            .last_resize_cells
            .lock()
            .map(|c| *c)
            .unwrap_or((DEFAULT_COLS, DEFAULT_ROWS));

        let session_id = self.next_local_terminal_session_id();

        let spawn_local = |sid: &str| match &source_cwd {
            Some(cwd) => LocalTerminalRuntime::spawn_local_in_dir_or_failed(sid, cwd, cols, rows),
            None => LocalTerminalRuntime::spawn_local_or_failed(sid, cols, rows),
        };

        let new_terminal = if let Some(host_id) = host_id {
            if let Some(plan) = self.host_state.connection_plan_for(&host_id) {
                match plan {
                    HostConnectionPlan::SavedSsh {
                        session_id: sid,
                        config,
                        ..
                    } => {
                        let tab_session_id = self.unique_terminal_tab_id(&sid);
                        LocalTerminalRuntime::spawn_remote_ssh_or_failed(
                            &tab_session_id,
                            Self::remote_ssh_config_from_host_config(&config),
                            format!(
                                "split SSH: {}@{}:{}",
                                config.username.trim(),
                                config.host.trim(),
                                config.port
                            ),
                            cols,
                            rows,
                        )
                    }
                    HostConnectionPlan::DirectPty {
                        session_id: sid,
                        command,
                        ..
                    } => {
                        let tab_session_id = self.unique_terminal_tab_id(&sid);
                        let args = command.args.iter().map(String::as_str).collect::<Vec<_>>();
                        LocalTerminalRuntime::spawn_command_or_failed(
                            &tab_session_id,
                            &command.program,
                            &args,
                            command.status,
                            cols,
                            rows,
                        )
                    }
                    HostConnectionPlan::Serial {
                        session_id: sid,
                        config,
                        ..
                    } => {
                        let tab_session_id = self.unique_terminal_tab_id(&sid);
                        LocalTerminalRuntime::spawn_serial_or_failed(
                            &tab_session_id,
                            Self::serial_config_from_host_config(&config),
                            Self::serial_status_from_host_config(&config),
                            cols,
                            rows,
                        )
                    }
                    // RDP 为整页 tab，不参与分屏（已定案）；分屏场景退回本地终端。
                    HostConnectionPlan::Rdp { .. } => spawn_local(&session_id),
                    HostConnectionPlan::Unsupported { .. } => spawn_local(&session_id),
                }
            } else {
                spawn_local(&session_id)
            }
        } else {
            spawn_local(&session_id)
        };

        let mut new_terminal = new_terminal;
        Self::attach_terminal_streams(&mut new_terminal, None, ctx);
        // Split pane: 新 PTY/SSH 是新 pane 上的，file_panel 当前按 tab 维度（per-tab）
        // 关联 ssh_handle，多 pane 场景的 handle 选择策略后续再细化。
        // 这里不接 ssh_handle_stream，receiver 自然 drop。
        let new_terminal = Arc::new(Mutex::new(new_terminal));

        let new_pane_id = NexPaneId::new();

        let tab = &mut self.terminal_tabs[self.active_tab_index];
        tab.pane_tree.split(current_focused, new_pane_id, direction);
        tab.pane_terminals
            .insert(new_pane_id, Arc::clone(&new_terminal));
        tab.focused_pane_id = new_pane_id;
        tab.terminal = Arc::clone(&new_terminal);
        tab.pane_presentation.clear_maximized();
        self.terminal = new_terminal;

        if let Ok(mut layout) = self.terminal_ime_layout.lock() {
            *layout = None;
        }
        self.reset_active_terminal_view_state();
        self.sync_foreground_flag_for_tab(self.active_tab_index);
        ctx.notify();
    }

    pub(crate) fn close_focused_pane(&mut self, ctx: &mut ViewContext<Self>) {
        let tab = match self.terminal_tabs.get_mut(self.active_tab_index) {
            Some(t) => t,
            None => return,
        };

        if tab.pane_tree.len() <= 1 {
            self.close_terminal_tab(self.active_tab_index, ctx);
            return;
        }

        let pane_id = tab.focused_pane_id;
        tab.pane_tree.remove(pane_id);
        tab.pane_terminals.remove(&pane_id);

        let new_focus = tab.pane_tree.pane_ids().first().copied();
        if let Some(id) = new_focus {
            tab.focused_pane_id = id;
            let t = tab
                .pane_terminals
                .get(&id)
                .cloned()
                .unwrap_or_else(|| Arc::clone(&tab.terminal));
            tab.terminal = Arc::clone(&t);
            self.terminal = t;
        }
        tab.pane_presentation.clear_maximized();

        if let Ok(mut layout) = self.terminal_ime_layout.lock() {
            *layout = None;
        }
        self.reset_active_terminal_view_state();
        self.sync_foreground_flag_for_tab(self.active_tab_index);
        ctx.notify();
    }

    pub(crate) fn navigate_pane(&mut self, direction: Direction, ctx: &mut ViewContext<Self>) {
        let tab = match self.terminal_tabs.get(self.active_tab_index) {
            Some(t) => t,
            None => return,
        };
        if tab.pane_tree.len() <= 1 {
            return;
        }
        let candidates = tab
            .pane_tree
            .panes_by_direction(tab.focused_pane_id, direction, ctx);
        if let Some(&target) = candidates.first() {
            let tab = &mut self.terminal_tabs[self.active_tab_index];
            tab.focused_pane_id = target;
            if let Some(t) = tab.pane_terminals.get(&target) {
                tab.terminal = Arc::clone(t);
                self.terminal = Arc::clone(t);
            }
            if let Ok(mut layout) = self.terminal_ime_layout.lock() {
                *layout = None;
            }
            self.reset_active_terminal_view_state();
            self.sync_foreground_flag_for_tab(self.active_tab_index);
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn handle_focus_pane(
        &mut self,
        pane_id: NexPaneId,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            tab.focused_pane_id = pane_id;
            if let Some(t) = tab.pane_terminals.get(&pane_id) {
                tab.terminal = Arc::clone(t);
                self.terminal = Arc::clone(t);
            }
        }
        if let Ok(mut layout) = self.terminal_ime_layout.lock() {
            *layout = None;
        }
        self.reset_active_terminal_view_state();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_start_pane_resizing(&mut self, border: DraggedBorder) {
        self.dragged_border = Some(border);
    }

    pub(in crate::root_view) fn handle_pane_resize_move(
        &mut self,
        position: Vector2F,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(border) = &mut self.dragged_border {
            let delta = match border.direction {
                SplitDirection::Horizontal => position.x() - border.previous_mouse_location.x(),
                SplitDirection::Vertical => position.y() - border.previous_mouse_location.y(),
            };
            if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
                tab.pane_tree.adjust_pane_size(border.border_id, delta, ctx);
            }
            border.previous_mouse_location = position;
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn handle_end_pane_resizing(&mut self) {
        self.dragged_border = None;
    }

    pub(in crate::root_view) fn handle_toggle_maximize_pane(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            tab.pane_presentation
                .toggle_maximize(tab.pane_tree.len(), tab.focused_pane_id);
        }
        ctx.notify();
    }
}
