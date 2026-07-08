// context_menus_section — RootView 的右键 / 上下文菜单：tab 右键、终端、文件面板、git 面板、
// 主机卡片、进程列表。render_* 由 mod.rs View::render 调用、show_* / toggle_* 由 mod.rs
// handle_action 分发——均 pub(in crate::root_view)。*_items 仅本文件内构造，保持私有。
// 菜单具体内容（i18n key / disable 条件）按面板分别成 fn。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。

use pathfinder_geometry::vector::Vector2F;
use warp_core::ui::appearance::Appearance;
use warpui::{Element, SingletonEntity as _, ViewContext};

use crate::file_panel_view_helpers::{
    local_file_panel_context_menu_items, remote_file_panel_context_menu_items,
};
use crate::git_panel_view_helpers::{
    git_panel_context_menu_items, git_panel_context_paths, GitPanelContextPaths,
};
use crate::terminal_grid_element::TerminalGridAction;
use crate::{RootView, TerminalSessionKind};
use nexshell::file_panel::{
    apply_file_panel_selection, apply_file_panel_tree_selection, FilePanelSelectMode,
};
use nexshell::git_ops::{GitDiffKind, GitDiffSelection};
use nexshell::git_panel::{apply_git_panel_selection, GitPanelSelectMode};
use nexshell::warp_tab_context_menu::{
    horizontal_tab_context_menu_items, HorizontalTabColorOptions, HorizontalTabContextMenuActions,
    TabContextMenuAnchor, TAB_COLOR_OPTIONS,
};

impl RootView {
    pub(in crate::root_view) fn render_tab_right_click_menu(&self) -> Box<dyn Element> {
        warpui::elements::ChildView::new(&self.tab_right_click_menu).finish()
    }

    pub(in crate::root_view) fn render_terminal_context_menu(&self) -> Box<dyn Element> {
        warpui::elements::ChildView::new(&self.terminal_context_menu).finish()
    }

    pub(in crate::root_view) fn render_file_panel_context_menu(&self) -> Box<dyn Element> {
        warpui::elements::ChildView::new(&self.file_panel_context_menu).finish()
    }

    pub(in crate::root_view) fn render_git_panel_context_menu(&self) -> Box<dyn Element> {
        warpui::elements::ChildView::new(&self.git_panel_context_menu).finish()
    }

    pub(in crate::root_view) fn render_process_list_context_menu(&self) -> Box<dyn Element> {
        warpui::elements::ChildView::new(&self.process_list_context_menu).finish()
    }

    pub(in crate::root_view) fn render_host_card_context_menu(&self) -> Box<dyn Element> {
        warpui::elements::ChildView::new(&self.host_card_context_menu).finish()
    }

    fn tab_right_click_menu_items(
        &self,
        index: usize,
        ctx: &ViewContext<Self>,
    ) -> Vec<nexshell::menu::MenuItem<TerminalGridAction>> {
        let terminal_colors = Appearance::as_ref(ctx).theme().terminal_colors().normal;
        let selected_color = self.tab_selected_colors.get(index).copied().flatten();
        // 内容标签（编辑器 / diff）无重命名 / 复制 / 重连语义，仅留移动 / 关闭 / 染色。
        let is_content_tab = self.terminal_tabs.get(index).map_or(false, |tab| {
            matches!(
                tab.kind,
                TerminalSessionKind::GitDiff | TerminalSessionKind::CodeViewer
            )
        });
        horizontal_tab_context_menu_items(
            index,
            self.terminal_tabs.len(),
            true,
            HorizontalTabContextMenuActions {
                rename_tab: (!is_content_tab).then(|| TerminalGridAction::RenameTab(index)),
                reset_tab_name: self
                    .terminal_tabs
                    .get(index)
                    .filter(|_| !is_content_tab)
                    .and_then(|tab| tab.custom_label.as_ref())
                    .map(|_| TerminalGridAction::ResetTabName(index)),
                duplicate_tab: (!is_content_tab).then(|| TerminalGridAction::DuplicateTab(index)),
                move_tab_right: TerminalGridAction::MoveTabRight(index),
                move_tab_left: TerminalGridAction::MoveTabLeft(index),
                close_tab: Some(TerminalGridAction::CloseTab(index)),
                close_other_tabs: TerminalGridAction::CloseOtherTabs(index),
                close_tabs_right: TerminalGridAction::CloseTabsRight(index),
                reconnect_tab: self
                    .terminal_tabs
                    .get(index)
                    .filter(|tab| !is_content_tab && tab.host_id.is_some())
                    .map(|_| TerminalGridAction::ReconnectTab(index)),
                disconnect_tab: self
                    .terminal_tabs
                    .get(index)
                    .filter(|tab| !is_content_tab && tab.can_disconnect())
                    .map(|_| TerminalGridAction::DisconnectTab(index)),
                connection_info: self
                    .terminal_tabs
                    .get(index)
                    .filter(|tab| tab.rdp.is_some())
                    .map(|_| TerminalGridAction::ToggleRdpConnectionInfo(index)),
                toggle_recording: (!is_content_tab)
                    .then(|| TerminalGridAction::ToggleTabRecording(index)),
                // 与 handler 同一解析：焦点 pane 回退主终端，分屏下文案才与实际一致。
                is_recording: self.terminal_tabs.get(index).map_or(false, |tab| {
                    tab.pane_terminals
                        .get(&tab.focused_pane_id)
                        .unwrap_or(&tab.terminal)
                        .lock()
                        .map_or(false, |rt| rt.is_recording())
                }),
                save_current_tab_as_new_config: None,
                color_options: Some(HorizontalTabColorOptions {
                    selected_color,
                    terminal_colors,
                    toggle_tab_color_actions: TAB_COLOR_OPTIONS.map(|color| {
                        TerminalGridAction::ToggleTabColor {
                            color,
                            tab_index: index,
                        }
                    }),
                }),
            },
        )
    }

