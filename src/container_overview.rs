//! 容器采集：远端 docker CLI over SSH exec，复刻 host_overview 采集模式。
//! 一条 shell 脚本探测 docker、拉 `docker ps -a` 与 `docker stats`，解析成快照推回 UI。

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::Deserialize;

use crate::{
    host_overview::{connect_authenticated_session, sleep_or_shutdown},
    terminal_runtime::RemoteSshConfig,
};

/// 采集脚本：section 式输出。缺 docker 输出 `[nodocker]`；`docker ps` 失败输出 `[pserr]` 带 stderr。
pub const CONTAINER_COLLECT_COMMAND: &str = r#"printf '%s\n' 'NEXSHELL_CONTAINER_V1'
if ! command -v docker >/dev/null 2>&1; then
  printf '%s\n' '[nodocker]'
  exit 0
fi
ps_out=$(docker ps -a --format '{{json .}}' 2>&1); ps_rc=$?
if [ "$ps_rc" -ne 0 ]; then
  printf '%s\n' '[pserr]'
  printf '%s\n' "$ps_out"
  exit 0
fi
printf '%s\n' '[ps]'
printf '%s\n' "$ps_out"
printf '%s\n' '[stats]'
docker stats --no-stream --format '{{json .}}' 2>/dev/null"#;

const MAGIC: &str = "NEXSHELL_CONTAINER_V1";
/// 采集 exec 超时：docker stats --no-stream 自带 ~2s 采样，给足余量。
const COLLECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 连续采集失败多少次才判定连接已死、重连。
const MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// action_error 横幅最短展示时长，避免被 5s 一帧的快照秒清。
const ACTION_ERROR_MIN_DISPLAY: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerState {
    Running,
    Exited,
    Paused,
    Restarting,
    Other,
}

impl ContainerState {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "running" => ContainerState::Running,
            "exited" => ContainerState::Exited,
            "paused" => ContainerState::Paused,
            "restarting" => ContainerState::Restarting,
            _ => ContainerState::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerHealth {
    Healthy,
    Unhealthy,
    Starting,
}

impl ContainerHealth {
    /// 从 `docker ps` 的 Status 文本提取健康态，如 "Up 3 days (healthy)"。
    fn parse(status_text: &str) -> Option<Self> {
        let lower = status_text.to_ascii_lowercase();
        if lower.contains("(unhealthy)") {
            Some(ContainerHealth::Unhealthy)
        } else if lower.contains("(health: starting)") {
            Some(ContainerHealth::Starting)
        } else if lower.contains("(healthy)") {
            Some(ContainerHealth::Healthy)
        } else {
            None
        }
    }
}

/// 累计量（docker stats 原生即累计，v1 不做差分速率）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContainerStats {
    pub cpu_percent: f32,
    pub mem_usage_bytes: u64,
    pub mem_limit_bytes: u64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub block_read_bytes: u64,
    pub block_write_bytes: u64,
    pub pids: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: ContainerState,
    /// 人话状态，如 "Up 3 days (healthy)" / "Exited (1) 2 years ago"。
    pub status_text: String,
    pub created_at: String,
    pub health: Option<ContainerHealth>,
    pub stats: Option<ContainerStats>,
}

