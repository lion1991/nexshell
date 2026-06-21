//! 多主机状态总览：把单主机 host_overview monitor 批量化成「舰队」。
//! 纯编排——采集/解析全复用 host_overview，本模块只管启停与状态聚合。

use std::collections::HashMap;
use std::time::Duration;

use crate::host_management::HostConnectionConfig;
use crate::host_overview::{
    remote_ssh_config_from_host_config, spawn_host_overview_monitor, HostOverviewEvent,
    HostOverviewMonitorHandle, HostOverviewUiState,
};

/// 各主机采集刷新周期（与单主机一致量级）。
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

struct FleetEntry {
    ui: HostOverviewUiState,
    _handle: Option<HostOverviewMonitorHandle>, // Drop 即 stop
}

/// 一台主机起监控后交回调用方消费的事件流。
pub type FleetStream = (String, async_channel::Receiver<HostOverviewEvent>);

/// 多主机监控舰队。RootView 进入状态总览视图时 start，离开时 stop_all。
#[derive(Default)]
pub struct HostOverviewFleet {
    entries: HashMap<String, FleetEntry>,
}

impl HostOverviewFleet {
    pub fn new() -> Self {
        Self::default()
    }

    /// 对每台主机起监控（已在跑的跳过，幂等）。返回新起的事件流，
    /// 由调用方用 spawn_stream_local 消费后回调 `apply_event`。
    // TODO: 主机数很大时这里会一次性起 N 个连接线程，后续可加并发闸门（信号量分批）。
    pub fn start(&mut self, hosts: &[(String, HostConnectionConfig)]) -> Vec<FleetStream> {
        let mut streams = Vec::new();
        for (host_id, config) in hosts {
            if self.entries.contains_key(host_id) {
                continue;
            }
            let display = config.host.clone();
            match spawn_host_overview_monitor(
                remote_ssh_config_from_host_config(config),
                REFRESH_INTERVAL,
            ) {
                Ok((handle, receiver)) => {
                    self.entries.insert(
                        host_id.clone(),
                        FleetEntry {
                            ui: HostOverviewUiState::waiting(display),
                            _handle: Some(handle),
                        },
                    );
                    streams.push((host_id.clone(), receiver));
                }
                Err(error) => {
                    // 起线程就失败：直接落错状态，不交事件流。
                    let mut ui = HostOverviewUiState::waiting(display);
                    ui.apply_event(HostOverviewEvent::Error(error));
                    self.entries
                        .insert(host_id.clone(), FleetEntry { ui, _handle: None });
                }
            }
        }
        streams
    }

    /// 把某台主机的采集事件合并进它的 UI 状态。
    pub fn apply_event(&mut self, host_id: &str, event: HostOverviewEvent) {
        if let Some(entry) = self.entries.get_mut(host_id) {
            entry.ui.apply_event(event);
        }
    }

    /// 取某台主机当前状态（渲染卡片用）。
    pub fn state(&self, host_id: &str) -> Option<&HostOverviewUiState> {
        self.entries.get(host_id).map(|entry| &entry.ui)
    }

    /// 停止全部监控并清空（Drop handle 即 stop）。
    pub fn stop_all(&mut self) {
        self.entries.clear();
    }

    pub fn is_running(&self) -> bool {
        !self.entries.is_empty()
    }
}