    pub(crate) fn toggle_tab_right_click_menu(
        &mut self,
        tab_index: usize,
        anchor: TabContextMenuAnchor,
        ctx: &mut ViewContext<Self>,
    ) {
        // warp/app/src/workspace/view.rs:6488-6507.
        if self.show_tab_right_click_menu.is_some() {
            self.show_tab_right_click_menu = None;
            ctx.notify();
            return;
        }
        if tab_index >= self.terminal_tabs.len() {
            return;
        }

        let menu_items = self.tab_right_click_menu_items(tab_index, ctx);
        let origin = match anchor {
            TabContextMenuAnchor::Pointer(position) => Some(position),
            TabContextMenuAnchor::VerticalTabsKebab => None,
        };
        self.tab_right_click_menu.update(ctx, |menu, view_ctx| {
            menu.set_items(menu_items, view_ctx);
            menu.set_origin(origin);
        });
        self.show_tab_right_click_menu = Some((tab_index, anchor));
        self.new_session_menu_open = false;
        self.settings_menu_open = false;
        self.show_terminal_context_menu = None;
        self.show_git_panel_context_menu = None;
        ctx.focus(&self.tab_right_click_menu);
        ctx.notify();
    }

    // warp: view.rs:16064-16113 (rebuild_alt_screen_context_menu_items)
    pub(crate) fn show_terminal_context_menu(
        &mut self,
        position: Vector2F,
        has_selection: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        if self.show_terminal_context_menu.is_some() {
            self.show_terminal_context_menu = None;
            ctx.notify();
            return;
        }

        let menu_items = self.terminal_context_menu_items(has_selection);
        self.terminal_context_menu.update(ctx, |menu, view_ctx| {
            menu.set_items(menu_items, view_ctx);
            menu.set_origin(Some(position));
        });
        self.show_terminal_context_menu = Some(position);
        self.show_tab_right_click_menu = None;
        self.show_file_panel_context_menu = None;
        self.show_git_panel_context_menu = None;
        self.show_process_list_context_menu = None;
        self.show_host_card_context_menu = None;
        self.new_session_menu_open = false;
        self.settings_menu_open = false;
        ctx.focus(&self.terminal_context_menu);
        ctx.notify();
    }

