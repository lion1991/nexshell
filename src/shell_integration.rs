//! Warp 式 shell integration（精简版，不含 AI 用 hook）：注入 wrapper rc，
//! 让 shell startup 完成后主动发 OSC marker 给我们，UI 由占位切到 grid。
//!
//! Warp 同等机制见 warp/app/assets/bundled/bootstrap/{zsh,bash,fish}_*.sh，
//! 但 Warp 用 DCS+JSON 协议且采集大量 shell 状态供 AI 使用；
//! 我们只需"prompt 已就绪"信号 + cwd 上报，所以走标准 OSC 7（VTE/iTerm 都识别）。

use std::{env, fs, path::PathBuf};

/// shell 在 startup 末尾发的 marker（OSC 9999）。process_output 扫描到就翻 latch。
pub const BOOTSTRAP_MARKER: &[u8] = b"\x1b]9999;NEXSHELL_BOOT\x07";

const ZSH_WRAPPER_RC: &str = r#"# nexshell shell integration: source user rc, then emit bootstrap marker
[ -r "$HOME/.zshenv" ] && . "$HOME/.zshenv"
[ -r "$HOME/.zprofile" ] && . "$HOME/.zprofile"
[ -r "$HOME/.zshrc" ] && . "$HOME/.zshrc"
[ -r "$HOME/.zlogin" ] && . "$HOME/.zlogin"

# 首次 precmd 发一次 marker 后自移除
__nexshell_emit_bootstrap_marker() {
  printf '\e]9999;NEXSHELL_BOOT\a'
  precmd_functions=("${(@)precmd_functions:#__nexshell_emit_bootstrap_marker}")
}
typeset -ga precmd_functions
precmd_functions+=(__nexshell_emit_bootstrap_marker)

# OSC 7 cwd 上报：每次目录变化 + 每次 prompt 都发一遍（首屏也覆盖）
__nexshell_emit_osc7() {
  printf '\e]7;file://%s%s\a' "${HOST:-localhost}" "$PWD"
}
typeset -ga chpwd_functions
chpwd_functions+=(__nexshell_emit_osc7)
precmd_functions+=(__nexshell_emit_osc7)
"#;

// bash 用 --rcfile 启 interactive non-login shell；wrapper 内部 source 用户的
// login + interactive rc 文件，模拟 login shell 语义。
const BASH_WRAPPER_RC: &str = r#"# nexshell shell integration: source user rc, then emit bootstrap marker
if [ -r /etc/profile ]; then . /etc/profile; fi
if [ -r "$HOME/.bash_profile" ]; then
    . "$HOME/.bash_profile"
elif [ -r "$HOME/.bash_login" ]; then
    . "$HOME/.bash_login"
elif [ -r "$HOME/.profile" ]; then
    . "$HOME/.profile"
fi
[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"

__nexshell_emit_bootstrap_marker() {
  printf '\e]9999;NEXSHELL_BOOT\a'
  PROMPT_COMMAND="${PROMPT_COMMAND//__nexshell_emit_bootstrap_marker;/}"
  PROMPT_COMMAND="${PROMPT_COMMAND//__nexshell_emit_bootstrap_marker/}"
}
PROMPT_COMMAND="__nexshell_emit_bootstrap_marker;${PROMPT_COMMAND:-:}"

# OSC 7 cwd 上报：用 PROMPT_COMMAND 在每次 prompt 前发一次
__nexshell_emit_osc7() {
  printf '\e]7;file://%s%s\a' "${HOSTNAME:-localhost}" "$PWD"
}
PROMPT_COMMAND="__nexshell_emit_osc7;${PROMPT_COMMAND}"
"#;

/// fish 的 init command（通过 `fish --init-command`），不写文件。
/// fish 启动顺序：~/.config/fish/config.fish → init-command → fish_prompt。
/// 这里在 fish_prompt 事件触发时发 marker 并自移除，确保 prompt 完全就绪。
/// 同时挂一个 PWD 变化的 OSC 7 上报函数（fish 用 --on-variable PWD，自动覆盖首屏）。
pub const FISH_INIT_COMMAND: &str = concat!(
    "function __nexshell_emit_bootstrap_marker --on-event fish_prompt;",
    " printf '\\e]9999;NEXSHELL_BOOT\\a';",
    " functions -e __nexshell_emit_bootstrap_marker;",
    " end;",
    "function __nexshell_emit_osc7 --on-variable PWD;",
    " printf '\\e]7;file://%s%s\\a' (hostname) $PWD;",
    " end;",
    "__nexshell_emit_osc7",
);

fn integration_dir() -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let base = if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Caches")
            .join("com.matt.nexshell")
    } else {
        env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"))
            .join("com.matt.nexshell")
    };
    Some(base.join("shell-integration"))
}

/// 写 wrapper `.zshrc` 到缓存目录，返回 ZDOTDIR 路径。
pub fn setup_zsh_integration() -> Option<PathBuf> {
    let dir = integration_dir()?;
    fs::create_dir_all(&dir).ok()?;
    fs::write(dir.join(".zshrc"), ZSH_WRAPPER_RC).ok()?;
    Some(dir)
}

/// 写 wrapper bashrc，返回 `--rcfile` 用的完整路径。
pub fn setup_bash_integration() -> Option<PathBuf> {
    let dir = integration_dir()?;
    fs::create_dir_all(&dir).ok()?;
    let rc = dir.join("bashrc");
    fs::write(&rc, BASH_WRAPPER_RC).ok()?;
    Some(rc)
}
