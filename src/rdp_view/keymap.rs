// rdp_view::keymap — macOS 按键 → RDP PC set-1 scancode 映射（纯函数，无 &self、无 UI 状态）。
// warpui 的 KeyDown 不带硬件 keycode，只给归一化字符串 key（见 mac/utils.rs unicode_char_to_key）
// 与 key_without_modifiers（基础字符）；故普通键走「字符串→scancode」降级路径。
// 修饰键走 ModifierKeyChanged 携带的物理 KeyCode，精确映射（左右可分）。
// 决策：⌘→Windows 键(LWin/RWin, 扩展 0xE0 0x5B/0x5C)、⌥→Alt、⌃→Ctrl、Shift→Shift。

use warpui_core::platform::keyboard::KeyCode;

use nexshell::rdp_session::RdpInputEvent;

/// 需 0xE0 前缀发送的扩展键（extended=true），scancode 取基础字节（不含 0xE0）。
const EXT: bool = true;
const NORM: bool = false;

/// 归一化按键字符串 → (set-1 scancode, extended)。命中 `None` 表示不发送该键。
/// `key` 应先小写。字母/数字/符号来自 key_without_modifiers，特殊键名来自 keystroke.key。
pub fn scancode_for_key(key: &str) -> Option<(u8, bool)> {
    // 单字符（字母/数字/符号/空格）。
    if key.chars().count() == 1 {
        if let Some(code) = scancode_for_char(key.chars().next().unwrap()) {
            return Some(code);
        }
    }
    let code = match key {
        "enter" | "return" => (0x1C, NORM),
        "numpadenter" => (0x1C, EXT),
        "tab" => (0x0F, NORM),
        "escape" | "esc" => (0x01, NORM),
        "backspace" => (0x0E, NORM),
        "space" => (0x39, NORM),
        "delete" => (0x53, EXT),
        "insert" => (0x52, EXT),
        "home" => (0x47, EXT),
        "end" => (0x4F, EXT),
        "pageup" => (0x49, EXT),
        "pagedown" => (0x51, EXT),
        "up" => (0x48, EXT),
        "down" => (0x50, EXT),
        "left" => (0x4B, EXT),
        "right" => (0x4D, EXT),
        "f1" => (0x3B, NORM),
        "f2" => (0x3C, NORM),
        "f3" => (0x3D, NORM),
        "f4" => (0x3E, NORM),
        "f5" => (0x3F, NORM),
        "f6" => (0x40, NORM),
        "f7" => (0x41, NORM),
        "f8" => (0x42, NORM),
        "f9" => (0x43, NORM),
        "f10" => (0x44, NORM),
        "f11" => (0x57, NORM),
        "f12" => (0x58, NORM),
        "f13" => (0x64, NORM),
        "f14" => (0x65, NORM),
        "f15" => (0x66, NORM),
        "f16" => (0x67, NORM),
        "f17" => (0x68, NORM),
        "f18" => (0x69, NORM),
        "f19" => (0x6A, NORM),
        "f20" => (0x6B, NORM),
        _ => return None,
    };
    Some(code)
}

/// 单字符（US 物理位）→ (scancode, extended)。全部非扩展。
fn scancode_for_char(ch: char) -> Option<(u8, bool)> {
    let code = match ch.to_ascii_lowercase() {
        'a' => 0x1E,
        'b' => 0x30,
        'c' => 0x2E,
        'd' => 0x20,
        'e' => 0x12,
        'f' => 0x21,
        'g' => 0x22,
        'h' => 0x23,
        'i' => 0x17,
        'j' => 0x24,
        'k' => 0x25,
        'l' => 0x26,
        'm' => 0x32,
        'n' => 0x31,
        'o' => 0x18,
        'p' => 0x19,
        'q' => 0x10,
        'r' => 0x13,
        's' => 0x1F,
        't' => 0x14,
        'u' => 0x16,
        'v' => 0x2F,
        'w' => 0x11,
        'x' => 0x2D,
        'y' => 0x15,
        'z' => 0x2C,
        '1' => 0x02,
        '2' => 0x03,
        '3' => 0x04,
        '4' => 0x05,
        '5' => 0x06,
        '6' => 0x07,
        '7' => 0x08,
        '8' => 0x09,
        '9' => 0x0A,
        '0' => 0x0B,
        '-' => 0x0C,
        '=' => 0x0D,
        '[' => 0x1A,
        ']' => 0x1B,
        '\\' => 0x2B,
        ';' => 0x27,
        '\'' => 0x28,
        '`' => 0x29,
        ',' => 0x33,
        '.' => 0x34,
        '/' => 0x35,
        ' ' => 0x39,
        _ => return None,
    };
    Some((code, NORM))
}

