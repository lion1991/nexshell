// host_monitor_section::actions — host_monitor 的 action handler 与开页/杀进程操作。
// 本文件只含 impl RootView，无自由函数。由 root_view/mod.rs handle_action 单行分发。

use crate::{RootView, TerminalSessionKind};
use nexshell::host_management::HostConnectionPlan;
use nexshell::host_overview::{NetworkSortKey, ProcessSortKey};
use nexshell::terminal_runtime::LocalTerminalRuntime;
use warpui::clipboard::ClipboardContent;
use warpui::ViewContext;

impl RootView {
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

    pub(crate) fn kill_remote_process(&mut self, pid: u32, label: String, ctx: &mut ViewContext<Self>) {
        self.show_process_list_context_menu = None;
        let Some(tab) = self.terminal_tabs.get(self.active_tab_index) else {
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

    pub(in crate::root_view) fn handle_toggle_host_network_dropdown(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            if tab.host_overview.snapshot.networks.len() > 1 {
                tab.host_overview.network_dropdown_open = !tab.host_overview.network_dropdown_open;
            } else {
                tab.host_overview.network_dropdown_open = false;
            }
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_select_host_network(&mut self, interface: String, ctx: &mut ViewContext<Self>) {
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            tab.host_overview.select_network(interface);
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_sort_host_processes(&mut self, key: ProcessSortKey, ctx: &mut ViewContext<Self>) {
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            tab.host_overview.cycle_process_sort(key);
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_sort_host_network(&mut self, key: NetworkSortKey, ctx: &mut ViewContext<Self>) {
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            tab.host_overview.cycle_network_sort(key);
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_copy_host_address(&mut self, text: &str, ctx: &mut ViewContext<Self>) {
        if !text.is_empty() {
            ctx.clipboard()
                .write(ClipboardContent::plain_text(text.to_owned()));
        }
    }

    // 当前活动 tab 的主机 id 与展示名（hostname 缺省回退到 tab 标签）。
    fn active_host_id_and_label(&self) -> Option<(String, String)> {
        let active_tab = self.terminal_tabs.get(self.active_tab_index)?;
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