    fn terminal_context_menu_items(
        &self,
        has_selection: bool,
    ) -> Vec<nexshell::menu::MenuItem<TerminalGridAction>> {
        use nexshell::menu::{MenuItem, MenuItemFields};

        let mut items = vec![];

        // warp: view.rs:16074-16080 — Copy（有选区时）
        if has_selection {
            items.push(
                MenuItemFields::new(rust_i18n::t!("menu_copy"))
                    .with_key_shortcut_label(Some("⌘C"))
                    .with_on_select_action(TerminalGridAction::CopySelection)
                    .into_item(),
            );
        }

        items.push(
            MenuItemFields::new(rust_i18n::t!("menu_paste"))
                .with_key_shortcut_label(Some("⌘V"))
                .with_on_select_action(TerminalGridAction::PasteClipboard)
                .into_item(),
        );

        items.push(MenuItem::Separator);

        items.push(
            MenuItemFields::new(rust_i18n::t!("menu_find"))
                .with_key_shortcut_label(Some("⌘F"))
                .with_on_select_action(TerminalGridAction::OpenFindBar)
                .into_item(),
        );

        items.push(
            MenuItemFields::new(rust_i18n::t!("menu_clear_screen"))
                .with_key_shortcut_label(Some("⌘K"))
                .with_on_select_action(TerminalGridAction::ClearVisibleScreen)
                .into_item(),
        );

        items.push(MenuItem::Separator);

        items.push(
            MenuItemFields::new(rust_i18n::t!("ctx_split_right"))
                .with_key_shortcut_label(Some("⌘D"))
                .with_on_select_action(TerminalGridAction::SplitRight)
                .into_item(),
        );
        items.push(
            MenuItemFields::new(rust_i18n::t!("ctx_split_left"))
                .with_on_select_action(TerminalGridAction::SplitLeft)
                .into_item(),
        );
        items.push(
            MenuItemFields::new(rust_i18n::t!("ctx_split_down"))
                .with_key_shortcut_label(Some("⇧⌘D"))
                .with_on_select_action(TerminalGridAction::SplitDown)
                .into_item(),
        );
        items.push(
            MenuItemFields::new(rust_i18n::t!("ctx_split_up"))
                .with_on_select_action(TerminalGridAction::SplitUp)
                .into_item(),
        );

        let in_split = self
            .terminal_tabs
            .get(self.active_tab_index)
            .map_or(false, |t| t.pane_tree.len() > 1);
        if in_split {
            let maximize_label = if self.maximized_pane.is_some() {
                rust_i18n::t!("ctx_restore_pane")
            } else {
                rust_i18n::t!("ctx_maximize_pane")
            };
            items.push(
                MenuItemFields::new(maximize_label)
                    .with_key_shortcut_label(Some("⇧⌘↩"))
                    .with_on_select_action(TerminalGridAction::ToggleMaximizePane)
                    .into_item(),
            );
            items.push(
                MenuItemFields::new(rust_i18n::t!("ctx_close_pane"))
                    .with_key_shortcut_label(Some("⇧⌘W"))
                    .with_on_select_action(TerminalGridAction::ClosePane)
                    .into_item(),
            );
        }

        items
    }

    pub(crate) fn show_file_panel_context_menu(
        &mut self,
        name: Option<String>,
        is_dir: bool,
        position: Vector2F,
        ctx: &mut ViewContext<Self>,
    ) {
        // 右键 entry：若该项不在选择集合内，则单独选中；已选中则保留现有多选集合
        if let Some(target) = &name {
            if let Some(tab) = self.file_panel_tab_mut() {
                if !tab.file_panel_state.selected_names.contains(target) {
                    if matches!(tab.kind, TerminalSessionKind::Local) {
                        apply_file_panel_tree_selection(
                            &mut tab.file_panel_state,
                            target,
                            FilePanelSelectMode::Replace,
                        );
                    } else {
                        apply_file_panel_selection(
                            &mut tab.file_panel_state,
                            target,
                            FilePanelSelectMode::Replace,
                        );
                    }
                }
            }
        }
        let items = self.file_panel_context_menu_items(name, is_dir);
        if items.is_empty() {
            return;
        }
        self.file_panel_context_menu.update(ctx, |menu, view_ctx| {
            menu.set_items(items, view_ctx);
            menu.set_origin(Some(position));
        });
        self.show_file_panel_context_menu = Some(position);
        self.show_terminal_context_menu = None;
        self.show_git_panel_context_menu = None;
        self.show_tab_right_click_menu = None;
        self.show_process_list_context_menu = None;
        self.show_host_card_context_menu = None;
        self.new_session_menu_open = false;
        self.settings_menu_open = false;
        ctx.focus(&self.file_panel_context_menu);
        ctx.notify();
    }

    pub(crate) fn show_file_panel_context_menu_close(&mut self, ctx: &mut ViewContext<Self>) {
        if self.show_file_panel_context_menu.is_some() {
            self.show_file_panel_context_menu = None;
            ctx.notify();
        }
    }

