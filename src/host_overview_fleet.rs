//! 多主机状态总览：把单主机 host_overview monitor 批量化成「舰队」。
//! 纯编排——采集/解析全复用 host_overview，本模块只管启停与状态聚合。

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::generation::{accepts_generation, Generation, GenerationAllocator};
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
    generation: Generation,
}

/// 一台主机起监控后交回调用方消费的事件流。
pub type FleetStream = (
    String,
    Generation,
    async_channel::Receiver<HostOverviewEvent>,
);

/// 多主机监控舰队。RootView 进入状态总览视图时 start，离开时 pause_all。
#[derive(Default)]
pub struct HostOverviewFleet {
    entries: HashMap<String, FleetEntry>,
    generations: GenerationAllocator,
}

impl HostOverviewFleet {
    pub fn new() -> Self {
        Self::default()
    }

    /// 对每台主机起监控（已在跑的跳过，幂等）；当前筛选外的主机暂停但保留快照。
    /// 返回新起的事件流，由调用方消费后回调 `apply_event_for_generation`。
    // TODO: 主机数很大时这里会一次性起 N 个连接线程，后续可加并发闸门（信号量分批）。
    pub fn start(&mut self, hosts: &[(String, HostConnectionConfig)]) -> Vec<FleetStream> {
        let desired = hosts.iter().map(|(id, _)| id.as_str()).collect();
        self.retain_targets(&desired);

        let mut streams = Vec::new();
        for (host_id, config) in hosts {
            if !self.entry_needs_start(host_id) {
                continue;
            }
            let display = config.host.clone();
            match spawn_host_overview_monitor(
                remote_ssh_config_from_host_config(config),
                REFRESH_INTERVAL,
            ) {
                Ok((handle, receiver)) => {
                    let generation = self.generations.allocate();
                    match self.entries.get_mut(host_id) {
                        Some(entry) => {
                            entry._handle = Some(handle);
                            entry.generation = generation;
                        }
                        None => {
                            self.entries.insert(
                                host_id.clone(),
                                FleetEntry {
                                    ui: HostOverviewUiState::waiting(display),
                                    _handle: Some(handle),
                                    generation,
                                },
                            );
                        }
                    }
                    streams.push((host_id.clone(), generation, receiver));
                }
                Err(error) => {
                    // 保留 entry 但不伪装成 running；下次同步可重试。
                    match self.entries.get_mut(host_id) {
                        Some(entry) => {
                            entry._handle = None;
                            entry.generation = Generation::INVALID;
                            entry.ui.apply_event(HostOverviewEvent::Error(error));
                        }
                        None => {
                            let mut ui = HostOverviewUiState::waiting(display);
                            ui.apply_event(HostOverviewEvent::Error(error));
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

    fn entry_needs_start(&self, host_id: &str) -> bool {
        self.entries
            .get(host_id)
            .map_or(true, |entry| entry._handle.is_none())
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

    /// 把某台主机的采集事件合并进它的 UI 状态。
    pub fn apply_event_for_generation(
        &mut self,
        host_id: &str,
        generation: Generation,
        event: HostOverviewEvent,
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

    /// 取某台主机当前状态（渲染卡片用）。
    pub fn state(&self, host_id: &str) -> Option<&HostOverviewUiState> {
        self.entries.get(host_id).map(|entry| &entry.ui)
    }

    /// 暂停采集并释放连接/线程，但保留上次快照供返回 Status 页立即展示。
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

    use super::{FleetEntry, HostOverviewFleet};
    use crate::generation::Generation;
    use crate::host_overview::HostOverviewUiState;

    fn paused_entry(label: &str) -> FleetEntry {
        FleetEntry {
            ui: HostOverviewUiState::waiting(label.to_string()),
            _handle: None,
            generation: Generation::new(2).unwrap(),
        }
    }

    #[test]
    fn failed_or_paused_entries_remain_retryable() {
        let mut fleet = HostOverviewFleet::new();
        fleet
            .entries
            .insert("host-a".to_string(), paused_entry("a.example"));

        assert!(fleet.entry_needs_start("host-a"));
        assert!(fleet.entry_needs_start("host-b"));
    }

    #[test]
    fn filtering_out_a_host_preserves_its_snapshot_for_search_reentry() {
        let mut fleet = HostOverviewFleet::new();
        fleet
            .entries
            .insert("keep".to_string(), paused_entry("keep.example"));
        fleet
            .entries
            .insert("stale".to_string(), paused_entry("stale.example"));
        let desired = HashSet::from(["keep"]);

        fleet.retain_targets(&desired);

        assert!(fleet.state("keep").is_some());
        assert!(fleet.state("stale").is_some());
        assert_eq!(fleet.entries["stale"].generation, Generation::INVALID);
    }

    #[test]
    fn removing_a_host_from_the_library_drops_its_cached_snapshot() {
        let mut fleet = HostOverviewFleet::new();
        fleet
            .entries
            .insert("deleted".to_string(), paused_entry("deleted.example"));

        fleet.retain_known_hosts(&HashSet::new());

        assert!(fleet.state("deleted").is_none());
    }

    #[test]
    fn event_generation_must_match_the_current_entry() {
        let mut fleet = HostOverviewFleet::new();
        fleet
            .entries
            .insert("host-a".to_string(), paused_entry("a.example"));

        assert!(!fleet.apply_event_for_generation(
            "host-a",
            Generation::new(1).unwrap(),
            super::HostOverviewEvent::Error("stale".to_string())
        ));
        assert!(fleet.apply_event_for_generation(
            "host-a",
            Generation::new(2).unwrap(),
            super::HostOverviewEvent::Error("current".to_string())
        ));
    }

    #[test]
    fn pausing_immediately_invalidates_events_from_dropped_monitors() {
        let mut fleet = HostOverviewFleet::new();
        fleet
            .entries
            .insert("host-a".to_string(), paused_entry("a.example"));

        fleet.pause_all();

        assert!(!fleet.apply_event_for_generation(
            "host-a",
            Generation::new(2).unwrap(),
            super::HostOverviewEvent::Error("stale after pause".to_string())
        ));
    }
}
