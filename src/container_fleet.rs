//! 多主机容器编排：把单主机 container monitor 批量化成「舰队」。
//! 纯编排——采集/解析全复用 container_overview，本模块只管启停与状态聚合。

use std::collections::HashMap;
use std::time::Duration;

use crate::container_overview::{
    spawn_container_monitor, ContainerMonitorHandle, ContainerOverviewEvent,
    ContainerOverviewUiState,
};
use crate::host_management::HostConnectionConfig;
use crate::host_overview::remote_ssh_config_from_host_config;

/// 各主机容器采集刷新周期。
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

struct FleetEntry {
    ui: ContainerOverviewUiState,
    _handle: Option<ContainerMonitorHandle>, // Drop 即 stop
}

/// 一台主机起监控后交回调用方消费的事件流。
pub type FleetStream = (String, async_channel::Receiver<ContainerOverviewEvent>);

/// 多主机容器监控舰队。进入 Containers 视图时 start，离开时 stop_all。
#[derive(Default)]
pub struct ContainerFleet {
    entries: HashMap<String, FleetEntry>,
}

impl ContainerFleet {
    pub fn new() -> Self {
        Self::default()
    }

    /// 对每台主机起监控。已在跑（handle 活着）的跳过；暂停态（ui 在、handle 空）复活时保留上次快照
    /// 只重挂 handle，不重置 ui，避免重连闪「加载中」。SSH 协议过滤由调用方负责（对齐 HostOverviewFleet）。
    /// 顺带清理已不在当前列表的主机（删除/筛选/协议变化），避免暂停态 entry 残留。
    /// 返回新起的事件流，由调用方消费后回调 `apply_event`。
    pub fn start(&mut self, hosts: &[(String, HostConnectionConfig)]) -> Vec<FleetStream> {
        let alive: std::collections::HashSet<&str> =
            hosts.iter().map(|(id, _)| id.as_str()).collect();
        self.entries.retain(|id, _| alive.contains(id.as_str()));

        let mut streams = Vec::new();
        for (host_id, config) in hosts {
            if let Some(entry) = self.entries.get(host_id) {
                if entry._handle.is_some() {
                    continue;
                }
            }
            let display = config.host.clone();
            match spawn_container_monitor(
                remote_ssh_config_from_host_config(config),
                REFRESH_INTERVAL,
            ) {
                Ok((handle, receiver)) => {
                    match self.entries.get_mut(host_id) {
                        // 暂停态复活：保留 ui（上次快照），只重挂 handle。
                        Some(entry) => entry._handle = Some(handle),
                        // 新主机：初始 waiting。
                        None => {
                            self.entries.insert(
                                host_id.clone(),
                                FleetEntry {
                                    ui: ContainerOverviewUiState::waiting(display),
                                    _handle: Some(handle),
                                },
                            );
                        }
                    }
                    streams.push((host_id.clone(), receiver));
                }
                Err(error) => {
                    // 起线程失败：已有 entry 保留其 ui 再叠加 Error；新主机 waiting+Error。
                    match self.entries.get_mut(host_id) {
                        Some(entry) => {
                            entry._handle = None;
                            entry.ui.apply_event(ContainerOverviewEvent::Error(error));
                        }
                        None => {
                            let mut ui = ContainerOverviewUiState::waiting(display);
                            ui.apply_event(ContainerOverviewEvent::Error(error));
                            self.entries
                                .insert(host_id.clone(), FleetEntry { ui, _handle: None });
                        }
                    }
                }
            }
        }
        streams
    }

    /// 把某台主机的采集事件合并进它的 UI 状态。
    pub fn apply_event(&mut self, host_id: &str, event: ContainerOverviewEvent) {
        if let Some(entry) = self.entries.get_mut(host_id) {
            entry.ui.apply_event(event);
        }
    }

    /// 取某台主机当前状态（渲染卡片用）。
    pub fn state(&self, host_id: &str) -> Option<&ContainerOverviewUiState> {
        self.entries.get(host_id).map(|entry| &entry.ui)
    }

    /// 停止全部监控并清空（Drop handle 即 stop）。
    pub fn stop_all(&mut self) {
        self.entries.clear();
    }

    /// 暂停全部采集：drop handle 停线程/释放连接，但保留 ui 快照，供切回容器视图时立即渲染。
    /// 与 stop_all（清空 entries）区别：pause 留态、可复活。
    pub fn pause_all(&mut self) {
        for entry in self.entries.values_mut() {
            entry._handle = None;
        }
    }

    pub fn is_running(&self) -> bool {
        !self.entries.is_empty()
    }
}
