// 终端内 SSH 密码输入的行编辑器：不回显，只认退格 / 清行 / 取消 / 回车，丢弃转义序列。

#[derive(Debug, PartialEq, Eq)]
pub enum PasswordInput {
    Pending,
    Submit(String),
    Cancel,
}

#[derive(Default)]
enum EscapeState {
    #[default]
    Ground,
    Escape,
    Csi,
    Ss3,
}

#[derive(Default)]
pub struct PasswordLineEditor {
    buf: Vec<u8>,
    escape: EscapeState,
}

impl PasswordLineEditor {
    /// 回车 / Ctrl-C 之后的剩余字节直接丢弃。
    pub fn feed(&mut self, bytes: &[u8]) -> PasswordInput {
        for &byte in bytes {
            match self.escape {
                EscapeState::Escape => {
                    self.escape = match byte {
                        b'[' => EscapeState::Csi,
                        b'O' => EscapeState::Ss3,
                        _ => EscapeState::Ground,
                    };
                    continue;
                }
                EscapeState::Csi => {
                    if (0x40..=0x7e).contains(&byte) {
                        self.escape = EscapeState::Ground;
                    }
                    continue;
                }
                EscapeState::Ss3 => {
                    self.escape = EscapeState::Ground;
                    continue;
                }
                EscapeState::Ground => {}
            }
            match byte {
                b'\r' | b'\n' => {
                    let password = String::from_utf8_lossy(&self.buf).into_owned();
                    self.buf.clear();
                    return PasswordInput::Submit(password);
                }
                0x03 | 0x04 => return PasswordInput::Cancel,
                0x7f | 0x08 => self.pop_char(),
                0x15 => self.buf.clear(),
                0x1b => self.escape = EscapeState::Escape,
                0x00..=0x1f => {}
                _ => self.buf.push(byte),
            }
        }
        PasswordInput::Pending
    }

    fn pop_char(&mut self) {
        while let Some(byte) = self.buf.pop() {
            // UTF-8 续字节（10xxxxxx）继续弹，直到弹掉首字节
            if byte & 0xc0 != 0x80 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_submits_line() {
        let mut editor = PasswordLineEditor::default();
        assert_eq!(editor.feed(b"sec"), PasswordInput::Pending);
        assert_eq!(
            editor.feed(b"ret\r\n"),
            PasswordInput::Submit("secret".to_string())
        );
    }

    #[test]
    fn empty_enter_submits_empty_password() {
        let mut editor = PasswordLineEditor::default();
        assert_eq!(editor.feed(b"\r"), PasswordInput::Submit(String::new()));
    }

    #[test]
    fn backspace_removes_whole_multibyte_char() {
        let mut editor = PasswordLineEditor::default();
        editor.feed("a密码".as_bytes());
        editor.feed(&[0x7f]);
        assert_eq!(editor.feed(b"\r"), PasswordInput::Submit("a密".to_string()));
    }

    #[test]
    fn ctrl_u_clears_and_ctrl_c_cancels() {
        let mut editor = PasswordLineEditor::default();
        editor.feed(b"wrong\x15ok");
        assert_eq!(editor.feed(b"\r"), PasswordInput::Submit("ok".to_string()));
        assert_eq!(editor.feed(b"abc\x03def\r"), PasswordInput::Cancel);
    }

    #[test]
    fn escape_sequences_are_dropped() {
        let mut editor = PasswordLineEditor::default();
        editor.feed(b"\x1b[200~pa\x1b[A\x1bOBss\x1b[201~");
        assert_eq!(
            editor.feed(b"\r"),
            PasswordInput::Submit("pass".to_string())
        );
    }
}
