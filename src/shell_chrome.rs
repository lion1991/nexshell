use crate::view_model::ShellViewSnapshot;
use crate::TabKind;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalPreview {
    pub session_id: Option<String>,
    pub attached: bool,
    pub prompt: String,
    pub lines: Vec<String>,
}

pub fn activity_glyph(id: &str) -> &'static str {
    match id {
        "hosts" => "[]",
        "sessions" => ">",
        "terminal" => ">",
        "snippets" => "{}",
        "files" => "~/",
        "account" => "@",
        _ => "?",
    }
}

pub fn tab_kind_glyph(kind: &TabKind) -> &'static str {
    match kind {
        TabKind::Task => "•",
        TabKind::Terminal => ">_",
    }
}

pub fn terminal_preview(view: &ShellViewSnapshot) -> TerminalPreview {
    let host = view.terminal_hosts.first();
    let session_id = host.and_then(|host| host.session_id).map(str::to_string);
    let session = session_id.as_deref().unwrap_or("local");
    let attached = host.is_some();
    let prompt = format!("nexshell@{session}:~$");

    let lines = if attached {
        vec![
            rust_i18n::t!("shell_last_login").to_string(),
            format!("ssh {session}"),
            rust_i18n::t!("shell_ready").to_string(),
        ]
    } else {
        vec![
            rust_i18n::t!("shell_detached").to_string(),
            rust_i18n::t!("shell_background", session = session).to_string(),
            rust_i18n::t!("shell_reattach").to_string(),
        ]
    };

    TerminalPreview {
        session_id,
        attached,
        prompt,
        lines,
    }
}
