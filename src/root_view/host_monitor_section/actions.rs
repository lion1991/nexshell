// host_monitor_section::actions — host_monitor 的 action handler 与开页/杀进程操作。
// 本文件只含 impl RootView，无自由函数。由 root_view/mod.rs handle_action 单行分发。

use crate::file_panel_view_helpers::PANEL_BORDER_W;
use crate::sidebar_helpers::OVERVIEW_PANEL_WIDTH;
use crate::{RootView, TerminalSessionKind};
use nexshell::host_management::HostConnectionPlan;
use nexshell::host_overview::{NetworkSortKey, ProcessSortKey};
use nexshell::terminal_runtime::LocalTerminalRuntime;
use warpui::clipboard::ClipboardContent;
use warpui::ViewContext;

impl RootView {
    /// 右侧概览面板开关。开合动画同 git 面板：面板是最右一块，文件面板已开时被它推着锁步平移。
    pub(in crate::root_view) fn handle_toggle_overview_panel(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        // 按钮在没有概览的标签上已收起，这里只是兜底。
        if self.overview_panel_source().is_none() {
            return;
        }
        let outer_w = OVERVIEW_PANEL_WIDTH + PANEL_BORDER_W;
        let file_follows =
            self.should_render_file_panel() && self.file_panel_closing.borrow().is_none();
        if self.overview_panel_closing.get() {
            // 关门滑出途中再按 = 取消关门滑回（跟随的文件面板同步回位）。
            self.overview_panel_closing.set(false);
            self.overview_panel_open = true;
            self.overview_panel_slide.borrow_mut().set_target(0.0);
            if file_follows {
                self.file_panel_slide.borrow_mut().set_target(0.0);
            }
        } else if self.overview_panel_open {
            // 开始关门：采集立即停，面板滑出收敛后 finalize_panel_closes 落闸。
            self.overview_panel_open = false;
            self.overview_panel_closing.set(true);
            self.overview_panel_slide.borrow_mut().set_target(outer_w);
            if file_follows {
                self.file_panel_slide.borrow_mut().set_target(outer_w);
            }
        } else {
            self.overview_panel_open = true;
            // 首帧按实际宽度起搏滑入；文件面板布局左移同量，补偏移从旧位置滑过去。
            self.overview_panel_slide_pending.set(true);
            if file_follows {
                let mut slide = self.file_panel_slide.borrow_mut();
                let current = slide.sample(std::time::Instant::now());
                slide.snap(current + outer_w);
                slide.set_target(0.0);
            }
        }
        self.sync_host_overview_monitor(ctx);
        self.save_ui_settings();
        ctx.notify();
    }

    fn open_process_list_tab(
        &mut self,
        host_id: String,
        host_label: String,
        ctx: &mut ViewContext<Self>,
    ) {
        // 同主机已有进程 tab 则直接切过去
        if let Some(idx) = self.terminal_tabs.iter().position(|tab| {
            matches!(tab.kind, TerminalSessionKind::ProcessList)
                && tab.host_id.as_deref() == Some(host_id.as_str())
        }) {
            self.activate_terminal_tab(idx, ctx);
            return;
        }
        let session_id = format!("process-list-{}", host_id);
        let terminal = LocalTerminalRuntime::failed(&session_id, "process list view");
        let label = format!(
            "{} · {}",
            host_label,
            TerminalSessionKind::ProcessList.default_label()
        );
        self.push_terminal_tab(
            terminal,
            &session_id,
            label,
            TerminalSessionKind::ProcessList,
            Some(host_id),
            None,
            ctx,
        );
    }

    fn open_network_list_tab(
        &mut self,
        host_id: String,
        host_label: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(idx) = self.terminal_tabs.iter().position(|tab| {
            matches!(tab.kind, TerminalSessionKind::NetworkList)
                && tab.host_id.as_deref() == Some(host_id.as_str())
        }) {
            self.activate_terminal_tab(idx, ctx);
            return;
        }
        let session_id = format!("network-list-{}", host_id);
        let terminal = LocalTerminalRuntime::failed(&session_id, "network list view");
        let label = format!(
            "{} · {}",
            host_label,
            TerminalSessionKind::NetworkList.default_label()
        );
        self.push_terminal_tab(
            terminal,
            &session_id,
            label,
            TerminalSessionKind::NetworkList,
            Some(host_id),
            None,
            ctx,
        );
    }

