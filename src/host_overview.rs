use std::{
    collections::{HashMap, VecDeque},
    env, fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    host_management::HostConnectionConfig,
    ssh_session::{SshConnectOptions, SshSession},
    terminal_runtime::RemoteSshConfig,
};

pub const HOST_OVERVIEW_COLLECT_COMMAND: &str = r#"printf '%s\n' 'NEXSHELL_HOST_OVERVIEW_V1'
printf '%s\n' '[identity]'
(hostname 2>/dev/null || uname -n 2>/dev/null || printf '%s\n' '')
(whoami 2>/dev/null || id -un 2>/dev/null || printf '%s\n' '')
(uname -srmo 2>/dev/null || uname -a 2>/dev/null || printf '%s\n' '')
printf '%s\n' '[uptime]'
(cat /proc/uptime 2>/dev/null || printf '%s\n' '')
printf '%s\n' '[load]'
(cat /proc/loadavg 2>/dev/null || uptime 2>/dev/null || printf '%s\n' '')
printf '%s\n' '[mem]'
(cat /proc/meminfo 2>/dev/null || printf '%s\n' '')
printf '%s\n' '[stat]'
(grep '^cpu ' /proc/stat 2>/dev/null || printf '%s\n' '')
printf '%s\n' '[ncpu]'
(nproc 2>/dev/null || grep -c '^processor' /proc/cpuinfo 2>/dev/null || printf '%s\n' '')
printf '%s\n' '[net]'
(cat /proc/net/dev 2>/dev/null || printf '%s\n' '')
printf '%s\n' '[ps]'
(ps -eo pid=,user=,rss=,pcpu=,comm=,args= --sort=-pcpu 2>/dev/null || printf '%s\n' '')
printf '%s\n' '[exe]'
(for d in /proc/[0-9]*; do pid=${d##*/}; link=$(readlink "$d/exe" 2>/dev/null); [ -n "$link" ] && printf '%s\t%s\n' "$pid" "$link"; done 2>/dev/null || printf '%s\n' '')
printf '%s\n' '[sock_tcp]'
(ss -Hntan -p -i 2>/dev/null || printf '%s\n' '')
printf '%s\n' '[sock_udp]'
(ss -Hnuan -p 2>/dev/null || printf '%s\n' '')
printf '%s\n' '[disk]'
(df -P -k 2>/dev/null | tail -n +2 || printf '%s\n' '')
printf '%s\n' '[diskio]'
(cat /proc/diskstats 2>/dev/null || printf '%s\n' '')"#;

const MAGIC: &str = "NEXSHELL_HOST_OVERVIEW_V1";
const HISTORY_LIMIT: usize = 150;
/// 采集 exec 超时：高延迟链路基线 ~1s，留足余量吸收尖峰，避免误判断连
const COLLECT_TIMEOUT: Duration = Duration::from_secs(8);
/// RTT 探测超时
const RTT_TIMEOUT: Duration = Duration::from_secs(4);
/// 连续采集失败多少次才判定连接已死、拆连接重连（单次抖动不拆）
const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// 监控独立连接诊断日志，置 NEXSHELL_HOST_OVERVIEW_DEBUG 开启。
fn host_overview_debug(args: std::fmt::Arguments<'_>) {
    if env::var_os("NEXSHELL_HOST_OVERVIEW_DEBUG").is_some() {
        eprintln!("[host-overview] {args}");
    }
}
macro_rules! ho_debug {
    ($($arg:tt)*) => { host_overview_debug(format_args!($($arg)*)) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuCounters {
    pub total: u64,
    pub idle: u64,
}

/// /proc/diskstats 聚合的累计扇区计数（整盘）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiskIoCounters {
    pub read_sectors: u64,
    pub write_sectors: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkCounters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkDeviceCounters {
    pub interface: String,
    pub counters: NetworkCounters,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageMetric {
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub percent: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessMetric {
    pub pid: u32,
    pub user: String,
    pub rss_bytes: u64,
    pub cpu_percent: f32,
    /// comm 字段，进程短名
    pub command: String,
    /// 完整命令行（含参数）
    pub args: String,
    /// readlink /proc/[pid]/exe 结果
    pub exe_path: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SocketProto {
    Tcp,
    Udp,
}

impl SocketProto {
    pub fn label(self) -> &'static str {
        match self {
            SocketProto::Tcp => "TCP",
            SocketProto::Udp => "UDP",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NetworkRowKind {
    Listen,
    Outbound,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkRow {
    pub kind: NetworkRowKind,
    pub proto: SocketProto,
    pub pid: Option<u32>,
    pub process: String,
    pub local_addr: String,
    pub local_port: u16,
    pub remote_addr: Option<String>,
    pub remote_port: Option<u16>,
    /// 监听行：聚合该监听口上的对端 IP 去重数；出站行恒为 1
    pub unique_ips: u32,
    /// 监听行：聚合该监听口上的 ESTAB 连接数；出站行恒为 1
    pub connections: u32,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiskMetric {
    pub mount: String,
    pub filesystem: String,
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub percent: f32,
}

impl DiskMetric {
    /// tmpfs/devtmpfs/overlay 等内存伪文件系统；真实设备（/dev/…）与网络挂载（host:/、//share）返回 false。
    pub fn is_pseudo_filesystem(&self) -> bool {
        !(self.filesystem.starts_with('/')
            || self.filesystem.contains(':')
            || self.filesystem.starts_with("//"))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkRatePoint {
    pub rx_bytes_per_sec: u64,
    pub tx_bytes_per_sec: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkMetric {
    pub interface: String,
    pub rx_bytes_per_sec: u64,
    pub tx_bytes_per_sec: u64,
    pub history: Vec<NetworkRatePoint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostOverviewProbe {
    pub hostname: Option<String>,
    pub username: Option<String>,
    pub kernel: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub load_average: Option<[f32; 3]>,
    pub memory: Option<UsageMetric>,
    pub swap: Option<UsageMetric>,
    pub cpu: Option<CpuCounters>,
    pub cpu_cores: Option<u32>,
    pub networks: Vec<NetworkDeviceCounters>,
    pub processes: Vec<ProcessMetric>,
    pub sockets: Vec<NetworkRow>,
    pub disks: Vec<DiskMetric>,
    pub disk_io: Option<DiskIoCounters>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostOverviewSnapshot {
    pub hostname: Option<String>,
    pub username: Option<String>,
    pub host: String,
    pub kernel: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub load_average: Option<[f32; 3]>,
    pub cpu_percent: Option<f32>,
    pub cpu_cores: Option<u32>,
    pub memory: Option<UsageMetric>,
    pub swap: Option<UsageMetric>,
    pub processes: Vec<ProcessMetric>,
    pub networks: Vec<NetworkMetric>,
    pub network: Option<NetworkMetric>,
    pub sockets: Vec<NetworkRow>,
    pub disks: Vec<DiskMetric>,
    pub disk_read_bytes_per_sec: Option<u64>,
    pub disk_write_bytes_per_sec: Option<u64>,
    pub latency_ms: Option<u64>,
    pub status: HostOverviewStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostOverviewStatus {
    Waiting,
    Collecting,
    Ready,
    Error(String),
}

impl HostOverviewSnapshot {
    pub fn waiting(host: impl Into<String>) -> Self {
        Self {
            hostname: None,
            username: None,
            host: host.into(),
            kernel: None,
            uptime_seconds: None,
            load_average: None,
            cpu_percent: None,
            cpu_cores: None,
            memory: None,
            swap: None,
            processes: Vec::new(),
            networks: Vec::new(),
            network: None,
            sockets: Vec::new(),
            disks: Vec::new(),
            disk_read_bytes_per_sec: None,
            disk_write_bytes_per_sec: None,
            latency_ms: None,
            status: HostOverviewStatus::Waiting,
        }
    }

    pub fn error(host: impl Into<String>, error: impl Into<String>) -> Self {
        let mut snapshot = Self::waiting(host);
        snapshot.status = HostOverviewStatus::Error(error.into());
        snapshot
    }

    pub fn has_collected_data(&self) -> bool {
        self.username.is_some()
            || self.kernel.is_some()
            || self.uptime_seconds.is_some()
            || self.load_average.is_some()
            || self.cpu_percent.is_some()
            || self.memory.is_some()
            || self.swap.is_some()
            || !self.processes.is_empty()
            || !self.networks.is_empty()
            || self.network.is_some()
            || !self.sockets.is_empty()
            || !self.disks.is_empty()
            || self.latency_ms.is_some()
    }
}

pub fn merge_overview_snapshot(
    current: &HostOverviewSnapshot,
    incoming: HostOverviewSnapshot,
) -> HostOverviewSnapshot {
    let incoming_is_collecting_placeholder = incoming.status == HostOverviewStatus::Collecting
        && !incoming.has_collected_data()
        && same_snapshot_host(current, &incoming);

    if incoming_is_collecting_placeholder {
        // 已有数据：保留旧帧，仅标 Collecting（更新中），避免闪烁
        if current.has_collected_data() {
            let mut merged = current.clone();
            if !incoming.host.trim().is_empty() {
                merged.host = incoming.host;
            }
            if merged.hostname.is_none() {
                merged.hostname = incoming.hostname;
            }
            merged.status = HostOverviewStatus::Collecting;
            return merged;
        }
        // 无数据但已是 Error：保留真实错误，别让重连占位冲回"正在采集"
        if matches!(current.status, HostOverviewStatus::Error(_)) {
            let mut merged = current.clone();
            if !incoming.host.trim().is_empty() {
                merged.host = incoming.host;
            }
            if merged.hostname.is_none() {
                merged.hostname = incoming.hostname;
            }
            return merged;
        }
    }

    incoming
}

fn same_snapshot_host(current: &HostOverviewSnapshot, incoming: &HostOverviewSnapshot) -> bool {
    let current_host = current.host.trim();
    let incoming_host = incoming.host.trim();
    current_host.is_empty() || incoming_host.is_empty() || current_host == incoming_host
}

#[derive(Clone, Debug, PartialEq)]
pub enum HostOverviewEvent {
    Snapshot(HostOverviewSnapshot),
    Error(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessSortKey {
    Pid,
    User,
    Memory,
    Cpu,
    Command,
    ExePath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkSortKey {
    Pid,
    Process,
    LocalAddr,
    LocalPort,
    UniqueIps,
    Connections,
    RxBytes,
    TxBytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostOverviewUiState {
    pub snapshot: HostOverviewSnapshot,
    pub selected_network: Option<String>,
    pub network_dropdown_open: bool,
    pub process_sort_key: ProcessSortKey,
    pub process_sort_direction: SortDirection,
    pub network_sort_key: NetworkSortKey,
    pub network_sort_direction: SortDirection,
}

impl HostOverviewUiState {
    pub fn waiting(host: impl Into<String>) -> Self {
        Self {
            snapshot: HostOverviewSnapshot::waiting(host),
            selected_network: None,
            network_dropdown_open: false,
            process_sort_key: ProcessSortKey::Cpu,
            process_sort_direction: SortDirection::Desc,
            network_sort_key: NetworkSortKey::LocalPort,
            network_sort_direction: SortDirection::Asc,
        }
    }

    pub fn set_waiting_snapshot(&mut self, snapshot: HostOverviewSnapshot) {
        self.snapshot = snapshot;
        self.selected_network = None;
        self.network_dropdown_open = false;
    }

    /// 点击列头：非当前列切到该列降序；当前列切方向
    pub fn cycle_process_sort(&mut self, key: ProcessSortKey) {
        if self.process_sort_key == key {
            self.process_sort_direction = match self.process_sort_direction {
                SortDirection::Asc => SortDirection::Desc,
                SortDirection::Desc => SortDirection::Asc,
            };
        } else {
            self.process_sort_key = key;
            self.process_sort_direction = SortDirection::Desc;
        }
    }

    pub fn cycle_network_sort(&mut self, key: NetworkSortKey) {
        if self.network_sort_key == key {
            self.network_sort_direction = match self.network_sort_direction {
                SortDirection::Asc => SortDirection::Desc,
                SortDirection::Desc => SortDirection::Asc,
            };
        } else {
            self.network_sort_key = key;
            self.network_sort_direction = match key {
                NetworkSortKey::Pid
                | NetworkSortKey::Process
                | NetworkSortKey::LocalAddr
                | NetworkSortKey::LocalPort => SortDirection::Asc,
                NetworkSortKey::UniqueIps
                | NetworkSortKey::Connections
                | NetworkSortKey::RxBytes
                | NetworkSortKey::TxBytes => SortDirection::Desc,
            };
        }
    }

    pub fn sorted_sockets(&self) -> Vec<&NetworkRow> {
        let mut items: Vec<&NetworkRow> = self.snapshot.sockets.iter().collect();
        let key = self.network_sort_key;
        items.sort_by(|a, b| match key {
            NetworkSortKey::Pid => a.pid.cmp(&b.pid),
            NetworkSortKey::Process => a.process.cmp(&b.process),
            NetworkSortKey::LocalAddr => a.local_addr.cmp(&b.local_addr),
            NetworkSortKey::LocalPort => a.local_port.cmp(&b.local_port),
            NetworkSortKey::UniqueIps => a.unique_ips.cmp(&b.unique_ips),
            NetworkSortKey::Connections => a.connections.cmp(&b.connections),
            NetworkSortKey::RxBytes => a.rx_bytes.cmp(&b.rx_bytes),
            NetworkSortKey::TxBytes => a.tx_bytes.cmp(&b.tx_bytes),
        });
        if matches!(self.network_sort_direction, SortDirection::Desc) {
            items.reverse();
        }
        items
    }

    pub fn sorted_processes(&self) -> Vec<&ProcessMetric> {
        let mut items: Vec<&ProcessMetric> = self.snapshot.processes.iter().collect();
        let key = self.process_sort_key;
        items.sort_by(|a, b| match key {
            ProcessSortKey::Pid => a.pid.cmp(&b.pid),
            ProcessSortKey::User => a.user.cmp(&b.user),
            ProcessSortKey::Memory => a.rss_bytes.cmp(&b.rss_bytes),
            ProcessSortKey::Cpu => a
                .cpu_percent
                .partial_cmp(&b.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal),
            ProcessSortKey::Command => a.command.cmp(&b.command),
            ProcessSortKey::ExePath => a.exe_path.cmp(&b.exe_path),
        });
        if matches!(self.process_sort_direction, SortDirection::Desc) {
            items.reverse();
        }
        items
    }

    pub fn apply_event(&mut self, event: HostOverviewEvent) {
        match event {
            HostOverviewEvent::Snapshot(snapshot) => {
                self.snapshot = merge_overview_snapshot(&self.snapshot, snapshot);
            }
            HostOverviewEvent::Error(error) => {
                if self.snapshot.host.trim().is_empty() {
                    self.snapshot.host = "未连接".to_string();
                }
                self.snapshot.status = HostOverviewStatus::Error(error);
            }
        }
        self.apply_network_selection();
    }

    pub fn select_network(&mut self, interface: impl Into<String>) -> bool {
        let interface = interface.into();
        let Some(network) = self
            .snapshot
            .networks
            .iter()
            .find(|network| network.interface == interface)
            .cloned()
        else {
            self.network_dropdown_open = false;
            return false;
        };

        self.selected_network = Some(interface);
        self.snapshot.network = Some(network);
        self.network_dropdown_open = false;
        true
    }

    pub fn apply_network_selection(&mut self) {
        if let Some(selected) = self.selected_network.as_deref() {
            if let Some(network) = self
                .snapshot
                .networks
                .iter()
                .find(|network| network.interface == selected)
                .cloned()
            {
                self.snapshot.network = Some(network);
                return;
            }
            self.selected_network = None;
            self.network_dropdown_open = false;
        }
    }
}

pub fn should_show_host_overview_sidebar(
    sidebar_open: bool,
    active_tab_supports_host_overview: bool,
) -> bool {
    sidebar_open && active_tab_supports_host_overview
}

pub fn should_run_host_overview_monitor(
    sidebar_open: bool,
    active_tab_supports_host_overview: bool,
    terminal_connected: bool,
) -> bool {
    sidebar_open && active_tab_supports_host_overview && terminal_connected
}

pub fn should_show_empty_overview_status(snapshot: &HostOverviewSnapshot) -> bool {
    !snapshot.has_collected_data()
}

#[derive(Debug)]
pub struct HostOverviewMonitorHandle {
    shutdown: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl HostOverviewMonitorHandle {
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl Drop for HostOverviewMonitorHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 主机库连接配置 → 远端 SSH 采集配置。fleet 与 RootView 共用。
pub fn remote_ssh_config_from_host_config(config: &HostConnectionConfig) -> RemoteSshConfig {
    // 私钥优先走密钥库引用(key_id)：取库内容/口令覆盖内联值，本地文件变动不影响连接。
    let (private_key, key_passphrase) = resolve_private_key_source(config);
    RemoteSshConfig {
        host: config.host.clone(),
        port: config.port,
        username: config.username.clone(),
        auth_method: config.auth_method.clone(),
        password: config.password.clone(),
        private_key,
        key_passphrase,
        ca_cert: config.ca_cert.clone(),
        keep_alive_enabled: config.keep_alive_enabled,
        keep_alive_interval: config.keep_alive_interval,
        keep_alive_max_failures: config.keep_alive_max_failures,
        tcp_connect_timeout: config.tcp_connect_timeout,
        auth_timeout: config.auth_timeout,
        term_encoding: config.term_encoding.clone(),
    }
}

// 解析私钥来源：key_id 命中密钥库则取库内容/口令，否则回退内联值（兼容旧数据）。
fn resolve_private_key_source(config: &HostConnectionConfig) -> (Option<String>, Option<String>) {
    if let Some(key_id) = config.key_id.as_deref().filter(|s| !s.is_empty()) {
        if let Some(db_path) = crate::host_management::default_database_path() {
            if let Ok(Some(record)) = crate::ssh_key_store::get_ssh_key(&db_path, key_id) {
                return (Some(record.content), record.passphrase);
            }
        }
    }
    (config.private_key.clone(), config.key_passphrase.clone())
}

/// 用同样的 SSH 认证流程跑一条一次性命令，结果通过 channel 推出。
/// 适合 kill / 复制等不需要持续监控的远端操作。
pub fn spawn_remote_exec(
    config: RemoteSshConfig,
    command: String,
) -> async_channel::Receiver<Result<String, String>> {
    let (tx, rx) = async_channel::bounded(1);
    let _ = thread::Builder::new()
        .name("nexshell-remote-exec".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    let _ = tx.try_send(Err(format!("runtime: {error}")));
                    return;
                }
            };
            runtime.block_on(async {
                let session = match connect_authenticated_session(&config).await {
                    Ok(s) => s,
                    Err(error) => {
                        let _ = tx.send(Err(error)).await;
                        return;
                    }
                };
                let result = match session.exec_command(&command, COLLECT_TIMEOUT).await {
                    Ok(output) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
                    Err(error) => Err(error),
                };
                session.close().await;
                let _ = tx.send(result).await;
            });
        });
    rx
}

pub fn spawn_host_overview_monitor(
    config: RemoteSshConfig,
    refresh_interval: Duration,
) -> Result<
    (
        HostOverviewMonitorHandle,
        async_channel::Receiver<HostOverviewEvent>,
    ),
    String,
> {
    let (tx, rx) = async_channel::bounded(8);
    let shutdown = Arc::new(AtomicBool::new(false));
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::Builder::new()
        .name(format!(
            "nexshell-host-overview-{}",
            config.host.trim().replace(['/', ':'], "_")
        ))
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = tx.try_send(HostOverviewEvent::Error(format!(
                        "failed to start host overview runtime: {error}"
                    )));
                    return;
                }
            };
            runtime.block_on(run_host_overview_monitor(
                config,
                refresh_interval.max(Duration::from_secs(1)),
                thread_shutdown,
                tx,
            ));
        })
        .map_err(|error| format!("spawn host overview thread: {error}"))?;

    Ok((
        HostOverviewMonitorHandle {
            shutdown,
            _thread: Some(thread),
        },
        rx,
    ))
}

pub fn parse_probe_output(output: &str) -> Result<HostOverviewProbe, String> {
    if !output.lines().any(|line| line.trim() == MAGIC) {
        return Err("host overview probe missing magic header".to_string());
    }

    let mut identity = Vec::new();
    let mut uptime = Vec::new();
    let mut load = Vec::new();
    let mut mem = Vec::new();
    let mut stat = Vec::new();
    let mut ncpu = Vec::new();
    let mut net = Vec::new();
    let mut ps = Vec::new();
    let mut exe = Vec::new();
    let mut sock_tcp = Vec::new();
    let mut sock_udp = Vec::new();
    let mut disk = Vec::new();
    let mut diskio = Vec::new();
    let mut section = "";

    for raw in output.lines() {
        let line = raw.trim_end();
        match line.trim() {
            MAGIC => continue,
            "[identity]" => section = "identity",
            "[uptime]" => section = "uptime",
            "[load]" => section = "load",
            "[mem]" => section = "mem",
            "[stat]" => section = "stat",
            "[ncpu]" => section = "ncpu",
            "[net]" => section = "net",
            "[ps]" => section = "ps",
            "[exe]" => section = "exe",
            "[sock_tcp]" => section = "sock_tcp",
            "[sock_udp]" => section = "sock_udp",
            "[disk]" => section = "disk",
            "[diskio]" => section = "diskio",
            _ => match section {
                "identity" => identity.push(line.to_string()),
                "uptime" => uptime.push(line.to_string()),
                "load" => load.push(line.to_string()),
                "mem" => mem.push(line.to_string()),
                "stat" => stat.push(line.to_string()),
                "ncpu" => ncpu.push(line.to_string()),
                "net" => net.push(line.to_string()),
                "ps" => ps.push(line.to_string()),
                "exe" => exe.push(line.to_string()),
                "sock_tcp" => sock_tcp.push(line.to_string()),
                "sock_udp" => sock_udp.push(line.to_string()),
                "disk" => disk.push(line.to_string()),
                "diskio" => diskio.push(line.to_string()),
                _ => {}
            },
        }
    }

    let mut identity_values = identity
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    Ok(HostOverviewProbe {
        hostname: identity_values.next(),
        username: identity_values.next(),
        kernel: identity_values.next(),
        uptime_seconds: parse_uptime_seconds(&uptime),
        load_average: parse_load_average(&load),
        memory: parse_memory_metric(&mem, "MemTotal", "MemAvailable"),
        swap: parse_memory_metric(&mem, "SwapTotal", "SwapFree"),
        cpu: parse_cpu_counters(&stat),
        cpu_cores: parse_cpu_cores(&ncpu),
        networks: parse_network_counters(&net),
        processes: parse_processes(&ps, &parse_exe_links(&exe)),
        sockets: parse_sockets(&sock_tcp, &sock_udp),
        disks: parse_disks(&disk),
        disk_io: parse_diskstats(&diskio),
    })
}

pub fn snapshot_from_probe(
    probe: HostOverviewProbe,
    previous: Option<(&HostOverviewProbe, Duration)>,
    latency: Option<Duration>,
) -> HostOverviewSnapshot {
    let cpu_percent = previous
        .and_then(|(prev, _)| Some((prev.cpu?, probe.cpu?)))
        .and_then(|(prev, current)| {
            let total_delta = current.total.checked_sub(prev.total)?;
            let idle_delta = current.idle.checked_sub(prev.idle)?;
            (total_delta > 0).then(|| {
                let busy_delta = total_delta.saturating_sub(idle_delta);
                ((busy_delta as f32 / total_delta as f32) * 100.0).clamp(0.0, 100.0)
            })
        });

    let networks = network_metrics_from_probe(&probe.networks, previous);
    let (disk_read_bytes_per_sec, disk_write_bytes_per_sec) = previous
        .and_then(|(prev, elapsed)| {
            let prev_io = prev.disk_io?;
            let cur_io = probe.disk_io?;
            let seconds = elapsed.as_secs_f64();
            if seconds <= 0.0 {
                return None;
            }
            let read =
                cur_io.read_sectors.saturating_sub(prev_io.read_sectors) as f64 * 512.0 / seconds;
            let write =
                cur_io.write_sectors.saturating_sub(prev_io.write_sectors) as f64 * 512.0 / seconds;
            Some((Some(read.round() as u64), Some(write.round() as u64)))
        })
        .unwrap_or((None, None));
    let default_network_interface =
        chosen_network(&probe.networks).map(|network| &network.interface);
    let network = default_network_interface
        .and_then(|interface| {
            networks
                .iter()
                .find(|network| network.interface == *interface)
                .cloned()
        })
        .or_else(|| chosen_network_metric(&networks).cloned());

    HostOverviewSnapshot {
        hostname: probe.hostname,
        username: probe.username,
        host: String::new(),
        kernel: probe.kernel,
        uptime_seconds: probe.uptime_seconds,
        load_average: probe.load_average,
        cpu_percent,
        cpu_cores: probe.cpu_cores,
        memory: probe.memory,
        swap: probe.swap,
        processes: probe.processes,
        networks,
        network,
        sockets: probe.sockets,
        disks: probe.disks,
        disk_read_bytes_per_sec,
        disk_write_bytes_per_sec,
        latency_ms: latency.map(|latency| latency.as_millis().min(u128::from(u64::MAX)) as u64),
        status: HostOverviewStatus::Ready,
    }
}

pub fn format_bytes_short(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1}G", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1}M", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0}K", bytes / KB)
    } else {
        format!("{bytes:.0}")
    }
}

async fn run_host_overview_monitor(
    config: RemoteSshConfig,
    refresh_interval: Duration,
    shutdown: Arc<AtomicBool>,
    tx: async_channel::Sender<HostOverviewEvent>,
) {
    let host_label = format!(
        "{}@{}:{}",
        config.username.trim(),
        config.host.trim(),
        config.port
    );
    let mut histories = HashMap::<String, VecDeque<NetworkRatePoint>>::new();

    while !shutdown.load(Ordering::Relaxed) {
        if tx
            .send(HostOverviewEvent::Snapshot({
                let mut snapshot = HostOverviewSnapshot::waiting(host_label.clone());
                snapshot.status = HostOverviewStatus::Collecting;
                snapshot
            }))
            .await
            .is_err()
        {
            return;
        }

        let connect_started = Instant::now();
        ho_debug!("connect start host={host_label}");
        let session = match connect_authenticated_session(&config).await {
            Ok(session) => {
                ho_debug!(
                    "connect ok host={host_label} elapsed={:?}",
                    connect_started.elapsed()
                );
                session
            }
            Err(error) => {
                ho_debug!(
                    "connect err host={host_label} elapsed={:?} error={error}",
                    connect_started.elapsed()
                );
                if tx.send(HostOverviewEvent::Error(error)).await.is_err() {
                    return;
                }
                sleep_or_shutdown(refresh_interval, &shutdown).await;
                continue;
            }
        };

        let mut previous_probe: Option<(HostOverviewProbe, Instant)> = None;
        let mut consecutive_failures: u32 = 0;
        while !shutdown.load(Ordering::Relaxed) {
            // 真实 SSH RTT：单独对 channel open 确认计时（≈1 个往返），
            // 不能用采集 exec 的耗时——那包含远端脚本执行 + 输出回传，比 ping 大数倍
            let rtt_started = Instant::now();
            let latency = session.measure_rtt(RTT_TIMEOUT).await.ok();
            ho_debug!(
                "rtt host={host_label} elapsed={:?} latency={latency:?}",
                rtt_started.elapsed()
            );
            let started = Instant::now();
            let exec_result = session
                .exec_command(HOST_OVERVIEW_COLLECT_COMMAND, COLLECT_TIMEOUT)
                .await;
            ho_debug!(
                "exec host={host_label} elapsed={:?} ok={}",
                started.elapsed(),
                exec_result.is_ok()
            );
            match exec_result {
                Ok(output) => match parse_probe_output(&String::from_utf8_lossy(&output.stdout)) {
                    Ok(probe) => {
                        consecutive_failures = 0;
                        let previous = previous_probe.as_ref().map(|(previous, at)| {
                            (previous, started.saturating_duration_since(*at))
                        });
                        let mut snapshot = snapshot_from_probe(probe.clone(), previous, latency);
                        snapshot.host = host_label.clone();
                        let default_network_interface = snapshot
                            .network
                            .as_ref()
                            .map(|network| network.interface.clone());
                        for network in &mut snapshot.networks {
                            if let Some(point) = network.history.last().cloned() {
                                let history =
                                    histories.entry(network.interface.clone()).or_default();
                                history.push_back(point);
                                while history.len() > HISTORY_LIMIT {
                                    history.pop_front();
                                }
                                network.history = history.iter().cloned().collect();
                            }
                        }
                        snapshot.network = default_network_interface
                            .and_then(|interface| {
                                snapshot
                                    .networks
                                    .iter()
                                    .find(|network| network.interface == interface)
                                    .cloned()
                            })
                            .or_else(|| chosen_network_metric(&snapshot.networks).cloned());
                        previous_probe = Some((probe, started));
                        if tx
                            .send(HostOverviewEvent::Snapshot(snapshot))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        ho_debug!("parse err host={host_label} error={error}");
                        if tx.send(HostOverviewEvent::Error(error)).await.is_err() {
                            return;
                        }
                    }
                },
                Err(error) => {
                    consecutive_failures += 1;
                    ho_debug!("exec fail #{consecutive_failures} host={host_label} error={error}");
                    // 单次抖动：保留上一帧数据、不拆连接，下轮重试；
                    // 连续多次才判定连接已死，报错并重连
                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        if tx.send(HostOverviewEvent::Error(error)).await.is_err() {
                            return;
                        }
                        session.close().await;
                        break;
                    }
                }
            }
            // 周期≈refresh_interval：扣掉本轮 rtt+exec 已耗时
            sleep_or_shutdown(
                refresh_interval.saturating_sub(rtt_started.elapsed()),
                &shutdown,
            )
            .await;
        }
    }
}

async fn connect_authenticated_session(config: &RemoteSshConfig) -> Result<SshSession, String> {
    validate_monitor_config(config)?;
    let host = config.host.trim();
    let username = config.username.trim();
    let connect_timeout_secs = u64::from(config.tcp_connect_timeout.clamp(5, 60));
    let mut session = tokio::time::timeout(
        Duration::from_secs(connect_timeout_secs),
        SshSession::connect(
            host,
            config.port,
            SshConnectOptions {
                keep_alive_enabled: config.keep_alive_enabled,
                keep_alive_interval_secs: config.keep_alive_interval.clamp(10, 300),
                keep_alive_max_failures: config.keep_alive_max_failures.clamp(1, 10),
            },
        ),
    )
    .await
    .map_err(|_| format!("host overview TCP timeout after {connect_timeout_secs}s"))?
    .map_err(|error| format!("host overview SSH connection failed: {error}"))?;

    let auth_timeout_secs = u64::from(config.auth_timeout.clamp(10, 120));
    let auth_result = if config.auth_method.eq_ignore_ascii_case("key") {
        let key_data = config
            .private_key
            .as_deref()
            .ok_or_else(|| "host overview private key is empty".to_string())
            .and_then(resolve_private_key_data)?;
        let key_passphrase = config
            .key_passphrase
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let ca_cert_data = config
            .ca_cert
            .as_deref()
            .map(resolve_ca_cert_data)
            .transpose()?;
        let ca_cert = ca_cert_data.as_deref();
        tokio::time::timeout(
            Duration::from_secs(auth_timeout_secs),
            session.auth_key(username, &key_data, key_passphrase, ca_cert),
        )
        .await
        .map_err(|_| format!("host overview authentication timeout after {auth_timeout_secs}s"))?
    } else {
        let password = config
            .password
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "host overview password is empty".to_string())?;
        tokio::time::timeout(
            Duration::from_secs(auth_timeout_secs),
            session.auth_password(username, password),
        )
        .await
        .map_err(|_| format!("host overview authentication timeout after {auth_timeout_secs}s"))?
    };

    auth_result.map_err(|error| format!("host overview authentication failed: {error}"))?;
    Ok(session)
}

async fn sleep_or_shutdown(duration: Duration, shutdown: &AtomicBool) {
    let deadline = Instant::now() + duration;
    while !shutdown.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        tokio::time::sleep((deadline - now).min(Duration::from_millis(100))).await;
    }
}

fn validate_monitor_config(config: &RemoteSshConfig) -> Result<(), String> {
    if config.host.trim().is_empty() {
        return Err("host overview host is empty".to_string());
    }
    if config.username.trim().is_empty() {
        return Err("host overview username is empty".to_string());
    }
    if config.auth_method.eq_ignore_ascii_case("key") {
        if config
            .private_key
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            return Err("host overview private key is empty".to_string());
        }
    } else if config
        .password
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        return Err("host overview password is empty".to_string());
    }
    Ok(())
}

fn parse_uptime_seconds(lines: &[String]) -> Option<u64> {
    let seconds = lines
        .iter()
        .find_map(|line| line.split_whitespace().next()?.parse::<f64>().ok())?;
    Some(seconds.max(0.0) as u64)
}

fn parse_load_average(lines: &[String]) -> Option<[f32; 3]> {
    for line in lines {
        let values = line
            .split_whitespace()
            .filter_map(|part| part.parse::<f32>().ok())
            .take(3)
            .collect::<Vec<_>>();
        if values.len() == 3 {
            return Some([values[0], values[1], values[2]]);
        }
    }
    None
}

fn parse_memory_metric(lines: &[String], total_key: &str, free_key: &str) -> Option<UsageMetric> {
    let total = meminfo_kb(lines, total_key)?.saturating_mul(1024);
    let free = meminfo_kb(lines, free_key)?.saturating_mul(1024);
    if total == 0 {
        return None;
    }
    let used = total.saturating_sub(free);
    Some(UsageMetric {
        used_bytes: used,
        total_bytes: total,
        percent: ((used as f32 / total as f32) * 100.0).clamp(0.0, 100.0),
    })
}

fn meminfo_kb(lines: &[String], key: &str) -> Option<u64> {
    lines.iter().find_map(|line| {
        let (line_key, rest) = line.split_once(':')?;
        (line_key == key).then(|| rest.split_whitespace().next()?.parse::<u64>().ok())?
    })
}

fn parse_cpu_counters(lines: &[String]) -> Option<CpuCounters> {
    for line in lines {
        let mut parts = line.split_whitespace();
        if parts.next()? != "cpu" {
            continue;
        }
        let values = parts
            .filter_map(|part| part.parse::<u64>().ok())
            .collect::<Vec<_>>();
        if values.len() < 4 {
            continue;
        }
        let idle = values.get(3).copied().unwrap_or(0) + values.get(4).copied().unwrap_or(0);
        let total = values.iter().copied().sum();
        return Some(CpuCounters { total, idle });
    }
    None
}

fn parse_cpu_cores(lines: &[String]) -> Option<u32> {
    lines
        .iter()
        .find_map(|line| line.trim().parse::<u32>().ok())
        .filter(|count| *count > 0)
}

fn parse_network_counters(lines: &[String]) -> Vec<NetworkDeviceCounters> {
    lines
        .iter()
        .filter_map(|line| {
            let (interface, rest) = line.split_once(':')?;
            let values = rest
                .split_whitespace()
                .filter_map(|part| part.parse::<u64>().ok())
                .collect::<Vec<_>>();
            if values.len() < 16 {
                return None;
            }
            Some(NetworkDeviceCounters {
                interface: interface.trim().to_string(),
                counters: NetworkCounters {
                    rx_bytes: values[0],
                    tx_bytes: values[8],
                },
            })
        })
        .collect()
}

/// 解析 /proc/diskstats，聚合所有整盘的累计扇区（排除分区/虚拟设备，避免重复计数）。
fn parse_diskstats(lines: &[String]) -> Option<DiskIoCounters> {
    let mut read_sectors = 0u64;
    let mut write_sectors = 0u64;
    let mut any = false;
    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 10 {
            continue;
        }
        if !is_whole_disk(parts[2]) {
            continue;
        }
        read_sectors = read_sectors.saturating_add(parts[5].parse::<u64>().unwrap_or(0));
        write_sectors = write_sectors.saturating_add(parts[9].parse::<u64>().unwrap_or(0));
        any = true;
    }
    any.then_some(DiskIoCounters {
        read_sectors,
        write_sectors,
    })
}

/// 整盘判定：排除虚拟设备与分区（整盘统计已含其分区，重复累加会翻倍）。
fn is_whole_disk(name: &str) -> bool {
    const VIRT: [&str; 6] = ["loop", "ram", "dm-", "sr", "md", "zram"];
    if VIRT.iter().any(|prefix| name.starts_with(prefix)) {
        return false;
    }
    if name.starts_with("nvme") || name.starts_with("mmcblk") {
        // 分区形如 nvme0n1p1 / mmcblk0p1，整盘无 pN 后缀
        return !name.contains('p');
    }
    // sd/vd/hd/xvd：整盘字母结尾，分区数字结尾
    !name.chars().last().is_some_and(|c| c.is_ascii_digit())
}

fn network_metrics_from_probe(
    current_networks: &[NetworkDeviceCounters],
    previous: Option<(&HostOverviewProbe, Duration)>,
) -> Vec<NetworkMetric> {
    let real_networks = current_networks
        .iter()
        .filter(|item| item.interface != "lo")
        .collect::<Vec<_>>();
    let visible_networks = if real_networks.is_empty() {
        current_networks.iter().collect::<Vec<_>>()
    } else {
        real_networks
    };

    visible_networks
        .into_iter()
        .map(|current| {
            let (rx_bytes_per_sec, tx_bytes_per_sec) = previous
                .and_then(|(prev, elapsed)| {
                    let prev_net = prev
                        .networks
                        .iter()
                        .find(|item| item.interface == current.interface)?;
                    let seconds = elapsed.as_secs_f64();
                    if seconds <= 0.0 {
                        return None;
                    }
                    let rx = current
                        .counters
                        .rx_bytes
                        .saturating_sub(prev_net.counters.rx_bytes)
                        as f64
                        / seconds;
                    let tx = current
                        .counters
                        .tx_bytes
                        .saturating_sub(prev_net.counters.tx_bytes)
                        as f64
                        / seconds;
                    Some((rx.round() as u64, tx.round() as u64))
                })
                .unwrap_or((0, 0));

            NetworkMetric {
                interface: current.interface.clone(),
                rx_bytes_per_sec,
                tx_bytes_per_sec,
                history: vec![NetworkRatePoint {
                    rx_bytes_per_sec,
                    tx_bytes_per_sec,
                }],
            }
        })
        .collect()
}

fn parse_disks(lines: &[String]) -> Vec<DiskMetric> {
    lines
        .iter()
        .filter_map(|line| {
            // df -P -k 输出：Filesystem 1024-blocks Used Available Capacity Mount
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 6 {
                return None;
            }
            let total_kb = parts[parts.len() - 5].parse::<u64>().ok()?;
            let used_kb = parts[parts.len() - 4].parse::<u64>().ok()?;
            let available_kb = parts[parts.len() - 3].parse::<u64>().ok()?;
            let mount = parts[parts.len() - 1].to_string();
            // 文件系统名可能含空格，取末 5 列之前的全部
            let filesystem = parts[..parts.len() - 5].join(" ");
            if mount.is_empty() || total_kb == 0 {
                return None;
            }
            let total_bytes = total_kb.saturating_mul(1024);
            let used_bytes = used_kb.saturating_mul(1024);
            let available_bytes = available_kb.saturating_mul(1024);
            let percent = ((used_bytes as f32 / total_bytes as f32) * 100.0).clamp(0.0, 100.0);
            Some(DiskMetric {
                mount,
                filesystem,
                used_bytes,
                total_bytes,
                available_bytes,
                percent,
            })
        })
        .collect()
}

fn parse_processes(lines: &[String], exe_map: &HashMap<u32, String>) -> Vec<ProcessMetric> {
    lines
        .iter()
        .filter_map(|line| {
            // 格式：pid user rss pcpu comm args...
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse::<u32>().ok()?;
            let user = parts.next()?.to_string();
            let rss_kb = parts.next()?.parse::<u64>().ok()?;
            let cpu_percent = parts.next()?.parse::<f32>().ok()?;
            let command = parts.next()?.to_string();
            let args = parts.collect::<Vec<_>>().join(" ");
            Some(ProcessMetric {
                pid,
                user,
                rss_bytes: rss_kb.saturating_mul(1024),
                cpu_percent,
                command,
                args,
                exe_path: exe_map.get(&pid).cloned(),
            })
        })
        .collect()
}

fn parse_exe_links(lines: &[String]) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for line in lines {
        let mut parts = line.splitn(2, '\t');
        let pid = match parts.next().and_then(|s| s.parse::<u32>().ok()) {
            Some(pid) => pid,
            None => continue,
        };
        if let Some(path) = parts.next() {
            if !path.is_empty() {
                map.insert(pid, path.to_string());
            }
        }
    }
    map
}

#[derive(Clone, Debug)]
struct SsRecord {
    proto: SocketProto,
    state: String,
    local_addr: String,
    local_port: u16,
    peer_addr: String,
    peer_port: Option<u16>,
    pid: Option<u32>,
    process: Option<String>,
    bytes_received: u64,
    bytes_sent: u64,
}

/// 把 ss -i 的多行输出合并为单条 record：以非空白开头另起一条
fn collect_ss_records(lines: &[String], proto: SocketProto) -> Vec<SsRecord> {
    let mut joined: Vec<String> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let starts_record = line
            .chars()
            .next()
            .map(|c| !c.is_whitespace())
            .unwrap_or(false);
        if starts_record || joined.is_empty() {
            joined.push(line.clone());
        } else if let Some(last) = joined.last_mut() {
            last.push(' ');
            last.push_str(line.trim());
        }
    }

    joined
        .into_iter()
        .filter_map(|record| parse_ss_record(&record, proto))
        .collect()
}

fn parse_ss_record(line: &str, proto: SocketProto) -> Option<SsRecord> {
    let mut tokens = line.split_whitespace();
    let state = tokens.next()?.to_string();
    let _recv_q = tokens.next()?;
    let _send_q = tokens.next()?;
    let local = tokens.next()?;
    let peer = tokens.next()?;
    let (local_addr, local_port) = split_socket_addr(local)?;
    let (peer_addr, peer_port) = split_socket_addr(peer)
        .map(|(a, p)| (a, Some(p)))
        .unwrap_or_else(|| (peer.to_string(), None));

    let rest = &line[line.find(peer).unwrap_or(0) + peer.len()..];
    let (pid, process) = extract_pid_process(rest);
    let bytes_received = extract_kv_u64(rest, "bytes_received:");
    // bytes_acked 是已被对端确认的发送字节，更接近"实际发出"
    let bytes_sent = extract_kv_u64(rest, "bytes_acked:").max(extract_kv_u64(rest, "bytes_sent:"));

    Some(SsRecord {
        proto,
        state,
        local_addr,
        local_port,
        peer_addr,
        peer_port,
        pid,
        process,
        bytes_received,
        bytes_sent,
    })
}

/// 拆 "ip:port"，支持 IPv6 [::]:80 / *:80 / 0.0.0.0:* 等
fn split_socket_addr(input: &str) -> Option<(String, u16)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (host, port) = trimmed.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = port.parse::<u16>().ok()?;
    Some((host.to_string(), port))
}

/// 提取 users:(("name",pid=N,fd=N)) 中第一组 pid 和 name
fn extract_pid_process(rest: &str) -> (Option<u32>, Option<String>) {
    let Some(start) = rest.find("users:((") else {
        return (None, None);
    };
    let segment = &rest[start + "users:((".len()..];
    let Some(end) = segment.find("))") else {
        return (None, None);
    };
    let body = &segment[..end];
    // body 形如 "name",pid=N,fd=N
    let name = body
        .split_once('"')
        .and_then(|(_, tail)| tail.split_once('"'))
        .map(|(name, _)| name.to_string());
    let pid = body
        .split(',')
        .find_map(|part| part.trim().strip_prefix("pid="))
        .and_then(|value| value.parse::<u32>().ok());
    (pid, name)
}

fn extract_kv_u64(rest: &str, key: &str) -> u64 {
    rest.find(key)
        .and_then(|idx| {
            let tail = &rest[idx + key.len()..];
            let end = tail.find(|c: char| c.is_whitespace()).unwrap_or(tail.len());
            tail[..end].parse::<u64>().ok()
        })
        .unwrap_or(0)
}

fn is_wildcard_addr(addr: &str) -> bool {
    matches!(addr, "0.0.0.0" | "::" | "*")
}

fn is_listen_state(state: &str, proto: SocketProto) -> bool {
    match proto {
        SocketProto::Tcp => state.eq_ignore_ascii_case("LISTEN"),
        // UDP 没有真正 LISTEN，ss 报为 UNCONN；把 UDP 的 UNCONN 视作监听
        SocketProto::Udp => state.eq_ignore_ascii_case("UNCONN"),
    }
}

fn is_established_state(state: &str, proto: SocketProto) -> bool {
    match proto {
        SocketProto::Tcp => state.eq_ignore_ascii_case("ESTAB"),
        SocketProto::Udp => state.eq_ignore_ascii_case("ESTAB"),
    }
}

fn parse_sockets(tcp_lines: &[String], udp_lines: &[String]) -> Vec<NetworkRow> {
    let mut records = collect_ss_records(tcp_lines, SocketProto::Tcp);
    records.extend(collect_ss_records(udp_lines, SocketProto::Udp));

    let mut listens: Vec<NetworkRow> = Vec::new();
    let mut estab: Vec<SsRecord> = Vec::new();
    for r in records {
        if is_listen_state(&r.state, r.proto) {
            listens.push(NetworkRow {
                kind: NetworkRowKind::Listen,
                proto: r.proto,
                pid: r.pid,
                process: r.process.unwrap_or_default(),
                local_addr: r.local_addr,
                local_port: r.local_port,
                remote_addr: None,
                remote_port: None,
                unique_ips: 0,
                connections: 0,
                rx_bytes: 0,
                tx_bytes: 0,
            });
        } else if is_established_state(&r.state, r.proto) {
            estab.push(r);
        }
    }

    // 每条 ESTAB 归属唯一监听行：精确 local_addr 命中优先，回退到通配 (0.0.0.0/::)
    // 避免同端口多 IP 监听时被重复累计
    let mut listen_peers: Vec<std::collections::HashSet<String>> = listens
        .iter()
        .map(|_| std::collections::HashSet::new())
        .collect();
    let mut outbound_pool: Vec<SsRecord> = Vec::new();
    for est in estab {
        let exact = listens.iter().position(|row| {
            row.proto == est.proto
                && row.local_port == est.local_port
                && row.local_addr == est.local_addr
        });
        let target = exact.or_else(|| {
            listens.iter().position(|row| {
                row.proto == est.proto
                    && row.local_port == est.local_port
                    && is_wildcard_addr(&row.local_addr)
            })
        });
        match target {
            Some(idx) => {
                listen_peers[idx].insert(est.peer_addr.clone());
                listens[idx].connections = listens[idx].connections.saturating_add(1);
                listens[idx].rx_bytes = listens[idx].rx_bytes.saturating_add(est.bytes_received);
                listens[idx].tx_bytes = listens[idx].tx_bytes.saturating_add(est.bytes_sent);
            }
            None => outbound_pool.push(est),
        }
    }
    for (idx, row) in listens.iter_mut().enumerate() {
        row.unique_ips = listen_peers[idx].len() as u32;
    }

    let mut outbound: Vec<NetworkRow> = outbound_pool
        .into_iter()
        .map(|e| NetworkRow {
            kind: NetworkRowKind::Outbound,
            proto: e.proto,
            pid: e.pid,
            process: e.process.unwrap_or_default(),
            local_addr: e.local_addr,
            local_port: e.local_port,
            remote_addr: Some(e.peer_addr),
            remote_port: e.peer_port,
            unique_ips: 1,
            connections: 1,
            rx_bytes: e.bytes_received,
            tx_bytes: e.bytes_sent,
        })
        .collect();

    let mut rows = listens;
    rows.append(&mut outbound);
    rows
}

fn chosen_network(networks: &[NetworkDeviceCounters]) -> Option<&NetworkDeviceCounters> {
    networks
        .iter()
        .filter(|item| item.interface != "lo")
        .max_by_key(|item| {
            item.counters
                .rx_bytes
                .saturating_add(item.counters.tx_bytes)
        })
        .or_else(|| networks.iter().max_by_key(|item| item.counters.rx_bytes))
}

fn chosen_network_metric(networks: &[NetworkMetric]) -> Option<&NetworkMetric> {
    networks.iter().max_by_key(|network| {
        network
            .rx_bytes_per_sec
            .saturating_add(network.tx_bytes_per_sec)
    })
}

fn resolve_private_key_data(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.contains("BEGIN ") || trimmed.contains('\n') {
        return Ok(trimmed.to_string());
    }

    let path = expand_tilde(trimmed);
    if path.is_file() {
        return fs::read_to_string(&path)
            .map_err(|error| format!("failed to read host overview private key: {error}"));
    }

    Err("host overview private key is neither key content nor a readable file path".to_string())
}

fn resolve_ca_cert_data(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("host overview certificate value is empty".to_string());
    }

    let path = expand_tilde(trimmed);
    if path.is_file() {
        return fs::read_to_string(&path)
            .map_err(|error| format!("failed to read host overview certificate: {error}"));
    }

    Ok(trimmed.to_string())
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}