/// docker 探测/采集失败原因。
#[derive(Clone, Debug, PartialEq)]
pub enum ContainerProbeError {
    NoDocker,
    PermissionDenied,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContainerCollectStatus {
    Waiting,
    Collecting,
    Ready,
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContainerSnapshot {
    pub host: String,
    pub containers: Vec<ContainerInfo>,
    /// docker 层错误（无 docker / 权限不足 / 其他）。与 SSH 连接失败区分。
    pub error: Option<ContainerProbeError>,
    pub status: ContainerCollectStatus,
}

impl ContainerSnapshot {
    pub fn waiting(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            containers: Vec::new(),
            error: None,
            status: ContainerCollectStatus::Waiting,
        }
    }

    pub fn error(host: impl Into<String>, error: impl Into<String>) -> Self {
        let mut snapshot = Self::waiting(host);
        snapshot.status = ContainerCollectStatus::Error(error.into());
        snapshot
    }

    pub fn has_collected_data(&self) -> bool {
        !self.containers.is_empty() || self.error.is_some()
    }
}

/// Collecting 占位帧合并：已有数据时保留上帧，仅标 Collecting，避免闪烁。
pub fn merge_container_snapshot(
    current: &ContainerSnapshot,
    incoming: ContainerSnapshot,
) -> ContainerSnapshot {
    let incoming_is_placeholder =
        incoming.status == ContainerCollectStatus::Collecting && !incoming.has_collected_data();
    // 已 Ready 的空列表（主机 0 容器）也算有效数据，不许被占位帧顶掉。
    let current_settled =
        current.has_collected_data() || current.status == ContainerCollectStatus::Ready;
    if incoming_is_placeholder && current_settled {
        let mut merged = current.clone();
        if !incoming.host.trim().is_empty() {
            merged.host = incoming.host;
        }
        merged.status = ContainerCollectStatus::Collecting;
        return merged;
    }
    incoming
}

#[derive(Clone, Debug, PartialEq)]
pub enum ContainerOverviewEvent {
    Snapshot(ContainerSnapshot),
    Error(String),
    /// 一次性操作（start/stop/restart）失败：只提示、不污染采集快照。
    ActionError(String),
}

/// 渲染侧持有的 UI 状态包装（对齐 HostOverviewUiState 惯例）。
#[derive(Clone, Debug, PartialEq)]
pub struct ContainerOverviewUiState {
    pub snapshot: ContainerSnapshot,
    /// 最近一次容器操作失败信息+发生时刻。最短展示 ACTION_ERROR_MIN_DISPLAY，之后由下帧快照清除。
    pub action_error: Option<(String, Instant)>,
}

impl ContainerOverviewUiState {
    pub fn waiting(host: impl Into<String>) -> Self {
        Self {
            snapshot: ContainerSnapshot::waiting(host),
            action_error: None,
        }
    }

    pub fn apply_event(&mut self, event: ContainerOverviewEvent) {
        match event {
            ContainerOverviewEvent::Snapshot(snapshot) => {
                self.snapshot = merge_container_snapshot(&self.snapshot, snapshot);
                if self
                    .action_error
                    .as_ref()
                    .is_some_and(|(_, at)| at.elapsed() >= ACTION_ERROR_MIN_DISPLAY)
                {
                    self.action_error = None;
                }
            }
            ContainerOverviewEvent::Error(error) => {
                if self.snapshot.host.trim().is_empty() {
                    self.snapshot.host = "未连接".to_string();
                }
                self.snapshot.status = ContainerCollectStatus::Error(error);
            }
            ContainerOverviewEvent::ActionError(error) => {
                self.action_error = Some((error, Instant::now()));
            }
        }
    }
}

/// 一次性操作（start/stop/restart）。执行走 host_overview::spawn_remote_exec，本步只造命令。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
}

impl ContainerAction {
    fn verb(self) -> &'static str {
        match self {
            ContainerAction::Start => "start",
            ContainerAction::Stop => "stop",
            ContainerAction::Restart => "restart",
        }
    }
}

/// 生成 `docker <verb> '<id>'` 命令。id/name 单引号包裹并转义内部单引号。
pub fn container_action_command(action: ContainerAction, name_or_id: &str) -> String {
    format!(
        "docker {} {}",
        action.verb(),
        shell_single_quote(name_or_id)
    )
}

/// 生成日志跟随命令：`docker logs -f --tail 200 '<id>'`，转义规则同 container_action_command。
pub fn container_logs_command(name_or_id: &str) -> String {
    format!(
        "docker logs -f --tail 200 {}",
        shell_single_quote(name_or_id)
    )
}

/// 生成交互 shell 命令：优先 bash，无 bash 落回 sh。转义规则同 container_action_command。
pub fn container_shell_command(name_or_id: &str) -> String {
    format!(
        "docker exec -it {} sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi'",
        shell_single_quote(name_or_id)
    )
}