    fn open_system_info_tab(
        &mut self,
        host_id: String,
        host_label: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(idx) = self.terminal_tabs.iter().position(|tab| {
            matches!(tab.kind, TerminalSessionKind::SystemInfo)
                && tab.host_id.as_deref() == Some(host_id.as_str())
        }) {
            self.activate_terminal_tab(idx, ctx);
            return;
        }
        let session_id = format!("system-info-{}", host_id);
        let terminal = LocalTerminalRuntime::failed(&session_id, "system info view");
        let label = format!(
            "{} · {}",
            host_label,
            TerminalSessionKind::SystemInfo.default_label()
        );
        self.push_terminal_tab(
            terminal,
            &session_id,
            label,
            TerminalSessionKind::SystemInfo,
            Some(host_id),
            None,
            ctx,
        );
    }

    pub(crate) fn kill_remote_process(
        &mut self,
        pid: u32,
        label: String,
        ctx: &mut ViewContext<Self>,
    ) {
        self.show_process_list_context_menu = None;
        let Some(tab) = self.terminal_tabs.get(self.overview_tab_index()) else {
            return;
        };
        let Some(host_id) = tab.host_id.as_deref() else {
            return;
        };
        let Some(plan) = self.host_state.connection_plan_for(host_id) else {
            return;
        };
        let HostConnectionPlan::SavedSsh { config, .. } = plan else {
            return;
        };
        let cfg = Self::remote_ssh_config_from_host_config(&config);
        let cmd = format!("kill {pid}");
        let rx = nexshell::host_overview::spawn_remote_exec(cfg, cmd);
        // 进程列表 3s 自刷新；成功则该行消失。失败把 SSH 层带回的 stderr 打到日志，便于排查权限/已退等场景
        ctx.spawn_stream_local(
            rx,
            move |_, result, _| {
                if let Err(err) = result {
                    eprintln!("[nexshell] kill {label} failed: {err}");
                }
            },
            |_, _| {},
        );
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_toggle_host_network_dropdown(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        let index = self.overview_tab_index();
        if let Some(tab) = self.terminal_tabs.get_mut(index) {
            if tab.host_overview.snapshot.networks.len() > 1 {
                tab.host_overview.network_dropdown_open = !tab.host_overview.network_dropdown_open;
            } else {
                tab.host_overview.network_dropdown_open = false;
            }
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_select_host_network(
        &mut self,
        interface: String,
        ctx: &mut ViewContext<Self>,
    ) {
        let index = self.overview_tab_index();
        if let Some(tab) = self.terminal_tabs.get_mut(index) {
            tab.host_overview.select_network(interface);
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_sort_host_processes(
        &mut self,
        key: ProcessSortKey,
        ctx: &mut ViewContext<Self>,
    ) {
        let index = self.overview_tab_index();
        if let Some(tab) = self.terminal_tabs.get_mut(index) {
            tab.host_overview.cycle_process_sort(key);
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_sort_host_network(
        &mut self,
        key: NetworkSortKey,
        ctx: &mut ViewContext<Self>,
    ) {
        let index = self.overview_tab_index();
        if let Some(tab) = self.terminal_tabs.get_mut(index) {
            tab.host_overview.cycle_network_sort(key);
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_copy_host_address(
        &mut self,
        text: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        if !text.is_empty() {
            ctx.clipboard()
                .write(ClipboardContent::plain_text(text.to_owned()));
        }
    }

    // 概览所在 tab 的主机 id 与展示名（hostname 缺省回退到 tab 标签）。
    fn active_host_id_and_label(&self) -> Option<(String, String)> {
        let active_tab = self.terminal_tabs.get(self.overview_tab_index())?;
        let host_id = active_tab.host_id.clone()?;
        let host_label = active_tab
            .host_overview
            .snapshot
            .hostname
            .clone()
            .unwrap_or_else(|| active_tab.label());
        Some((host_id, host_label))
    }

    pub(in crate::root_view) fn handle_open_process_list(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some((host_id, host_label)) = self.active_host_id_and_label() {
            self.open_process_list_tab(host_id, host_label, ctx);
        }
    }

    pub(in crate::root_view) fn handle_open_network_list(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some((host_id, host_label)) = self.active_host_id_and_label() {
            self.open_network_list_tab(host_id, host_label, ctx);
        }
    }

    pub(in crate::root_view) fn handle_open_system_info(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some((host_id, host_label)) = self.active_host_id_and_label() {
            self.open_system_info_tab(host_id, host_label, ctx);
        }
    }
}
