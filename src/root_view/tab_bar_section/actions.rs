// tab_bar_section::actions — RootView 标签栏 action handler + 标签生命周期（移动/重命名/关闭/重连/复制/拖拽）。
// 本文件只含 impl RootView，无自由函数。由 root_view/mod.rs handle_action 分发。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。

use std::sync::{Arc, Mutex};

use pathfinder_geometry::rect::RectF;

use crate::terminal_grid_element::TerminalGridAction;
use crate::terminal_view_helpers::serial_port_from_host_config;
use crate::{AppPage, RootView, TabMoveDirection, DEFAULT_COLS, DEFAULT_ROWS};
use nexshell::host_management::HostConnectionPlan;
use nexshell::terminal_runtime::LocalTerminalRuntime;
use nexshell::warp_tab_context_menu::{
    custom_title_from_editor, selected_tab_color_after_toggle,
};
use nexshell::text_editor::Event as EditorEvent;
use warp_core::ui::theme::AnsiColorIdentifier;
use warpui::ViewContext;

impl RootView {
    // === Chrome / 窗口控制 ===
    pub(in crate::root_view) fn handle_toggle_sidebar(&mut self, ctx: &mut ViewContext<Self>) {
        self.sidebar_open = !self.sidebar_open;
        if self.sidebar_open && self.active_tab_supports_host_overview() {
            self.sync_host_overview_monitor(ctx);
        }
        self.save_ui_settings();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_window_minimize(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.minimize_window();
    }

    pub(in crate::root_view) fn handle_window_toggle_maximize(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.toggle_maximized_window();
    }

    pub(in crate::root_view) fn handle_window_close(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.close_window();
    }

    pub(in crate::root_view) fn handle_show_host_management(&mut self, ctx: &mut ViewContext<Self>) {
        self.app_page = AppPage::HostManagement;
        self.reload_host_recent(); // 进页面即刷新最近访问，不必等刷新/连接
        ctx.notify();
    }

    // === 标签：新建 / 切换 / 移动 ===
    pub(in crate::root_view) fn handle_new_tab(&mut self, ctx: &mut ViewContext<Self>) {
        self.open_local_terminal_tab(ctx);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_toggle_new_session_menu(&mut self, ctx: &mut ViewContext<Self>) {
        if self.new_session_menu_open {
            self.new_session_menu_open = false;
        } else {
            use nexshell::menu::MenuItemFields;
            let items = vec![MenuItemFields::new(rust_i18n::t!("menu_terminal"))
                .with_on_select_action(TerminalGridAction::NewTab)
                .with_icon(warp_core::ui::icons::Icon::Terminal)
                .with_key_shortcut_label(Some("⌘T"))
                .into_item()];
            self.new_session_menu.update(ctx, |menu, view_ctx| {
                menu.set_items(items, view_ctx);
            });
            ctx.focus(&self.new_session_menu);
            self.new_session_menu_open = true;
            self.show_terminal_context_menu = None;
            self.settings_menu_open = false;
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_select_tab(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        if self.app_page != AppPage::Terminal {
            self.app_page = AppPage::Terminal;
        }
        self.activate_terminal_tab(index, ctx);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_move_tab_left(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        self.move_terminal_tab(index, TabMoveDirection::Left, ctx);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_move_tab_right(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        self.move_terminal_tab(index, TabMoveDirection::Right, ctx);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_activate_prev_tab(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.terminal_tabs.is_empty() {
            let index = if self.active_tab_index == 0 {
                self.terminal_tabs.len() - 1
            } else {
                self.active_tab_index - 1
            };
            self.activate_terminal_tab(index, ctx);
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn handle_activate_next_tab(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.terminal_tabs.is_empty() {
            let index = (self.active_tab_index + 1) % self.terminal_tabs.len();
            self.activate_terminal_tab(index, ctx);
            ctx.notify();
        }
    }

    // === 标签：关闭 / 重连 / 复制 ===
    pub(in crate::root_view) fn handle_close_tab(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        self.close_terminal_tab(index, ctx);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_close_other_tabs(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        self.close_other_terminal_tabs(index, ctx);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_close_tabs_right(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        self.close_terminal_tabs_right(index, ctx);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_reconnect_tab(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        self.reconnect_terminal_tab(index, ctx);
        ctx.notify();
    }

    /// 主动断开：保留终端内容，仅关闭底层 IO（远程/串口），UI 转为离线（横幅 + 红点）。
    pub(in crate::root_view) fn handle_disconnect_tab(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        if let Some(tab) = self.terminal_tabs.get(index) {
            for rt in tab
                .pane_terminals
                .values()
                .chain(std::iter::once(&tab.terminal))
            {
                if let Ok(mut rt) = rt.lock() {
                    rt.disconnect();
                }
            }
        }
        ctx.notify();
    }

    /// 切换录制：未录制则开始；录制中则停止并弹保存对话框写盘。
    pub(in crate::root_view) fn handle_toggle_tab_recording(
        &mut self,
        index: usize,
        ctx: &mut ViewContext<Self>,
    ) {
        let (stopped, title) = {
            let Some(tab) = self.terminal_tabs.get(index) else {
                return;
            };
            // 先取标题：window_title 内部会 lock tab.terminal，持锁后再调会重入死锁。
            let title = tab.window_title();
            // 与菜单状态读取同一解析：焦点 pane 回退主终端。
            let runtime = tab
                .pane_terminals
                .get(&tab.focused_pane_id)
                .unwrap_or(&tab.terminal);
            let Ok(runtime) = runtime.lock() else {
                return;
            };
            let stopped = runtime.stop_recording();
            if stopped.is_none() {
                runtime.start_recording();
            }
            (stopped, title)
        };
        self.show_tab_right_click_menu = None;

        let Some(bytes) = stopped else {
            self.host_state.notice =
                Some(rust_i18n::t!("toast_recording_started").to_string());
            ctx.notify();
            return;
        };

        let safe_title: String = title
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || "-_.".contains(c) {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let default_name = format!(
            "{}-{}.log",
            safe_title,
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let config = warpui::platform::SaveFilePickerConfiguration::new()
            .with_default_filename(default_name);
        ctx.open_save_file_picker(
            move |chosen, view, sub_ctx| {
                let Some(path_str) = chosen else {
                    sub_ctx.notify();
                    return;
                };
                view.host_state.notice = Some(match std::fs::write(&path_str, &bytes) {
                    Ok(()) => {
                        rust_i18n::t!("toast_recording_saved", path = path_str).to_string()
                    }
                    Err(error) => rust_i18n::t!(
                        "toast_recording_save_failed",
                        error = error.to_string()
                    )
                    .to_string(),
                });
                sub_ctx.notify();
            },
            config,
        );
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_duplicate_tab(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        self.duplicate_terminal_tab(index, ctx);
        ctx.notify();
    }

    // === 标签：颜色 / hover 宽度 / 拖拽 ===
    pub(in crate::root_view) fn handle_toggle_tab_color(
        &mut self,
        color: AnsiColorIdentifier,
        tab_index: usize,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(selected_color) = self.tab_selected_colors.get_mut(tab_index) {
            *selected_color = selected_tab_color_after_toggle(*selected_color, color);
        }
        self.show_tab_right_click_menu = None;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_tab_hover_width_start(&mut self, width: f32, ctx: &mut ViewContext<Self>) {
        self.tab_fixed_width = Some(width);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_tab_hover_width_end(&mut self, ctx: &mut ViewContext<Self>) {
        self.tab_fixed_width = None;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_start_tab_drag(&mut self, ctx: &mut ViewContext<Self>) {
        self.tab_drag_in_progress = true;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_drop_tab(&mut self, ctx: &mut ViewContext<Self>) {
        self.tab_drag_in_progress = false;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_tab_rename_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        if self.tab_being_renamed.is_none() {
            return;
        }
        match event {
            EditorEvent::Blurred | EditorEvent::Enter => self.finish_tab_rename(ctx),
            EditorEvent::Escape => self.cancel_tab_rename(ctx),
            _ => {}
        }
    }

    pub(crate) fn active_tab_supports_host_overview(&self) -> bool {
        if self.app_page != AppPage::Terminal {
            return false;
        }
        let Some(tab) = self.terminal_tabs.get(self.active_tab_index) else {
            return false;
        };
        let Some(host_id) = tab.host_id.as_deref() else {
            return false;
        };
        matches!(
            self.host_state.connection_plan_for(host_id),
            Some(HostConnectionPlan::SavedSsh { .. })
        )
    }

    pub(crate) fn rename_terminal_tab(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        if index >= self.terminal_tabs.len() {
            return;
        }

        self.activate_terminal_tab(index, ctx);
        self.tab_being_renamed = Some(index);
        self.show_tab_right_click_menu = None;

        let title = self.terminal_tabs[index].label();
        self.tab_rename_editor.update(ctx, move |editor, ctx| {
            editor.clear_buffer_and_reset_undo_stack(ctx);
            editor.insert_selected_text(&title, ctx);
        });
        ctx.focus(&self.tab_rename_editor);
        ctx.notify();
    }

    pub(crate) fn finish_tab_rename(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(index) = self.tab_being_renamed.take() else {
            return;
        };

        let title = self.tab_rename_editor.as_ref(ctx).buffer_text(ctx);
        if let Some(tab) = self.terminal_tabs.get_mut(index) {
            if tab.label() != title {
                tab.custom_label = custom_title_from_editor(&title);
            }
        }
        self.clear_tab_name_editor(ctx);
        self.sync_active_terminal_after_tab_list_change(ctx);
        ctx.focus_self();
        ctx.notify();
    }

    fn cancel_tab_rename(&mut self, ctx: &mut ViewContext<Self>) {
        if self.tab_being_renamed.take().is_none() {
            return;
        }
        self.clear_tab_name_editor(ctx);
        ctx.focus_self();
        ctx.notify();
    }

    fn clear_tab_name_editor(&mut self, ctx: &mut ViewContext<Self>) {
        self.tab_rename_editor.update(ctx, |editor, ctx| {
            editor.clear_buffer_and_reset_undo_stack(ctx);
        });
    }

    pub(crate) fn clear_terminal_tab_name(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        if let Some(tab) = self.terminal_tabs.get_mut(index) {
            tab.custom_label = None;
        }
        if self.tab_being_renamed == Some(index) {
            self.tab_being_renamed = None;
            self.clear_tab_name_editor(ctx);
        }
        self.show_tab_right_click_menu = None;
        self.sync_active_terminal_after_tab_list_change(ctx);
        ctx.notify();
    }

    pub(crate) fn on_tab_drag(&mut self, current_index: usize, position: RectF, ctx: &mut ViewContext<Self>) {
        let new_index = self.calculate_updated_tab_index(current_index, position, ctx);
        if new_index != current_index {
            self.move_terminal_tab(
                current_index,
                if new_index < current_index {
                    TabMoveDirection::Left
                } else {
                    TabMoveDirection::Right
                },
                ctx,
            );
        }
    }

    fn calculate_updated_tab_index(
        &self,
        current_index: usize,
        drag_position: RectF,
        ctx: &mut ViewContext<Self>,
    ) -> usize {
        let midpoint_x = (drag_position.min_x() + drag_position.max_x()) / 2.0;

        if current_index > 0 {
            let left_id = format!("nexshell_tab_position_{}", current_index - 1);
            if let Some(left_pos) = ctx.element_position_by_id(&left_id) {
                if midpoint_x < left_pos.max_x() {
                    return current_index - 1;
                }
            }
        }

        if current_index < self.terminal_tabs.len() - 1 {
            let right_id = format!("nexshell_tab_position_{}", current_index + 1);
            if let Some(right_pos) = ctx.element_position_by_id(&right_id) {
                if midpoint_x > right_pos.min_x() {
                    return current_index + 1;
                }
            }
        }

        current_index
    }

    fn move_terminal_tab(
        &mut self,
        index: usize,
        direction: TabMoveDirection,
        ctx: &mut ViewContext<Self>,
    ) {
        let tabs_len = self.terminal_tabs.len();
        let new_index = match direction {
            TabMoveDirection::Left if index > 0 => index - 1,
            TabMoveDirection::Right if index < tabs_len.saturating_sub(1) => index + 1,
            _ => return,
        };

        self.terminal_tabs.swap(index, new_index);
        self.tab_states.swap(index, new_index);
        self.tab_tooltip_states.swap(index, new_index);
        self.tab_close_states.swap(index, new_index);
        self.tab_draggable_states.swap(index, new_index);
        self.tab_selected_colors.swap(index, new_index);
        if let Ok(mut flags) = self.foreground_flags.lock() {
            if index < flags.len() && new_index < flags.len() {
                flags.swap(index, new_index);
            }
        }

        if index == self.active_tab_index {
            self.active_tab_index = new_index;
        } else if new_index == self.active_tab_index {
            self.active_tab_index = index;
        }
        if let Some(rename_index) = self.tab_being_renamed {
            if rename_index == index {
                self.tab_being_renamed = Some(new_index);
            } else if rename_index == new_index {
                self.tab_being_renamed = Some(index);
            }
        }
        self.show_tab_right_click_menu = None;
        self.sync_active_terminal_after_tab_list_change(ctx);
    }

    fn close_other_terminal_tabs(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        if index >= self.terminal_tabs.len() {
            return;
        }
        // 未保存保护（审查 #2）：待关集合含 dirty CodeViewer 时先弹一次汇总确认。
        let close_indices: Vec<usize> = (0..self.terminal_tabs.len())
            .filter(|&i| i != index)
            .collect();
        let dirty_ids = self.dirty_code_viewer_ids_in(&close_indices);
        if !dirty_ids.is_empty() {
            let anchor = self.terminal_tabs[index].id.clone();
            self.confirm_discard_code_viewer_batch(dirty_ids, anchor, false, ctx);
            return;
        }
        self.close_other_terminal_tabs_inner(index, ctx);
    }

    pub(in crate::root_view) fn close_other_terminal_tabs_inner(
        &mut self,
        index: usize,
        ctx: &mut ViewContext<Self>,
    ) {
        if index >= self.terminal_tabs.len() {
            return;
        }
        for remove_index in (0..self.terminal_tabs.len()).rev() {
            if remove_index != index {
                self.remove_terminal_tab_at(remove_index);
            }
        }
        self.active_tab_index = 0;
        self.tab_fixed_width = None;
        self.show_tab_right_click_menu = None;
        self.sync_active_terminal_after_tab_list_change(ctx);
    }

    fn close_terminal_tabs_right(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        if index >= self.terminal_tabs.len() {
            return;
        }
        let close_indices: Vec<usize> =
            ((index + 1)..self.terminal_tabs.len()).collect();
        let dirty_ids = self.dirty_code_viewer_ids_in(&close_indices);
        if !dirty_ids.is_empty() {
            let anchor = self.terminal_tabs[index].id.clone();
            self.confirm_discard_code_viewer_batch(dirty_ids, anchor, true, ctx);
            return;
        }
        self.close_terminal_tabs_right_inner(index, ctx);
    }

    pub(in crate::root_view) fn close_terminal_tabs_right_inner(
        &mut self,
        index: usize,
        ctx: &mut ViewContext<Self>,
    ) {
        if index >= self.terminal_tabs.len() {
            return;
        }
        for remove_index in ((index + 1)..self.terminal_tabs.len()).rev() {
            self.remove_terminal_tab_at(remove_index);
        }
        if self.active_tab_index > index {
            self.active_tab_index = index;
        }
        self.tab_fixed_width = None;
        self.show_tab_right_click_menu = None;
        self.sync_active_terminal_after_tab_list_change(ctx);
    }

    fn reconnect_terminal_tab(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        let Some(tab) = self.terminal_tabs.get(index) else {
            return;
        };
        let Some(host_id) = tab.host_id.clone() else {
            return;
        };
        let Some(plan) = self.host_state.connection_plan_for(&host_id) else {
            return;
        };

        let serial_port = match &plan {
            HostConnectionPlan::Serial { config, .. } => {
                let Some(port) = serial_port_from_host_config(config) else {
                    self.host_state.notice = Some("串口为空".to_string());
                    ctx.notify();
                    return;
                };
                if let Some(open_index) = self.open_serial_tab_index(&port, Some(index)) {
                    self.activate_terminal_tab(open_index, ctx);
                    self.host_state.notice = Some(format!(
                        "串口 {port} 已在标签页「{}」中打开",
                        self.terminal_tab_label(open_index)
                    ));
                    ctx.notify();
                    return;
                }
                self.release_terminal_runtime_for_tab(index);
                Some(port)
            }
            _ => None,
        };

        let (cols, rows) = self
            .last_resize_cells
            .lock()
            .map(|cells| *cells)
            .unwrap_or((DEFAULT_COLS, DEFAULT_ROWS));

        let mut new_terminal = match plan {
            HostConnectionPlan::SavedSsh {
                session_id, config, ..
            } => {
                let tab_session_id = self.unique_terminal_tab_id(&session_id);
                let status = format!(
                    "reconnecting SSH: {}@{}:{}",
                    config.username.trim(),
                    config.host.trim(),
                    config.port
                );
                LocalTerminalRuntime::spawn_remote_ssh_or_failed(
                    &tab_session_id,
                    Self::remote_ssh_config_from_host_config(&config),
                    status,
                    cols,
                    rows,
                )
            }
            HostConnectionPlan::DirectPty {
                session_id,
                command,
                ..
            } => {
                let tab_session_id = self.unique_terminal_tab_id(&session_id);
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
                session_id, config, ..
            } => {
                let tab_session_id = self.unique_terminal_tab_id(&session_id);
                LocalTerminalRuntime::spawn_serial_or_failed(
                    &tab_session_id,
                    Self::serial_config_from_host_config(&config),
                    Self::serial_status_from_host_config(&config),
                    cols,
                    rows,
                )
            }
            HostConnectionPlan::Unsupported { title, reason } => {
                self.host_state.notice = Some(
                    rust_i18n::t!("toast_connect_failed", title = title, reason = reason)
                        .to_string(),
                );
                return;
            }
        };

        let tab_id_for_ssh = self.terminal_tabs[index].id.clone();
        Self::attach_terminal_streams(&mut new_terminal, Some(tab_id_for_ssh.clone()), ctx);
        Self::attach_ssh_handle_stream(&mut new_terminal, tab_id_for_ssh, ctx);
        // 重连：旧 handle / sftp worker 都失效，全部清掉，等新 handle 推上来再重建。
        self.terminal_tabs[index].ssh_handle = None;
        // 旧 worker 持有的 SftpSession 已随原 SSH 通道关闭，留着会一直返回 "session closed"。
        self.terminal_tabs[index].sftp_worker = None;
        self.terminal_tabs[index].host_overview_monitor = None;
        self.terminal_tabs[index].file_panel_state.loading =
            self.terminal_tabs[index].file_panel_open;
        self.terminal_tabs[index].file_panel_state.error = None;
        self.terminal_tabs[index].serial_port = serial_port;
        let fg_handle = new_terminal.shell_is_foreground_handle();
        let new_terminal = Arc::new(Mutex::new(new_terminal));

        if let Ok(mut flags) = self.foreground_flags.lock() {
            if index < flags.len() {
                flags[index] = fg_handle;
            }
        }

        let focused_id = self.terminal_tabs[index].focused_pane_id;
        self.terminal_tabs[index]
            .pane_terminals
            .insert(focused_id, Arc::clone(&new_terminal));
        self.terminal_tabs[index].terminal = Arc::clone(&new_terminal);
        if self.active_tab_index == index {
            self.terminal = new_terminal;
            self.reset_active_terminal_view_state();
            self.sync_host_overview_monitor(ctx);
        }

        self.show_tab_right_click_menu = None;
        ctx.focus_self(); // 收回键盘焦点，否则回车会重放右键菜单项
    }

    fn duplicate_terminal_tab(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        let Some(tab) = self.terminal_tabs.get(index) else {
            return;
        };
        self.show_tab_right_click_menu = None;

        if let Some(host_id) = tab.host_id.clone() {
            self.connect_host(&host_id, ctx);
        } else {
            self.open_local_terminal_tab(ctx);
        }
    }
}
