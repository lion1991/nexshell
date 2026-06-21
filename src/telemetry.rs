//! 替代 warp::server::telemetry 的本地 no-op stub（仅非门控埋点用）。

pub enum TelemetryEvent {
    AtMenuInteracted,
    AttachedImagesToAgentModeQuery,
}

impl TelemetryEvent {
    pub fn track(&self) {}
}
