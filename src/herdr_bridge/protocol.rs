//! herdr JSON-line 协议：请求编码、响应/事件信封解析。纯函数，无 I/O。
//!
//! 事件只当「session 变脏」的信号用，不再从事件负载里取 cwd —— 权威数据一律
//! 来自 `session.snapshot`（见 `snapshot.rs`）。

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

/// 默认 socket 相对 HOME 的位置（`HERDR_SOCKET_PATH` 优先）。
const DEFAULT_SOCKET_REL: &str = ".config/herdr/herdr.sock";

/// 订阅的事件类型（点号形式）。凡是可能改变 workspace / tab / pane 结构、
/// 焦点或 cwd 的都订上，收到任意一条就重新拉 snapshot。
pub const SUBSCRIPTIONS: &[&str] = &[
    "workspace.focused",
    "workspace.created",
    "workspace.updated",
    "workspace.renamed",
    "workspace.closed",
    "tab.focused",
    "tab.created",
    "tab.closed",
    "pane.focused",
    "pane.updated",
    "pane.created",
    "pane.closed",
    "layout.updated",
];

/// socket 路径：`HERDR_SOCKET_PATH` → `~/.config/herdr/herdr.sock`。
pub fn resolve_socket_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HERDR_SOCKET_PATH") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    let home = std::env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(DEFAULT_SOCKET_REL))
}

/// 编码一行请求（末尾带换行，可直接写 socket）。
pub fn encode_request(id: &str, method: &str, params: Value) -> String {
    let mut line = json!({ "id": id, "method": method, "params": params }).to_string();
    line.push('\n');
    line
}

/// `session.snapshot` 请求：params 是空对象，返回整个 session 的权威快照。
pub fn encode_session_snapshot(id: &str) -> String {
    encode_request(id, "session.snapshot", json!({}))
}

/// `events.subscribe` 请求：订阅名用点号（推送信封是下划线）。
pub fn encode_subscribe(id: &str) -> String {
    let subscriptions: Vec<Value> = SUBSCRIPTIONS.iter().map(|t| json!({ "type": t })).collect();
    encode_request(
        id,
        "events.subscribe",
        json!({ "subscriptions": subscriptions }),
    )
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct ErrorBody {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
}

/// 请求响应信封。invalid_request 时 server 回的 id 是空串。
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<ErrorBody>,
}

/// 推送事件信封。只保留类型名——负载一律不信，改拉 snapshot。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventEnvelope {
    /// 信封上的 `event` 字段（下划线形式，如 `workspace_focused`）。
    pub event: String,
    /// `data.type`，正常与 `event` 相同；缺失时为空串。
    pub data_type: String,
}

impl EventEnvelope {
    /// 用于判定「是不是我们订的那些事件」的类型名，优先取 data.type。
    pub fn kind(&self) -> &str {
        if self.data_type.is_empty() {
            &self.event
        } else {
            &self.data_type
        }
    }
}

/// 一行既可能是响应也可能是事件：带 `event` 字段的是事件。
#[derive(Clone, Debug)]
pub enum Line {
    Response(Response),
    Event(EventEnvelope),
}