/// 物理修饰键 KeyCode → (set-1 scancode, extended)。⌘→Win 键。
pub fn scancode_for_modifier(code: KeyCode) -> Option<(u8, bool)> {
    let mapped = match code {
        KeyCode::ShiftLeft => (0x2A, NORM),
        KeyCode::ShiftRight => (0x36, NORM),
        KeyCode::ControlLeft => (0x1D, NORM),
        KeyCode::ControlRight => (0x1D, EXT),
        KeyCode::AltLeft => (0x38, NORM),
        KeyCode::AltRight => (0x38, EXT),
        KeyCode::SuperLeft => (0x5B, EXT),
        KeyCode::SuperRight => (0x5C, EXT),
        KeyCode::CapsLock => (0x3A, NORM),
        _ => return None,
    };
    Some(mapped)
}

/// 失焦/切走时批量抬起全部修饰键，防 Windows 卡键（尤其卡 Win 键）。
/// CapsLock 属切换键、非按住态，不含在内。
pub fn modifier_release_events() -> [RdpInputEvent; 8] {
    const MODS: [(u8, bool); 8] = [
        (0x2A, NORM), // LShift
        (0x36, NORM), // RShift
        (0x1D, NORM), // LCtrl
        (0x1D, EXT),  // RCtrl
        (0x38, NORM), // LAlt
        (0x38, EXT),  // RAlt
        (0x5B, EXT),  // LWin
        (0x5C, EXT),  // RWin
    ];
    MODS.map(|(scancode, extended)| RdpInputEvent::Key {
        scancode,
        extended,
        pressed: false,
    })
}

/// tracker 跟踪的 8 个修饰键：(scancode, extended, 类别)。类别 0=Shift 1=Ctrl 2=Alt 3=Cmd/Win。
/// 顺序与 modifier_release_events 一致。
const MOD_KEYS: [(u8, bool, usize); 8] = [
    (0x2A, NORM, 0), // LShift
    (0x36, NORM, 0), // RShift
    (0x1D, NORM, 1), // LCtrl
    (0x1D, EXT, 1),  // RCtrl
    (0x38, NORM, 2), // LAlt
    (0x38, EXT, 2),  // RAlt
    (0x5B, EXT, 3),  // LWin
    (0x5C, EXT, 3),  // RWin
];

/// 本地 modifiers 快照（供对账）。`None`=该类别本事件不携带、不参与对账（如 MouseMoved 无 ctrl/alt）。
#[derive(Clone, Copy, Default, Debug)]
pub struct ModifierFlags {
    pub shift: Option<bool>,
    pub control: Option<bool>,
    pub alt: Option<bool>,
    pub command: Option<bool>,
}

impl ModifierFlags {
    /// 完整 flags（ScrollWheel / LeftMouse* / KeyDown 均可给全 4 类）。
    pub fn full(shift: bool, control: bool, alt: bool, command: bool) -> Self {
        Self {
            shift: Some(shift),
            control: Some(control),
            alt: Some(alt),
            command: Some(command),
        }
    }

    /// 仅 cmd/shift 已知（MouseMoved / Right / Middle 只带这两个 bool）；ctrl/alt 未知不对账。
    pub fn cmd_shift(command: bool, shift: bool) -> Self {
        Self {
            shift: Some(shift),
            command: Some(command),
            control: None,
            alt: None,
        }
    }
}

/// 修饰键持续对账器：记「已发 down 未发 up」的 8 键；本地 flags 显示某类未按而 tracker 记为按下时，
/// 补发该键 release。左右无法从 flags 区分，故按类别（该类 flag=false 则左右都补）。
#[derive(Default)]
pub struct ModifierTracker {
    down: [bool; 8],
}

impl ModifierTracker {
    /// 记账一次修饰键的 down/up 发送。非修饰 scancode 静默忽略。
    pub fn on_sent(&mut self, scancode: u8, extended: bool, pressed: bool) {
        if let Some(i) = MOD_KEYS
            .iter()
            .position(|&(s, e, _)| s == scancode && e == extended)
        {
            self.down[i] = pressed;
        }
    }

    /// 对账：类别 flag 明确为 false 而 tracker 记为按下 → 补发 release 并清账。`None` 类别跳过。
    /// flag 为 true（用户真按着）绝不误发。
    pub fn reconcile(&mut self, flags: ModifierFlags) -> Vec<RdpInputEvent> {
        let cats = [flags.shift, flags.control, flags.alt, flags.command];
        let mut out = Vec::new();
        for (i, &(scancode, extended, cat)) in MOD_KEYS.iter().enumerate() {
            if self.down[i] && cats[cat] == Some(false) {
                out.push(RdpInputEvent::Key {
                    scancode,
                    extended,
                    pressed: false,
                });
                self.down[i] = false;
            }
        }
        out
    }

