// rdp_view::hotkey_guard — RDP 前台时接管 macOS 符号热键（Spotlight/输入法切换等），
// 使 ⌘Space、⌃Space 等不被系统层吞掉，能透传到远端 Windows。
// Carbon HIToolbox 未文档化导出，无需 TCC 权限；模式仅本 App 前台生效，切走/崩溃系统自动恢复。
// 参考 RoyalVNC：view 成 firstResponder 时 Push、resign 时 Pop（成对）。crate 仅 mac，不做跨平台分支。

use std::os::raw::c_void;
use std::sync::OnceLock;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    /// 接管符号热键，返回不透明 token（须原样传回 Pop）。重复 Push 会叠 token。
    fn PushSymbolicHotKeyMode(mode: u32) -> *mut c_void;
    /// 释放一次接管，token 取自对应 Push。
    fn PopSymbolicHotKeyMode(token: *mut c_void);
}

/// 禁用全部符号热键但保留辅助功能（VoiceOver 等），故用 2 而非 1(AllDisabled)。
const MODE_ALL_DISABLED_EXCEPT_UNIVERSAL_ACCESS: u32 = 2;

/// 链路追踪开关（NEXSHELL_RDP_HOTKEY_TRACE=1），对齐 rdp_session 的 ptr_trace 风格。
fn hotkey_trace() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("NEXSHELL_RDP_HOTKEY_TRACE").is_ok_and(|v| v == "1"))
}

/// 持 Push 返回的 token，Drop 时 Pop（RAII 成对）。非 Clone/Copy，move 语义保证 token 唯一。
pub struct HotkeyGuard {
    token: *mut c_void,
}

impl HotkeyGuard {
    /// 接管符号热键。调用方须保证同时只持一个 guard（叠 Push 会泄 token）——用 HotkeyGuardSlot 兜底。
    pub fn acquire() -> Self {
        let token = unsafe { PushSymbolicHotKeyMode(MODE_ALL_DISABLED_EXCEPT_UNIVERSAL_ACCESS) };
        if hotkey_trace() {
            eprintln!("[rdp-hotkey] Push mode=2 token={token:?}");
        }
        Self { token }
    }
}

impl Drop for HotkeyGuard {
    fn drop(&mut self) {
        unsafe { PopSymbolicHotKeyMode(self.token) };
        if hotkey_trace() {
            eprintln!("[rdp-hotkey] Pop token={:?}", self.token);
        }
    }
}

/// 幂等转移决策：依 (期望接管, 当前已接管) 给出动作。纯逻辑，脱离 FFI 可单测。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HotkeyTransition {
    Push,
    Pop,
    None,
}

pub(crate) fn hotkey_transition(desired: bool, engaged: bool) -> HotkeyTransition {
    match (desired, engaged) {
        (true, false) => HotkeyTransition::Push,
        (false, true) => HotkeyTransition::Pop,
        _ => HotkeyTransition::None, // 状态未变：不重复 Push/Pop
    }
}

/// 幂等封装：以「持有 = 已接管」为状态，set_engaged 只在翻转时 Push(acquire)/Pop(drop)。
#[derive(Default)]
pub struct HotkeyGuardSlot {
    guard: Option<HotkeyGuard>,
}

impl HotkeyGuardSlot {
    /// 同步期望态；重复调用同值幂等，绝不叠 Push。
    pub fn set_engaged(&mut self, desired: bool) {
        match hotkey_transition(desired, self.guard.is_some()) {
            HotkeyTransition::Push => self.guard = Some(HotkeyGuard::acquire()),
            HotkeyTransition::Pop => self.guard = None, // Drop → Pop
            HotkeyTransition::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_is_idempotent() {
        // 仅在状态翻转时动作，同态重复为 None（防叠 Push/多 Pop）。
        assert_eq!(hotkey_transition(true, false), HotkeyTransition::Push);
        assert_eq!(hotkey_transition(true, true), HotkeyTransition::None);
        assert_eq!(hotkey_transition(false, true), HotkeyTransition::Pop);
        assert_eq!(hotkey_transition(false, false), HotkeyTransition::None);
    }
}