    pub(crate) fn show_git_panel_context_menu(
        &mut self,
        tab_id: String,
        path: String,
        kind: GitDiffKind,
        discard_enabled: bool,
        position: Vector2F,
        ctx: &mut ViewContext<Self>,
    ) {
        let target = GitDiffSelection {
            path: path.clone(),
            kind,
        };
        let paths = if let Some(tab) = self.terminal_tabs.iter_mut().find(|tab| tab.id == tab_id) {
            if !tab.git_panel_state.selected_entries.contains(&target) {
                apply_git_panel_selection(
                    &mut tab.git_panel_state,
                    target.clone(),
                    GitPanelSelectMode::Replace,
                );
            }
            git_panel_context_paths(&tab.git_panel_state, &target)
        } else {
            GitPanelContextPaths {
                same_kind: vec![path.clone()],
                stageable: vec![path],
            }
        };
        let items = git_panel_context_menu_items(&tab_id, kind, paths, discard_enabled);
        self.git_panel_context_menu.update(ctx, |menu, view_ctx| {
            menu.set_items(items, view_ctx);
            menu.set_origin(Some(position));
        });
        self.show_git_panel_context_menu = Some(position);
        self.show_file_panel_context_menu = None;
        self.show_terminal_context_menu = None;
        self.show_tab_right_click_menu = None;
        self.show_process_list_context_menu = None;
        self.show_host_card_context_menu = None;
        self.new_session_menu_open = false;
        self.settings_menu_open = false;
        ctx.focus(&self.git_panel_context_menu);
        ctx.notify();
    }

    pub(crate) fn show_git_panel_context_menu_close(&mut self, ctx: &mut ViewContext<Self>) {
        if self.show_git_panel_context_menu.is_some() {
            self.show_git_panel_context_menu = None;
            ctx.notify();
        }
    }

    pub(crate) fn show_host_card_context_menu(
        &mut self,
        host_id: String,
        position: Vector2F,
        ctx: &mut ViewContext<Self>,
    ) {
        // 右键目标高亮：独立状态，不进 selected_host_ids（不触发底部选择栏）
        self.host_state.context_menu_target = Some(host_id.clone());
        let items = self.host_card_context_menu_items(&host_id);
        if items.is_empty() {
            return;
        }
        self.host_card_context_menu.update(ctx, |menu, view_ctx| {
            menu.set_items(items, view_ctx);
            menu.set_origin(Some(position));
        });
        self.show_host_card_context_menu = Some(position);
        self.show_file_panel_context_menu = None;
        self.show_git_panel_context_menu = None;
        self.show_terminal_context_menu = None;
        self.show_tab_right_click_menu = None;
        self.show_process_list_context_menu = None;
        self.new_session_menu_open = false;
        self.settings_menu_open = false;
        ctx.focus(&self.host_card_context_menu);
        ctx.notify();
    }

