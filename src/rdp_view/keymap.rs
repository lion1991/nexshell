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

#[cfg(test)]
mod tests {
    use super::*;

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
