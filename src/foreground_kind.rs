//! PTY 前台进程分类。从 `proc_bsdinfo.pbi_comm` 判断前台跑的是 shell、herdr
//! client，还是别的程序（ssh / vim / ...）。
//!
//! 单独成文件是为了不把 `terminal_runtime.rs` 的 `impl LocalTerminalRuntime`
//! 劈成两段。

/// herdr client 的 `proc_bsdinfo.pbi_comm`。
pub const HERDR_PROCESS_NAME: &str = "herdr";

/// PTY 前台进程分类。`Shell` 的判定与改动前的 `query_shell_foreground` 完全一致。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ForegroundKind {
    /// 也是查询失败（tcgetpgrp / proc_pidinfo 返回错误）时的默认值：
    /// 保持改动前「查不到就当 shell 在前台」的行为。
    #[default]
    Shell,
    /// herdr（终端多路复用器）client。
    Herdr,
    Other,
}

impl ForegroundKind {
    pub fn classify(name: &str) -> Self {
        if name == HERDR_PROCESS_NAME {
            return Self::Herdr;
        }
        if matches!(
            name,
            "bash"
                | "zsh"
                | "fish"
                | "sh"
                | "dash"
                | "ksh"
                | "tcsh"
                | "csh"
                | "nu"
                | "nushell"
                | "pwsh"
                | "powershell"
                | "elvish"
                | "oil"
                | "osh"
                | "xonsh"
        ) {
            Self::Shell
        } else {
            Self::Other
        }
    }

    /// 与改动前 `query_shell_foreground` 的返回值同义。
    pub fn is_shell(self) -> bool {
        matches!(self, Self::Shell)
    }

    pub fn is_herdr(self) -> bool {
        matches!(self, Self::Herdr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_shells() {
        for name in ["bash", "zsh", "fish", "sh", "nu", "pwsh", "xonsh"] {
            assert_eq!(
                ForegroundKind::classify(name),
                ForegroundKind::Shell,
                "{name}"
            );
            assert!(ForegroundKind::classify(name).is_shell());
        }
    }

    #[test]
    fn classifies_herdr() {
        assert_eq!(ForegroundKind::classify("herdr"), ForegroundKind::Herdr);
        assert!(ForegroundKind::classify("herdr").is_herdr());
        // herdr 不是 shell —— shell_is_foreground 语义不变。
        assert!(!ForegroundKind::classify("herdr").is_shell());
    }

    #[test]
    fn classifies_others() {
        for name in ["ssh", "mosh", "vim", "herdrd", "myherdr", ""] {
            assert_eq!(
                ForegroundKind::classify(name),
                ForegroundKind::Other,
                "{name}"
            );
            assert!(!ForegroundKind::classify(name).is_shell());
            assert!(!ForegroundKind::classify(name).is_herdr());
        }
    }

    /// tcgetpgrp / proc_pidinfo 失败时 `query_foreground_kind` 返回默认值，
    /// 必须等价于改动前的 `return true`（当 shell 在前台）。
    #[test]
    fn query_failure_default_is_shell_and_not_herdr() {
        let fallback = ForegroundKind::default();
        assert_eq!(fallback, ForegroundKind::Shell);
        assert!(fallback.is_shell());
        assert!(!fallback.is_herdr());
    }
}