/// 解析一行 JSON。非法 JSON 返回 None（静默降级，不 panic）。
pub fn parse_line(line: &str) -> Option<Line> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    if let Some(event) = value.get("event").and_then(Value::as_str) {
        return Some(Line::Event(EventEnvelope {
            event: event.to_string(),
            data_type: value
                .get("data")
                .and_then(|d| d.get("type"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }));
    }
    serde_json::from_value::<Response>(value)
        .ok()
        .map(Line::Response)
}

/// 从 `session.snapshot` 的响应里取出 `result.snapshot`。
pub fn snapshot_from_response(response: &Response) -> Option<&Value> {
    response.result.as_ref()?.get("snapshot")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUB_STARTED: &str = r#"{"id":"sub_1","result":{"type":"subscription_started"}}"#;
    const PANE_FOCUSED: &str = r#"{"data":{"pane_id":"wR:p1","type":"pane_focused","workspace_id":"wR"},"event":"pane_focused"}"#;
    const WORKSPACE_FOCUSED: &str =
        r#"{"data":{"type":"workspace_focused","workspace_id":"wG"},"event":"workspace_focused"}"#;
    const TAB_FOCUSED: &str = r#"{"data":{"tab_id":"wG:t1","type":"tab_focused","workspace_id":"wG"},"event":"tab_focused"}"#;
    const AGENT_STATUS: &str = r#"{"data":{"pane_id":"wG:p1","type":"pane_agent_status_changed","agent_status":"idle"},"event":"pane_agent_status_changed"}"#;
    const ERROR_EMPTY_ID: &str =
        r#"{"id":"","error":{"code":"invalid_request","message":"missing field `method`"}}"#;

    #[test]
    fn encodes_session_snapshot_with_empty_params() {
        let line = encode_session_snapshot("x");
        assert!(line.ends_with('\n'));
        let v: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(v["method"], "session.snapshot");
        assert_eq!(v["params"], json!({}));
    }

    /// per-client 焦点的坑：任何结构 / 焦点 / cwd 变化都得订上，否则
    /// nexshell 里的 herdr client 切 workspace 时收不到任何信号。
    #[test]
    fn subscribe_covers_workspace_tab_pane_and_layout() {
        let v: Value = serde_json::from_str(encode_subscribe("s").trim()).unwrap();
        let types: Vec<&str> = v["params"]["subscriptions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, SUBSCRIPTIONS.to_vec());
        for required in [
            "workspace.focused",
            "workspace.renamed",
            "tab.focused",
            "pane.focused",
            "pane.updated",
            "layout.updated",
        ] {
            assert!(types.contains(&required), "missing {required}");
        }
    }

    #[test]
    fn parses_subscription_ack() {
        let Some(Line::Response(resp)) = parse_line(SUB_STARTED) else {
            panic!("expected response");
        };
        assert_eq!(resp.id, "sub_1");
        assert!(resp.error.is_none());
        assert!(snapshot_from_response(&resp).is_none());
    }

    #[test]
    fn parses_error_with_empty_id() {
        let Some(Line::Response(resp)) = parse_line(ERROR_EMPTY_ID) else {
            panic!("expected response");
        };
        assert_eq!(resp.id, "");
        assert_eq!(resp.error.unwrap().code, "invalid_request");
    }

    #[test]
    fn parses_event_envelopes_by_type() {
        for (line, kind) in [
            (PANE_FOCUSED, "pane_focused"),
            (WORKSPACE_FOCUSED, "workspace_focused"),
            (TAB_FOCUSED, "tab_focused"),
            (AGENT_STATUS, "pane_agent_status_changed"),
        ] {
            let Some(Line::Event(ev)) = parse_line(line) else {
                panic!("expected event for {kind}");
            };
            assert_eq!(ev.kind(), kind);
        }
    }

    #[test]
    fn event_kind_falls_back_to_envelope_when_data_type_missing() {
        let line = r#"{"data":{"workspace_id":"wG"},"event":"workspace_focused"}"#;
        let Some(Line::Event(ev)) = parse_line(line) else {
            panic!("expected event");
        };
        assert_eq!(ev.data_type, "");
        assert_eq!(ev.kind(), "workspace_focused");
    }

    #[test]
    fn garbage_lines_are_none() {
        assert!(parse_line("not json").is_none());
        assert!(parse_line("").is_none());
    }

    #[test]
    fn socket_path_falls_back_under_home() {
        let home = PathBuf::from("/home/u");
        assert_eq!(
            home.join(DEFAULT_SOCKET_REL),
            PathBuf::from("/home/u/.config/herdr/herdr.sock")
        );
    }
}