/// POSIX 单引号转义：' → '\''。
fn shell_single_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[derive(Debug)]
pub struct ContainerMonitorHandle {
    shutdown: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl ContainerMonitorHandle {
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl Drop for ContainerMonitorHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn spawn_container_monitor(
    config: RemoteSshConfig,
    refresh_interval: Duration,
) -> Result<
    (
        ContainerMonitorHandle,
        async_channel::Receiver<ContainerOverviewEvent>,
    ),
    String,
> {
    let (tx, rx) = async_channel::bounded(8);
    let shutdown = Arc::new(AtomicBool::new(false));
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::Builder::new()
        .name(format!(
            "nexshell-container-{}",
            config.host.trim().replace(['/', ':'], "_")
        ))
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = tx.try_send(ContainerOverviewEvent::Error(format!(
                        "failed to start container runtime: {error}"
                    )));
                    return;
                }
            };
            runtime.block_on(run_container_monitor(
                config,
                refresh_interval.max(Duration::from_secs(1)),
                thread_shutdown,
                tx,
            ));
        })
        .map_err(|error| format!("spawn container thread: {error}"))?;

    Ok((
        ContainerMonitorHandle {
            shutdown,
            _thread: Some(thread),
        },
        rx,
    ))
}

async fn run_container_monitor(
    config: RemoteSshConfig,
    refresh_interval: Duration,
    shutdown: Arc<AtomicBool>,
    tx: async_channel::Sender<ContainerOverviewEvent>,
) {
    let host_label = format!(
        "{}@{}:{}",
        config.username.trim(),
        config.host.trim(),
        config.port
    );

    while !shutdown.load(Ordering::Relaxed) {
        // 占位帧：标 Collecting，合并时保留上帧数据避免闪烁。
        let mut placeholder = ContainerSnapshot::waiting(host_label.clone());
        placeholder.status = ContainerCollectStatus::Collecting;
        if tx
            .send(ContainerOverviewEvent::Snapshot(placeholder))
            .await
            .is_err()
        {
            return;
        }

        let session = match connect_authenticated_session(&config).await {
            Ok(session) => session,
            Err(error) => {
                if tx.send(ContainerOverviewEvent::Error(error)).await.is_err() {
                    return;
                }
                sleep_or_shutdown(refresh_interval, &shutdown).await;
                continue;
            }
        };

        let mut consecutive_failures: u32 = 0;
        while !shutdown.load(Ordering::Relaxed) {
            let started = Instant::now();
            let exec_result = session
                .exec_command(CONTAINER_COLLECT_COMMAND, COLLECT_TIMEOUT)
                .await;
            match exec_result {
                Ok(output) => {
                    consecutive_failures = 0;
                    // docker 层错误（无 docker/权限）是"成功采集出的错误态"，非连接失败。
                    let mut snapshot =
                        parse_container_output(&String::from_utf8_lossy(&output.stdout));
                    snapshot.host = host_label.clone();
                    if tx
                        .send(ContainerOverviewEvent::Snapshot(snapshot))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => {
                    consecutive_failures += 1;
                    // 单次抖动保留上帧、不拆连接；连续多次才判定连接死、报错重连。
                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        if tx.send(ContainerOverviewEvent::Error(error)).await.is_err() {
                            return;
                        }
                        session.close().await;
                        break;
                    }
                }
            }
            sleep_or_shutdown(
                refresh_interval.saturating_sub(started.elapsed()),
                &shutdown,
            )
            .await;
        }
    }
}

// ---- 解析 ----

#[derive(Deserialize)]
struct PsJson {
    #[serde(rename = "ID", default)]
    id: String,
    #[serde(rename = "Names", default)]
    names: String,
    #[serde(rename = "Image", default)]
    image: String,
    #[serde(rename = "State", default)]
    state: String,
    #[serde(rename = "Status", default)]
    status: String,
    #[serde(rename = "CreatedAt", default)]
    created_at: String,
}

#[derive(Deserialize)]
struct StatsJson {
    #[serde(rename = "ID", default)]
    id: String,
    #[serde(rename = "CPUPerc", default)]
    cpu_perc: String,
    #[serde(rename = "MemUsage", default)]
    mem_usage: String,
    #[serde(rename = "NetIO", default)]
    net_io: String,
    #[serde(rename = "BlockIO", default)]
    block_io: String,
    #[serde(rename = "PIDs", default)]
    pids: String,
}

