use crate::ipc_dispatcher::IpcRuntimeInputs;
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct NativeShellRuntimeSettings {
    ipc_inputs: IpcRuntimeInputs,
}

impl NativeShellRuntimeSettings {
    pub fn nexshell_default() -> Self {
        Self {
            ipc_inputs: IpcRuntimeInputs {
                theme_json: warp_dark_theme_json(),
                highlight_rules: default_highlight_rules(),
                highlight_perf: default_highlight_perf(),
            },
        }
    }

    pub fn ipc_inputs(&self) -> &IpcRuntimeInputs {
        &self.ipc_inputs
    }
}

impl Default for NativeShellRuntimeSettings {
    fn default() -> Self {
        Self::nexshell_default()
    }
}

fn warp_dark_theme_json() -> Value {
    json!({
        "background": "#121212",
        "foreground": "#FAF9F6",
        "cursor": "#2E5D9E",
        "cursorAccent": "#121212",
        "selectionBackground": "rgba(46, 93, 158, 0.32)",
        "selectionForeground": "#FAF9F6",
        "black": "#121212",
        "red": "#D22D1E",
        "green": "#1CA05A",
        "yellow": "#E5A01A",
        "blue": "#3780E9",
        "magenta": "#BF409D",
        "cyan": "#2EBDB0",
        "white": "#FAF9F6",
        "brightBlack": "#5A5A5A",
        "brightRed": "#E5544A",
        "brightGreen": "#4ED687",
        "brightYellow": "#FFB836",
        "brightBlue": "#5C9DEE",
        "brightMagenta": "#D965BF",
        "brightCyan": "#5DDBCD",
        "brightWhite": "#FFFFFF",
    })
}

fn default_highlight_rules() -> Value {
    Value::Array(
        DEFAULT_HIGHLIGHT_PRESETS
            .iter()
            .map(HighlightPreset::to_json)
            .collect(),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HighlightPreset {
    id: &'static str,
    name: &'static str,
    pattern: &'static str,
    flags: &'static str,
    color: &'static str,
    priority: u32,
    enabled: bool,
    validate_filesystem: bool,
}

impl HighlightPreset {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "pattern": self.pattern,
            "flags": self.flags,
            "color": self.color,
            "priority": self.priority,
            "enabled": self.enabled,
            "validateFilesystem": self.validate_filesystem,
        })
    }
}

const DEFAULT_HIGHLIGHT_PRESETS: &[HighlightPreset] = &[
    HighlightPreset {
        id: "preset-url",
        name: "URL/URI",
        pattern: "(https?|ftp|file):\\/\\/[-A-Za-z0-9+&@#/%?=~_!:,.;]*[-A-Za-z0-9+&@#/%=~_]",
        flags: "gi",
        color: "#60a5fa",
        priority: 1,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-ipv4",
        name: "IPv4",
        pattern: "\\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)(?::\\d{1,5})?\\b",
        flags: "g",
        color: "#4ade80",
        priority: 2,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-ipv6",
        name: "IPv6",
        pattern: "(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}|(?:[0-9a-fA-F]{1,4}:){1,7}:|(?:[0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}",
        flags: "gi",
        color: "#4ade80",
        priority: 3,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-email",
        name: "Email",
        pattern: "[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}",
        flags: "gi",
        color: "#f472b6",
        priority: 4,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-iso-date",
        name: "ISO Date (YYYY-MM-DD)",
        pattern: "\\b\\d{4}[-/](?:0[1-9]|1[0-2])[-/](?:0[1-9]|[12]\\d|3[01])\\b",
        flags: "g",
        color: "#f97316",
        priority: 5,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-unix-date",
        name: "Unix Date (Mon Jan 01)",
        pattern: "(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\\s+(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\\s+\\d{1,2}",
        flags: "g",
        color: "#fbbf24",
        priority: 6,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-time",
        name: "Time (HH:MM:SS)",
        pattern: "\\b(?:[01]?\\d|2[0-3]):[0-5]\\d(?::[0-5]\\d)?(?:\\.\\d+)?\\b",
        flags: "g",
        color: "#38bdf8",
        priority: 7,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-syslog-date",
        name: "Syslog Date",
        pattern: "(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\\s+(?:[0-2]?\\d|3[01])\\s+(?:[01]?\\d|2[0-3]):\\d{2}:\\d{2}",
        flags: "g",
        color: "#fb7185",
        priority: 8,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-uuid",
        name: "UUID",
        pattern: "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
        flags: "gi",
        color: "#f87171",
        priority: 9,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-error",
        name: "Error/Failed",
        pattern: "\\b(?:error|failed|failure|exception|fatal|critical)\\b",
        flags: "gi",
        color: "#f87171",
        priority: 10,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-warning",
        name: "Warning",
        pattern: "\\b(?:warning|warn|caution)\\b",
        flags: "gi",
        color: "#fbbf24",
        priority: 11,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-success",
        name: "Success/OK",
        pattern: "\\b(?:success|succeeded|ok|passed|done|complete|completed)\\b",
        flags: "gi",
        color: "#4ade80",
        priority: 12,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-filepath",
        name: "File Path",
        pattern: "(?:\\/[\\w.-]+)+\\/?|[A-Za-z]:\\\\(?:[\\w.-]+\\\\)*[\\w.-]*",
        flags: "g",
        color: "#a78bfa",
        priority: 13,
        enabled: true,
        validate_filesystem: true,
    },
    HighlightPreset {
        id: "preset-mac-addr",
        name: "MAC Address",
        pattern: "\\b[0-9a-fA-F]{2}(?::[0-9a-fA-F]{2}){5}\\b",
        flags: "gi",
        color: "#22d3ee",
        priority: 14,
        enabled: true,
        validate_filesystem: false,
    },
    HighlightPreset {
        id: "preset-number",
        name: "Numbers",
        pattern: "\\b\\d+(?:\\.\\d+)?(?:%|KB|MB|GB|TB|ms|s|Hz|MHz|GHz)?\\b",
        flags: "g",
        color: "#a3e635",
        priority: 15,
        enabled: false,
        validate_filesystem: false,
    },
];

fn default_highlight_perf() -> Value {
    json!({
        "maxLineLength": 2000,
        "maxDecorations": 2000,
        "skipAltBuffer": true,
    })
}