    /// 全量抬起（切 tab / 失焦）后清账，与 RootView 侧全量 release 同步复位。
    pub fn clear(&mut self) {
        self.down = [false; 8];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_up(scancode: u8, extended: bool) -> RdpInputEvent {
        RdpInputEvent::Key {
            scancode,
            extended,
            pressed: false,
        }
    }

    #[test]
    fn reconcile_releases_lost_keyup() {
        // 发了 LAlt down，随后本地 flags 显示 alt 未按 → 补发 LAlt release 一次。
        let mut t = ModifierTracker::default();
        t.on_sent(0x38, NORM, true);
        let out = t.reconcile(ModifierFlags::full(false, false, false, false));
        assert_eq!(out, vec![key_up(0x38, NORM)]);
        // 已清账：再对账不重复补发。
        assert!(t
            .reconcile(ModifierFlags::full(false, false, false, false))
            .is_empty());
    }

    #[test]
    fn reconcile_keeps_held_modifier() {
        // 用户真按着 Alt（flags.alt=true）：绝不误发 release。
        let mut t = ModifierTracker::default();
        t.on_sent(0x38, NORM, true);
        assert!(t
            .reconcile(ModifierFlags::full(false, false, true, false))
            .is_empty());
    }

    #[test]
    fn reconcile_releases_both_sides_of_category() {
        // 左右 Ctrl 都记为按下，flags.ctrl=false → 左右都补 release。
        let mut t = ModifierTracker::default();
        t.on_sent(0x1D, NORM, true);
        t.on_sent(0x1D, EXT, true);
        let out = t.reconcile(ModifierFlags::full(false, false, false, false));
        assert_eq!(out, vec![key_up(0x1D, NORM), key_up(0x1D, EXT)]);
    }

    #[test]
    fn reconcile_skips_unknown_category() {
        // MouseMoved 只带 cmd/shift：ctrl/alt 为 None，不对账（不误抬 Alt）。
        let mut t = ModifierTracker::default();
        t.on_sent(0x38, NORM, true);
        assert!(t
            .reconcile(ModifierFlags::cmd_shift(false, false))
            .is_empty());
    }

    #[test]
    fn on_sent_up_clears_account() {
        let mut t = ModifierTracker::default();
        t.on_sent(0x2A, NORM, true);
        t.on_sent(0x2A, NORM, false);
        assert!(t
            .reconcile(ModifierFlags::full(false, false, false, false))
            .is_empty());
    }

    #[test]
    fn clear_resets_all() {
        let mut t = ModifierTracker::default();
        t.on_sent(0x5B, EXT, true);
        t.clear();
        assert!(t
            .reconcile(ModifierFlags::full(false, false, false, false))
            .is_empty());
    }

    #[test]
    fn letters_map_to_set1() {
        assert_eq!(scancode_for_key("a"), Some((0x1E, false)));
        assert_eq!(scancode_for_key("z"), Some((0x2C, false)));
        // 大写也接受（key_without_modifiers 理应小写，但防御性 lower）。
        assert_eq!(scancode_for_key("Q"), Some((0x10, false)));
    }

    #[test]
    fn digits_and_symbols() {
        assert_eq!(scancode_for_key("1"), Some((0x02, false)));
        assert_eq!(scancode_for_key("0"), Some((0x0B, false)));
        assert_eq!(scancode_for_key("/"), Some((0x35, false)));
        assert_eq!(scancode_for_key(" "), Some((0x39, false)));
    }

    #[test]
    fn named_keys_and_extended() {
        assert_eq!(scancode_for_key("enter"), Some((0x1C, false)));
        assert_eq!(scancode_for_key("escape"), Some((0x01, false)));
        assert_eq!(scancode_for_key("f5"), Some((0x3F, false)));
        assert_eq!(scancode_for_key("f11"), Some((0x57, false)));
        // f13~f20 补漏，均非扩展。
        assert_eq!(scancode_for_key("f13"), Some((0x64, false)));
        assert_eq!(scancode_for_key("f20"), Some((0x6B, false)));
        // 方向/编辑键必须是扩展键。
        assert_eq!(scancode_for_key("left"), Some((0x4B, true)));
        assert_eq!(scancode_for_key("delete"), Some((0x53, true)));
        assert_eq!(scancode_for_key("pageup"), Some((0x49, true)));
    }

    #[test]
    fn unknown_key_is_none() {
        assert_eq!(scancode_for_key("fn"), None);
        assert_eq!(scancode_for_key("§"), None);
    }

    #[test]
    fn modifiers_map_physically() {
        // ⌘ → Win 键，扩展。
        assert_eq!(
            scancode_for_modifier(KeyCode::SuperLeft),
            Some((0x5B, true))
        );
        assert_eq!(
            scancode_for_modifier(KeyCode::SuperRight),
            Some((0x5C, true))
        );
        // ⌥ → Alt，左非扩展 / 右扩展。
        assert_eq!(scancode_for_modifier(KeyCode::AltLeft), Some((0x38, false)));
        assert_eq!(scancode_for_modifier(KeyCode::AltRight), Some((0x38, true)));
        // ⌃ → Ctrl，Shift → Shift。
        assert_eq!(
            scancode_for_modifier(KeyCode::ControlLeft),
            Some((0x1D, false))
        );
        assert_eq!(
            scancode_for_modifier(KeyCode::ShiftRight),
            Some((0x36, false))
        );
        assert_eq!(scancode_for_modifier(KeyCode::F1), None);
    }

    #[test]
    fn release_events_are_all_key_up() {
        let events = modifier_release_events();
        assert_eq!(events.len(), 8);
        assert!(events
            .iter()
            .all(|e| matches!(e, RdpInputEvent::Key { pressed: false, .. })));
    }
}
