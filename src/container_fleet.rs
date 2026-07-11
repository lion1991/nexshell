//! 多主机容器编排：把单主机 container monitor 批量化成「舰队」。
//! 纯编排——采集/解析全复用 container_overview，本模块只管启停与状态聚合。

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::container_overview::{
    spawn_container_monitor, ContainerMonitorHandle, ContainerOverviewEvent,
    ContainerOverviewUiState,
};
use crate::generation::{accepts_generation, Generation, GenerationAllocator};
use crate::host_management::HostConnectionConfig;
use crate::host_overview::remote_ssh_config_from_host_config;

/// 各主机容器采集刷新周期。
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

struct FleetEntry {
    ui: ContainerOverviewUiState,
    _handle: Option<ContainerMonitorHandle>, // Drop 即 stop
    generation: Generation,
}

/// 一台主机起监控后交回调用方消费的事件流。
pub type FleetStream = (
    String,
    Generation,
    async_channel::Receiver<ContainerOverviewEvent>,
);

/// 多主机容器监控舰队。进入 Containers 视图时 start，离开时 pause_all。
#[derive(Default)]
pub struct ContainerFleet {
    entries: HashMap<String, FleetEntry>,
    generations: GenerationAllocator,
}

impl ContainerFleet {
    pub fn new() -> Self {
        Self::default()
    }

    /// 对每台主机起监控。已在跑（handle 活着）的跳过；暂停态（ui 在、handle 空）复活时保留上次快照
    /// 只重挂 handle，不重置 ui，避免重连闪「加载中」。SSH 协议过滤由调用方负责（对齐 HostOverviewFleet）。
    /// 当前筛选外的主机只暂停；真实删除/协议变化由调用方先通过 `retain_known_hosts` 清理。
    /// 返回新起的事件流，由调用方消费后回调 `apply_event_for_generation`。
    pub fn start(&mut self, hosts: &[(String, HostConnectionConfig)]) -> Vec<FleetStream> {
        let desired = hosts.iter().map(|(id, _)| id.as_str()).collect();
        self.retain_targets(&desired);

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
                    let generation = self.generations.allocate();
                    match self.entries.get_mut(host_id) {
                        // 暂停态复活：保留 ui（上次快照），只重挂 handle。
                        Some(entry) => {
                            entry._handle = Some(handle);
                            entry.generation = generation;
                        }
                        // 新主机：初始 waiting。
                        None => {
                            self.entries.insert(
                                host_id.clone(),
                                FleetEntry {
                                    ui: ContainerOverviewUiState::waiting(display),
                                    _handle: Some(handle),
                                    generation,
                                },
                            );
                        }
                    }
                    streams.push((host_id.clone(), generation, receiver));
                }
                Err(error) => {
                    // 起线程失败：已有 entry 保留其 ui 再叠加 Error；新主机 waiting+Error。
                    match self.entries.get_mut(host_id) {
                        Some(entry) => {
                            entry._handle = None;
                            entry.generation = Generation::INVALID;
                            entry.ui.apply_event(ContainerOverviewEvent::Error(error));
                        }
                        None => {
                            let mut ui = ContainerOverviewUiState::waiting(display);
                            ui.apply_event(ContainerOverviewEvent::Error(error));
                            self.entries.insert(
                                host_id.clone(),
                                FleetEntry {
                                    ui,
                                    _handle: None,
                                    generation: Generation::INVALID,
                                },
                            );
                        }
                    }
                }
            }
        }
        streams
    }

    fn retain_targets(&mut self, desired: &HashSet<&str>) {
        for (host_id, entry) in &mut self.entries {
            if !desired.contains(host_id.as_str()) {
                entry._handle = None;
                entry.generation = Generation::INVALID;
            }
        }
    }

    /// 清理已从主机库删除或已不再使用 SSH 协议的缓存 entry。
    pub fn retain_known_hosts(&mut self, known_host_ids: &HashSet<&str>) {
        self.entries
            .retain(|host_id, _| known_host_ids.contains(host_id.as_str()));
    }

    /// 仅把当前 monitor generation 的事件合并进 UI 状态。
    pub fn apply_event_for_generation(
        &mut self,
        host_id: &str,
        generation: Generation,
        event: ContainerOverviewEvent,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(host_id) else {
            return false;
        };
        if !accepts_generation(Some(entry.generation), generation) {
            return false;
        }
        entry.ui.apply_event(event);
        true
    }

    /// 本地容器 action 的结果不来自 monitor 流，直接合并到当前 entry。
    pub fn apply_action_event(&mut self, host_id: &str, event: ContainerOverviewEvent) {
        if let Some(entry) = self.entries.get_mut(host_id) {
            entry.ui.apply_event(event);
        }
    }

    /// 取某台主机当前状态（渲染卡片用）。
    pub fn state(&self, host_id: &str) -> Option<&ContainerOverviewUiState> {
        self.entries.get(host_id).map(|entry| &entry.ui)
    }

    /// 暂停全部采集：drop handle 停线程/释放连接，但保留 ui 快照，供切回容器视图时立即渲染。
    pub fn pause_all(&mut self) {
        for entry in self.entries.values_mut() {
            entry._handle = None;
            entry.generation = Generation::INVALID;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{ContainerFleet, FleetEntry};
    use crate::container_overview::{ContainerOverviewEvent, ContainerOverviewUiState};
    use crate::generation::Generation;

    #[test]
    fn pause_rejects_events_from_the_dropped_monitor_generation() {
        let mut fleet = ContainerFleet::new();
        fleet.entries.insert(
            "host-a".to_string(),
            FleetEntry {
                ui: ContainerOverviewUiState::waiting("a.example".to_string()),
                _handle: None,
                generation: Generation::new(7).unwrap(),
            },
        );

        fleet.pause_all();

        assert!(!fleet.apply_event_for_generation(
            "host-a",
            Generation::new(7).unwrap(),
            ContainerOverviewEvent::Error("stale after pause".to_string()),
        ));
    }

    #[test]
    fn filtering_out_a_host_preserves_its_snapshot_for_search_reentry() {
        let mut fleet = ContainerFleet::new();
        fleet.entries.insert(
            "host-a".to_string(),
            FleetEntry {
                ui: ContainerOverviewUiState::waiting("a.example".to_string()),
                _handle: None,
                generation: Generation::new(3).unwrap(),
            },
        );

        fleet.retain_targets(&HashSet::new());

        assert!(fleet.state("host-a").is_some());
        assert_eq!(fleet.entries["host-a"].generation, Generation::INVALID);
    }

    #[test]
    fn removing_a_host_from_the_library_drops_its_cached_snapshot() {
        let mut fleet = ContainerFleet::new();
        fleet.entries.insert(
            "deleted".to_string(),
            FleetEntry {
                ui: ContainerOverviewUiState::waiting("deleted.example".to_string()),
                _handle: None,
                generation: Generation::INVALID,
            },
        );

        fleet.retain_known_hosts(&HashSet::new());

        assert!(fleet.state("deleted").is_none());
    }
}