    fn host_card_context_menu_items(
        &self,
        host_id: &str,
    ) -> Vec<nexshell::menu::MenuItem<TerminalGridAction>> {
        use nexshell::menu::{MenuItem, MenuItemFields};
        let endpoint = self
            .host_state
            .host_by_id(host_id)
            .map(|h| h.endpoint.clone())
            .unwrap_or_default();
        let can_paste = self.host_state.host_clipboard.is_some();
        let can_restore = !self.host_state.deleted_host_backup.is_empty();
        vec![
            MenuItemFields::new(rust_i18n::t!("host_ctx_connect"))
                .with_on_select_action(TerminalGridAction::HostQuickConnect(host_id.to_string()))
                .into_item(),
            MenuItem::Separator,
            MenuItemFields::new(rust_i18n::t!("host_ctx_edit"))
                .with_on_select_action(TerminalGridAction::HostEditOne(host_id.to_string()))
                .into_item(),
            MenuItemFields::new(rust_i18n::t!("host_ctx_rename"))
                .with_on_select_action(TerminalGridAction::HostRenameInline(host_id.to_string()))
                .into_item(),
            MenuItem::Separator,
            MenuItemFields::new(rust_i18n::t!("host_ctx_copy"))
                .with_on_select_action(TerminalGridAction::HostClipboardCopy(host_id.to_string()))
                .into_item(),
            MenuItemFields::new(rust_i18n::t!("host_ctx_cut"))
                .with_on_select_action(TerminalGridAction::HostClipboardCut(host_id.to_string()))
                .into_item(),
            MenuItemFields::new(rust_i18n::t!("host_ctx_paste"))
                .with_disabled(!can_paste)
                .with_on_select_action(TerminalGridAction::HostClipboardPaste)
                .into_item(),
            MenuItem::Separator,
            MenuItemFields::new(rust_i18n::t!("host_ctx_delete"))
                .with_on_select_action(TerminalGridAction::HostDeleteOne(host_id.to_string()))
                .into_item(),
            MenuItemFields::new(rust_i18n::t!("host_ctx_restore"))
                .with_disabled(!can_restore)
                .with_on_select_action(TerminalGridAction::HostRestoreDeleted)
                .into_item(),
            MenuItem::Separator,
            MenuItemFields::new(rust_i18n::t!("host_ctx_copy_address"))
                .with_on_select_action(TerminalGridAction::CopyHostAddress(endpoint))
                .into_item(),
            MenuItem::Separator,
            MenuItemFields::new(rust_i18n::t!("host_ctx_new_host"))
                .with_on_select_action(TerminalGridAction::HostNewHost)
                .into_item(),
            MenuItemFields::new(rust_i18n::t!("host_ctx_sort"))
                .with_on_select_action(TerminalGridAction::HostEnterReorderMode)
                .into_item(),
        ]
    }

    pub(in crate::root_view) fn render_container_card_context_menu(&self) -> Box<dyn Element> {
        warpui::elements::ChildView::new(&self.container_card_context_menu).finish()
    }

    /// 容器卡片右键 / "⋯" 菜单：内容按容器当前状态出（Running 停止+重启，非 Running 启动），日志常显。
    pub(crate) fn show_container_card_context_menu(
        &mut self,
        host_id: String,
        container_id: String,
        container_name: String,
        state: nexshell::container_overview::ContainerState,
        position: Vector2F,
        ctx: &mut ViewContext<Self>,
    ) {
        let items =
            self.container_card_context_menu_items(host_id, container_id, container_name, state);
        self.container_card_context_menu
            .update(ctx, |menu, view_ctx| {
                menu.set_items(items, view_ctx);
                menu.set_origin(Some(position));
            });
        self.show_container_card_context_menu = Some(position);
        self.show_file_panel_context_menu = None;
        self.show_git_panel_context_menu = None;
        self.show_terminal_context_menu = None;
        self.show_tab_right_click_menu = None;
        self.show_process_list_context_menu = None;
        self.show_host_card_context_menu = None;
        self.new_session_menu_open = false;
        self.settings_menu_open = false;
        ctx.focus(&self.container_card_context_menu);
        ctx.notify();
    }

    fn container_card_context_menu_items(
        &self,
        host_id: String,
        container_id: String,
        container_name: String,
        state: nexshell::container_overview::ContainerState,
    ) -> Vec<nexshell::menu::MenuItem<TerminalGridAction>> {
        use nexshell::container_overview::{ContainerAction, ContainerState};
        use nexshell::menu::{MenuItem, MenuItemFields};

        let mut items = Vec::new();
        if state == ContainerState::Running {
            items.push(
                MenuItemFields::new(rust_i18n::t!("host_container_menu_stop"))
                    .with_on_select_action(TerminalGridAction::ContainerExec {
                        host_id: host_id.clone(),
                        container_id: container_id.clone(),
                        action: ContainerAction::Stop,
                    })
                    .into_item(),
            );
            items.push(
                MenuItemFields::new(rust_i18n::t!("host_container_menu_restart"))
                    .with_on_select_action(TerminalGridAction::ContainerExec {
                        host_id: host_id.clone(),
                        container_id: container_id.clone(),
                        action: ContainerAction::Restart,
                    })
                    .into_item(),
            );
        } else {
            items.push(
                MenuItemFields::new(rust_i18n::t!("host_container_menu_start"))
                    .with_on_select_action(TerminalGridAction::ContainerExec {
                        host_id: host_id.clone(),
                        container_id: container_id.clone(),
                        action: ContainerAction::Start,
                    })
                    .into_item(),
            );
        }
        items.push(MenuItem::Separator);
        items.push(
            MenuItemFields::new(rust_i18n::t!("host_container_menu_logs"))
                .with_on_select_action(TerminalGridAction::ContainerOpenLogs {
                    host_id,
                    container_id,
                    container_name,
                })
                .into_item(),
        );
        items
    }

