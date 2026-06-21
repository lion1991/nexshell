use std::env;
use std::fs;
use std::path::PathBuf;

/// 基于 shell 历史文件做前缀匹配的 autosuggestion（类 zsh-autosuggestions）。
pub struct HistorySuggester {
    entries: Vec<String>,
}

impl HistorySuggester {
    pub fn load() -> Self {
        Self {
            entries: read_history().unwrap_or_default(),
        }
    }

    /// 返回最近一条以 `prefix` 开头且不等于 `prefix` 的历史命令。
    pub fn suggest(&self, prefix: &str) -> Option<String> {
        if prefix.is_empty() {
            return None;
        }
        self.entries
            .iter()
            .rev()
            .find(|e| e.starts_with(prefix) && e.as_str() != prefix)
            .cloned()
    }

    pub fn reload(&mut self) {
        self.entries = read_history().unwrap_or_default();
    }
}

fn history_path() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    let shell = env::var("SHELL").unwrap_or_default();
    if shell.contains("zsh") {
        Some(PathBuf::from(home).join(".zsh_history"))
    } else {
        Some(PathBuf::from(home).join(".bash_history"))
    }
}

fn read_history() -> Option<Vec<String>> {
    let path = history_path()?;
    let bytes = fs::read(&path).ok()?;
    // zsh 历史可能是 metafied encoding，先按 UTF-8 lossy 读取
    let content = String::from_utf8_lossy(&bytes);
    let entries: Vec<String> = content
        .lines()
        .filter_map(|line| {
            // zsh EXTENDED_HISTORY 格式: : timestamp:0;command
            if line.starts_with(": ") {
                line.find(';').map(|pos| line[pos + 1..].to_string())
            } else {
                Some(line.to_string())
            }
        })
        .filter(|s| !s.is_empty())
        .collect();
    Some(entries)
}
