rust_i18n::i18n!("locales", fallback = "en");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityItem {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tab {
    pub id: &'static str,
    pub title: &'static str,
    pub kind: TabKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TabKind {
    Task,
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencePolicy {
    pub primary: &'static str,
    pub adopt_warp_blocks: bool,
}

pub mod actions;
#[cfg(feature = "warpui-app")]
pub mod code_editor;
pub mod container_fleet;
pub mod container_overview;
#[cfg(feature = "warpui-app")]
pub mod design_tokens;
#[cfg(feature = "warpui-app")]
pub mod features;
pub mod file_drop_target;
pub mod file_panel;
pub mod frame_export;
pub mod generation;
pub mod git_ops;
pub mod git_panel;
pub mod glass_backdrop;
pub mod history_suggester;
pub mod host_management;
pub mod host_overview;
pub mod host_overview_fleet;
pub mod ipc_dispatcher;
pub mod layout;
#[cfg(feature = "warpui-app")]
pub mod menu;
pub mod native_adapter;
pub mod native_shell_adapter;
pub mod native_shell_host;
pub mod pane_state;
#[cfg(feature = "warpui-app")]
pub mod pane_tree;
pub mod platform;
pub mod pty_event_loop;
pub mod rdp_session;
pub mod remote_edit_io;
pub mod renderer_ipc;
pub mod runtime_settings;
#[cfg(feature = "warpui-app")]
pub mod safe_triangle;
pub mod sftp_ops;
pub mod shell_chrome;
pub mod shell_integration;
pub mod ssh_key_store;
pub mod ssh_session;
pub mod stat_widgets;
#[cfg(feature = "warpui-app")]
pub mod telemetry;
pub mod terminal_lifecycle;
pub mod terminal_mount;
pub mod terminal_recorder;
pub mod terminal_runtime;
#[cfg(feature = "warpui-app")]
pub mod text_editor;
#[cfg(feature = "warpui-app")]
pub mod themes;
#[cfg(feature = "warpui-app")]
pub mod time_format;
#[cfg(feature = "warpui-app")]
pub mod ui_anim;
#[cfg(feature = "warpui-app")]
pub mod ui_components;
#[cfg(feature = "warpui-app")]
pub mod util;
#[cfg(feature = "warpui-app")]
pub mod view_components;
pub mod view_model;
pub mod warp_horizontal_tabs;
pub mod warp_source_plan;
#[cfg(feature = "warpui-app")]
pub mod warp_tab_context_menu;
pub mod warp_ui_plan;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellModel {
    window_title: &'static str,
    activity_items: Vec<ActivityItem>,
    tabs: Vec<Tab>,
    active_activity_index: usize,
    active_tab_index: usize,
    active_terminal_tab_index: usize,
    terminal_pane_count: usize,
    focused_terminal_pane_index: usize,
    bottom_tools: Vec<&'static str>,
    monitor_panel_in_first_spike: bool,
    reference_policy: ReferencePolicy,
}

impl ShellModel {
    pub fn nexshell_default() -> Self {
        Self {
            window_title: "NexShell",
            activity_items: vec![
                ActivityItem {
                    id: "hosts",
                    label: "Hosts",
                },
                ActivityItem {
                    id: "sessions",
                    label: "Sessions",
                },
                ActivityItem {
                    id: "terminal",
                    label: "Terminal",
                },
                ActivityItem {
                    id: "snippets",
                    label: "Snippets",
                },
                ActivityItem {
                    id: "files",
                    label: "Files",
                },
                ActivityItem {
                    id: "account",
                    label: "Account",
                },
            ],
            tabs: vec![
                Tab {
                    id: "fix-cursor",
                    title: "* Fix terminal cursor a...",
                    kind: TabKind::Task,
                },
                Tab {
                    id: "sshtool",
                    title: ".: sshtool",
                    kind: TabKind::Terminal,
                },
            ],
            active_activity_index: 2,
            active_tab_index: 1,
            active_terminal_tab_index: 1,
            terminal_pane_count: 1,
            focused_terminal_pane_index: 0,
            bottom_tools: vec![
                "批量执行",
                "隧道",
                "高亮",
                "重新连接",
                "历史",
                "录制",
                "字体",
            ],
            monitor_panel_in_first_spike: false,
            reference_policy: ReferencePolicy {
                primary: "warp-source-first-for-terminal-engineering",
                adopt_warp_blocks: false,
            },
        }
    }

    pub fn window_title(&self) -> &'static str {
        self.window_title
    }

    pub fn activity_items(&self) -> &[ActivityItem] {
        &self.activity_items
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active_activity(&self) -> &ActivityItem {
        &self.activity_items[self.active_activity_index]
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab_index]
    }

    pub fn active_terminal_tab(&self) -> &Tab {
        &self.tabs[self.active_terminal_tab_index]
    }

    pub fn activate_tab(&mut self, id: &str) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
            self.active_tab_index = index;
            if self.tabs[index].kind == TabKind::Terminal {
                self.active_terminal_tab_index = index;
            }
        }
    }

    pub fn terminal_pane_count(&self) -> usize {
        self.terminal_pane_count
    }

    pub fn focused_terminal_pane_index(&self) -> usize {
        self.focused_terminal_pane_index
    }

    pub fn bottom_tools(&self) -> &[&'static str] {
        &self.bottom_tools
    }

    pub fn monitor_panel_in_first_spike(&self) -> bool {
        self.monitor_panel_in_first_spike
    }

    pub fn reference_policy(&self) -> &ReferencePolicy {
        &self.reference_policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::Path, sync::Arc};

    #[test]
    fn windows_icon_resource_uses_taskbar_sized_cropped_assets() {
        let build_script = include_str!("../build.rs");

        for size in [16, 24, 32, 48, 64, 128, 256] {
            let path = format!("assets/AppIcon.windows/icon_{size}x{size}.png");
            assert!(Path::new(&path).exists(), "missing {path}");
            assert_eq!(
                png_dimensions(&path),
                (size, size),
                "{path} should match its ICO directory size"
            );
            assert!(
                build_script.contains(&path),
                "build.rs does not embed {path}"
            );
        }

        assert!(
            !build_script.contains("assets/AppIcon.iconset/icon_128x128@2x.png"),
            "Windows exe icons should use the cropped Windows icon assets"
        );
    }

    fn png_dimensions(path: &str) -> (u32, u32) {
        let bytes = std::fs::read(path).unwrap_or_else(|err| panic!("read {path}: {err}"));
        assert!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "{path} is not a PNG"
        );
        assert_eq!(&bytes[12..16], b"IHDR", "{path} does not start with IHDR");

        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        (width, height)
    }

    #[test]
    fn host_overview_probe_parses_linux_metrics_and_rates() {
        let previous = host_overview::parse_probe_output(
            r#"
NEXSHELL_HOST_OVERVIEW_V1
[identity]
VM-B69Q0H1E3EQ5
root
Linux 6.8.0 x86_64 GNU/Linux
[uptime]
21945600.00 0.00
[load]
0.02 0.10 0.10 1/123 456
[mem]
MemTotal:        7864320 kB
MemAvailable:   6763316 kB
SwapTotal:       2097152 kB
SwapFree:        2096884 kB
[stat]
cpu  1000 0 1000 98000 0 0 0 0 0 0
[net]
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 1000 0 0 0 0 0 0 0 1000 0 0 0 0 0 0 0
 ens18: 100000 0 0 0 0 0 0 0 200000 0 0 0 0 0 0 0
[ps]
123 root 7270 0.7 sshd-session sshd-session: root@pts/0
124 root 6246 0.7 top top -b -n 1
[exe]
123	/usr/sbin/sshd
124	/usr/bin/top
"#,
        )
        .expect("previous probe should parse");
        let current = host_overview::parse_probe_output(
            r#"
NEXSHELL_HOST_OVERVIEW_V1
[identity]
VM-B69Q0H1E3EQ5
root
Linux 6.8.0 x86_64 GNU/Linux
[uptime]
21945630.00 0.00
[load]
0.02 0.10 0.10 1/124 457
[mem]
MemTotal:        7864320 kB
MemAvailable:   6763316 kB
SwapTotal:       2097152 kB
SwapFree:        2096884 kB
[stat]
cpu  1010 0 1010 98980 0 0 0 0 0 0
[net]
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 1000 0 0 0 0 0 0 0 1000 0 0 0 0 0 0 0
 ens18: 109000 0 0 0 0 0 0 0 266000 0 0 0 0 0 0 0
[ps]
123 root 7270 0.7 sshd-session sshd-session: root@pts/0
124 root 6246 0.7 top top -b -n 1
2 root 0 0.3 rcu_preempt [rcu_preempt]
[exe]
123	/usr/sbin/sshd
124	/usr/bin/top
"#,
        )
        .expect("current probe should parse");

        let snapshot = host_overview::snapshot_from_probe(
            current,
            Some((&previous, std::time::Duration::from_secs(3))),
            Some(std::time::Duration::from_millis(18)),
        );

        assert_eq!(snapshot.hostname.as_deref(), Some("VM-B69Q0H1E3EQ5"));
        assert_eq!(snapshot.username.as_deref(), Some("root"));
        assert_eq!(snapshot.uptime_seconds, Some(21_945_630));
        assert_eq!(snapshot.load_average, Some([0.02, 0.10, 0.10]));
        assert_eq!(
            snapshot.cpu_percent.map(|value| value.round() as u8),
            Some(2)
        );
        assert_eq!(
            snapshot.memory.as_ref().map(|m| m.percent.round() as u8),
            Some(14)
        );
        assert_eq!(
            snapshot.swap.as_ref().map(|m| m.percent.round() as u8),
            Some(0)
        );
        assert_eq!(
            snapshot.network.as_ref().map(|network| (
                network.interface.as_str(),
                network.rx_bytes_per_sec,
                network.tx_bytes_per_sec
            )),
            Some(("ens18", 3_000, 22_000))
        );
        assert_eq!(snapshot.latency_ms, Some(18));
        assert_eq!(snapshot.processes.len(), 3);
        assert_eq!(snapshot.processes[0].command, "sshd-session");
    }

    #[test]
    fn host_overview_collect_command_is_procfs_based_and_non_interactive() {
        let command = host_overview::HOST_OVERVIEW_COLLECT_COMMAND;

        assert!(command.contains("NEXSHELL_HOST_OVERVIEW_V1"));
        assert!(command.contains("/proc/loadavg"));
        assert!(command.contains("/proc/meminfo"));
        assert!(command.contains("/proc/stat"));
        assert!(command.contains("/proc/net/dev"));
        assert!(command.contains("ps -eo"));
        assert!(command.contains("[sock_tcp]"));
        assert!(command.contains("[sock_udp]"));
        assert!(command.contains("ss -Hntan"));
        assert!(!command.contains("top"));
    }

    #[test]
    fn host_overview_parses_listen_and_outbound_sockets() {
        let probe = host_overview::parse_probe_output(
            r#"
NEXSHELL_HOST_OVERVIEW_V1
[identity]
sock-host
root
Linux 6.8.0 x86_64 GNU/Linux
[uptime]
10.00 0.00
[load]
0.01 0.02 0.03 1/10 20
[mem]
MemTotal:        1024 kB
MemAvailable:   512 kB
SwapTotal:       0 kB
SwapFree:        0 kB
[stat]
cpu  1000 0 1000 98000 0 0 0 0 0 0
[net]
[ps]
[exe]
[sock_tcp]
LISTEN 0      511    0.0.0.0:8080       0.0.0.0:*       users:(("nginx",pid=2036,fd=8))
ESTAB  0      0      10.0.0.5:8080      10.0.0.7:55142  users:(("nginx",pid=2036,fd=9))
	 cubic bytes_sent:5000 bytes_acked:5000 bytes_received:1234
ESTAB  0      0      10.0.0.5:8080      10.0.0.8:55143  users:(("nginx",pid=2036,fd=10))
	 cubic bytes_sent:200 bytes_acked:200 bytes_received:100
ESTAB  0      0      10.0.0.5:55200     93.184.216.34:443 users:(("curl",pid=4001,fd=3))
	 cubic bytes_sent:80 bytes_acked:80 bytes_received:9000
[sock_udp]
UNCONN 0      0      0.0.0.0:53         0.0.0.0:*       users:(("named",pid=900,fd=22))
[disk]
"#,
        )
        .expect("probe should parse");

        let listen_8080 = probe
            .sockets
            .iter()
            .find(|s| {
                s.local_port == 8080 && matches!(s.kind, host_overview::NetworkRowKind::Listen)
            })
            .expect("listen 8080 row");
        assert_eq!(listen_8080.pid, Some(2036));
        assert_eq!(listen_8080.process, "nginx");
        assert_eq!(listen_8080.proto, host_overview::SocketProto::Tcp);
        assert_eq!(listen_8080.connections, 2);
        assert_eq!(listen_8080.unique_ips, 2);
        assert_eq!(listen_8080.rx_bytes, 1334);
        assert_eq!(listen_8080.tx_bytes, 5200);

        let listen_dns = probe
            .sockets
            .iter()
            .find(|s| s.local_port == 53)
            .expect("udp 53 row");
        assert_eq!(listen_dns.proto, host_overview::SocketProto::Udp);
        assert!(matches!(
            listen_dns.kind,
            host_overview::NetworkRowKind::Listen
        ));
        assert_eq!(listen_dns.process, "named");

        let outbound = probe
            .sockets
            .iter()
            .find(|s| matches!(s.kind, host_overview::NetworkRowKind::Outbound))
            .expect("outbound row");
        assert_eq!(outbound.pid, Some(4001));
        assert_eq!(outbound.process, "curl");
        assert_eq!(outbound.remote_addr.as_deref(), Some("93.184.216.34"));
        assert_eq!(outbound.remote_port, Some(443));
        assert_eq!(outbound.rx_bytes, 9000);
        assert_eq!(outbound.tx_bytes, 80);
    }

    #[test]
    fn host_overview_aggregates_estab_to_specific_ip_listen_first() {
        // 同端口同时绑了 0.0.0.0 和 127.0.0.1 两个监听
        // 期望：local_addr=127.0.0.1 的 ESTAB 只归 127.0.0.1:5432，不归 0.0.0.0:5432
        let probe = host_overview::parse_probe_output(
            r#"
NEXSHELL_HOST_OVERVIEW_V1
[identity]
multi-bind
root
Linux 6.8.0 x86_64 GNU/Linux
[uptime]
10.00 0.00
[load]
0.01 0.02 0.03 1/10 20
[mem]
MemTotal:        1024 kB
MemAvailable:   512 kB
SwapTotal:       0 kB
SwapFree:        0 kB
[stat]
cpu  1000 0 1000 98000 0 0 0 0 0 0
[net]
[ps]
[exe]
[sock_tcp]
LISTEN 0 511 0.0.0.0:5432 0.0.0.0:* users:(("postgres",pid=100,fd=8))
LISTEN 0 511 127.0.0.1:5432 0.0.0.0:* users:(("postgres",pid=100,fd=9))
ESTAB  0 0 127.0.0.1:5432 127.0.0.1:55142 users:(("postgres",pid=100,fd=20))
	 cubic bytes_sent:100 bytes_acked:100 bytes_received:50
ESTAB  0 0 10.0.0.5:5432 10.0.0.7:55200 users:(("postgres",pid=100,fd=21))
	 cubic bytes_sent:300 bytes_acked:300 bytes_received:200
[sock_udp]
[disk]
"#,
        )
        .expect("probe should parse");

        let listen_loopback = probe
            .sockets
            .iter()
            .find(|s| s.local_addr == "127.0.0.1" && s.local_port == 5432)
            .expect("loopback listen row");
        assert_eq!(
            listen_loopback.connections, 1,
            "loopback 监听只收 127.0.0.1 来的连接"
        );
        assert_eq!(listen_loopback.rx_bytes, 50);
        assert_eq!(listen_loopback.tx_bytes, 100);

        let listen_wildcard = probe
            .sockets
            .iter()
            .find(|s| s.local_addr == "0.0.0.0" && s.local_port == 5432)
            .expect("wildcard listen row");
        assert_eq!(
            listen_wildcard.connections, 1,
            "通配监听只收余下的 10.0.0.5 连接"
        );
        assert_eq!(listen_wildcard.rx_bytes, 200);
        assert_eq!(listen_wildcard.tx_bytes, 300);
    }

    #[test]
    fn host_overview_collecting_refresh_preserves_last_metrics() {
        let mut current = host_overview::HostOverviewSnapshot::waiting("root@example.test:22");
        current.hostname = Some("example.test".to_string());
        current.load_average = Some([0.83, 0.67, 0.65]);
        current.cpu_percent = Some(16.0);
        current.memory = Some(host_overview::UsageMetric {
            used_bytes: 1_610_612_736,
            total_bytes: 4_080_218_112,
            percent: 39.5,
        });
        current.processes = vec![host_overview::ProcessMetric {
            pid: 1234,
            user: "root".to_string(),
            rss_bytes: 7_270_400,
            cpu_percent: 0.7,
            command: "sshd-session".to_string(),
            args: "sshd-session".to_string(),
            exe_path: None,
        }];
        current.latency_ms = Some(1403);
        current.status = host_overview::HostOverviewStatus::Ready;

        let mut collecting = host_overview::HostOverviewSnapshot::waiting("root@example.test:22");
        collecting.status = host_overview::HostOverviewStatus::Collecting;

        let merged = host_overview::merge_overview_snapshot(&current, collecting);

        assert_eq!(merged.status, host_overview::HostOverviewStatus::Collecting);
        assert_eq!(merged.hostname.as_deref(), Some("example.test"));
        assert_eq!(merged.load_average, Some([0.83, 0.67, 0.65]));
        assert_eq!(merged.cpu_percent, Some(16.0));
        assert_eq!(
            merged.memory.as_ref().map(|metric| metric.percent),
            Some(39.5)
        );
        assert_eq!(merged.processes[0].command, "sshd-session");
        assert_eq!(merged.latency_ms, Some(1403));
    }

    #[test]
    fn host_overview_snapshot_includes_all_real_network_interfaces() {
        let previous = host_overview::parse_probe_output(
            r#"
NEXSHELL_HOST_OVERVIEW_V1
[identity]
multi-net
root
Linux 6.8.0 x86_64 GNU/Linux
[uptime]
10.00 0.00
[load]
0.01 0.02 0.03 1/10 20
[mem]
MemTotal:        1024 kB
MemAvailable:   512 kB
SwapTotal:       0 kB
SwapFree:        0 kB
[stat]
cpu  1000 0 1000 98000 0 0 0 0 0 0
[net]
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 1000 0 0 0 0 0 0 0 1000 0 0 0 0 0 0 0
 ens18: 100000 0 0 0 0 0 0 0 200000 0 0 0 0 0 0 0
  eth1: 300000 0 0 0 0 0 0 0 400000 0 0 0 0 0 0 0
[ps]
"#,
        )
        .expect("previous probe should parse");
        let current = host_overview::parse_probe_output(
            r#"
NEXSHELL_HOST_OVERVIEW_V1
[identity]
multi-net
root
Linux 6.8.0 x86_64 GNU/Linux
[uptime]
13.00 0.00
[load]
0.01 0.02 0.03 1/10 21
[mem]
MemTotal:        1024 kB
MemAvailable:   512 kB
SwapTotal:       0 kB
SwapFree:        0 kB
[stat]
cpu  1010 0 1010 98980 0 0 0 0 0 0
[net]
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 1100 0 0 0 0 0 0 0 1100 0 0 0 0 0 0 0
 ens18: 103000 0 0 0 0 0 0 0 209000 0 0 0 0 0 0 0
  eth1: 315000 0 0 0 0 0 0 0 418000 0 0 0 0 0 0 0
[ps]
"#,
        )
        .expect("current probe should parse");

        let snapshot = host_overview::snapshot_from_probe(
            current,
            Some((&previous, std::time::Duration::from_secs(3))),
            Some(std::time::Duration::from_millis(9)),
        );

        let networks = snapshot
            .networks
            .iter()
            .map(|network| {
                (
                    network.interface.as_str(),
                    network.rx_bytes_per_sec,
                    network.tx_bytes_per_sec,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            networks,
            vec![("ens18", 1_000, 3_000), ("eth1", 5_000, 6_000)]
        );
        assert_eq!(
            snapshot
                .network
                .as_ref()
                .map(|network| network.interface.as_str()),
            Some("eth1")
        );
    }

    #[test]
    fn host_overview_ui_state_keeps_tab_snapshots_independent() {
        let mut tab_a = host_overview::HostOverviewUiState::waiting("root@a.example:22");
        let mut tab_b = host_overview::HostOverviewUiState::waiting("root@b.example:22");
        let mut snapshot_a = host_overview::HostOverviewSnapshot::waiting("root@a.example:22");
        snapshot_a.hostname = Some("a.example".to_string());
        snapshot_a.cpu_percent = Some(12.0);
        snapshot_a.status = host_overview::HostOverviewStatus::Ready;
        let mut snapshot_b = host_overview::HostOverviewSnapshot::waiting("root@b.example:22");
        snapshot_b.hostname = Some("b.example".to_string());
        snapshot_b.cpu_percent = Some(44.0);
        snapshot_b.status = host_overview::HostOverviewStatus::Ready;

        tab_a.apply_event(host_overview::HostOverviewEvent::Snapshot(snapshot_a));
        tab_b.apply_event(host_overview::HostOverviewEvent::Snapshot(snapshot_b));

        let mut collecting_b = host_overview::HostOverviewSnapshot::waiting("root@b.example:22");
        collecting_b.status = host_overview::HostOverviewStatus::Collecting;
        tab_b.apply_event(host_overview::HostOverviewEvent::Snapshot(collecting_b));

        assert_eq!(tab_a.snapshot.hostname.as_deref(), Some("a.example"));
        assert_eq!(tab_a.snapshot.cpu_percent, Some(12.0));
        assert_eq!(
            tab_a.snapshot.status,
            host_overview::HostOverviewStatus::Ready
        );
        assert_eq!(tab_b.snapshot.hostname.as_deref(), Some("b.example"));
        assert_eq!(tab_b.snapshot.cpu_percent, Some(44.0));
        assert_eq!(
            tab_b.snapshot.status,
            host_overview::HostOverviewStatus::Collecting
        );
    }

    #[test]
    fn host_overview_empty_status_only_shows_without_collected_data() {
        let mut empty = host_overview::HostOverviewSnapshot::waiting("root@example.test:22");
        empty.status = host_overview::HostOverviewStatus::Error("probe failed".to_string());
        assert!(host_overview::should_show_empty_overview_status(&empty));

        let mut collected = empty.clone();
        collected.latency_ms = Some(41);
        assert!(!host_overview::should_show_empty_overview_status(
            &collected
        ));
    }

    #[test]
    fn host_overview_sidebar_only_shows_for_supported_ssh_tabs() {
        assert!(host_overview::should_show_host_overview_sidebar(true, true));
        assert!(!host_overview::should_show_host_overview_sidebar(
            false, true
        ));
        assert!(!host_overview::should_show_host_overview_sidebar(
            true, false
        ));
    }

    #[test]
    fn host_overview_monitor_only_runs_while_terminal_is_connected() {
        assert!(host_overview::should_run_host_overview_monitor(
            true, true, true
        ));
        assert!(!host_overview::should_run_host_overview_monitor(
            true, true, false
        ));
        assert!(!host_overview::should_run_host_overview_monitor(
            false, true, true
        ));
        assert!(!host_overview::should_run_host_overview_monitor(
            true, false, true
        ));
    }

    #[test]
    fn default_shell_scope_matches_first_native_spike() {
        let shell = ShellModel::nexshell_default();

        assert_eq!(shell.window_title(), "NexShell");
        assert_eq!(shell.activity_items().len(), 6);
        assert_eq!(shell.tabs().len(), 2);
        assert_eq!(shell.active_activity().id, "terminal");
        assert_eq!(shell.active_tab().title, ".: sshtool");
        assert_eq!(shell.active_terminal_tab().id, "sshtool");
        assert_eq!(shell.terminal_pane_count(), 1);
        assert_eq!(shell.focused_terminal_pane_index(), 0);
        assert_eq!(
            shell.bottom_tools(),
            [
                "批量执行",
                "隧道",
                "高亮",
                "重新连接",
                "历史",
                "录制",
                "字体"
            ]
        );
        assert!(!shell.monitor_panel_in_first_spike());
        assert_eq!(
            shell.reference_policy().primary,
            "warp-source-first-for-terminal-engineering"
        );
        assert!(!shell.reference_policy().adopt_warp_blocks);
    }

    #[test]
    fn shell_chrome_uses_compact_activity_glyphs_for_the_native_rail() {
        assert_eq!(shell_chrome::activity_glyph("hosts"), "[]");
        assert_eq!(shell_chrome::activity_glyph("sessions"), ">");
        assert_eq!(shell_chrome::activity_glyph("terminal"), ">");
        assert_eq!(shell_chrome::activity_glyph("snippets"), "{}");
        assert_eq!(shell_chrome::activity_glyph("files"), "~/");
        assert_eq!(shell_chrome::activity_glyph("account"), "@");
        assert_eq!(shell_chrome::activity_glyph("unknown"), "?");
    }

    #[test]
    fn warp_horizontal_tabs_default_to_insert_after_current_tab() {
        use warp_horizontal_tabs::{new_tab_insert_index, NewTabPlacement};

        assert_eq!(
            new_tab_insert_index(4, 1, NewTabPlacement::AfterCurrentTab),
            2
        );
        assert_eq!(
            new_tab_insert_index(0, 99, NewTabPlacement::AfterCurrentTab),
            0
        );
        assert_eq!(
            new_tab_insert_index(4, 99, NewTabPlacement::AfterCurrentTab),
            4
        );
    }

    #[test]
    fn warp_horizontal_tabs_can_append_after_all_tabs() {
        use warp_horizontal_tabs::{new_tab_insert_index, NewTabPlacement};

        assert_eq!(new_tab_insert_index(4, 1, NewTabPlacement::AfterAllTabs), 4);
    }

    #[test]
    fn warp_horizontal_tabs_use_warp_compact_width_threshold() {
        use warp_horizontal_tabs::TabWidthMode;

        assert_eq!(TabWidthMode::for_width(41.99), TabWidthMode::Compact);
        assert_eq!(TabWidthMode::for_width(42.0), TabWidthMode::Full);
        assert_eq!(TabWidthMode::for_width(200.0), TabWidthMode::Full);
    }

    #[test]
    fn warp_horizontal_tabs_fix_width_while_close_button_is_hovered() {
        use warp_horizontal_tabs::{TabWidthConstraint, TAB_MAX_WIDTH};

        assert_eq!(
            TabWidthConstraint::from_hover_width(Some(73.0)),
            TabWidthConstraint::Fixed(73.0)
        );
        assert_eq!(
            TabWidthConstraint::from_hover_width(None),
            TabWidthConstraint::Max(TAB_MAX_WIDTH)
        );
    }

    #[cfg(feature = "warpui-app")]
    #[test]
    fn warp_tab_context_menu_matches_horizontal_warp_sections() {
        rust_i18n::set_locale("en");
        use crate::menu::MenuItem;
        use warp_tab_context_menu::{
            horizontal_tab_context_menu_items, HorizontalTabContextMenuActions,
        };

        #[derive(Clone, Debug, PartialEq)]
        enum Action {
            Rename,
            Duplicate,
            MoveRight,
            MoveLeft,
            Close,
            CloseOther,
            CloseRight,
        }

        let items = horizontal_tab_context_menu_items(
            1,
            4,
            true,
            HorizontalTabContextMenuActions {
                rename_tab: Some(Action::Rename),
                reset_tab_name: None,
                duplicate_tab: Some(Action::Duplicate),
                move_tab_right: Action::MoveRight,
                move_tab_left: Action::MoveLeft,
                close_tab: Some(Action::Close),
                close_other_tabs: Action::CloseOther,
                close_tabs_right: Action::CloseRight,
                reconnect_tab: None,
                disconnect_tab: None,
                connection_info: None,
                toggle_recording: None,
                is_recording: false,
                save_current_tab_as_new_config: None,
                color_options: None,
            },
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
                "Rename tab",
                "Duplicate Tab",
                "Move Tab Right",
                "Move Tab Left",
                "---",
                "Close tab",
                "Close other tabs",
                "Close Tabs to the Right",
            ]
        );
        assert_eq!(items[0].item_on_select_action(), Some(&Action::Rename));
        assert_eq!(items[7].item_on_select_action(), Some(&Action::CloseRight));
    }

    #[cfg(feature = "warpui-app")]
    #[test]
    fn warp_tab_context_menu_omits_irrelevant_edge_actions() {
        rust_i18n::set_locale("en");
        use crate::menu::MenuItem;
        use warp_tab_context_menu::{
            horizontal_tab_context_menu_items, HorizontalTabContextMenuActions,
        };

        #[derive(Clone, Debug, PartialEq)]
        enum Action {
            Rename,
            Duplicate,
            MoveRight,
            MoveLeft,
            Close,
            CloseOther,
            CloseRight,
        }

        let items = horizontal_tab_context_menu_items(
            2,
            3,
            true,
            HorizontalTabContextMenuActions {
                rename_tab: Some(Action::Rename),
                reset_tab_name: None,
                duplicate_tab: Some(Action::Duplicate),
                move_tab_right: Action::MoveRight,
                move_tab_left: Action::MoveLeft,
                close_tab: Some(Action::Close),
                close_other_tabs: Action::CloseOther,
                close_tabs_right: Action::CloseRight,
                reconnect_tab: None,
                disconnect_tab: None,
                connection_info: None,
                toggle_recording: None,
                is_recording: false,
                save_current_tab_as_new_config: None,
                color_options: None,
            },
        );
        let labels = items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Item(fields) => Some(fields.label().to_string()),
                MenuItem::Separator => None,
                _ => Some("unexpected".to_string()),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            [
                "Rename tab",
                "Duplicate Tab",
                "Move Tab Left",
                "Close tab",
                "Close other tabs",
            ]
        );
    }

    // 内容标签（编辑器 / diff）右键裁剪：rename / duplicate / reconnect 置 None 后只剩移动 + 关闭。
    #[cfg(feature = "warpui-app")]
    #[test]
    fn warp_tab_context_menu_content_tab_drops_rename_duplicate_reconnect() {
        rust_i18n::set_locale("en");
        use crate::menu::MenuItem;
        use warp_tab_context_menu::{
            horizontal_tab_context_menu_items, HorizontalTabContextMenuActions,
        };

        #[derive(Clone, Debug, PartialEq)]
        enum Action {
            MoveRight,
            MoveLeft,
            Close,
            CloseOther,
            CloseRight,
        }

        // 内容标签：rename / reset / duplicate / reconnect 全 None，仅留 move + close。
        let content_actions = || HorizontalTabContextMenuActions {
            rename_tab: None,
            reset_tab_name: None,
            duplicate_tab: None,
            move_tab_right: Action::MoveRight,
            move_tab_left: Action::MoveLeft,
            close_tab: Some(Action::Close),
            close_other_tabs: Action::CloseOther,
            close_tabs_right: Action::CloseRight,
            reconnect_tab: None,
            disconnect_tab: None,
            connection_info: None,
            toggle_recording: None,
            is_recording: false,
            save_current_tab_as_new_config: None,
            color_options: None,
        };
        // 保留分隔符（映射成 "---"），锁定 modify(仅 move) 与 close 段之间恰好一个分隔符、无前导分隔符。
        let labels = |index: usize, len: usize| {
            horizontal_tab_context_menu_items(index, len, true, content_actions())
                .iter()
                .map(|item| match item {
                    MenuItem::Item(fields) => fields.label().to_string(),
                    MenuItem::Separator => "---".to_string(),
                    _ => "unexpected".to_string(),
                })
                .collect::<Vec<_>>()
        };

        // 中间位：move right + move left 都在。
        assert_eq!(
            labels(1, 3),
            [
                "Move Tab Right",
                "Move Tab Left",
                "---",
                "Close tab",
                "Close other tabs",
                "Close Tabs to the Right",
            ]
        );
        // 首位：只剩 move right，无前导分隔符。
        assert_eq!(
            labels(0, 3),
            [
                "Move Tab Right",
                "---",
                "Close tab",
                "Close other tabs",
                "Close Tabs to the Right",
            ]
        );
        // 末位：只剩 move left，close right 不出现。
        assert_eq!(
            labels(2, 3),
            ["Move Tab Left", "---", "Close tab", "Close other tabs"]
        );
    }

    #[cfg(feature = "warpui-app")]
    #[test]
    fn warp_tab_context_menu_keeps_warp_legacy_color_row_shape() {
        use crate::menu::MenuItem;
        use warp_core::ui::theme::{AnsiColor, AnsiColorIdentifier, AnsiColors};
        use warp_tab_context_menu::{
            horizontal_tab_context_menu_items, HorizontalTabColorOptions,
            HorizontalTabContextMenuActions, TAB_COLOR_OPTIONS,
        };

        #[derive(Clone, Debug, PartialEq)]
        enum Action {
            Rename,
            Duplicate,
            MoveRight,
            MoveLeft,
            Close,
            CloseOther,
            CloseRight,
            Color(AnsiColorIdentifier),
        }

        let terminal_colors = AnsiColors {
            black: AnsiColor::from_u32(0x000000ff),
            red: AnsiColor::from_u32(0xff0000ff),
            green: AnsiColor::from_u32(0x00ff00ff),
            yellow: AnsiColor::from_u32(0xffff00ff),
            blue: AnsiColor::from_u32(0x0000ffff),
            magenta: AnsiColor::from_u32(0xff00ffff),
            cyan: AnsiColor::from_u32(0x00ffffff),
            white: AnsiColor::from_u32(0xffffffff),
        };

        let items = horizontal_tab_context_menu_items(
            0,
            1,
            true,
            HorizontalTabContextMenuActions {
                rename_tab: Some(Action::Rename),
                reset_tab_name: None,
                duplicate_tab: Some(Action::Duplicate),
                move_tab_right: Action::MoveRight,
                move_tab_left: Action::MoveLeft,
                close_tab: Some(Action::Close),
                close_other_tabs: Action::CloseOther,
                close_tabs_right: Action::CloseRight,
                reconnect_tab: None,
                disconnect_tab: None,
                connection_info: None,
                toggle_recording: None,
                is_recording: false,
                save_current_tab_as_new_config: None,
                color_options: Some(HorizontalTabColorOptions {
                    selected_color: Some(AnsiColorIdentifier::Red),
                    terminal_colors,
                    toggle_tab_color_actions: TAB_COLOR_OPTIONS.map(Action::Color),
                }),
            },
        );

        let color_row = items.iter().find_map(|item| match item {
            MenuItem::ItemsRow { items } => Some(items),
            _ => None,
        });

        let color_row = color_row.expect("Warp color row is present");
        assert_eq!(color_row.len(), TAB_COLOR_OPTIONS.len());
        assert_eq!(
            color_row[0].on_select_action(),
            Some(&Action::Color(AnsiColorIdentifier::Red))
        );
    }

    #[cfg(feature = "warpui-app")]
    #[test]
    fn warp_tab_color_toggle_matches_warp_clear_and_set_behavior() {
        use warp_core::ui::theme::AnsiColorIdentifier;
        use warp_tab_context_menu::selected_tab_color_after_toggle;

        assert_eq!(
            selected_tab_color_after_toggle(
                Some(AnsiColorIdentifier::Red),
                AnsiColorIdentifier::Red
            ),
            None
        );
        assert_eq!(
            selected_tab_color_after_toggle(
                Some(AnsiColorIdentifier::Red),
                AnsiColorIdentifier::Blue
            ),
            Some(AnsiColorIdentifier::Blue)
        );
    }

    #[cfg(feature = "warpui-app")]
    #[test]
    fn warp_tab_rename_title_matches_warp_empty_title_filtering() {
        use warp_tab_context_menu::custom_title_from_editor;

        assert_eq!(
            custom_title_from_editor("Production SSH"),
            Some("Production SSH".to_string())
        );
        assert_eq!(custom_title_from_editor(""), None);
    }

    #[cfg(feature = "warpui-app")]
    #[test]
    fn warp_tab_rename_editor_margin_matches_warp_tab_content() {
        use warp_tab_context_menu::tab_rename_editor_top_margin;

        assert_eq!(tab_rename_editor_top_margin(true), 8.0);
        assert_eq!(tab_rename_editor_top_margin(false), 3.0);
    }

    #[cfg(feature = "warpui-app")]
    #[test]
    fn warp_tab_rename_finishes_on_external_mouse_down_only_when_editing() {
        use warp_tab_context_menu::should_finish_tab_rename_on_external_mouse_down;

        assert!(should_finish_tab_rename_on_external_mouse_down(Some(0)));
        assert!(!should_finish_tab_rename_on_external_mouse_down(None));
    }

    #[test]
    fn shell_chrome_terminal_preview_prefers_the_active_terminal_session() {
        rust_i18n::set_locale("en");
        let shell = ShellModel::nexshell_default();
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });
        let view = view_model::project(&shell, layout);

        let preview = shell_chrome::terminal_preview(&view);

        assert_eq!(preview.session_id.as_deref(), Some("sshtool"));
        assert!(preview.attached);
        assert_eq!(preview.prompt, "nexshell@sshtool:~$");
        assert!(preview
            .lines
            .iter()
            .any(|line| line.contains("NexShell native terminal host is ready")));
    }

    #[test]
    fn shell_chrome_terminal_preview_marks_detached_task_tabs_without_losing_session() {
        let mut shell = ShellModel::nexshell_default();
        shell.activate_tab("fix-cursor");
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });
        let view = view_model::project(&shell, layout);

        let preview = shell_chrome::terminal_preview(&view);

        assert_eq!(preview.session_id.as_deref(), Some("sshtool"));
        assert!(preview.attached);
        assert_eq!(preview.prompt, "nexshell@sshtool:~$");
        assert!(preview
            .lines
            .iter()
            .any(|line| line.contains("ssh sshtool")));
    }

    #[test]
    fn terminal_runtime_text_buffer_strips_ansi_and_tracks_visible_lines() {
        let mut buffer = terminal_runtime::TerminalTextBuffer::new(4);

        buffer.push_output(b"\x1b[32mhello\x1b[0m\r\nworld");

        assert_eq!(buffer.lines(), ["hello", "world"]);
    }

    #[test]
    fn terminal_runtime_text_buffer_handles_carriage_return_rewrites() {
        let mut buffer = terminal_runtime::TerminalTextBuffer::new(4);

        buffer.push_output(b"progress 10%\rprogress 20%\r\nready");

        assert_eq!(buffer.lines(), ["progress 20%", "ready"]);
    }

    #[test]
    fn terminal_runtime_text_buffer_handles_carriage_return_across_chunks() {
        let mut buffer = terminal_runtime::TerminalTextBuffer::new(4);

        buffer.push_output(b"progress 10%\r");
        buffer.push_output(b"progress 20%\r\nready");

        assert_eq!(buffer.lines(), ["progress 20%", "ready"]);
    }

    #[test]
    fn terminal_runtime_grid_snapshot_keeps_visible_text_and_ansi_style() {
        let mut grid = terminal_runtime::TerminalGridCore::new(16, 3, 100);

        grid.process_output(b"plain\r\n\x1b[31mred\x1b[0m");
        let snapshot = grid.snapshot(&[], None);

        assert_eq!(snapshot.cols, 16);
        assert_eq!(snapshot.rows, 3);
        assert_eq!(snapshot.lines[0].text, "plain");
        assert_eq!(snapshot.lines[1].text, "red");
        assert_eq!(
            snapshot.cell_style(&snapshot.lines[1].cells[0]).fg,
            terminal_runtime::TerminalColorSnapshot::Named("red")
        );
        assert!(snapshot.lines[1].cells[0].bold() == false);
    }

    #[test]
    fn terminal_runtime_grid_cell_snapshot_uses_compact_flags() {
        assert_eq!(
            std::mem::size_of::<terminal_runtime::TerminalCellFlags>(),
            4
        );

        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        grid.process_output(b"\x1b[1;2;3;4;9;8;7mA\x1b[0m");
        let snapshot = grid.snapshot(&[], None);
        let flags = snapshot.lines[0].cells[0].flags;

        assert!(flags.contains(terminal_runtime::TerminalCellFlags::BOLD));
        assert!(flags.contains(terminal_runtime::TerminalCellFlags::DIM));
        assert!(flags.contains(terminal_runtime::TerminalCellFlags::ITALIC));
        assert!(flags.contains(terminal_runtime::TerminalCellFlags::UNDERLINE));
        assert!(flags.contains(terminal_runtime::TerminalCellFlags::STRIKEOUT));
        assert!(flags.contains(terminal_runtime::TerminalCellFlags::HIDDEN));
        assert!(flags.contains(terminal_runtime::TerminalCellFlags::INVERSE));
    }

    #[test]
    fn terminal_runtime_grid_snapshot_reuses_cell_style_ids() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);

        grid.process_output(b"\x1b[31mred\x1b[0m");
        let snapshot = grid.snapshot(&[], None);

        let red_style_id = snapshot.lines[0].cells[0].style_id;
        assert_eq!(snapshot.lines[0].cells[1].style_id, red_style_id);
        assert_eq!(snapshot.lines[0].cells[2].style_id, red_style_id);
        assert_eq!(snapshot.styles.len(), 2);
        assert_eq!(
            snapshot.styles[red_style_id as usize].fg,
            terminal_runtime::TerminalColorSnapshot::Named("red")
        );
    }

    #[test]
    fn terminal_runtime_grid_snapshot_handles_clear_screen_and_cursor_home() {
        let mut grid = terminal_runtime::TerminalGridCore::new(16, 3, 100);

        grid.process_output(b"old\r\nline\x1b[2J\x1b[Hnew");
        let snapshot = grid.snapshot(&[], None);

        assert_eq!(snapshot.lines[0].text, "new");
        assert_eq!(snapshot.lines[1].text, "");
        assert_eq!(snapshot.cursor_row, 0);
        assert_eq!(snapshot.cursor_col, 3);
    }

    #[test]
    fn terminal_runtime_ctrl_l_clear_screen_homes_viewport_with_scrollback() {
        let mut grid = terminal_runtime::TerminalGridCore::new(16, 3, 100);
        for line in 0..8 {
            grid.process_output(format!("line{line}\r\n").as_bytes());
        }
        assert!(grid.snapshot(&[], None).history_size > 0);
        grid.scroll_lines(2);
        assert_eq!(grid.snapshot(&[], None).display_offset, 2);

        grid.process_output(b"\x1b[H\x1b[2Jprompt");
        let snapshot = grid.snapshot(&[], None);

        assert_eq!(snapshot.display_offset, 0);
        assert_eq!(snapshot.lines[0].text, "prompt");
        assert_eq!(snapshot.cursor_row, 0);
        assert_eq!(snapshot.cursor_col, 6);
    }

    #[test]
    fn terminal_runtime_grid_dirty_rows_track_single_line_input() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 3, 100);
        let initial = grid.snapshot(&[], None);
        assert_eq!(initial.dirty_rows, vec![true, true, true]);

        grid.clear_dirty_rows();
        grid.process_output(b"abc");
        let snapshot = grid.snapshot(&[], None);

        assert_eq!(snapshot.dirty_rows, vec![true, false, false]);
    }

    #[test]
    fn terminal_runtime_grid_dirty_rows_track_multiline_input() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 3, 100);
        grid.clear_dirty_rows();
        grid.process_output(b"ab\r\ncd");
        let snapshot = grid.snapshot(&[], None);

        assert_eq!(snapshot.dirty_rows, vec![true, true, false]);
    }

    #[test]
    fn terminal_runtime_grid_snapshot_reuses_clean_rows() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"top\r\nbottom");
        let first = grid.snapshot(&[], None);
        let clean_row_cell = first.lines[0].cells[0].content.clone();

        grid.clear_dirty_rows();
        grid.process_output(b"!");
        let second = grid.snapshot(&[], None);

        assert_eq!(second.dirty_rows, vec![false, true]);
        assert!(Arc::ptr_eq(
            &clean_row_cell,
            &second.lines[0].cells[0].content
        ));
    }

    #[test]
    fn terminal_runtime_grid_snapshot_shifts_clean_rows_on_scroll() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 3, 100);
        for line in 0..6 {
            grid.process_output(format!("line-{line}\r\n").as_bytes());
        }
        let first = grid.snapshot(&[], None);
        let old_top_cell = first.lines[0].cells[0].content.clone();

        grid.clear_dirty_rows();
        grid.scroll_lines(1);
        let second = grid.snapshot(&[], None);

        assert_eq!(second.dirty_rows, vec![true, false, false]);
        assert!(Arc::ptr_eq(
            &old_top_cell,
            &second.lines[1].cells[0].content
        ));
    }

    #[test]
    fn terminal_runtime_render_rows_resolve_cell_colors_and_cursor() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);

        grid.process_output(b"\x1b[31mred\x1b[0m");
        let snapshot = grid.snapshot(&[], None);
        let rows = terminal_runtime::terminal_render_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].cells.len(), 8);
        assert_eq!(rows[0].cells[0].ch, 'r');
        assert_eq!(rows[0].cells[0].fg, 0xff8272);
        assert_eq!(rows[0].cells[0].bg, 0x000000);
        assert!(!rows[0].cells[0].cursor);
        assert_eq!(rows[0].cells[3].ch, ' ');
        assert!(rows[0].cells[3].cursor);
        assert_eq!(rows[0].cells[3].fg, 0x000000);
        assert_eq!(rows[0].cells[3].bg, 0x19aad8);
    }

    #[test]
    fn terminal_runtime_render_rows_apply_dim_and_hidden_flags() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);

        grid.process_output(b"\x1b[2;31mD\x1b[0m\x1b[8mH\x1b[0m");
        let snapshot = grid.snapshot(&[], None);
        let rows = terminal_runtime::terminal_render_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );

        assert_eq!(snapshot.lines[0].text, "DH");
        assert_eq!(rows[0].cells[0].ch, 'D');
        assert_eq!(rows[0].cells[0].fg, 0xa8564b);
        assert_eq!(rows[0].cells[1].ch, ' ');
        assert_eq!(rows[0].cells[1].fg, 0xffffff);
        assert_eq!(rows[0].cells[1].bg, 0x000000);
    }

    #[test]
    fn terminal_runtime_render_rows_preserve_strikeout_and_double_underline() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);

        grid.process_output(b"\x1b[9mS\x1b[0m\x1b[4:2mU\x1b[0m");
        let snapshot = grid.snapshot(&[], None);
        let rows = terminal_runtime::terminal_render_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );

        assert!(snapshot.lines[0].cells[0].strikeout());
        assert!(!snapshot.lines[0].cells[0].double_underline());
        assert!(snapshot.lines[0].cells[1].underline());
        assert!(snapshot.lines[0].cells[1].double_underline());
        assert!(rows[0].cells[0].strikeout);
        assert!(rows[0].cells[1].double_underline);
    }

    #[test]
    fn terminal_runtime_render_rows_preserve_undercurl_dotted_dashed_underline() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);

        grid.process_output(b"\x1b[4:3mC\x1b[0m\x1b[4:4mD\x1b[0m\x1b[4:5mA\x1b[0m");
        let snapshot = grid.snapshot(&[], None);
        let rows = terminal_runtime::terminal_render_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );

        assert!(snapshot.lines[0].cells[0].undercurl());
        assert!(!snapshot.lines[0].cells[0].dotted_underline());
        assert!(snapshot.lines[0].cells[1].dotted_underline());
        assert!(!snapshot.lines[0].cells[1].undercurl());
        assert!(snapshot.lines[0].cells[2].dashed_underline());

        assert!(rows[0].cells[0].undercurl);
        assert!(rows[0].cells[0].underline);
        assert!(rows[0].cells[1].dotted_underline);
        assert!(rows[0].cells[2].dashed_underline);
    }

    #[test]
    fn terminal_runtime_render_rows_preserve_underline_color() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);

        grid.process_output(b"\x1b[4;58;2;1;2;3mU\x1b[0m");
        let snapshot = grid.snapshot(&[], None);
        let rows = terminal_runtime::terminal_render_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );

        assert!(snapshot.lines[0].cells[0].underline());
        assert_eq!(
            snapshot
                .cell_style(&snapshot.lines[0].cells[0])
                .underline_color,
            Some(terminal_runtime::TerminalColorSnapshot::Rgb { r: 1, g: 2, b: 3 })
        );
        assert_eq!(rows[0].cells[0].underline_color, Some(0x010203));
    }

    #[test]
    fn terminal_runtime_render_rows_preserve_zero_width_cell_content() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);

        grid.process_output(b"e\xcc\x81x");
        let snapshot = grid.snapshot(&[], None);
        let rows = terminal_runtime::terminal_render_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );
        let runs = terminal_runtime::terminal_render_run_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );
        let combined = format!("e{}x", '\u{0301}');
        let first_cell = format!("e{}", '\u{0301}');

        assert_eq!(snapshot.lines[0].text, combined);
        assert_eq!(snapshot.lines[0].cells[0].ch, 'e');
        assert_eq!(&*snapshot.lines[0].cells[0].content, first_cell.as_str());
        assert_eq!(rows[0].cells[0].ch, 'e');
        assert_eq!(&*rows[0].cells[0].content, first_cell.as_str());
        assert_eq!(&*runs[0].runs[0].text, first_cell.as_str());
        assert_eq!(rows[0].cells[1].ch, 'x');
    }

    #[test]
    fn terminal_runtime_render_rows_keep_beam_cursor_cell_colors_for_overlay() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);

        // CSI 6 SP q => steady beam cursor. Beam/underline cursors are drawn
        // as overlays, so the underlying cell should keep its normal colors.
        grid.process_output(b"\x1b[31mred\x1b[0m\x1b[6 q");
        let snapshot = grid.snapshot(&[], None);
        let rows = terminal_runtime::terminal_render_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );

        assert_eq!(
            snapshot.cursor_shape,
            terminal_runtime::TerminalCursorShape::Beam
        );
        assert!(rows[0].cells[3].cursor);
        assert_eq!(rows[0].cells[3].fg, 0xffffff);
        assert_eq!(rows[0].cells[3].bg, 0x000000);
    }

    #[test]
    fn terminal_runtime_render_run_rows_merge_same_style_runs() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);

        grid.process_output(b"\x1b[31mred\x1b[0m");
        let snapshot = grid.snapshot(&[], None);
        let rows = terminal_runtime::terminal_render_run_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );

        assert_eq!(rows[0].runs.len(), 3);
        assert_eq!(&*rows[0].runs[0].text, "red");
        assert_eq!(rows[0].runs[0].cols, 3);
        assert_eq!(rows[0].runs[0].fg, 0xff8272);
        assert!(!rows[0].runs[0].cursor);
        assert_eq!(&*rows[0].runs[1].text, " ");
        assert_eq!(rows[0].runs[1].cols, 1);
        assert!(rows[0].runs[1].cursor);
        assert_eq!(&*rows[0].runs[2].text, "    ");
        assert_eq!(rows[0].runs[2].cols, 4);
        assert!(!rows[0].runs[2].cursor);
        let row1_cols: usize = rows[1].runs.iter().map(|r| r.cols).sum();
        assert_eq!(row1_cols, 8);
    }

    #[test]
    fn terminal_runtime_failed_snapshot_is_detached_but_keeps_session_identity() {
        let runtime = terminal_runtime::LocalTerminalRuntime::failed("sshtool", "spawn failed");

        assert_eq!(
            *runtime.snapshot(),
            terminal_runtime::TerminalRuntimeSnapshot {
                session_id: "sshtool".to_string(),
                connected: false,
                status: "spawn failed".to_string(),
                title: None,
                bootstrapped: true,
                shell_display_name: None,
                bell_pulse: 0,
                find_pulse: 0,
                find_query: None,
                find_match_count: 0,
                find_current_match: None,
                marked_text: None,
                lines: [""].map(String::from).to_vec(),
                grid: terminal_runtime::TerminalGridSnapshot::empty(),
                local_cwd: None,
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn terminal_runtime_can_spawn_direct_pty_command() {
        let runtime = terminal_runtime::LocalTerminalRuntime::spawn_command_or_failed(
            "direct-pty",
            "/bin/sh",
            &["-lc", "printf direct-ready"],
            "running direct pty command",
            24,
            4,
        );

        let mut snapshot = runtime.snapshot();
        for _ in 0..50 {
            if snapshot
                .lines
                .iter()
                .any(|line| line.contains("direct-ready"))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            snapshot = runtime.snapshot();
        }

        assert_eq!(snapshot.session_id, "direct-pty");
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line.contains("direct-ready")));
    }

    #[cfg(unix)]
    #[test]
    fn terminal_runtime_direct_command_starts_bootstrapped_without_shell_placeholder() {
        let runtime = terminal_runtime::LocalTerminalRuntime::spawn_command_or_failed(
            "serial-direct",
            "/bin/sh",
            &["-lc", "printf serial-ready"],
            "opening serial",
            24,
            4,
        );

        let snapshot = runtime.snapshot();
        assert!(snapshot.bootstrapped);
        assert_eq!(snapshot.shell_display_name, None);
    }

    #[cfg(windows)]
    #[test]
    fn terminal_runtime_can_spawn_windows_direct_pty_command() {
        let runtime = terminal_runtime::LocalTerminalRuntime::spawn_command_or_failed(
            "windows-direct-pty",
            "cmd.exe",
            &["/C", "echo direct-ready"],
            "running windows direct pty command",
            24,
            4,
        );

        let mut snapshot = runtime.snapshot();
        for _ in 0..100 {
            if snapshot
                .lines
                .iter()
                .any(|line| line.contains("direct-ready"))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            snapshot = runtime.snapshot();
        }

        assert_eq!(snapshot.session_id, "windows-direct-pty");
        assert!(snapshot.connected, "{}", snapshot.status);
        assert!(snapshot.bootstrapped);
        assert!(snapshot
            .lines
            .iter()
            .any(|line| line.contains("direct-ready")));
    }

    #[cfg(windows)]
    #[test]
    fn terminal_runtime_windows_local_shell_starts_without_bootstrap_placeholder() {
        let runtime =
            terminal_runtime::LocalTerminalRuntime::spawn_local_or_failed("windows-local", 24, 4);

        let snapshot = runtime.snapshot();
        assert!(snapshot.connected, "{}", snapshot.status);
        assert!(snapshot.bootstrapped);
        assert_eq!(snapshot.shell_display_name, None);
    }

    #[test]
    fn terminal_runtime_key_encoding_covers_basic_terminal_input() {
        assert_eq!(
            terminal_runtime::encode_terminal_key("a", Some("a"), false, false, false),
            Some(b"a".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key("enter", None, false, false, false),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key("backspace", None, false, false, false),
            Some(vec![0x7f])
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key("c", None, true, false, false),
            Some(vec![0x03])
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key("l", None, true, false, false),
            Some(vec![0x0c])
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key("up", None, false, false, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key("f1", None, false, false, false),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key("left", None, true, false, false),
            Some(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key("left", None, false, true, false),
            Some(b"\x1b[1;3D".to_vec())
        );
    }

    #[test]
    fn terminal_runtime_key_encoding_uses_app_cursor_mode() {
        let modes = terminal_runtime::TerminalInputModes {
            app_cursor: true,
            ..Default::default()
        };

        assert_eq!(
            terminal_runtime::encode_terminal_key_with_modes(
                "up", None, false, false, false, false, modes
            ),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key_with_modes(
                "up", None, false, false, true, false, modes
            ),
            Some(b"\x1b[1;2A".to_vec())
        );
    }

    #[test]
    fn terminal_runtime_key_encoding_covers_warp_special_keys() {
        let modes = terminal_runtime::TerminalInputModes::default();

        assert_eq!(
            terminal_runtime::encode_terminal_key_with_modes(
                "insert", None, false, false, false, false, modes
            ),
            Some(b"\x1b[2~".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key_with_modes(
                "delete", None, false, false, false, false, modes
            ),
            Some(b"\x1b[3~".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key_with_modes(
                "pageup", None, false, false, false, false, modes
            ),
            Some(b"\x1b[5~".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key_with_modes(
                "pagedown", None, false, false, false, false, modes
            ),
            Some(b"\x1b[6~".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key_with_modes(
                "tab", None, false, false, true, false, modes
            ),
            Some(b"\x1b[Z".to_vec())
        );
    }

    #[test]
    fn terminal_runtime_key_encoding_uses_platform_base_key_for_kitty_alternate_keys() {
        let modes = terminal_runtime::TerminalInputModes {
            keyboard_report_all_as_escape: true,
            keyboard_report_alternate_keys: true,
            ..Default::default()
        };

        assert_eq!(
            terminal_runtime::encode_terminal_key_event_with_modes(
                "@",
                Some("2"),
                Some("@"),
                false,
                false,
                true,
                false,
                modes
            ),
            Some(b"\x1b[50:64;2u".to_vec())
        );
    }

    #[test]
    fn terminal_runtime_shift_enter_falls_back_to_backslash_cr_without_kitty() {
        // kitty 未激活：shift+enter 兜底发 `\` + CR（对齐 iTerm2 terminal-setup / Warp）。
        let modes = terminal_runtime::TerminalInputModes::default();
        assert_eq!(
            terminal_runtime::encode_terminal_key_with_modes(
                "enter", None, false, false, true, false, modes
            ),
            Some(b"\\\r".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_key_with_modes(
                "numpadenter",
                None,
                false,
                false,
                true,
                false,
                modes
            ),
            Some(b"\\\r".to_vec())
        );
        // shift 未按时仍是普通回车。
        assert_eq!(
            terminal_runtime::encode_terminal_key_with_modes(
                "enter", None, false, false, false, false, modes
            ),
            Some(b"\r".to_vec())
        );

        // kitty 激活：shift+enter 走 CSI-u，不落兜底分支。
        let kitty = terminal_runtime::TerminalInputModes {
            keyboard_disambiguate_escape: true,
            ..Default::default()
        };
        assert_eq!(
            terminal_runtime::encode_terminal_key_with_modes(
                "enter", None, false, false, true, false, kitty
            ),
            Some(b"\x1b[13;2u".to_vec())
        );
    }

    #[test]
    fn terminal_runtime_ctrl_l_uses_platform_base_key_when_key_is_control_char() {
        assert_eq!(
            terminal_runtime::encode_terminal_key_event_with_modes(
                "\x0c",
                Some("l"),
                Some("\x0c"),
                true,
                false,
                false,
                false,
                terminal_runtime::TerminalInputModes::default(),
            ),
            Some(vec![0x0c])
        );
    }

    #[test]
    fn terminal_runtime_ctrl_arrow_is_not_misread_as_ctrl_letter() {
        // 仿 Warp cursor_movement_keystroke_to_escape_sequence：Ctrl+Left → CSI 1;5 D
        assert_eq!(
            terminal_runtime::encode_terminal_key_event_with_modes(
                "left",
                None,
                None,
                true,
                false,
                false,
                false,
                terminal_runtime::TerminalInputModes::default(),
            ),
            Some(b"\x1b[1;5D".to_vec())
        );
    }

    #[test]
    fn terminal_input_editor_keeps_text_local_until_enter_submits() {
        let mut editor = terminal_runtime::TerminalInputEditor::default();
        editor.insert("echo 你好");

        assert_eq!(editor.buffer(), "echo 你好");
        assert_eq!(
            editor.submit_bytes(),
            Some("echo 你好\r".as_bytes().to_vec())
        );
        assert!(editor.is_empty());
    }

    #[test]
    fn terminal_input_editor_forwards_history_navigation_to_shell() {
        let mut editor = terminal_runtime::TerminalInputEditor::default();
        editor.insert("ec");

        assert_eq!(editor.flush_for_history(b"\x1b[A"), b"ec\x1b[A".to_vec());
        assert!(editor.is_empty());
        assert!(editor.shell_owns_line());
    }

    #[test]
    fn terminal_input_editor_capture_is_limited_to_primary_prompt_view() {
        let mut snapshot = terminal_runtime::TerminalGridSnapshot::empty();
        snapshot.input_modes.alt_screen = false;
        snapshot.sgr_mouse = false;
        snapshot.display_offset = 0;
        assert!(terminal_runtime::terminal_input_editor_should_capture(
            &snapshot
        ));

        snapshot.input_modes.alt_screen = true;
        assert!(!terminal_runtime::terminal_input_editor_should_capture(
            &snapshot
        ));

        snapshot.input_modes.alt_screen = false;
        snapshot.sgr_mouse = true;
        snapshot.mouse_report_click = true;
        assert!(!terminal_runtime::terminal_input_editor_should_capture(
            &snapshot
        ));
    }

    #[test]
    fn terminal_input_editor_projects_buffer_at_prompt_cursor() {
        let mut grid = terminal_runtime::TerminalGridCore::new(16, 2, 100);
        grid.process_output("~ > ".as_bytes());
        let snapshot = grid.snapshot(&[], None);
        let mut editor = terminal_runtime::TerminalInputEditor::default();
        editor.insert("你好");

        let projected = terminal_runtime::terminal_snapshot_with_input_editor(&snapshot, &editor);

        assert_eq!(projected.lines[0].text, "~ > 你好");
        assert_eq!(projected.cursor_row, snapshot.cursor_row);
        assert_eq!(projected.cursor_col, snapshot.cursor_col + 4);
        assert_eq!(projected.cursor_shape, snapshot.cursor_shape);
        assert!(projected.dirty_rows.iter().all(|dirty| *dirty));
    }

    #[test]
    fn terminal_input_editor_projects_marked_text_after_buffer() {
        let mut grid = terminal_runtime::TerminalGridCore::new(16, 2, 100);
        grid.process_output("~ > ".as_bytes());
        let snapshot = grid.snapshot(&[], None);
        let mut editor = terminal_runtime::TerminalInputEditor::default();
        editor.insert("dd");
        editor.set_marked_text("中".to_string(), 0..1);

        let projected = terminal_runtime::terminal_snapshot_with_input_editor(&snapshot, &editor);

        assert_eq!(projected.lines[0].text, "~ > dd中");
        assert_eq!(projected.cursor_row, snapshot.cursor_row);
        assert_eq!(projected.cursor_col, snapshot.cursor_col + 4);
        assert!(projected.lines[0].cells[snapshot.cursor_col + 2]
            .flags
            .contains(terminal_runtime::TerminalCellFlags::UNDERLINE));
        assert!(projected.dirty_rows.iter().all(|dirty| *dirty));
    }

    #[test]
    fn terminal_input_editor_projects_marked_text_without_buffer() {
        let mut grid = terminal_runtime::TerminalGridCore::new(16, 2, 100);
        grid.process_output("~ > ".as_bytes());
        let snapshot = grid.snapshot(&[], None);
        let mut editor = terminal_runtime::TerminalInputEditor::default();
        editor.set_marked_text("中".to_string(), 0..1);

        let projected = terminal_runtime::terminal_snapshot_with_input_editor(&snapshot, &editor);

        assert_eq!(projected.lines[0].text, "~ > 中");
        assert_eq!(projected.cursor_col, snapshot.cursor_col + 2);
        assert!(projected.marked_text_active);
    }

    #[test]
    fn terminal_input_editor_moves_cursor_and_inserts_committed_text_locally() {
        let mut grid = terminal_runtime::TerminalGridCore::new(16, 2, 100);
        grid.process_output("~ > ".as_bytes());
        let snapshot = grid.snapshot(&[], None);
        let mut editor = terminal_runtime::TerminalInputEditor::default();
        editor.insert("abcd");

        assert!(editor.move_left());
        assert!(editor.move_left());
        editor.insert("你");

        let projected = terminal_runtime::terminal_snapshot_with_input_editor(&snapshot, &editor);

        assert_eq!(editor.buffer(), "ab你cd");
        assert_eq!(projected.lines[0].text, "~ > ab你cd");
        assert_eq!(projected.cursor_row, snapshot.cursor_row);
        assert_eq!(projected.cursor_col, snapshot.cursor_col + 4);
        assert_eq!(editor.submit_bytes(), Some("ab你cd\r".as_bytes().to_vec()));
    }

    #[test]
    fn terminal_input_editor_backspace_removes_before_local_cursor() {
        let mut editor = terminal_runtime::TerminalInputEditor::default();
        editor.insert("ab你cd");

        assert!(editor.move_left());
        assert!(editor.move_left());
        assert!(editor.backspace());

        assert_eq!(editor.buffer(), "abcd");
    }

    #[test]
    fn terminal_input_editor_revision_changes_when_visible_buffer_changes() {
        let mut editor = terminal_runtime::TerminalInputEditor::default();
        let initial = editor.revision();

        editor.insert("a");
        assert!(editor.revision() > initial);
        let inserted = editor.revision();

        editor.backspace();
        assert!(editor.revision() > inserted);
        let removed = editor.revision();

        editor.clear();
        assert_eq!(editor.revision(), removed);

        editor.set_marked_text("中".to_string(), 0..1);
        assert!(editor.revision() > removed);
        let marked = editor.revision();

        assert!(editor.clear_marked_text());
        assert!(editor.revision() > marked);
        assert!(!editor.clear_marked_text());
    }

    #[test]
    fn terminal_runtime_modifier_key_encoding_follows_warp_kitty_protocol() {
        use warpui::platform::keyboard::KeyCode;

        let press_only_modes = terminal_runtime::TerminalInputModes {
            keyboard_report_all_as_escape: true,
            ..Default::default()
        };

        assert_eq!(
            terminal_runtime::encode_terminal_modifier_key_with_modes(
                &KeyCode::ShiftLeft,
                true,
                press_only_modes
            ),
            Some(b"\x1b[57441;2u".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_modifier_key_with_modes(
                &KeyCode::ShiftLeft,
                false,
                press_only_modes
            ),
            None
        );

        let event_modes = terminal_runtime::TerminalInputModes {
            keyboard_report_all_as_escape: true,
            keyboard_report_event_types: true,
            ..Default::default()
        };

        assert_eq!(
            terminal_runtime::encode_terminal_modifier_key_with_modes(
                &KeyCode::ShiftLeft,
                false,
                event_modes
            ),
            Some(b"\x1b[57441;2:3u".to_vec())
        );
        assert_eq!(
            terminal_runtime::encode_terminal_modifier_key_with_modes(
                &KeyCode::KeyA,
                true,
                event_modes
            ),
            None
        );
    }

    #[test]
    fn terminal_grid_snapshot_reports_app_cursor_for_warp_key_encoding() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        grid.process_output(b"\x1b[?1h");

        assert!(grid.snapshot(&[], None).input_modes.app_cursor);
    }

    #[test]
    fn terminal_grid_snapshot_reports_focus_in_out_mode() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        grid.process_output(b"\x1b[?1004h");

        assert!(grid.snapshot(&[], None).input_modes.focus_in_out);
    }

    #[test]
    fn terminal_grid_snapshot_reports_kitty_keyboard_protocol_mode() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        grid.process_output(b"\x1b[>1u");

        assert!(
            grid.snapshot(&[], None)
                .input_modes
                .keyboard_disambiguate_escape
        );
    }

    #[test]
    fn terminal_focus_report_bytes_follow_warp_escape_sequences() {
        let enabled_modes = terminal_runtime::TerminalInputModes {
            focus_in_out: true,
            ..Default::default()
        };

        assert_eq!(
            terminal_runtime::terminal_focus_report_bytes(true, enabled_modes),
            Some(b"\x1b[I".to_vec())
        );
        assert_eq!(
            terminal_runtime::terminal_focus_report_bytes(false, enabled_modes),
            Some(b"\x1b[O".to_vec())
        );
        assert_eq!(
            terminal_runtime::terminal_focus_report_bytes(
                true,
                terminal_runtime::TerminalInputModes::default()
            ),
            None
        );
    }

    #[test]
    fn terminal_grid_snapshot_reports_alt_screen_for_alt_scroll() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);

        // ALTERNATE_SCROLL 默认关：new() 关掉 alacritty 的默认开（iTerm2 同款，见 ADR 0006），
        // 使备用屏滚轮默认走本地 scrollback；应用显式 ?1007h 才转发 ↑/↓。
        assert!(!grid.snapshot(&[], None).input_modes.alternate_scroll);
        assert!(!grid.snapshot(&[], None).input_modes.alt_screen);

        grid.process_output(b"\x1b[?1049h");
        assert!(grid.snapshot(&[], None).input_modes.alt_screen);

        grid.process_output(b"\x1b[?1007l");
        assert!(!grid.snapshot(&[], None).input_modes.alternate_scroll);

        grid.process_output(b"\x1b[?1007h");
        assert!(grid.snapshot(&[], None).input_modes.alternate_scroll);

        grid.process_output(b"\x1b[?1049l");
        assert!(!grid.snapshot(&[], None).input_modes.alt_screen);
    }

    #[test]
    fn terminal_alt_screen_exit_clears_residual_click_mouse() {
        // alacritty mouse 协议互斥（?1000/?1002/?1003 只保留最后一个），
        // 单独覆盖 click，确保 ?1049l 退出真正清掉 click mode。
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"\x1b[?1049h\x1b[?1000h\x1b[?1006h");
        let snap = grid.snapshot(&[], None);
        assert!(snap.input_modes.alt_screen);
        assert!(snap.mouse_report_click);
        assert!(snap.sgr_mouse);

        grid.process_output(b"\x1b[?1049l");
        let snap = grid.snapshot(&[], None);
        assert!(!snap.input_modes.alt_screen);
        assert!(!snap.mouse_report_click);
        assert!(!snap.sgr_mouse);
    }

    #[test]
    fn terminal_alt_screen_exit_clears_residual_drag_mouse() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"\x1b[?1049h\x1b[?1002h\x1b[?1006h");
        let snap = grid.snapshot(&[], None);
        assert!(snap.mouse_report_drag && snap.sgr_mouse);

        grid.process_output(b"\x1b[?1049l");
        let snap = grid.snapshot(&[], None);
        assert!(!snap.mouse_report_drag);
        assert!(!snap.sgr_mouse);
    }

    #[test]
    fn terminal_alt_screen_exit_clears_residual_motion_mouse() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"\x1b[?1049h\x1b[?1003h\x1b[?1006h");
        let snap = grid.snapshot(&[], None);
        assert!(snap.mouse_report_motion && snap.sgr_mouse);

        grid.process_output(b"\x1b[?1049l");
        let snap = grid.snapshot(&[], None);
        assert!(!snap.mouse_report_motion);
        assert!(!snap.sgr_mouse);
    }

    #[test]
    fn terminal_alt_screen_exit_via_47l_clears_residual_mouse() {
        // vte 不把 ?47 映射为 alacritty SwapScreen，alt_before/alt_after
        // 边沿不会触发；依赖字节字面匹配 ?47l 兜底。
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"\x1b[?47h\x1b[?1000h\x1b[?1006h");
        grid.process_output(b"\x1b[?47l");
        let snap = grid.snapshot(&[], None);
        assert!(!snap.mouse_report_click);
        assert!(!snap.sgr_mouse);
    }

    #[test]
    fn terminal_alt_screen_exit_via_1047l_clears_residual_mouse() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"\x1b[?1047h\x1b[?1000h\x1b[?1006h");
        grid.process_output(b"\x1b[?1047l");
        let snap = grid.snapshot(&[], None);
        assert!(!snap.mouse_report_click);
        assert!(!snap.sgr_mouse);
    }

    #[test]
    fn terminal_single_chunk_alt_roundtrip_clears_residual_mouse() {
        // 单 chunk 内 ?1049h…?1049l 抵消，alt_before==alt_after==false，
        // 边沿不变；字面匹配兜底应仍触发 reset。
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"\x1b[?1049h\x1b[?1000h\x1b[?1006h\x1b[?1049l");
        let snap = grid.snapshot(&[], None);
        assert!(!snap.input_modes.alt_screen);
        assert!(!snap.mouse_report_click);
        assert!(!snap.sgr_mouse);
    }

    #[test]
    fn terminal_alt_screen_exit_clears_focus_and_bracketed_paste() {
        // ?1004 / ?2004 残留同样会让 shell readline echo focus / paste 序列，
        // 应一并复位。
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"\x1b[?1049h\x1b[?1004h\x1b[?2004h");
        let snap = grid.snapshot(&[], None);
        assert!(snap.input_modes.focus_in_out);
        assert!(snap.bracketed_paste);

        grid.process_output(b"\x1b[?1049l");
        let snap = grid.snapshot(&[], None);
        assert!(!snap.input_modes.focus_in_out);
        assert!(!snap.bracketed_paste);
    }

    #[test]
    fn terminal_main_screen_mouse_tracking_is_not_cleared() {
        // 守门：主屏内合法启用 mouse tracking 的 TUI（less -X / mc 等）
        // 在没有 alt-screen 退出时绝不应被错误清掉。
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"\x1b[?1000h\x1b[?1006h");
        let snap = grid.snapshot(&[], None);
        assert!(!snap.input_modes.alt_screen);
        assert!(snap.mouse_report_click);
        assert!(snap.sgr_mouse);

        grid.process_output(b"hello");
        let snap = grid.snapshot(&[], None);
        assert!(snap.mouse_report_click);
        assert!(snap.sgr_mouse);
    }

    #[test]
    fn terminal_mouse_modes_handle_mirrors_live_term_state() {
        // UI 线程发鼠标报告前查这个原子镜像（而非渲染快照），TUI 退出瞬间
        // 必须立即读到"已关闭"，否则 \e[<35;x;yM 会漏进 shell 回显。
        use std::sync::atomic::Ordering;
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        let handle = grid.mouse_modes_handle();
        assert_eq!(handle.load(Ordering::Relaxed), 0);

        grid.process_output(b"\x1b[?1003h\x1b[?1006h");
        let bits = handle.load(Ordering::Relaxed);
        assert!(terminal_runtime::mouse_mode_bits_app_active(bits));
        assert!(terminal_runtime::mouse_mode_bits_motion_active(bits));

        grid.process_output(b"\x1b[?1003l\x1b[?1006l");
        assert!(!terminal_runtime::mouse_mode_bits_app_active(
            handle.load(Ordering::Relaxed)
        ));

        // 走 alt-screen 退出兜底（reset_leaked_tui_modes）也要同步镜像。
        grid.process_output(b"\x1b[?1049h\x1b[?1002h\x1b[?1006h");
        assert!(terminal_runtime::mouse_mode_bits_drag_active(
            handle.load(Ordering::Relaxed)
        ));
        grid.process_output(b"\x1b[?1049l");
        assert_eq!(handle.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn terminal_alt_scroll_bytes_follow_warp_escape_sequences() {
        let enabled_modes = terminal_runtime::TerminalInputModes {
            alt_screen: true,
            alternate_scroll: true,
            ..Default::default()
        };

        assert_eq!(
            terminal_runtime::terminal_alt_scroll_bytes(2, enabled_modes),
            Some(b"\x1bOA\x1bOA".to_vec())
        );
        assert_eq!(
            terminal_runtime::terminal_alt_scroll_bytes(-1, enabled_modes),
            Some(b"\x1bOB".to_vec())
        );
        assert_eq!(
            terminal_runtime::terminal_alt_scroll_bytes(0, enabled_modes),
            None
        );
        assert_eq!(
            terminal_runtime::terminal_alt_scroll_bytes(
                1,
                terminal_runtime::TerminalInputModes {
                    alt_screen: false,
                    alternate_scroll: true,
                    ..Default::default()
                }
            ),
            None
        );
    }

    #[test]
    fn terminal_event_response_bytes_forward_alacritty_pty_write() {
        assert_eq!(
            terminal_runtime::terminal_event_response_bytes(
                &alacritty_terminal::event::Event::PtyWrite("\x1b[?6c".to_string())
            ),
            Some(b"\x1b[?6c".to_vec())
        );
        assert_eq!(
            terminal_runtime::terminal_event_response_bytes(
                &alacritty_terminal::event::Event::Title("ignored".to_string())
            ),
            None
        );
    }

    #[test]
    fn terminal_grid_device_attributes_event_can_be_forwarded_to_pty() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"\x1b[c");

        let response = grid
            .drain_events()
            .iter()
            .find_map(terminal_runtime::terminal_event_response_bytes);

        assert_eq!(response, Some(b"\x1b[?6c".to_vec()));
    }

    #[test]
    fn terminal_clipboard_store_request_for_event_follows_warp_model_event() {
        assert_eq!(
            terminal_runtime::terminal_clipboard_store_request_for_event(
                &alacritty_terminal::event::Event::ClipboardStore(
                    alacritty_terminal::term::ClipboardType::Clipboard,
                    "copied from app".to_string(),
                )
            ),
            Some(terminal_runtime::TerminalClipboardStoreRequest {
                text: "copied from app".to_string(),
            })
        );
        assert_eq!(
            terminal_runtime::terminal_clipboard_store_request_for_event(
                &alacritty_terminal::event::Event::Title("ignored".to_string())
            ),
            None
        );
    }

    #[test]
    fn terminal_grid_osc52_clipboard_store_can_be_forwarded_to_view() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"\x1b]52;c;Y29waWVkIGZyb20gb3NjNTI=\x07");

        let request = grid
            .drain_events()
            .iter()
            .find_map(terminal_runtime::terminal_clipboard_store_request_for_event);

        assert_eq!(
            request,
            Some(terminal_runtime::TerminalClipboardStoreRequest {
                text: "copied from osc52".to_string(),
            })
        );
    }

    #[test]
    fn terminal_clipboard_load_request_for_event_formats_clipboard_response() {
        use std::sync::Arc;

        let event = alacritty_terminal::event::Event::ClipboardLoad(
            alacritty_terminal::term::ClipboardType::Clipboard,
            Arc::new(|text| format!("reply:{text}")),
        );
        let request = terminal_runtime::terminal_clipboard_load_request_for_event(&event)
            .expect("clipboard load event should become a request");

        assert_eq!(
            request.response_bytes("clipboard text"),
            b"reply:clipboard text".to_vec()
        );
        assert!(terminal_runtime::terminal_clipboard_load_request_for_event(
            &alacritty_terminal::event::Event::Title("ignored".to_string())
        )
        .is_none());
    }

    #[test]
    fn terminal_text_area_size_request_uses_current_cell_pixel_size() {
        use std::sync::Arc;

        let event =
            alacritty_terminal::event::Event::TextAreaSizeRequest(Arc::new(|window_size| {
                let height = window_size.num_lines * window_size.cell_height;
                let width = window_size.num_cols * window_size.cell_width;
                format!("\x1b[4;{height};{width}t")
            }));

        assert_eq!(
            terminal_runtime::terminal_event_response_bytes_with_window_size(
                &event,
                alacritty_terminal::event::WindowSize {
                    num_lines: 24,
                    num_cols: 80,
                    cell_width: 10,
                    cell_height: 20,
                },
                &terminal_runtime::TerminalPalette::default(),
            ),
            Some(b"\x1b[4;480;800t".to_vec())
        );
    }

    #[test]
    fn terminal_color_request_response_uses_current_palette() {
        use alacritty_terminal::vte::ansi::{NamedColor, Rgb};
        use std::sync::Arc;

        let window_size = alacritty_terminal::event::WindowSize {
            num_lines: 24,
            num_cols: 80,
            cell_width: 8,
            cell_height: 18,
        };
        let foreground = alacritty_terminal::event::Event::ColorRequest(
            NamedColor::Foreground as usize,
            Arc::new(|Rgb { r, g, b }| format!("rgb:{r:02x}/{g:02x}/{b:02x}")),
        );
        let background = alacritty_terminal::event::Event::ColorRequest(
            NamedColor::Background as usize,
            Arc::new(|Rgb { r, g, b }| format!("rgb:{r:02x}/{g:02x}/{b:02x}")),
        );

        let p = terminal_runtime::TerminalPalette::default();
        assert_eq!(
            terminal_runtime::terminal_event_response_bytes_with_window_size(
                &foreground,
                window_size,
                &p,
            ),
            Some(b"rgb:ff/ff/ff".to_vec())
        );
        assert_eq!(
            terminal_runtime::terminal_event_response_bytes_with_window_size(
                &background,
                window_size,
                &p,
            ),
            Some(b"rgb:00/00/00".to_vec())
        );
    }

    #[test]
    fn terminal_grid_osc_color_query_can_be_forwarded_to_pty() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"\x1b]10;?\x07");

        let p = terminal_runtime::TerminalPalette::default();
        let response = grid.drain_events().iter().find_map(|event| {
            terminal_runtime::terminal_event_response_bytes_with_window_size(
                event,
                alacritty_terminal::event::WindowSize {
                    num_lines: 24,
                    num_cols: 80,
                    cell_width: 8,
                    cell_height: 18,
                },
                &p,
            )
        });

        assert_eq!(response, Some(b"\x1b]10;rgb:ffff/ffff/ffff\x07".to_vec()));
    }

    #[test]
    fn terminal_render_rows_use_dynamic_osc_palette_colors() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        grid.process_output(b"\x1b]4;1;rgb:00/ff/00\x07\x1b[31mX");

        let snapshot = grid.snapshot(&[], None);
        let rows = terminal_runtime::terminal_render_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );

        assert_eq!(rows[0].cells[0].fg, 0x00ff00);
    }

    #[test]
    fn terminal_grid_osc_indexed_color_query_uses_dynamic_palette() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        grid.process_output(b"\x1b]4;1;rgb:00/ff/00\x07\x1b]4;1;?\x07");

        let p = terminal_runtime::TerminalPalette::default();
        let response = grid.drain_events().iter().find_map(|event| {
            grid.event_response_bytes_with_window_size(
                event,
                alacritty_terminal::event::WindowSize {
                    num_lines: 24,
                    num_cols: 80,
                    cell_width: 8,
                    cell_height: 18,
                },
                &p,
            )
        });

        assert_eq!(response, Some(b"\x1b]4;1;rgb:0000/ffff/0000\x07".to_vec()));
    }

    #[test]
    fn tab_activation_changes_only_active_terminal_surface() {
        let mut shell = ShellModel::nexshell_default();

        shell.activate_tab("fix-cursor");

        assert_eq!(shell.active_tab().id, "fix-cursor");
        assert_eq!(shell.terminal_pane_count(), 1);
        assert!(!shell.monitor_panel_in_first_spike());
    }

    #[test]
    fn warp_reference_plan_prioritizes_terminal_infrastructure_not_blocks() {
        use std::path::Path;

        let plan = warp_source_plan::first_pass_sources();

        assert!(plan
            .iter()
            .any(|source| source.path.ends_with("local_tty/event_loop.rs")));
        assert!(plan
            .iter()
            .any(|source| source.path.contains("flat_storage")));
        assert!(plan
            .iter()
            .any(|source| source.path.ends_with("grid_renderer.rs")));
        assert!(plan.iter().all(|source| !source.path.contains("/block")));
        assert!(plan.iter().all(|source| Path::new(source.path).exists()));
    }

    #[test]
    fn native_shell_layout_prioritizes_terminal_surface_without_monitor_panel() {
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });

        assert_eq!(layout.title_bar, layout::Rect::new(0, 0, 1200, 36));
        assert_eq!(layout.activity_rail, layout::Rect::new(0, 36, 70, 764));
        assert_eq!(layout.tab_bar, layout::Rect::new(70, 36, 1130, 52));
        assert_eq!(layout.terminal_host, layout::Rect::new(70, 88, 1130, 660));
        assert_eq!(layout.bottom_toolbar, layout::Rect::new(70, 748, 1130, 52));
        assert_eq!(layout.monitor_panel, None);
    }

    #[test]
    fn native_shell_layout_keeps_terminal_host_non_negative_on_tiny_windows() {
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 64,
            height: 120,
        });

        assert_eq!(layout.terminal_host.width, 0);
        assert_eq!(layout.terminal_host.height, 0);
        assert_eq!(layout.monitor_panel, None);
    }

    #[test]
    fn warp_ui_reference_plan_prioritizes_shell_chrome_not_terminal_blocks() {
        use std::path::Path;

        let plan = warp_ui_plan::first_pass_sources();

        assert!(plan
            .iter()
            .any(|source| source.path.ends_with("root_view.rs")));
        assert!(plan.iter().any(|source| source.path.ends_with("tab.rs")));
        assert!(plan
            .iter()
            .any(|source| source.path.ends_with("workspace/view/vertical_tabs.rs")));
        assert!(plan
            .iter()
            .any(|source| source.path.ends_with("pane_group/tree.rs")));
        assert!(plan.iter().all(|source| !source.path.contains("/block")));
        assert!(plan.iter().all(|source| Path::new(source.path).exists()));
    }

    #[test]
    fn shell_actions_update_focus_without_expanding_first_spike_scope() {
        let mut shell = ShellModel::nexshell_default();

        assert_eq!(
            actions::reduce(&mut shell, actions::ShellAction::ActivateTab("fix-cursor")),
            actions::ShellEffect::Handled
        );
        assert_eq!(shell.active_tab().id, "fix-cursor");

        assert_eq!(
            actions::reduce(&mut shell, actions::ShellAction::ActivateActivity("hosts")),
            actions::ShellEffect::Handled
        );
        assert_eq!(shell.active_activity().id, "hosts");

        assert_eq!(
            actions::reduce(&mut shell, actions::ShellAction::FocusTerminalPane(0)),
            actions::ShellEffect::Handled
        );
        assert_eq!(shell.focused_terminal_pane_index(), 0);
        assert!(!shell.monitor_panel_in_first_spike());
    }

    #[test]
    fn shell_actions_ignore_unknown_targets() {
        let mut shell = ShellModel::nexshell_default();
        let original = shell.clone();

        assert_eq!(
            actions::reduce(&mut shell, actions::ShellAction::ActivateTab("missing")),
            actions::ShellEffect::Noop
        );
        assert_eq!(
            actions::reduce(
                &mut shell,
                actions::ShellAction::ActivateActivity("missing")
            ),
            actions::ShellEffect::Noop
        );
        assert_eq!(
            actions::reduce(&mut shell, actions::ShellAction::FocusTerminalPane(4)),
            actions::ShellEffect::Noop
        );
        assert_eq!(shell, original);
    }

    #[test]
    fn view_projection_marks_active_shell_regions_for_native_adapters() {
        let shell = ShellModel::nexshell_default();
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });

        let view = view_model::project(&shell, layout);

        assert_eq!(view.window_title, "NexShell");
        assert_eq!(view.layout, layout);
        assert_eq!(view.activities.len(), 6);
        assert_eq!(
            view.activities
                .iter()
                .find(|item| item.id == "terminal")
                .map(|item| item.active),
            Some(true)
        );
        assert_eq!(
            view.tabs
                .iter()
                .find(|tab| tab.id == "sshtool")
                .map(|tab| (tab.active, tab.kind.clone())),
            Some((true, TabKind::Terminal))
        );
        assert_eq!(
            view.terminal_hosts,
            vec![view_model::TerminalHostView {
                index: 0,
                session_id: Some("sshtool"),
                rect: layout.terminal_host,
                visible: true,
                focused: true,
            }]
        );
        assert_eq!(view.bottom_tools.len(), 7);
        assert_eq!(view.bottom_tools[0].label, "批量执行");
        assert_eq!(view.monitor_panel, None);
    }

    #[test]
    fn view_projection_follows_reduced_shell_state() {
        let mut shell = ShellModel::nexshell_default();
        actions::reduce(&mut shell, actions::ShellAction::ActivateActivity("hosts"));
        actions::reduce(&mut shell, actions::ShellAction::ActivateTab("fix-cursor"));

        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 900,
            height: 600,
        });
        let view = view_model::project(&shell, layout);

        assert_eq!(
            view.activities
                .iter()
                .find(|item| item.id == "hosts")
                .map(|item| item.active),
            Some(true)
        );
        assert_eq!(
            view.tabs
                .iter()
                .find(|tab| tab.id == "fix-cursor")
                .map(|tab| tab.active),
            Some(true)
        );
        assert_eq!(view.terminal_hosts[0].visible, false);
        assert_eq!(view.terminal_hosts[0].session_id, Some("sshtool"));
    }

    // Host-management storage is intentionally disabled in the default
    // WarpUI spike build while rusqlite conflicts with Warp's sqlite stack.
    #[cfg(any())]
    #[test]
    fn host_management_snapshot_matches_legacy_page_structure() {
        let snapshot = host_management::legacy_host_management_snapshot();

        assert_eq!(snapshot.title, "主机管理");
        assert_eq!(snapshot.top_actions, ["隐私", "导入", "云同步", "新建主机"]);
        assert_eq!(snapshot.groups[0].label, "所有主机");
        assert_eq!(snapshot.groups[0].count, 10);
        assert!(snapshot.groups[0].selected);
        assert_eq!(snapshot.groups[1].label, "默认分组");
        assert_eq!(snapshot.search_placeholder, "搜索主机...");
        assert_eq!(snapshot.protocol_filter_label, "所有协议");
        assert_eq!(snapshot.available_tags, ["测试标签"]);
        assert_eq!(snapshot.hosts.len(), 10);
        assert_eq!(snapshot.hosts[0].name, "7省-X86");
        assert_eq!(snapshot.hosts[0].protocol, "SSH");
        assert_eq!(snapshot.hosts[0].endpoint, "root@192.168.248.120:22");
        assert!(snapshot.hosts.iter().any(|host| host.protocol == "Serial"));
    }

    #[cfg(any())]
    #[test]
    fn host_management_unavailable_snapshot_has_no_demo_hosts() {
        let snapshot = host_management::unavailable_host_management_snapshot();

        assert_eq!(snapshot.title, "主机管理");
        assert_eq!(snapshot.groups[0].label, "所有主机");
        assert_eq!(snapshot.groups[0].count, 0);
        assert!(snapshot.hosts.is_empty());
        assert!(snapshot.available_tags.is_empty());
    }

    #[cfg(any())]
    #[test]
    fn host_management_initializes_missing_database_as_empty_real_store() {
        let db_path = std::env::temp_dir().join(format!(
            "nexshell-native-hosts-init-{}.sqlite3",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db_path);

        let snapshot =
            host_management::load_or_initialize_host_management_snapshot_from_db_path(&db_path)
                .expect("initialize db");

        assert!(db_path.exists());
        assert!(snapshot.hosts.is_empty());
        assert_eq!(snapshot.groups[0].count, 0);

        let draft_id =
            host_management::create_draft_host_in_db_path(&db_path, None).expect("create draft");
        let snapshot =
            host_management::load_host_management_snapshot_from_db_path(&db_path).expect("reload");
        assert!(snapshot.hosts.iter().any(|host| host.id == draft_id));

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn host_sort_order_persists_and_determines_display_order() {
        let db_path = std::env::temp_dir().join(format!(
            "nexshell-sort-order-{}.sqlite3",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db_path);

        host_management::load_or_initialize_host_management_snapshot_from_db_path(&db_path)
            .expect("init");

        let id_a = host_management::create_draft_host_in_db_path(&db_path, None).expect("create a");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let id_b = host_management::create_draft_host_in_db_path(&db_path, None).expect("create b");

        let snap =
            host_management::load_host_management_snapshot_from_db_path(&db_path).expect("load");
        assert_eq!(snap.hosts[0].id, id_a);
        assert_eq!(snap.hosts[1].id, id_b);
        assert_eq!(snap.hosts[0].sort_order, 0);
        assert_eq!(snap.hosts[1].sort_order, 1);

        host_management::update_host_sort_orders(&db_path, &[(id_b.clone(), 0), (id_a.clone(), 1)])
            .expect("reorder");

        let snap =
            host_management::load_host_management_snapshot_from_db_path(&db_path).expect("reload");
        assert_eq!(snap.hosts[0].id, id_b);
        assert_eq!(snap.hosts[1].id, id_a);

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn host_management_all_group_and_protocol_filter_follow_current_locale() {
        rust_i18n::set_locale("zh-CN");
        let state = host_management::HostManagementState::new(
            host_management::unavailable_host_management_snapshot(),
        );

        rust_i18n::set_locale("en");

        assert_eq!(state.groups_for_render()[0].label, "All hosts");
        assert_eq!(
            host_management::ProtocolFilter::All.label(),
            "All protocols"
        );
    }

    #[cfg(any())]
    #[test]
    fn host_management_state_filters_by_query_group_tag_and_protocol() {
        let mut state = host_management::HostManagementState::new(
            host_management::legacy_host_management_snapshot(),
        );

        state.set_query("腾讯");
        assert_eq!(state.filtered_hosts().len(), 1);
        assert_eq!(state.filtered_hosts()[0].name, "腾讯云-北京");

        state.set_query("");
        state.select_group("default");
        assert_eq!(state.filtered_hosts().len(), 1);
        assert_eq!(state.filtered_hosts()[0].name, "7省-X86");

        state.select_group("all");
        state.toggle_tag("测试标签");
        assert_eq!(state.filtered_hosts().len(), 1);
        assert_eq!(state.filtered_hosts()[0].name, "腾讯云-北京");

        state.toggle_tag("测试标签");
        state.cycle_protocol_filter();
        assert_eq!(state.protocol_filter, host_management::ProtocolFilter::Ssh);
        assert!(state
            .filtered_hosts()
            .iter()
            .all(|host| host.protocol == "SSH"));

        state.cycle_protocol_filter();
        assert_eq!(
            state.protocol_filter,
            host_management::ProtocolFilter::Serial
        );
        assert_eq!(state.filtered_hosts()[0].name, "串口连接");
    }

    #[cfg(any())]
    #[test]
    fn host_management_state_selects_deletes_and_adds_local_hosts() {
        let mut state = host_management::HostManagementState::new(
            host_management::legacy_host_management_snapshot(),
        );

        state.set_query("公司");
        state.toggle_select_all_filtered();
        assert_eq!(state.selected_count(), 2);
        state.delete_selected();
        assert_eq!(state.selected_count(), 0);
        assert!(state
            .snapshot
            .hosts
            .iter()
            .all(|host| !host.name.starts_with("公司")));
        assert_eq!(state.notice.as_deref(), Some("已删除 2 台本地主机"));

        state.add_draft_host();
        assert!(state
            .snapshot
            .hosts
            .iter()
            .any(|host| host.id == "draft-host-1"));
        assert_eq!(state.notice.as_deref(), Some("已新增本地草稿主机"));
    }

    #[cfg(any())]
    #[test]
    fn host_management_state_search_input_and_view_toggles_are_stateful() {
        let mut state = host_management::HostManagementState::new(
            host_management::legacy_host_management_snapshot(),
        );

        state.push_search_text("syn");
        assert_eq!(state.query, "syn");
        assert_eq!(state.filtered_hosts()[0].name, "Syno");
        state.backspace_search();
        assert_eq!(state.query, "sy");
        state.clear_search();
        assert_eq!(state.filtered_hosts().len(), 10);

        assert_eq!(state.view_mode, host_management::HostViewMode::Grid);
        state.set_view_mode(host_management::HostViewMode::List);
        assert_eq!(state.view_mode, host_management::HostViewMode::List);

        assert!(!state.privacy_mode);
        state.toggle_privacy_mode();
        assert!(state.privacy_mode);
    }

    #[cfg(any())]
    #[test]
    fn host_management_state_builds_ssh_command_for_quick_connect() {
        let state = host_management::HostManagementState::new(
            host_management::legacy_host_management_snapshot(),
        );

        assert_eq!(
            state.connect_command_for("seven-province-x86").as_deref(),
            Some(b"ssh -p 22 root@192.168.248.120\r".as_slice())
        );
        assert_eq!(state.connect_command_for("serial"), None);
    }

    #[cfg(any())]
    #[test]
    fn host_management_state_builds_direct_pty_plan_for_quick_connect() {
        let state = host_management::HostManagementState::new(
            host_management::legacy_host_management_snapshot(),
        );

        assert_eq!(
            state.connection_plan_for("seven-province-x86"),
            Some(host_management::HostConnectionPlan::DirectPty {
                session_id: "host-seven-province-x86".to_string(),
                title: "7省-X86".to_string(),
                command: host_management::PtyCommandSpec {
                    program: "ssh".to_string(),
                    args: vec![
                        "-p".to_string(),
                        "22".to_string(),
                        "root@192.168.248.120".to_string(),
                    ],
                    status: "connecting SSH: root@192.168.248.120:22".to_string(),
                },
            })
        );

        let serial = state
            .connection_plan_for("serial")
            .expect("serial should produce a direct pty plan");
        assert!(matches!(
            serial,
            host_management::HostConnectionPlan::DirectPty { .. }
        ));
    }

    #[test]
    fn host_management_serial_plan_uses_native_serial_transport() {
        let state = host_management::HostManagementState::new(
            host_management::legacy_host_management_snapshot(),
        );

        let host_management::HostConnectionPlan::Serial { config, .. } = state
            .connection_plan_for("serial")
            .expect("serial should produce a native serial plan")
        else {
            panic!("serial should produce a native serial plan");
        };

        assert_eq!(
            config.serial_port.as_deref(),
            Some("/dev/cu.Bluetooth-Incoming-Port")
        );
        assert_eq!(config.serial_baud_rate, 115_200);
    }

    #[test]
    fn ssh_keyref_only_host_builds_saved_ssh_plan() {
        // 回归：仅 key_id 引用（无内联私钥）的 SSH 主机应得 SavedSsh，
        // 而非被门禁误判 Unsupported（私钥本体在连接时按 key_id 取库）。
        let mut connection = host_management::HostConnectionConfig::ssh("10.0.0.1", 22, "root");
        connection.auth_method = "key".to_string();
        connection.private_key = None;
        connection.key_id = Some("sshkey-1".to_string());

        let snapshot = host_management::HostManagementSnapshot {
            title: "",
            top_actions: ["", "", "", ""],
            groups: vec![],
            search_placeholder: "",
            protocol_filter_label: "",
            available_tags: vec![],
            hosts: vec![host_management::HostCardSnapshot {
                id: "keyref".to_string(),
                name: "keyref".to_string(),
                protocol: "SSH".to_string(),
                endpoint: "10.0.0.1:22".to_string(),
                description: String::new(),
                connection,
                group_id: None,
                tags: vec![],
                system: host_management::HostSystemIcon::Terminal,
                sort_order: 0,
            }],
        };

        let state = host_management::HostManagementState::new(snapshot);
        assert!(matches!(
            state.connection_plan_for("keyref"),
            Some(host_management::HostConnectionPlan::SavedSsh { .. })
        ));
    }

    #[cfg(any())]
    #[test]
    fn host_management_snapshot_can_load_real_tauri_host_database() {
        let db_path = std::env::temp_dir().join(format!(
            "nexshell-native-hosts-{}.sqlite3",
            std::process::id()
        ));
        let conn = rusqlite::Connection::open(&db_path).expect("open temp db");
        conn.execute_batch(
            r#"
            CREATE TABLE groups (id TEXT PRIMARY KEY, name TEXT NOT NULL, sort_order INTEGER NOT NULL);
            CREATE TABLE hosts (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              description TEXT NOT NULL,
              host TEXT NOT NULL,
              port INTEGER NOT NULL,
              username TEXT NOT NULL,
              protocol TEXT NOT NULL,
              serial_port TEXT,
              serial_baud_rate INTEGER NOT NULL,
              group_id TEXT,
              tags TEXT NOT NULL,
              created_at INTEGER NOT NULL
            );
            INSERT INTO groups (id, name, sort_order) VALUES ('g1', '生产环境', 1);
            INSERT INTO hosts (
              id, name, description, host, port, username, protocol,
              serial_port, serial_baud_rate, group_id, tags, created_at
            ) VALUES (
              'h1', 'prod-a', '', '10.0.0.2', 2222, 'root', 'ssh',
              NULL, 115200, 'g1', '["prod"]', 10
            );
            "#,
        )
        .expect("seed temp db");
        drop(conn);

        let snapshot =
            host_management::load_host_management_snapshot_from_db_path(&db_path).expect("load db");

        assert_eq!(snapshot.groups[1].label, "生产环境");
        assert_eq!(snapshot.groups[1].count, 1);
        assert_eq!(snapshot.available_tags, ["prod"]);
        assert_eq!(snapshot.hosts[0].name, "prod-a");
        assert_eq!(snapshot.hosts[0].endpoint, "root@10.0.0.2:2222");

        let _ = std::fs::remove_file(db_path);
    }

    #[cfg(any())]
    #[test]
    fn host_management_persists_draft_create_and_selected_delete_to_database() {
        let db_path = std::env::temp_dir().join(format!(
            "nexshell-native-hosts-write-{}.sqlite3",
            std::process::id()
        ));
        let conn = rusqlite::Connection::open(&db_path).expect("open temp db");
        conn.execute_batch(
            r#"
            CREATE TABLE groups (id TEXT PRIMARY KEY, name TEXT NOT NULL, sort_order INTEGER NOT NULL);
            CREATE TABLE hosts (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              description TEXT NOT NULL DEFAULT '',
              host TEXT NOT NULL,
              port INTEGER NOT NULL DEFAULT 22,
              username TEXT NOT NULL DEFAULT 'root',
              protocol TEXT NOT NULL DEFAULT 'ssh',
              auth_method TEXT NOT NULL DEFAULT 'password',
              password TEXT,
              private_key TEXT,
              key_passphrase TEXT,
              ca_cert TEXT,
              serial_port TEXT,
              serial_baud_rate INTEGER NOT NULL DEFAULT 115200,
              serial_data_bits INTEGER NOT NULL DEFAULT 8,
              serial_stop_bits INTEGER NOT NULL DEFAULT 1,
              serial_parity TEXT NOT NULL DEFAULT 'none',
              serial_flow_control TEXT NOT NULL DEFAULT 'none',
              serial_dtr INTEGER NOT NULL DEFAULT 0,
              serial_rts INTEGER NOT NULL DEFAULT 0,
              group_id TEXT,
              keep_alive_enabled INTEGER NOT NULL DEFAULT 1,
              keep_alive_interval INTEGER NOT NULL DEFAULT 30,
              keep_alive_max_failures INTEGER NOT NULL DEFAULT 3,
              tcp_connect_timeout INTEGER NOT NULL DEFAULT 15,
              auth_timeout INTEGER NOT NULL DEFAULT 30,
              term_encoding TEXT NOT NULL DEFAULT 'utf-8',
              tags TEXT NOT NULL DEFAULT '[]',
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL
            );
            INSERT INTO groups (id, name, sort_order) VALUES ('default', '默认分组', 0);
            INSERT INTO hosts (
              id, name, description, host, port, username, protocol, auth_method,
              serial_baud_rate, serial_data_bits, serial_stop_bits, serial_parity,
              serial_flow_control, serial_dtr, serial_rts, group_id, tags, created_at, updated_at
            ) VALUES (
              'h1', 'prod-a', '', '10.0.0.2', 22, 'root', 'ssh', 'password',
              115200, 8, 1, 'none', 'none', 0, 0, 'default', '[]', 10, 10
            );
            "#,
        )
        .expect("seed temp db");
        drop(conn);

        let draft_id = host_management::create_draft_host_in_db_path(&db_path, Some("default"))
            .expect("create draft host");
        let snapshot =
            host_management::load_host_management_snapshot_from_db_path(&db_path).expect("reload");
        assert!(snapshot.hosts.iter().any(|host| host.id == draft_id));

        let mut ids = std::collections::BTreeSet::new();
        ids.insert("h1".to_string());
        ids.insert(draft_id.clone());
        let deleted = host_management::delete_hosts_from_db_path(&db_path, &ids)
            .expect("delete selected hosts");
        assert_eq!(deleted, 2);

        let snapshot =
            host_management::load_host_management_snapshot_from_db_path(&db_path).expect("reload");
        assert!(snapshot.hosts.is_empty());

        let _ = std::fs::remove_file(db_path);
    }

    #[cfg(any())]
    #[test]
    fn host_management_updates_basic_host_fields_in_database() {
        let db_path = std::env::temp_dir().join(format!(
            "nexshell-native-hosts-edit-{}.sqlite3",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db_path);
        host_management::load_or_initialize_host_management_snapshot_from_db_path(&db_path)
            .expect("initialize db");
        let id =
            host_management::create_draft_host_in_db_path(&db_path, None).expect("create draft");

        host_management::update_host_basic_in_db_path(
            &db_path,
            &host_management::HostEditDraft {
                id: id.clone(),
                name: "prod-edited".to_string(),
                host: "10.1.2.3".to_string(),
                port: 2222,
                username: "deploy".to_string(),
                description: "edited from native shell".to_string(),
            },
        )
        .expect("update host");

        let snapshot =
            host_management::load_host_management_snapshot_from_db_path(&db_path).expect("reload");
        let host = snapshot.hosts.iter().find(|host| host.id == id).unwrap();
        assert_eq!(host.name, "prod-edited");
        assert_eq!(host.endpoint, "deploy@10.1.2.3:2222");
        assert_eq!(host.description, "edited from native shell");

        let draft = host_management::HostEditDraft::from_card(host).expect("edit draft");
        assert_eq!(draft.host, "10.1.2.3");
        assert_eq!(draft.port, 2222);
        assert_eq!(draft.username, "deploy");

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn view_projection_hides_terminal_mount_when_hosts_activity_is_active() {
        let mut shell = ShellModel::nexshell_default();
        actions::reduce(&mut shell, actions::ShellAction::ActivateActivity("hosts"));

        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 900,
            height: 600,
        });
        let view = view_model::project(&shell, layout);

        assert_eq!(view.terminal_hosts[0].visible, false);
        assert_eq!(view.terminal_hosts[0].session_id, Some("sshtool"));
    }

    #[test]
    fn terminal_mounts_target_existing_window_wgpu_surface() {
        let shell = ShellModel::nexshell_default();
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });
        let view = view_model::project(&shell, layout);

        let mounts = terminal_mount::mounts_for(&view);

        assert_eq!(
            mounts,
            vec![terminal_mount::TerminalMount {
                pane_index: 0,
                session_id: Some("sshtool"),
                rect: layout.terminal_host,
                visible: true,
                focused: true,
                backend: terminal_mount::TerminalBackend::ExistingWindowWgpuSurface,
            }]
        );
    }

    #[test]
    fn terminal_mounts_keep_inactive_terminal_hosts_detached_not_destroyed() {
        let mut shell = ShellModel::nexshell_default();
        actions::reduce(&mut shell, actions::ShellAction::ActivateTab("fix-cursor"));
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });
        let view = view_model::project(&shell, layout);

        let mounts = terminal_mount::mounts_for(&view);

        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].session_id, Some("sshtool"));
        assert_eq!(mounts[0].visible, false);
        assert_eq!(mounts[0].focused, true);
        assert_eq!(
            mounts[0].backend,
            terminal_mount::TerminalBackend::ExistingWindowWgpuSurface
        );
    }

    #[test]
    fn renderer_ipc_plan_attaches_visible_terminal_mount() {
        let shell = ShellModel::nexshell_default();
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });
        let view = view_model::project(&shell, layout);
        let mounts = terminal_mount::mounts_for(&view);

        let commands = renderer_ipc::plan(&mounts);

        assert_eq!(
            commands,
            vec![
                renderer_ipc::RendererIpcCommand::StartRender {
                    session_id: "sshtool"
                },
                renderer_ipc::RendererIpcCommand::SetViewport {
                    session_id: "sshtool",
                    rect: layout.terminal_host,
                },
                renderer_ipc::RendererIpcCommand::SetFocused {
                    session_id: "sshtool",
                    focused: true,
                },
            ]
        );
    }

    #[test]
    fn renderer_ipc_plan_detaches_invisible_terminal_mount_without_closing_term_core() {
        let mut shell = ShellModel::nexshell_default();
        actions::reduce(&mut shell, actions::ShellAction::ActivateTab("fix-cursor"));
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });
        let view = view_model::project(&shell, layout);
        let mounts = terminal_mount::mounts_for(&view);

        let commands = renderer_ipc::plan(&mounts);

        assert_eq!(
            commands,
            vec![renderer_ipc::RendererIpcCommand::StopRender {
                session_id: "sshtool"
            }]
        );
    }

    #[test]
    fn renderer_ipc_plan_skips_mounts_without_session_id() {
        let commands = renderer_ipc::plan(&[terminal_mount::TerminalMount {
            pane_index: 0,
            session_id: None,
            rect: layout::Rect::new(70, 88, 1130, 660),
            visible: true,
            focused: true,
            backend: terminal_mount::TerminalBackend::ExistingWindowWgpuSurface,
        }]);

        assert_eq!(commands, Vec::<renderer_ipc::RendererIpcCommand>::new());
    }

    #[test]
    fn renderer_ipc_commands_map_to_existing_tauri_bridge_names() {
        use renderer_ipc::RendererIpcCommand;

        assert_eq!(
            RendererIpcCommand::StartRender {
                session_id: "sshtool"
            }
            .tauri_command_name(),
            "terminal_start_render"
        );
        assert_eq!(
            RendererIpcCommand::StopRender {
                session_id: "sshtool"
            }
            .tauri_command_name(),
            "terminal_stop_render"
        );
        assert_eq!(
            RendererIpcCommand::SetViewport {
                session_id: "sshtool",
                rect: layout::Rect::new(70, 88, 1130, 660),
            }
            .tauri_command_name(),
            "terminal_set_viewport"
        );
        assert_eq!(
            RendererIpcCommand::SetFocused {
                session_id: "sshtool",
                focused: true,
            }
            .tauri_command_name(),
            "terminal_set_focused"
        );
    }

    #[test]
    fn renderer_ipc_diff_skips_unchanged_visible_mount() {
        let mount = terminal_mount::TerminalMount {
            pane_index: 0,
            session_id: Some("sshtool"),
            rect: layout::Rect::new(70, 88, 1130, 660),
            visible: true,
            focused: true,
            backend: terminal_mount::TerminalBackend::ExistingWindowWgpuSurface,
        };

        assert_eq!(
            renderer_ipc::diff_plan(&[mount], &[mount]),
            Vec::<renderer_ipc::RendererIpcCommand>::new()
        );
    }

    #[test]
    fn renderer_ipc_diff_updates_viewport_and_focus_without_restart() {
        let previous = terminal_mount::TerminalMount {
            pane_index: 0,
            session_id: Some("sshtool"),
            rect: layout::Rect::new(70, 88, 1130, 660),
            visible: true,
            focused: false,
            backend: terminal_mount::TerminalBackend::ExistingWindowWgpuSurface,
        };
        let current = terminal_mount::TerminalMount {
            pane_index: 0,
            session_id: Some("sshtool"),
            rect: layout::Rect::new(70, 88, 1100, 628),
            visible: true,
            focused: true,
            backend: terminal_mount::TerminalBackend::ExistingWindowWgpuSurface,
        };

        assert_eq!(
            renderer_ipc::diff_plan(&[previous], &[current]),
            vec![
                renderer_ipc::RendererIpcCommand::SetViewport {
                    session_id: "sshtool",
                    rect: current.rect,
                },
                renderer_ipc::RendererIpcCommand::SetFocused {
                    session_id: "sshtool",
                    focused: true,
                },
            ]
        );
    }

    #[test]
    fn renderer_ipc_diff_stops_render_when_mount_becomes_invisible() {
        let previous = terminal_mount::TerminalMount {
            pane_index: 0,
            session_id: Some("sshtool"),
            rect: layout::Rect::new(70, 88, 1130, 660),
            visible: true,
            focused: true,
            backend: terminal_mount::TerminalBackend::ExistingWindowWgpuSurface,
        };
        let current = terminal_mount::TerminalMount {
            visible: false,
            ..previous
        };

        assert_eq!(
            renderer_ipc::diff_plan(&[previous], &[current]),
            vec![renderer_ipc::RendererIpcCommand::StopRender {
                session_id: "sshtool",
            }]
        );
    }

    #[test]
    fn renderer_ipc_diff_does_not_stop_new_invisible_mount() {
        let current = terminal_mount::TerminalMount {
            pane_index: 0,
            session_id: Some("sshtool"),
            rect: layout::Rect::new(70, 88, 1130, 660),
            visible: false,
            focused: true,
            backend: terminal_mount::TerminalBackend::ExistingWindowWgpuSurface,
        };

        assert_eq!(
            renderer_ipc::diff_plan(&[], &[current]),
            Vec::<renderer_ipc::RendererIpcCommand>::new()
        );
    }

    #[test]
    fn native_adapter_invisible_first_transition_initializes_without_detach() {
        let mut shell = ShellModel::nexshell_default();
        actions::reduce(&mut shell, actions::ShellAction::ActivateTab("fix-cursor"));
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });
        let view = view_model::project(&shell, layout);
        let config = native_adapter::NativeAdapterConfig::default_for_surface(
            terminal_lifecycle::SurfaceSize {
                width: 1200,
                height: 800,
            },
        );

        let plan = native_adapter::plan_transition(
            &native_adapter::NativeAdapterState::default(),
            &view,
            &config,
        );

        assert!(plan.lifecycle.iter().any(|command| matches!(
            command,
            terminal_lifecycle::TerminalLifecycleCommand::Create {
                session_id: "sshtool",
                ..
            }
        )));
        assert_eq!(
            plan.renderer,
            Vec::<renderer_ipc::RendererIpcCommand>::new()
        );
    }

    #[test]
    fn terminal_lifecycle_initial_plan_matches_current_native_terminal_bootstrap() {
        let rect = layout::Rect::new(70, 88, 1130, 660);
        let config = terminal_lifecycle::TerminalLifecycleConfig {
            session_id: "sshtool",
            rect,
            surface: terminal_lifecycle::SurfaceSize {
                width: 1200,
                height: 800,
            },
            font: terminal_lifecycle::FontSpec {
                family: "JetBrains Mono",
                size: 10.0,
                letter_spacing: 0.0,
                line_height: 2.0,
                dpr: 2.0,
            },
            scrollback_lines: 10_000,
            is_local: true,
            cursor_style: "block",
            sync_highlight_rules: true,
        };

        let commands = terminal_lifecycle::initial_plan(&config);

        assert_eq!(
            commands,
            vec![
                terminal_lifecycle::TerminalLifecycleCommand::UpdateTheme {
                    session_id: "sshtool",
                },
                terminal_lifecycle::TerminalLifecycleCommand::Create {
                    session_id: "sshtool",
                    cols: 185,
                    rows: 33,
                    cell_width: 6.0,
                    cell_height: 20.0,
                    scrollback_lines: 10_000,
                    is_local: true,
                },
                terminal_lifecycle::TerminalLifecycleCommand::SetCursorStyle {
                    session_id: "sshtool",
                    style: "block",
                },
                terminal_lifecycle::TerminalLifecycleCommand::UpdateHighlightRules {
                    session_id: "sshtool",
                },
                terminal_lifecycle::TerminalLifecycleCommand::UpdateFont {
                    session_id: "sshtool",
                    font: config.font,
                },
                terminal_lifecycle::TerminalLifecycleCommand::ResizeSurface {
                    session_id: "sshtool",
                    surface: config.surface,
                },
            ]
        );
    }

    #[test]
    fn terminal_lifecycle_resize_plan_uses_authoritative_cell_metrics() {
        let commands = terminal_lifecycle::resize_plan(
            "sshtool",
            layout::Rect::new(70, 88, 1130, 660),
            terminal_lifecycle::SurfaceSize {
                width: 1200,
                height: 800,
            },
            terminal_lifecycle::CellMetrics {
                width: 7.0,
                height: 22.0,
            },
        );

        assert_eq!(
            commands,
            vec![
                terminal_lifecycle::TerminalLifecycleCommand::Resize {
                    session_id: "sshtool",
                    cols: 159,
                    rows: 30,
                    cell_width: 7.0,
                    cell_height: 22.0,
                },
                terminal_lifecycle::TerminalLifecycleCommand::ResizeSurface {
                    session_id: "sshtool",
                    surface: terminal_lifecycle::SurfaceSize {
                        width: 1200,
                        height: 800,
                    },
                },
            ]
        );
    }

    #[test]
    fn terminal_lifecycle_commands_map_to_existing_tauri_bridge_names() {
        use terminal_lifecycle::TerminalLifecycleCommand;

        let names = [
            TerminalLifecycleCommand::UpdateTheme {
                session_id: "sshtool",
            },
            TerminalLifecycleCommand::Create {
                session_id: "sshtool",
                cols: 80,
                rows: 24,
                cell_width: 8.0,
                cell_height: 16.0,
                scrollback_lines: 10_000,
                is_local: true,
            },
            TerminalLifecycleCommand::SetCursorStyle {
                session_id: "sshtool",
                style: "block",
            },
            TerminalLifecycleCommand::UpdateHighlightRules {
                session_id: "sshtool",
            },
            TerminalLifecycleCommand::UpdateFont {
                session_id: "sshtool",
                font: terminal_lifecycle::FontSpec {
                    family: "JetBrains Mono",
                    size: 10.0,
                    letter_spacing: 0.0,
                    line_height: 2.0,
                    dpr: 2.0,
                },
            },
            TerminalLifecycleCommand::Resize {
                session_id: "sshtool",
                cols: 80,
                rows: 24,
                cell_width: 8.0,
                cell_height: 16.0,
            },
            TerminalLifecycleCommand::ResizeSurface {
                session_id: "sshtool",
                surface: terminal_lifecycle::SurfaceSize {
                    width: 1200,
                    height: 800,
                },
            },
        ]
        .map(|command| command.tauri_command_name());

        assert_eq!(
            names,
            [
                "terminal_update_theme",
                "terminal_create",
                "terminal_set_cursor_style",
                "terminal_update_highlight_rules",
                "terminal_update_font",
                "terminal_resize",
                "terminal_resize_surface",
            ]
        );
    }

    #[test]
    fn native_adapter_first_transition_initializes_and_attaches_terminal() {
        let shell = ShellModel::nexshell_default();
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });
        let view = view_model::project(&shell, layout);
        let config = native_adapter::NativeAdapterConfig {
            surface: terminal_lifecycle::SurfaceSize {
                width: 1200,
                height: 800,
            },
            font: terminal_lifecycle::FontSpec {
                family: "JetBrains Mono",
                size: 10.0,
                letter_spacing: 0.0,
                line_height: 2.0,
                dpr: 2.0,
            },
            cell: terminal_lifecycle::CellMetrics {
                width: 7.0,
                height: 22.0,
            },
            scrollback_lines: 10_000,
            is_local: true,
            cursor_style: "block",
            sync_highlight_rules: true,
        };

        let plan = native_adapter::plan_transition(
            &native_adapter::NativeAdapterState::default(),
            &view,
            &config,
        );

        assert!(plan.lifecycle.iter().any(|command| matches!(
            command,
            terminal_lifecycle::TerminalLifecycleCommand::Create {
                session_id: "sshtool",
                ..
            }
        )));
        assert_eq!(
            plan.renderer,
            vec![
                renderer_ipc::RendererIpcCommand::StartRender {
                    session_id: "sshtool"
                },
                renderer_ipc::RendererIpcCommand::SetViewport {
                    session_id: "sshtool",
                    rect: layout.terminal_host,
                },
                renderer_ipc::RendererIpcCommand::SetFocused {
                    session_id: "sshtool",
                    focused: true,
                },
            ]
        );
        assert_eq!(plan.next_state.initialized_sessions, vec!["sshtool"]);
        assert_eq!(plan.next_state.mounts, terminal_mount::mounts_for(&view));
    }

    #[test]
    fn native_adapter_unchanged_transition_emits_no_ipc() {
        let shell = ShellModel::nexshell_default();
        let layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });
        let view = view_model::project(&shell, layout);
        let config = native_adapter::NativeAdapterConfig::default_for_surface(
            terminal_lifecycle::SurfaceSize {
                width: 1200,
                height: 800,
            },
        );
        let first = native_adapter::plan_transition(
            &native_adapter::NativeAdapterState::default(),
            &view,
            &config,
        );

        let second = native_adapter::plan_transition(&first.next_state, &view, &config);

        assert_eq!(
            second.lifecycle,
            Vec::<terminal_lifecycle::TerminalLifecycleCommand>::new()
        );
        assert_eq!(
            second.renderer,
            Vec::<renderer_ipc::RendererIpcCommand>::new()
        );
        assert_eq!(second.next_state, first.next_state);
    }

    #[test]
    fn native_adapter_resize_transition_resizes_term_core_and_updates_viewport() {
        let shell = ShellModel::nexshell_default();
        let previous_layout = layout::ShellLayout::for_window(layout::Size {
            width: 1200,
            height: 800,
        });
        let next_layout = layout::ShellLayout::for_window(layout::Size {
            width: 1000,
            height: 720,
        });
        let previous_view = view_model::project(&shell, previous_layout);
        let next_view = view_model::project(&shell, next_layout);
        let first_config = native_adapter::NativeAdapterConfig::default_for_surface(
            terminal_lifecycle::SurfaceSize {
                width: 1200,
                height: 800,
            },
        );
        let next_config = native_adapter::NativeAdapterConfig::default_for_surface(
            terminal_lifecycle::SurfaceSize {
                width: 1000,
                height: 720,
            },
        );
        let first = native_adapter::plan_transition(
            &native_adapter::NativeAdapterState::default(),
            &previous_view,
            &first_config,
        );

        let resized = native_adapter::plan_transition(&first.next_state, &next_view, &next_config);

        assert_eq!(
            resized.lifecycle,
            terminal_lifecycle::resize_plan(
                "sshtool",
                next_layout.terminal_host,
                next_config.surface,
                next_config.cell,
            )
        );
        assert_eq!(
            resized.renderer,
            vec![renderer_ipc::RendererIpcCommand::SetViewport {
                session_id: "sshtool",
                rect: next_layout.terminal_host,
            }]
        );
    }

    #[test]
    fn native_shell_host_first_frame_projects_shell_and_generates_ipc() {
        let mut host = native_shell_host::NativeShellHost::new(ShellModel::nexshell_default());

        let frame = host.render_frame(layout::Size {
            width: 1200,
            height: 800,
        });

        assert_eq!(frame.view.window_title, "NexShell");
        assert_eq!(
            frame.view.layout.terminal_host,
            layout::Rect::new(70, 88, 1130, 660)
        );
        assert_eq!(
            frame.view.terminal_hosts,
            vec![view_model::TerminalHostView {
                index: 0,
                session_id: Some("sshtool"),
                rect: frame.view.layout.terminal_host,
                visible: true,
                focused: true,
            }]
        );
        assert!(frame.plan.lifecycle.iter().any(|command| matches!(
            command,
            terminal_lifecycle::TerminalLifecycleCommand::Create {
                session_id: "sshtool",
                ..
            }
        )));
        assert_eq!(
            frame.renderer_command_names(),
            vec![
                "terminal_start_render",
                "terminal_set_viewport",
                "terminal_set_focused"
            ]
        );
    }

    #[test]
    fn native_shell_host_advances_adapter_state_across_frames_and_actions() {
        let mut host = native_shell_host::NativeShellHost::new(ShellModel::nexshell_default());
        let size = layout::Size {
            width: 1200,
            height: 800,
        };

        host.render_frame(size);
        let unchanged = host.render_frame(size);
        assert_eq!(
            unchanged.plan.lifecycle,
            Vec::<terminal_lifecycle::TerminalLifecycleCommand>::new()
        );
        assert_eq!(
            unchanged.plan.renderer,
            Vec::<renderer_ipc::RendererIpcCommand>::new()
        );

        assert_eq!(
            host.dispatch(actions::ShellAction::ActivateTab("fix-cursor")),
            actions::ShellEffect::Handled
        );
        let inactive_terminal = host.render_frame(size);

        assert_eq!(inactive_terminal.view.terminal_hosts[0].visible, false);
        assert_eq!(
            inactive_terminal.plan.lifecycle,
            Vec::<terminal_lifecycle::TerminalLifecycleCommand>::new()
        );
        assert_eq!(
            inactive_terminal.plan.renderer,
            vec![renderer_ipc::RendererIpcCommand::StopRender {
                session_id: "sshtool",
            }]
        );
    }

    #[test]
    fn native_shell_host_first_frame_exposes_tauri_invoke_batch() {
        let mut host = native_shell_host::NativeShellHost::new(ShellModel::nexshell_default());

        let frame = host.render_frame(layout::Size {
            width: 1200,
            height: 800,
        });

        assert_eq!(
            frame.ipc_command_names(),
            vec![
                "terminal_update_theme",
                "terminal_create",
                "terminal_set_cursor_style",
                "terminal_update_highlight_rules",
                "terminal_update_font",
                "terminal_resize_surface",
                "terminal_start_render",
                "terminal_set_viewport",
                "terminal_set_focused",
            ]
        );
        assert_eq!(
            frame.ipc.find("terminal_create"),
            Some(&ipc_dispatcher::IpcCall {
                command: "terminal_create",
                args: vec![
                    ipc_dispatcher::IpcArg::string("sessionId", "sshtool"),
                    ipc_dispatcher::IpcArg::usize("cols", 185),
                    ipc_dispatcher::IpcArg::usize("rows", 33),
                    ipc_dispatcher::IpcArg::f32("cellWidth", 6.0),
                    ipc_dispatcher::IpcArg::f32("cellHeight", 20.0),
                    ipc_dispatcher::IpcArg::usize("scrollback", 10_000),
                    ipc_dispatcher::IpcArg::bool("isLocal", true),
                ],
            })
        );
        assert_eq!(
            frame.ipc.find("terminal_start_render"),
            Some(&ipc_dispatcher::IpcCall {
                command: "terminal_start_render",
                args: vec![
                    ipc_dispatcher::IpcArg::string("sessionId", "sshtool"),
                    ipc_dispatcher::IpcArg::string("fontFamily", "JetBrains Mono"),
                    ipc_dispatcher::IpcArg::f32("fontSize", 10.0),
                    ipc_dispatcher::IpcArg::f32("letterSpacing", 0.0),
                    ipc_dispatcher::IpcArg::f32("lineHeight", 2.0),
                    ipc_dispatcher::IpcArg::f64("dpr", 2.0),
                ],
            })
        );
        assert_eq!(
            frame.ipc.find("terminal_set_viewport"),
            Some(&ipc_dispatcher::IpcCall {
                command: "terminal_set_viewport",
                args: vec![
                    ipc_dispatcher::IpcArg::string("sessionId", "sshtool"),
                    ipc_dispatcher::IpcArg::f32("x", 70.0),
                    ipc_dispatcher::IpcArg::f32("y", 88.0),
                    ipc_dispatcher::IpcArg::f32("width", 1130.0),
                    ipc_dispatcher::IpcArg::f32("height", 660.0),
                ],
            })
        );
    }

    #[test]
    fn native_shell_host_inactive_terminal_frame_exposes_stop_render_batch() {
        let mut host = native_shell_host::NativeShellHost::new(ShellModel::nexshell_default());
        let size = layout::Size {
            width: 1200,
            height: 800,
        };

        host.render_frame(size);
        host.dispatch(actions::ShellAction::ActivateTab("fix-cursor"));
        let frame = host.render_frame(size);

        assert_eq!(frame.ipc_command_names(), vec!["terminal_stop_render"]);
        assert_eq!(
            frame.ipc.calls,
            vec![ipc_dispatcher::IpcCall {
                command: "terminal_stop_render",
                args: vec![ipc_dispatcher::IpcArg::string("sessionId", "sshtool")],
            }]
        );
    }

    #[test]
    fn ipc_dispatcher_resolves_runtime_payloads_and_invokes_in_order() {
        let mut host = native_shell_host::NativeShellHost::new(ShellModel::nexshell_default());
        let frame = host.render_frame(layout::Size {
            width: 1200,
            height: 800,
        });
        let runtime = ipc_dispatcher::IpcRuntimeInputs {
            theme_json: serde_json::json!({ "background": "#0b0c0f" }),
            highlight_rules: serde_json::json!([
                { "id": "ip", "pattern": "\\d+\\.\\d+\\.\\d+\\.\\d+", "color": "#58a6ff" }
            ]),
            highlight_perf: serde_json::json!({ "maxLineLength": 1200 }),
        };
        let mut invoker = RecordingInvoker::default();

        let report = ipc_dispatcher::dispatch_batch(&frame.ipc, &runtime, &mut invoker)
            .expect("dispatch should succeed");

        assert_eq!(report.invoked, frame.ipc_command_names());
        assert_eq!(invoker.calls.len(), frame.ipc.calls.len());
        let theme = invoker
            .calls
            .iter()
            .find(|call| call.command == "terminal_update_theme")
            .expect("theme call should be dispatched");
        assert_eq!(theme.args["sessionId"], serde_json::json!("sshtool"));
        assert_eq!(
            theme.args["themeJson"],
            serde_json::json!({ "background": "#0b0c0f" })
        );
        let highlights = invoker
            .calls
            .iter()
            .find(|call| call.command == "terminal_update_highlight_rules")
            .expect("highlight call should be dispatched");
        assert_eq!(highlights.args["rules"], runtime.highlight_rules);
        assert_eq!(highlights.args["perf"], runtime.highlight_perf);
    }

    #[test]
    fn ipc_dispatcher_stops_at_first_failed_invoke() {
        let mut host = native_shell_host::NativeShellHost::new(ShellModel::nexshell_default());
        let frame = host.render_frame(layout::Size {
            width: 1200,
            height: 800,
        });
        let runtime = ipc_dispatcher::IpcRuntimeInputs::placeholder();
        let mut invoker = RecordingInvoker {
            fail_command: Some("terminal_update_font"),
            ..RecordingInvoker::default()
        };

        let error = ipc_dispatcher::dispatch_batch(&frame.ipc, &runtime, &mut invoker)
            .expect_err("dispatch should stop at configured failure");

        assert_eq!(error.command, "terminal_update_font");
        assert_eq!(error.message, "injected failure");
        assert_eq!(
            invoker
                .calls
                .iter()
                .map(|call| call.command)
                .collect::<Vec<_>>(),
            vec![
                "terminal_update_theme",
                "terminal_create",
                "terminal_set_cursor_style",
                "terminal_update_highlight_rules",
                "terminal_update_font",
            ]
        );
    }

    #[test]
    fn native_shell_frame_export_matches_frontend_bridge_shape() {
        let mut host = native_shell_host::NativeShellHost::new(ShellModel::nexshell_default());
        let frame = host.render_frame(layout::Size {
            width: 1200,
            height: 800,
        });
        let runtime = ipc_dispatcher::IpcRuntimeInputs {
            theme_json: serde_json::json!({ "background": "#0b0c0f" }),
            highlight_rules: serde_json::json!([
                { "id": "ip", "pattern": "\\d+\\.\\d+\\.\\d+\\.\\d+", "color": "#58a6ff" }
            ]),
            highlight_perf: serde_json::json!({ "maxLineLength": 1200 }),
        };

        let exported =
            frame_export::resolve_frame(&frame, &runtime).expect("frame should resolve to JSON");
        let calls = exported["ipc"]["calls"]
            .as_array()
            .expect("exported frame should expose ipc.calls");

        assert_eq!(calls.len(), frame.ipc.calls.len());
        assert_eq!(
            calls[0]["command"],
            serde_json::json!("terminal_update_theme")
        );
        assert_eq!(calls[0]["args"]["themeJson"], runtime.theme_json);

        let start_render = calls
            .iter()
            .find(|call| call["command"] == serde_json::json!("terminal_start_render"))
            .expect("start render call should be exported");
        assert_eq!(
            start_render["args"]["fontFamily"],
            serde_json::json!("JetBrains Mono")
        );

        let viewport = calls
            .iter()
            .find(|call| call["command"] == serde_json::json!("terminal_set_viewport"))
            .expect("viewport call should be exported");
        assert_eq!(viewport["args"]["x"], serde_json::json!(70.0));
        assert_eq!(viewport["args"]["y"], serde_json::json!(88.0));
        assert_eq!(viewport["args"]["width"], serde_json::json!(1130.0));
        assert_eq!(viewport["args"]["height"], serde_json::json!(660.0));
    }

    #[test]
    fn native_shell_adapter_render_exposes_view_snapshot_and_frontend_ipc() {
        let mut adapter =
            native_shell_adapter::NativeShellAdapter::new(ShellModel::nexshell_default());
        let runtime = ipc_dispatcher::IpcRuntimeInputs {
            theme_json: serde_json::json!({ "background": "#0b0c0f" }),
            highlight_rules: serde_json::json!([
                { "id": "ip", "pattern": "\\d+\\.\\d+\\.\\d+\\.\\d+", "color": "#58a6ff" }
            ]),
            highlight_perf: serde_json::json!({ "maxLineLength": 1200 }),
        };

        let adapter_frame = adapter
            .render_with_runtime(
                layout::Size {
                    width: 1200,
                    height: 800,
                },
                &runtime,
            )
            .expect("adapter render should resolve frontend IPC");

        assert_eq!(adapter_frame.view().window_title, "NexShell");
        assert_eq!(
            adapter_frame.view().layout.terminal_host,
            layout::Rect::new(70, 88, 1130, 660)
        );
        assert_eq!(
            adapter_frame
                .view()
                .activities
                .iter()
                .find(|activity| activity.id == "terminal")
                .map(|activity| activity.active),
            Some(true)
        );
        assert_eq!(
            adapter_frame
                .view()
                .tabs
                .iter()
                .find(|tab| tab.id == "sshtool")
                .map(|tab| tab.active),
            Some(true)
        );
        assert_eq!(adapter_frame.view().bottom_tools.len(), 7);
        assert_eq!(
            adapter_frame.frontend_frame()["ipc"]["calls"][0]["command"],
            serde_json::json!("terminal_update_theme")
        );
    }

    #[test]
    fn native_shell_runtime_defaults_match_current_terminal_payloads() {
        let settings = runtime_settings::NativeShellRuntimeSettings::nexshell_default();
        let runtime = settings.ipc_inputs();

        assert_eq!(
            runtime.theme_json["background"],
            serde_json::json!("#121212")
        );
        assert_eq!(
            runtime.theme_json["foreground"],
            serde_json::json!("#FAF9F6")
        );
        assert_eq!(runtime.theme_json["cursor"], serde_json::json!("#2E5D9E"));
        assert_eq!(
            runtime.theme_json["selectionBackground"],
            serde_json::json!("rgba(46, 93, 158, 0.32)")
        );

        let rules = runtime
            .highlight_rules
            .as_array()
            .expect("highlight rules should be a JSON array");
        assert_eq!(rules.len(), 15);
        assert_eq!(rules[0]["id"], serde_json::json!("preset-url"));
        assert_eq!(rules[0]["enabled"], serde_json::json!(true));
        assert_eq!(rules[14]["id"], serde_json::json!("preset-number"));
        assert_eq!(rules[14]["enabled"], serde_json::json!(false));
        assert_eq!(rules[12]["validateFilesystem"], serde_json::json!(true));

        assert_eq!(
            runtime.highlight_perf,
            serde_json::json!({
                "maxLineLength": 2000,
                "maxDecorations": 2000,
                "skipAltBuffer": true,
            })
        );
    }

    #[test]
    fn native_shell_adapter_default_render_exports_real_runtime_payloads() {
        let mut adapter =
            native_shell_adapter::NativeShellAdapter::new(ShellModel::nexshell_default());

        let frame = adapter
            .render(layout::Size {
                width: 1200,
                height: 800,
            })
            .expect("adapter render should resolve with default runtime");
        let calls = frame.frontend_frame()["ipc"]["calls"]
            .as_array()
            .expect("frontend frame should expose IPC calls");
        let theme = calls
            .iter()
            .find(|call| call["command"] == serde_json::json!("terminal_update_theme"))
            .expect("theme call should be present");
        let highlights = calls
            .iter()
            .find(|call| call["command"] == serde_json::json!("terminal_update_highlight_rules"))
            .expect("highlight call should be present");

        assert_eq!(
            theme["args"]["themeJson"]["background"],
            serde_json::json!("#121212")
        );
        assert_ne!(theme["args"]["themeJson"], serde_json::json!("<themeJson>"));
        assert_eq!(
            highlights["args"]["perf"]["maxLineLength"],
            serde_json::json!(2000)
        );
        assert_eq!(
            highlights["args"]["rules"][0]["id"],
            serde_json::json!("preset-url")
        );
    }

    #[test]
    fn terminal_grid_reports_bracketed_paste_mode_after_dec_set() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        assert!(!grid.bracketed_paste_enabled());

        // CSI ? 2004 h enables bracketed paste mode.
        grid.process_output(b"\x1b[?2004h");
        assert!(grid.bracketed_paste_enabled());
        assert!(grid.snapshot(&[], None).bracketed_paste);

        grid.process_output(b"\x1b[?2004l");
        assert!(!grid.bracketed_paste_enabled());
    }

    #[test]
    fn terminal_grid_drains_title_events_emitted_by_osc_0_and_osc_2() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);

        // OSC 2 (window title) — terminated by BEL.
        grid.process_output(b"\x1b]2;hello world\x07");
        let events = grid.drain_events();
        assert!(events
            .iter()
            .any(|event| matches!(event, alacritty_terminal::event::Event::Title(t) if t == "hello world")));

        // OSC 0 (icon + window title) using ST terminator.
        grid.process_output(b"\x1b]0;another\x1b\\");
        let events = grid.drain_events();
        assert!(events.iter().any(
            |event| matches!(event, alacritty_terminal::event::Event::Title(t) if t == "another")
        ));
    }

    #[test]
    fn terminal_grid_snapshot_carries_hyperlink_uri_for_osc_8_cells() {
        let mut grid = terminal_runtime::TerminalGridCore::new(16, 1, 100);
        // OSC 8 ; ; https://example.com ST text OSC 8 ; ; ST
        grid.process_output(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\");
        let snapshot = grid.snapshot(&[], None);
        let row = &snapshot.lines[0];
        assert_eq!(
            row.cells[0].hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            row.cells[3].hyperlink.as_deref(),
            Some("https://example.com")
        );
        assert!(row.cells[4].hyperlink.is_none());
    }

    #[test]
    fn terminal_grid_snapshot_reports_sgr_mouse_after_dec_set() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        // CSI ? 1000 h enables MOUSE_REPORT_CLICK; CSI ? 1006 h adds SGR encoding.
        grid.process_output(b"\x1b[?1000h\x1b[?1006h");
        let snapshot = grid.snapshot(&[], None);
        assert!(snapshot.mouse_report_click);
        assert!(snapshot.sgr_mouse);
        assert!(snapshot.mouse_app_active());
    }

    #[test]
    fn sgr_mouse_report_matches_warp_format_for_left_press_at_origin() {
        use terminal_runtime::{
            encode_sgr_mouse_report, MouseReportAction, MouseReportButton, MouseReportModifiers,
        };

        let bytes = encode_sgr_mouse_report(
            MouseReportButton::Left,
            MouseReportAction::Press,
            1,
            1,
            MouseReportModifiers::default(),
        );
        assert_eq!(bytes, b"\x1b[<0;1;1M");
    }

    #[test]
    fn sgr_mouse_report_encodes_drag_and_release_and_modifiers() {
        use terminal_runtime::{
            encode_sgr_mouse_report, MouseReportAction, MouseReportButton, MouseReportModifiers,
        };

        let drag = encode_sgr_mouse_report(
            MouseReportButton::Left,
            MouseReportAction::Drag,
            12,
            5,
            MouseReportModifiers {
                shift: false,
                alt: true,
                ctrl: false,
            },
        );
        // base 0 + drag 32 + alt 8 = 40
        assert_eq!(drag, b"\x1b[<40;12;5M");

        let release = encode_sgr_mouse_report(
            MouseReportButton::Left,
            MouseReportAction::Release,
            7,
            3,
            MouseReportModifiers::default(),
        );
        assert_eq!(release, b"\x1b[<0;7;3m");

        let wheel_up = encode_sgr_mouse_report(
            MouseReportButton::WheelUp,
            MouseReportAction::Press,
            10,
            10,
            MouseReportModifiers {
                shift: true,
                alt: false,
                ctrl: false,
            },
        );
        // 64 + shift 4 = 68
        assert_eq!(wheel_up, b"\x1b[<68;10;10M");
    }

    #[test]
    fn terminal_grid_simple_selection_writes_into_snapshot_and_yields_text() {
        use alacritty_terminal::index::{Column, Line, Point, Side};
        use alacritty_terminal::selection::SelectionType;

        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        grid.process_output(b"hello\r\nworld");

        // Select 'ello' on line 0 col 1..5 (Right side anchor / Left side head matches alacritty drag).
        grid.start_selection(
            SelectionType::Simple,
            Point::new(Line(0), Column(1)),
            Side::Left,
        );
        grid.update_selection(Point::new(Line(0), Column(4)), Side::Right);

        let snapshot = grid.snapshot(&[], None);
        assert!(snapshot.lines[0].cells[1].selected());
        assert!(snapshot.lines[0].cells[4].selected());
        assert!(!snapshot.lines[0].cells[5].selected());

        assert_eq!(grid.selected_text().as_deref(), Some("ello"));
    }

    #[test]
    fn terminal_grid_semantic_selection_expands_to_word_boundaries() {
        use alacritty_terminal::index::{Column, Line, Point, Side};
        use alacritty_terminal::selection::SelectionType;

        let mut grid = terminal_runtime::TerminalGridCore::new(20, 1, 100);
        grid.process_output(b"hello world foo");

        // Click in the middle of "world" — semantic should snap to the whole word.
        grid.start_selection(
            SelectionType::Semantic,
            Point::new(Line(0), Column(8)),
            Side::Left,
        );

        assert_eq!(grid.selected_text().as_deref(), Some("world"));
    }

    #[test]
    fn terminal_grid_lines_selection_returns_full_line() {
        use alacritty_terminal::index::{Column, Line, Point, Side};
        use alacritty_terminal::selection::SelectionType;

        let mut grid = terminal_runtime::TerminalGridCore::new(20, 2, 100);
        grid.process_output(b"hello world\r\nsecond line");

        grid.start_selection(
            SelectionType::Lines,
            Point::new(Line(0), Column(3)),
            Side::Left,
        );

        let text = grid.selected_text().expect("lines selection produces text");
        assert!(text.starts_with("hello world"));
    }

    #[test]
    fn terminal_grid_clear_selection_drops_per_cell_flag() {
        use alacritty_terminal::index::{Column, Line, Point, Side};
        use alacritty_terminal::selection::SelectionType;

        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        grid.process_output(b"abcdefgh");
        grid.start_selection(
            SelectionType::Simple,
            Point::new(Line(0), Column(2)),
            Side::Left,
        );
        grid.update_selection(Point::new(Line(0), Column(4)), Side::Right);
        assert!(grid.snapshot(&[], None).lines[0].cells[3].selected());

        grid.clear_selection();
        assert!(grid.snapshot(&[], None).lines[0]
            .cells
            .iter()
            .all(|cell| !cell.selected()));
        assert!(grid.selected_text().is_none());
    }

    #[test]
    fn terminal_grid_selection_dirty_rows_track_selection_range() {
        use alacritty_terminal::index::{Column, Line, Point, Side};
        use alacritty_terminal::selection::SelectionType;

        let mut grid = terminal_runtime::TerminalGridCore::new(8, 3, 100);
        grid.process_output(b"line-0\r\nline-1\r\nline-2");
        grid.snapshot(&[], None);
        grid.clear_dirty_rows();

        grid.start_selection(
            SelectionType::Simple,
            Point::new(Line(1), Column(1)),
            Side::Left,
        );
        assert_eq!(
            grid.snapshot(&[], None).dirty_rows,
            vec![false, true, false]
        );

        grid.clear_dirty_rows();
        grid.update_selection(Point::new(Line(2), Column(2)), Side::Right);
        assert_eq!(grid.snapshot(&[], None).dirty_rows, vec![false, true, true]);

        grid.clear_dirty_rows();
        grid.clear_selection();
        assert_eq!(grid.snapshot(&[], None).dirty_rows, vec![false, true, true]);
    }

    #[test]
    fn terminal_grid_marked_text_dirty_rows_track_overlay_range() {
        let mut grid = terminal_runtime::TerminalGridCore::new(4, 3, 100);
        grid.process_output(b"abcd\r\nefgh\r\nijkl\x1b[H");
        grid.snapshot(&[], None);
        grid.clear_dirty_rows();

        let marked = terminal_runtime::MarkedText {
            text: "abcdef".to_string(),
            selected_range_utf16: 0..0,
        };
        grid.mark_dirty_for_marked_text(None, Some(&marked));

        assert_eq!(
            grid.snapshot_with_marked_text(&[], None, Some(&marked))
                .dirty_rows,
            vec![true, true, false]
        );
    }

    #[test]
    fn terminal_grid_clear_visible_screen_drops_viewport_and_scrollback() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        for line in 0..6 {
            grid.process_output(format!("line{line}\r\n").as_bytes());
        }
        assert!(grid.snapshot(&[], None).history_size > 0);

        grid.clear_visible_screen();

        let after = grid.snapshot(&[], None);
        assert_eq!(after.history_size, 0);
        for line in &after.lines {
            assert!(line.text.trim().is_empty());
        }
    }

    #[test]
    fn terminal_grid_clear_visible_screen_keeps_only_current_prompt_marker_when_requested() {
        let mut grid = terminal_runtime::TerminalGridCore::new(20, 3, 100);
        grid.process_output(
            "\x1b[36m~\x1b[0m \x1b[31m>\x1b[0m old\r\nzsh: nope\r\n\x1b[36m~\x1b[0m \x1b[31m>\x1b[0m "
                .as_bytes(),
        );
        let before = grid.snapshot(&[], None);
        assert_eq!(before.cursor_row, 2);
        assert_eq!(before.cursor_col, 4);
        assert_eq!(before.lines[0].text, "~ > old");
        assert_eq!(before.lines[1].text, "zsh: nope");
        assert_eq!(before.lines[2].text, "~ >");

        grid.clear_visible_screen_preserving_prompt_prefix();

        let after = grid.snapshot(&[], None);
        assert_eq!(after.history_size, 0);
        assert_eq!(after.lines[0].text, "");
        assert_eq!(after.lines[1].text, "");
        assert_eq!(after.lines[2].text, "~ >");
        assert_eq!(after.cursor_row, 2);
        assert_eq!(after.cursor_col, 4);
        assert_ne!(
            after.lines[2].cells[0].style_id,
            after.lines[2].cells[1].style_id
        );

        grid.process_output(b"next");
        let with_first_input = grid.snapshot(&[], None);
        assert_eq!(with_first_input.lines[2].text, "~ > next");
    }

    #[test]
    fn terminal_grid_cursor_visibility_tracks_dec_show_cursor_mode() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        let snap = grid.snapshot(&[], None);
        assert!(snap.cursor_visible);
        assert_eq!(
            snap.cursor_shape,
            terminal_runtime::TerminalCursorShape::Block
        );
        assert!(!snap.cursor_blinking);

        grid.process_output(b"\x1b[?25l");
        assert!(!grid.snapshot(&[], None).cursor_visible);

        grid.process_output(b"\x1b[?25h");
        assert!(grid.snapshot(&[], None).cursor_visible);
    }

    #[test]
    fn terminal_grid_cursor_blink_flag_follows_dec_blink_request() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        // `CSI 5 SP q` → blinking bar; alacritty sets `cursor_style.blinking = true`.
        grid.process_output(b"\x1b[5 q");
        assert!(grid.snapshot(&[], None).cursor_blinking);
    }

    #[test]
    fn terminal_grid_find_all_returns_matches_in_document_order() {
        let mut grid = terminal_runtime::TerminalGridCore::new(40, 4, 100);
        grid.process_output(b"alpha bravo alpha\r\n");
        grid.process_output(b"charlie alpha delta");
        let matches = grid
            .find_all("alpha")
            .expect("regex compiles for literal alpha");
        assert_eq!(matches.len(), 3);
        assert!(matches[0].start <= matches[1].start);
        assert!(matches[1].start <= matches[2].start);
    }

    #[test]
    fn terminal_runtime_set_find_query_records_match_count_and_zeros_on_clear() {
        let runtime =
            terminal_runtime::LocalTerminalRuntime::failed("find-test", "no pty for unit");
        let count = runtime.set_find_query(Some("anything".to_string()));
        assert_eq!(count, 0);
        runtime.set_find_query(None);
        let snap = runtime.snapshot();
        assert_eq!(snap.find_match_count, 0);
        assert!(snap.find_query.is_none());
    }

    #[test]
    fn terminal_render_runs_emit_one_run_per_cell_with_wide_chars_taking_two_cols() {
        // Mirrors Warp's `grid_renderer.rs` cell-by-cell rendering: each
        // non-spacer cell becomes its own run; the wide-char spacer cell only
        // bumps the preceding run's `cols` so the wide-char div is sized to
        // exactly 2 cell widths. No style coalescence — each cell is drawn at
        // its own column, eliminating cross-cell font advance drift that the
        // CJK fallback font (PingFang etc.) introduces over multi-char runs.
        let mut grid = terminal_runtime::TerminalGridCore::new(20, 1, 50);
        grid.process_output("你好".as_bytes());
        let snap = grid.snapshot(&[], None);
        let rows = terminal_runtime::terminal_render_run_rows(
            &snap,
            &terminal_runtime::TerminalPalette::default(),
        );
        let runs = &rows[0].runs;
        // Wide chars: each cjk char is its own 2-cell run; rest of the row are
        // single-cell empty runs.
        assert_eq!(&*runs[0].text, "你");
        assert_eq!(runs[0].cols, 2);
        assert_eq!(&*runs[1].text, "好");
        assert_eq!(runs[1].cols, 2);
        // Total visible text spans 4 cells.
        let total: usize = runs.iter().map(|r| r.cols).sum();
        assert_eq!(total, 20);
    }

    #[test]
    fn terminal_grid_snapshot_with_marked_text_overrides_cells_at_cursor_with_underline() {
        // Mirrors Warp's `grid_renderer.rs:639-695` cell-override behaviour:
        // marked text overlays cells starting at the cursor, each cell carries
        // an underline, and CJK wide chars consume two cells (the second is
        // a wide-char spacer).
        let mut grid = terminal_runtime::TerminalGridCore::new(20, 2, 100);
        // Put cursor at column 0 of row 0 (default after construction).
        let marked = terminal_runtime::MarkedText {
            text: "你好zi".to_string(),
            selected_range_utf16: 0..2,
        };
        let snap = grid.snapshot_with_marked_text(&[], None, Some(&marked));
        let row = &snap.lines[0];
        // 你 (wide) → cell0 = '你', cell1 = wide_spacer
        assert_eq!(row.cells[0].ch, '你');
        assert!(row.cells[0].underline());
        assert!(row.cells[1].wide_spacer());
        // 好 (wide) → cell2 = '好', cell3 = wide_spacer
        assert_eq!(row.cells[2].ch, '好');
        assert!(row.cells[2].underline());
        assert!(row.cells[3].wide_spacer());
        // z i (narrow) → cell4 = 'z', cell5 = 'i'
        assert_eq!(row.cells[4].ch, 'z');
        assert_eq!(row.cells[5].ch, 'i');
        assert!(row.cells[4].underline());
        assert!(row.cells[5].underline());
        for cell in &row.cells[0..6] {
            assert!(
                !cell.inverse(),
                "Warp marked text should not inverse/cover composition characters"
            );
        }
    }

    #[test]
    fn terminal_runtime_marked_text_round_trips_through_snapshot_and_clears() {
        // Equivalent to Warp's `TerminalModel::set_marked_text` /
        // `clear_marked_text` round-trip — used by the IME path so the
        // composition preview survives the snapshot copy that the renderer
        // pulls every frame.
        let runtime = terminal_runtime::LocalTerminalRuntime::failed("ime-test", "no pty for unit");
        runtime.set_marked_text("ni hao".to_string(), 0..2);
        let snap = runtime.snapshot();
        let marked = snap
            .marked_text
            .as_ref()
            .expect("marked text should be present after set_marked_text");
        assert_eq!(marked.text, "ni hao");
        assert_eq!(marked.selected_range_utf16, 0..2);

        runtime.clear_marked_text();
        let cleared = runtime.snapshot();
        assert!(cleared.marked_text.is_none());

        // Empty marked text is treated as "clear" — matches Warp's
        // `replace_and_mark_text_in_range` behaviour for empty `new_text`.
        runtime.set_marked_text(String::new(), 0..0);
        assert!(runtime.snapshot().marked_text.is_none());
    }

    #[test]
    fn terminal_runtime_marked_text_moves_render_cursor_to_composition_end() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 4, 100);
        let marked = terminal_runtime::MarkedText {
            text: "你好是".to_string(),
            selected_range_utf16: 0..3,
        };

        let snapshot = grid.snapshot_with_marked_text(&[], None, Some(&marked));

        assert_eq!(snapshot.cursor_row, 0);
        assert_eq!(snapshot.cursor_col, 6);
    }

    #[test]
    fn terminal_runtime_marked_text_does_not_paint_block_cursor_over_cells() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 4, 100);
        let marked = terminal_runtime::MarkedText {
            text: "你好是".to_string(),
            selected_range_utf16: 0..3,
        };

        let snapshot = grid.snapshot_with_marked_text(&[], None, Some(&marked));
        let rows = terminal_runtime::terminal_render_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );
        let cursor_cell = &rows[0].cells[6];

        assert!(cursor_cell.cursor);
        assert_ne!(
            cursor_cell.bg,
            terminal_runtime::TerminalPalette::default().cursor
        );
    }

    #[test]
    fn terminal_runtime_block_cursor_does_not_paint_over_existing_text() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        grid.process_output(b"ab\x1b[D");
        let snapshot = grid.snapshot(&[], None);
        let rows = terminal_runtime::terminal_render_rows(
            &snapshot,
            &terminal_runtime::TerminalPalette::default(),
        );
        let cursor_cell = &rows[0].cells[1];

        assert_eq!(cursor_cell.ch, 'b');
        assert!(cursor_cell.cursor);
        assert_ne!(
            cursor_cell.bg,
            terminal_runtime::TerminalPalette::default().cursor
        );
    }

    #[test]
    fn terminal_runtime_bell_pulse_advances_when_grid_emits_bell_event() {
        // Build a grid, feed a BEL byte, drain events, observe pulse increment.
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 1, 100);
        grid.process_output(b"\x07");
        let events = grid.drain_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, alacritty_terminal::event::Event::Bell)));
    }

    #[test]
    fn terminal_grid_core_scrollback_lets_user_view_history_then_jump_back_to_bottom() {
        let mut grid = terminal_runtime::TerminalGridCore::new(8, 2, 100);
        // Generate enough output to push prior lines into scrollback.
        for line in 0..6 {
            grid.process_output(format!("line{line}\r\n").as_bytes());
        }
        let snapshot_at_bottom = grid.snapshot(&[], None);
        assert_eq!(snapshot_at_bottom.display_offset, 0);
        assert!(snapshot_at_bottom.history_size >= 4);

        grid.scroll_lines(3);
        let snapshot_scrolled = grid.snapshot(&[], None);
        assert_eq!(snapshot_scrolled.display_offset, 3);
        // Top-most visible line should now be older content.
        assert_ne!(
            snapshot_scrolled.lines[0].text,
            snapshot_at_bottom.lines[0].text
        );

        grid.scroll_to_bottom();
        assert_eq!(grid.snapshot(&[], None).display_offset, 0);
    }

    #[test]
    fn cell_metrics_from_em_advance_clamps_to_one_pixel_minimum() {
        let cell = terminal_lifecycle::CellMetrics::from_em_advance(0.0, 0.0);
        assert_eq!(cell.width, 1.0);
        assert_eq!(cell.height, 1.0);

        let cell = terminal_lifecycle::CellMetrics::from_em_advance(9.4, 23.6);
        assert_eq!(cell.width, 9.4);
        assert_eq!(cell.height, 23.6);
    }

    #[test]
    fn cell_metrics_from_font_metrics_uses_ratio_scaled_natural_line_height() {
        // ascent=12, |descent|=3, line_gap=1 → natural=16; ratio=1.5 → 24, ceil → 24
        let cell = terminal_lifecycle::CellMetrics::from_font_metrics(9.0, 12.0, -3.0, 1.0, 1.5);
        assert_eq!(cell.width, 9.0);
        assert_eq!(cell.height, 24.0);
    }

    #[derive(Default)]
    struct RecordingInvoker {
        calls: Vec<ipc_dispatcher::ResolvedIpcCall>,
        fail_command: Option<&'static str>,
    }

    impl ipc_dispatcher::IpcInvoker for RecordingInvoker {
        fn invoke(&mut self, call: &ipc_dispatcher::ResolvedIpcCall) -> Result<(), String> {
            self.calls.push(call.clone());
            if self.fail_command == Some(call.command) {
                Err("injected failure".to_string())
            } else {
                Ok(())
            }
        }
    }
}