    pub(crate) fn show_process_list_context_menu(
        &mut self,
        pid: u32,
        command: String,
        args: String,
        exe_path: String,
        position: Vector2F,
        ctx: &mut ViewContext<Self>,
    ) {
        use nexshell::menu::{MenuItem, MenuItemFields};
        let label = if command.is_empty() {
            format!("pid {pid}")
        } else {
            format!("{} (pid {})", command, pid)
        };
        let mut items: Vec<MenuItem<TerminalGridAction>> = Vec::new();
        items.push(
            MenuItemFields::new(rust_i18n::t!("process_ctx_kill"))
                .with_on_select_action(TerminalGridAction::KillRemoteProcess { pid, label })
                .into_item(),
        );
        items.push(MenuItem::Separator);
        items.push(
            MenuItemFields::new(rust_i18n::t!("process_ctx_copy_pid"))
                .with_on_select_action(TerminalGridAction::CopyHostAddress(pid.to_string()))
                .into_item(),
        );
        items.push(
            MenuItemFields::new(rust_i18n::t!("process_ctx_copy_name"))
                .with_disabled(command.is_empty())
                .with_on_select_action(TerminalGridAction::CopyHostAddress(command))
                .into_item(),
        );
        items.push(
            MenuItemFields::new(rust_i18n::t!("process_ctx_copy_args"))
                .with_disabled(args.is_empty())
                .with_on_select_action(TerminalGridAction::CopyHostAddress(args))
                .into_item(),
        );
        items.push(
            MenuItemFields::new(rust_i18n::t!("process_ctx_copy_path"))
                .with_disabled(exe_path.is_empty())
                .with_on_select_action(TerminalGridAction::CopyHostAddress(exe_path))
                .into_item(),
        );
        self.process_list_context_menu
            .update(ctx, |menu, view_ctx| {
                menu.set_items(items, view_ctx);
                menu.set_origin(Some(position));
            });
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            tab.process_list_selected_pid = Some(pid);
        }
        self.show_process_list_context_menu = Some(position);
        self.show_file_panel_context_menu = None;
        self.show_git_panel_context_menu = None;
        self.show_terminal_context_menu = None;
        self.show_tab_right_click_menu = None;
        self.new_session_menu_open = false;
        self.settings_menu_open = false;
        ctx.focus(&self.process_list_context_menu);
        ctx.notify();
    }

    fn file_panel_context_menu_items(
        &self,
        name: Option<String>,
        is_dir: bool,
    ) -> Vec<nexshell::menu::MenuItem<TerminalGridAction>> {
        let Some(tab) = self.file_panel_tab() else {
            return Vec::new();
        };
        if matches!(tab.kind, TerminalSessionKind::Local) {
            return local_file_panel_context_menu_items(name, &tab.file_panel_state.cwd, is_dir);
        }

        let multi_count = name.as_ref().and_then(|target| {
            let s = &tab.file_panel_state;
            (s.selected_names.len() > 1 && s.selected_names.contains(target))
                .then_some(s.selected_names.len())
        });
        remote_file_panel_context_menu_items(name, &tab.file_panel_state.cwd, is_dir, multi_count)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn git_panel_context_menu_uses_section_appropriate_batch_actions() {
        use nexshell::menu::MenuItem;

        let items = super::git_panel_context_menu_items(
            "tab-1",
            nexshell::git_ops::GitDiffKind::Staged,
            super::GitPanelContextPaths {
                same_kind: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
                stageable: Vec::new(),
            },
            true,
        );
        let MenuItem::Item(fields) = &items[0] else {
            panic!("first git context item should be a menu action");
        };
        assert_eq!(
            fields.on_select_action(),
            Some(&super::TerminalGridAction::GitPanelUnstagePaths {
                tab_id: "tab-1".into(),
                paths: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
            })
        );

        // 跨区混选：暂存吃全部 stageable，gitignore 只吃同区(untracked)。
        let items = super::git_panel_context_menu_items(
            "tab-1",
            nexshell::git_ops::GitDiffKind::Untracked,
            super::GitPanelContextPaths {
                same_kind: vec!["new.txt".to_string()],
                stageable: vec!["src/main.rs".to_string(), "new.txt".to_string()],
            },
            false,
        );
        let MenuItem::Item(fields) = &items[0] else {
            panic!("first git context item should be a menu action");
        };
        assert_eq!(
            fields.on_select_action(),
            Some(&super::TerminalGridAction::GitPanelStagePaths {
                tab_id: "tab-1".into(),
                paths: vec!["src/main.rs".to_string(), "new.txt".to_string()],
            })
        );
        let MenuItem::Item(fields) = &items[1] else {
            panic!("second git context item should be add-to-gitignore");
        };
        assert_eq!(
            fields.on_select_action(),
            Some(&super::TerminalGridAction::GitPanelAddToGitignore {
                tab_id: "tab-1".into(),
                paths: vec!["new.txt".to_string()],
            })
        );
    }

    #[test]
    fn local_file_panel_context_menu_matches_warp_project_explorer_order() {
        rust_i18n::set_locale("en");
        use nexshell::menu::MenuItem;

        let items = super::local_file_panel_context_menu_items(
            Some("/Users/example/.codex".to_string()),
            "/Users/example",
            true,
        );
        let labels = items
            .iter()
            .map(|item| match item {
                MenuItem::Item(fields) => fields.label().to_string(),
                MenuItem::Separator => "---".to_string(),
                _ => "unexpected".to_string(),
            })
            .collect::<Vec<_>>();
        let reveal_label = if cfg!(target_os = "macos") {
            "Reveal in Finder"
        } else if cfg!(target_os = "windows") {
            "Reveal in Explorer"
        } else {
            "Reveal in file manager"
        };

        assert_eq!(
            labels,
            [
                "New file",
                "---",
                "Go to terminal directory",
                "cd to directory",
                "Open in new tab",
                reveal_label,
                "Rename",
                "Delete",
                "---",
                "Copy path",
                "Copy relative path",
            ]
        );
    }

    #[test]
    fn local_file_panel_context_menu_prepends_open_edit_for_files() {
        rust_i18n::set_locale("en");
        use nexshell::menu::MenuItem;

        let items = super::local_file_panel_context_menu_items(
            Some("/Users/example/notes.txt".to_string()),
            "/Users/example",
            false,
        );
        let labels = items
            .iter()
            .map(|item| match item {
                MenuItem::Item(fields) => fields.label().to_string(),
                MenuItem::Separator => "---".to_string(),
                _ => "unexpected".to_string(),
            })
            .collect::<Vec<_>>();
        let reveal_label = if cfg!(target_os = "macos") {
            "Reveal in Finder"
        } else if cfg!(target_os = "windows") {
            "Reveal in Explorer"
        } else {
            "Reveal in file manager"
        };

        assert_eq!(
            labels,
            [
                "Open",
                "Edit",
                "Open in editor",
                "---",
                "New file",
                "---",
                "Go to terminal directory",
                "cd to directory",
                "Open in new tab",
                reveal_label,
                "Rename",
                "Delete",
                "---",
                "Copy path",
                "Copy relative path",
            ]
        );
        // 「打开/编辑」必须携带完整路径
        let MenuItem::Item(open) = &items[0] else {
            panic!("first item should be Open");
        };
        assert_eq!(
            open.on_select_action(),
            Some(&super::TerminalGridAction::FilePanelOpenWithDefault {
                path: "/Users/example/notes.txt".to_string(),
            })
        );
        let MenuItem::Item(edit) = &items[1] else {
            panic!("second item should be Edit");
        };
        assert_eq!(
            edit.on_select_action(),
            Some(&super::TerminalGridAction::FilePanelOpenInEditor {
                path: "/Users/example/notes.txt".to_string(),
            })
        );
    }

    #[test]
    fn remote_file_panel_context_menu_keeps_sftp_actions() {
        rust_i18n::set_locale("en");
        use nexshell::menu::MenuItem;

        let items = super::remote_file_panel_context_menu_items(
            Some("logs".to_string()),
            "/root",
            true,
            None,
        );
        let labels = items
            .iter()
            .map(|item| match item {
                MenuItem::Item(fields) => fields.label().to_string(),
                MenuItem::Separator => "---".to_string(),
                _ => "unexpected".to_string(),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            [
                "Download to…",
                "Rename",
                "Delete",
                "Copy path",
                "---",
                "New folder",
                "New file",
            ]
        );
    }
}