/// 顶层解析：分节 → 判 nodocker/pserr → 解析 ps + stats 合并。host 字段留空由监控回填。
pub fn parse_container_output(output: &str) -> ContainerSnapshot {
    if !output.lines().any(|line| line.trim() == MAGIC) {
        return ContainerSnapshot {
            host: String::new(),
            containers: Vec::new(),
            error: Some(ContainerProbeError::Other(
                "container probe missing magic header".to_string(),
            )),
            status: ContainerCollectStatus::Ready,
        };
    }

    let mut section = "";
    let mut ps_lines: Vec<&str> = Vec::new();
    let mut stats_lines: Vec<&str> = Vec::new();
    let mut pserr_lines: Vec<&str> = Vec::new();
    let mut no_docker = false;

    for raw in output.lines() {
        match raw.trim() {
            MAGIC => continue,
            "[nodocker]" => {
                no_docker = true;
                section = "";
            }
            "[pserr]" => section = "pserr",
            "[ps]" => section = "ps",
            "[stats]" => section = "stats",
            _ => match section {
                "ps" => ps_lines.push(raw),
                "stats" => stats_lines.push(raw),
                "pserr" => pserr_lines.push(raw),
                _ => {}
            },
        }
    }

    if no_docker {
        return ready_error(ContainerProbeError::NoDocker);
    }
    if !pserr_lines.is_empty() {
        let text = pserr_lines.join("\n");
        let error = if text.to_ascii_lowercase().contains("permission denied") {
            ContainerProbeError::PermissionDenied
        } else {
            ContainerProbeError::Other(text.trim().to_string())
        };
        return ready_error(error);
    }

    let stats = parse_stats_lines(&stats_lines);
    let containers = parse_ps_lines(&ps_lines, &stats);
    ContainerSnapshot {
        host: String::new(),
        containers,
        error: None,
        status: ContainerCollectStatus::Ready,
    }
}

fn ready_error(error: ContainerProbeError) -> ContainerSnapshot {
    ContainerSnapshot {
        host: String::new(),
        containers: Vec::new(),
        error: Some(error),
        status: ContainerCollectStatus::Ready,
    }
}

fn parse_stats_lines(lines: &[&str]) -> Vec<(String, ContainerStats)> {
    lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let row: StatsJson = serde_json::from_str(line.trim()).ok()?;
            let (mem_usage_bytes, mem_limit_bytes) = parse_pair(&row.mem_usage);
            let (net_rx_bytes, net_tx_bytes) = parse_pair(&row.net_io);
            let (block_read_bytes, block_write_bytes) = parse_pair(&row.block_io);
            Some((
                row.id.trim().to_string(),
                ContainerStats {
                    cpu_percent: parse_percent(&row.cpu_perc).unwrap_or(0.0),
                    mem_usage_bytes,
                    mem_limit_bytes,
                    net_rx_bytes,
                    net_tx_bytes,
                    block_read_bytes,
                    block_write_bytes,
                    pids: row.pids.trim().parse::<u32>().unwrap_or(0),
                },
            ))
        })
        .collect()
}

fn parse_ps_lines(lines: &[&str], stats: &[(String, ContainerStats)]) -> Vec<ContainerInfo> {
    lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let row: PsJson = serde_json::from_str(line.trim()).ok()?;
            let id = row.id.trim().to_string();
            if id.is_empty() {
                return None;
            }
            Some(ContainerInfo {
                health: ContainerHealth::parse(&row.status),
                stats: match_stats(&id, stats),
                state: ContainerState::parse(&row.state),
                status_text: row.status,
                created_at: row.created_at,
                name: row.names,
                image: row.image,
                id,
            })
        })
        .collect()
}

/// stats 按短 ID 前缀对齐 ps（两侧同为 12 位短 ID，容错互为前缀）。
fn match_stats(ps_id: &str, stats: &[(String, ContainerStats)]) -> Option<ContainerStats> {
    stats
        .iter()
        .find(|(sid, _)| sid == ps_id || sid.starts_with(ps_id) || ps_id.starts_with(sid.as_str()))
        .map(|(_, s)| *s)
}

/// 拆 "457.2MiB / 15.66GiB" → (usage, limit) bytes。缺失/无法解析给 0。
fn parse_pair(raw: &str) -> (u64, u64) {
    let mut parts = raw.split('/');
    let left = parts.next().map(parse_size).unwrap_or(None).unwrap_or(0);
    let right = parts.next().map(parse_size).unwrap_or(None).unwrap_or(0);
    (left, right)
}

/// docker 混用十进制(kB/MB/GB)与二进制(KiB/MiB/GiB)单位，两套都认。
fn parse_size(raw: &str) -> Option<u64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    // 数值前缀（含小数）与单位后缀分界。
    let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let value = num.trim().parse::<f64>().ok()?;
    let mult: f64 = match unit.trim() {
        "" | "B" => 1.0,
        "kB" | "KB" => 1e3,
        "KiB" => 1024.0,
        "MB" => 1e6,
        "MiB" => 1024.0 * 1024.0,
        "GB" => 1e9,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TB" => 1e12,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((value * mult).round() as u64)
}

fn parse_percent(raw: &str) -> Option<f32> {
    raw.trim().trim_end_matches('%').trim().parse::<f32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_handles_decimal_and_binary_units() {
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(parse_size("126B"), Some(126));
        assert_eq!(parse_size("939kB"), Some(939_000));
        assert_eq!(parse_size("1.84kB"), Some(1_840));
        assert_eq!(parse_size("391.2MiB"), Some(410_202_931));
        assert_eq!(parse_size("15.66GiB"), Some(16_814_796_964));
        assert_eq!(parse_size("330GB"), Some(330_000_000_000));
        assert_eq!(parse_size("1KiB"), Some(1024));
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("bogus"), None);
    }

    #[test]
    fn parse_percent_strips_sign() {
        assert_eq!(parse_percent("0.05%"), Some(0.05));
        assert_eq!(parse_percent("1.10%"), Some(1.10));
        assert_eq!(parse_percent(""), None);
    }

    #[test]
    fn parse_pair_splits_usage_and_limit() {
        assert_eq!(parse_pair("457.2MiB / 15.66GiB").0, 479_408_947);
        assert_eq!(parse_pair("1.84kB / 126B"), (1_840, 126));
        assert_eq!(parse_pair("0B / 0B"), (0, 0));
    }

    #[test]
    fn parses_ps_and_merges_stats_by_short_id() {
        let output = "\
NEXSHELL_CONTAINER_V1
[ps]
{\"CreatedAt\":\"2026-06-26 22:59:55 -0700 PDT\",\"ID\":\"8fc30568616f\",\"Image\":\"moby/buildkit:buildx-stable-1\",\"Names\":\"buildx_buildkit\",\"State\":\"running\",\"Status\":\"Up 10 days\"}
{\"CreatedAt\":\"2026-04-25 20:50:31 -0700 PDT\",\"ID\":\"35742645a6e9\",\"Image\":\"clipbridge:latest\",\"Names\":\"clipbridge-test\",\"State\":\"exited\",\"Status\":\"Exited (137) 8 weeks ago\"}
[stats]
{\"BlockIO\":\"330GB / 6.68MB\",\"CPUPerc\":\"0.03%\",\"Container\":\"8fc30568616ffa434c141df4cc2afe555ffc8d851333c6b714d7ffb3da9a6a8b\",\"ID\":\"8fc30568616f\",\"MemPerc\":\"0.11%\",\"MemUsage\":\"17.5MiB / 15.66GiB\",\"Name\":\"buildx_buildkit\",\"NetIO\":\"939kB / 61.9kB\",\"PIDs\":\"16\"}";
        let snapshot = parse_container_output(output);
        assert!(snapshot.error.is_none());
        assert_eq!(snapshot.containers.len(), 2);

        let running = &snapshot.containers[0];
        assert_eq!(running.id, "8fc30568616f");
        assert_eq!(running.state, ContainerState::Running);
        let stats = running.stats.expect("running container has stats");
        assert_eq!(stats.pids, 16);
        assert_eq!(stats.net_rx_bytes, 939_000);
        assert_eq!(stats.block_read_bytes, 330_000_000_000);
        assert!((stats.cpu_percent - 0.03).abs() < 1e-4);

        // exited 容器无 stats 行。
        let exited = &snapshot.containers[1];
        assert_eq!(exited.state, ContainerState::Exited);
        assert!(exited.stats.is_none());
    }

    #[test]
    fn extracts_health_from_status_text() {
        assert_eq!(
            ContainerHealth::parse("Up 3 days (healthy)"),
            Some(ContainerHealth::Healthy)
        );
        assert_eq!(
            ContainerHealth::parse("Up 2 minutes (unhealthy)"),
            Some(ContainerHealth::Unhealthy)
        );
        assert_eq!(
            ContainerHealth::parse("Up 5 seconds (health: starting)"),
            Some(ContainerHealth::Starting)
        );
        assert_eq!(ContainerHealth::parse("Up 10 days"), None);
        assert_eq!(ContainerHealth::parse("Exited (0) 6 months ago"), None);
    }

    #[test]
    fn classifies_no_docker() {
        let output = "NEXSHELL_CONTAINER_V1\n[nodocker]\n";
        let snapshot = parse_container_output(output);
        assert_eq!(snapshot.error, Some(ContainerProbeError::NoDocker));
        assert!(snapshot.containers.is_empty());
    }

    #[test]
    fn classifies_permission_denied() {
        let output = "\
NEXSHELL_CONTAINER_V1
[pserr]
permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock";
        let snapshot = parse_container_output(output);
        assert_eq!(snapshot.error, Some(ContainerProbeError::PermissionDenied));
    }

    #[test]
    fn classifies_other_ps_error() {
        let output = "\
NEXSHELL_CONTAINER_V1
[pserr]
Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?";
        let snapshot = parse_container_output(output);
        match snapshot.error {
            Some(ContainerProbeError::Other(text)) => assert!(text.contains("Cannot connect")),
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn action_command_escapes_single_quotes() {
        assert_eq!(
            container_action_command(ContainerAction::Restart, "web"),
            "docker restart 'web'"
        );
        assert_eq!(
            container_action_command(ContainerAction::Start, "a'b"),
            "docker start 'a'\\''b'"
        );
        assert_eq!(
            container_action_command(ContainerAction::Stop, "8fc30568616f"),
            "docker stop '8fc30568616f'"
        );
    }

    #[test]
    fn logs_command_escapes_single_quotes() {
        assert_eq!(
            container_logs_command("web"),
            "docker logs -f --tail 200 'web'"
        );
        assert_eq!(
            container_logs_command("a'b"),
            "docker logs -f --tail 200 'a'\\''b'"
        );
    }

    #[test]
    fn shell_command_prefers_bash_falls_back_to_sh() {
        assert_eq!(
            container_shell_command("web"),
            "docker exec -it 'web' sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi'"
        );
        assert_eq!(
            container_shell_command("a'b"),
            "docker exec -it 'a'\\''b' sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi'"
        );
    }

    #[test]
    fn collecting_placeholder_preserves_last_containers() {
        let mut current = ContainerSnapshot::waiting("root@h:22");
        current.containers = vec![ContainerInfo {
            id: "abc".into(),
            name: "web".into(),
            image: "nginx".into(),
            state: ContainerState::Running,
            status_text: "Up 1 day".into(),
            created_at: String::new(),
            health: None,
            stats: None,
        }];
        current.status = ContainerCollectStatus::Ready;

        let mut placeholder = ContainerSnapshot::waiting("root@h:22");
        placeholder.status = ContainerCollectStatus::Collecting;

        let merged = merge_container_snapshot(&current, placeholder);
        assert_eq!(merged.status, ContainerCollectStatus::Collecting);
        assert_eq!(merged.containers.len(), 1);
    }

    #[test]
    fn collecting_placeholder_preserves_ready_empty_list() {
        // 主机 0 容器：Ready 空列表是有效数据，重连占位帧不许把状态打回 Collecting 空态。
        let mut current = ContainerSnapshot::waiting("root@h:22");
        current.status = ContainerCollectStatus::Ready;

        let mut placeholder = ContainerSnapshot::waiting("root@h:22");
        placeholder.status = ContainerCollectStatus::Collecting;

        let merged = merge_container_snapshot(&current, placeholder);
        assert_eq!(merged.status, ContainerCollectStatus::Collecting);
        assert!(merged.containers.is_empty());
        assert!(merged.error.is_none());
    }

    #[test]
    fn missing_magic_yields_error() {
        let snapshot = parse_container_output("garbage output\n");
        assert!(matches!(
            snapshot.error,
            Some(ContainerProbeError::Other(_))
        ));
    }
}
