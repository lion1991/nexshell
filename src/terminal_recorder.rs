// terminal_recorder — 终端录制：raw 输出字节 → 去 ANSI 纯文本 transcript。
// 生命周期对齐 Warp PtyRecorder：start/stop 显式取出；未取出就被 drop（关 tab / 关 pane /
// 重连）时由 Drop 兜底写进录制默认目录，不静默丢。流式剥离为自研：
// Warp 只录 raw 字节（recorder.rs），无流式纯文本实现。
// 局限：vim/top 等全屏 TUI 靠光标定位原地重绘，纯文本会线性堆叠每帧内容。

/// 录制累积软上限；到顶停止追加并在结束 banner 注明截断。
const RECORDING_SOFT_CAP_BYTES: usize = 128 * 1024 * 1024;

const BANNER_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

const FILE_TIME_FORMAT: &str = "%Y%m%d-%H%M%S";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StripState {
    Normal,
    Esc,
    Csi,
    /// OSC 及 DCS/SOS/PM/APC，统一按 BEL / ST 终止。
    Osc,
    OscEsc,
}

/// 流式 ANSI 剥离器：按「终端显示语义」把字节流转成纯文本行。
/// 状态跨多次 push 保留，半截 escape 序列天然跨 chunk 续接。
pub struct AnsiTranscriptStripper {
    state: StripState,
    /// 当前未 flush 的行（\r 覆盖、\b 弹字符的作用对象）。
    line: Vec<u8>,
    /// \r 已回行首、等下个可打印字符触发覆盖；CRLF 时被 \n 取消。
    cr_pending: bool,
    out: Vec<u8>,
    cap: usize,
    truncated: bool,
}

impl AnsiTranscriptStripper {
    pub fn new(cap: usize) -> Self {
        Self {
            state: StripState::Normal,
            line: Vec::new(),
            cr_pending: false,
            out: Vec::new(),
            cap,
            truncated: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.push_byte(b);
        }
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// 还没录到任何内容（Drop 兜底据此跳过空文件）。
    pub fn is_empty(&self) -> bool {
        self.out.is_empty() && self.line.is_empty()
    }

    /// 取出累积文本；残留末行（无 \n 收尾）一并带出不丢。
    pub fn finish(mut self) -> Vec<u8> {
        if !self.line.is_empty() && !self.truncated {
            self.out.append(&mut self.line);
        }
        self.out
    }

    fn push_byte(&mut self, b: u8) {
        match self.state {
            StripState::Normal => match b {
                0x1b => self.state = StripState::Esc,
                b'\n' => {
                    self.cr_pending = false;
                    self.flush_line();
                }
                b'\r' => self.cr_pending = true,
                0x08 => {
                    if !self.cr_pending {
                        self.pop_char();
                    }
                }
                b'\t' => self.append_printable(b),
                0x00..=0x1f | 0x7f => {}
                _ => self.append_printable(b),
            },
            StripState::Esc => match b {
                b'[' => self.state = StripState::Csi,
                b']' | b'P' | b'X' | b'^' | b'_' => self.state = StripState::Osc,
                0x20..=0x2f => {}
                _ => self.state = StripState::Normal,
            },
            StripState::Csi => match b {
                0x1b => self.state = StripState::Esc,
                0x20..=0x3f => {}
                0x40..=0x7e => self.state = StripState::Normal,
                _ => {}
            },
            StripState::Osc => match b {
                0x07 => self.state = StripState::Normal,
                0x1b => self.state = StripState::OscEsc,
                _ => {}
            },
            StripState::OscEsc => match b {
                b'\\' => self.state = StripState::Normal,
                0x1b => {}
                _ => self.state = StripState::Osc,
            },
        }
    }

    fn append_printable(&mut self, b: u8) {
        if self.cr_pending {
            self.line.clear();
            self.cr_pending = false;
        }
        if self.truncated || self.out.len() + self.line.len() >= self.cap {
            self.truncated = true;
            return;
        }
        self.line.push(b);
    }

    fn flush_line(&mut self) {
        if !self.truncated && self.out.len() + self.line.len() < self.cap {
            self.out.append(&mut self.line);
            self.out.push(b'\n');
        } else {
            self.truncated = true;
        }
        self.line.clear();
    }

    /// 退格弹掉一个完整 UTF-8 字符（续接字节一起弹）。
    fn pop_char(&mut self) {
        while let Some(b) = self.line.pop() {
            if b & 0xc0 != 0x80 {
                break;
            }
        }
    }
}

/// 录制默认目录：与主机库同级的 `recordings/`（Drop 兜底和显式落盘共用）。
pub fn default_recording_dir() -> Option<std::path::PathBuf> {
    let db_path = crate::host_management::default_database_path()?;
    Some(db_path.parent()?.join("recordings"))
}

/// 把 transcript 写进录制默认目录，文件名 `<label>-<时间戳>.log`。
pub fn save_recording_to_default_dir(
    bytes: &[u8],
    label: &str,
) -> Result<std::path::PathBuf, String> {
    let dir = default_recording_dir().ok_or_else(|| "no recording dir".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = label
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    let safe = if safe.trim_matches('-').is_empty() {
        "recording".to_string()
    } else {
        safe
    };
    let path = dir.join(format!(
        "{safe}-{}.log",
        chrono::Local::now().format(FILE_TIME_FORMAT)
    ));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

/// 单次录制会话：start 记时 → push_bytes 累积 → finalize 拼 banner 出字节。
/// stripper 为 None 表示内容已被 finalize 取走，Drop 不再兜底。
pub struct TerminalRecorder {
    stripper: Option<AnsiTranscriptStripper>,
    started_at: chrono::DateTime<chrono::Local>,
    /// Drop 兜底落盘用的文件名前缀（会话标识）。
    label: String,
}

impl TerminalRecorder {
    pub fn start() -> Self {
        Self::start_labeled("recording")
    }

    pub fn start_labeled(label: &str) -> Self {
        Self {
            stripper: Some(AnsiTranscriptStripper::new(RECORDING_SOFT_CAP_BYTES)),
            started_at: chrono::Local::now(),
            label: label.to_string(),
        }
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) {
        if let Some(stripper) = self.stripper.as_mut() {
            stripper.push(bytes);
        }
    }

    /// 首行前「日志开始」banner + transcript（保证 \n 收尾）+ 末行后「日志结束」banner。
    pub fn finalize(mut self) -> Vec<u8> {
        match self.stripper.take() {
            Some(stripper) => wrap_with_banners(stripper, self.started_at),
            None => Vec::new(),
        }
    }
}

/// Drop 兜底：内容还没被 finalize 取走就落盘到录制默认目录，避免关 tab / 重连静默丢日志。
impl Drop for TerminalRecorder {
    fn drop(&mut self) {
        let Some(stripper) = self.stripper.take() else {
            return;
        };
        if stripper.is_empty() {
            return;
        }
        let bytes = wrap_with_banners(stripper, self.started_at);
        let _ = save_recording_to_default_dir(&bytes, &self.label);
    }
}

fn wrap_with_banners(
    stripper: AnsiTranscriptStripper,
    started_at: chrono::DateTime<chrono::Local>,
) -> Vec<u8> {
    let truncated = stripper.is_truncated();
    let started = started_at.format(BANNER_TIME_FORMAT).to_string();
    let ended = chrono::Local::now().format(BANNER_TIME_FORMAT).to_string();

    let mut body = stripper.finish();
    if !body.is_empty() && !body.ends_with(b"\n") {
        body.push(b'\n');
    }

    let start_banner = format!(
        "===== {} {} =====\n",
        rust_i18n::t!("log_banner_start"),
        started
    );
    let end_label = if truncated {
        format!(
            "{}（{}）",
            rust_i18n::t!("log_banner_end"),
            rust_i18n::t!("log_banner_truncated")
        )
    } else {
        rust_i18n::t!("log_banner_end").into_owned()
    };
    let end_banner = format!("===== {end_label} {ended} =====\n");

    let mut out = Vec::with_capacity(start_banner.len() + body.len() + end_banner.len());
    out.extend_from_slice(start_banner.as_bytes());
    out.append(&mut body);
    out.extend_from_slice(end_banner.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(chunks: &[&[u8]]) -> Vec<u8> {
        let mut s = AnsiTranscriptStripper::new(RECORDING_SOFT_CAP_BYTES);
        for chunk in chunks {
            s.push(chunk);
        }
        s.finish()
    }

    #[test]
    fn crlf_lines() {
        assert_eq!(strip(&[b"hello\r\nworld\n"]), b"hello\nworld\n");
    }

    #[test]
    fn carriage_return_overwrites_line() {
        assert_eq!(strip(&[b"loading...\rdone\n"]), b"done\n");
    }

    #[test]
    fn progress_updates_keep_final_state() {
        assert_eq!(strip(&[b"1%\r50%\r100%\n"]), b"100%\n");
    }

    #[test]
    fn trailing_cr_keeps_line() {
        assert_eq!(strip(&[b"abc\r"]), b"abc");
    }

    #[test]
    fn strips_sgr_color() {
        assert_eq!(strip(&[b"\x1b[31mred\x1b[0m\n"]), b"red\n");
    }

    #[test]
    fn csi_split_across_chunks() {
        assert_eq!(strip(&[b"\x1b[3", b"1mX\n"]), b"X\n");
    }

    #[test]
    fn osc_split_across_chunks() {
        assert_eq!(strip(&[b"\x1b]0;ti", b"tle\x07A\n"]), b"A\n");
    }

    #[test]
    fn osc_st_terminated() {
        assert_eq!(strip(&[b"\x1b]0;title\x1b\\A\n"]), b"A\n");
    }

    #[test]
    fn backspace_pops_char() {
        assert_eq!(strip(&[b"abc\x08\x08X\n"]), b"aX\n");
    }

    #[test]
    fn backspace_pops_full_utf8_char() {
        let mut input = "中文".as_bytes().to_vec();
        input.extend_from_slice(b"\x08!\n");
        assert_eq!(strip(&[&input]), "中!\n".as_bytes());
    }

    #[test]
    fn utf8_passthrough() {
        assert_eq!(strip(&["中文\n".as_bytes()]), "中文\n".as_bytes());
    }

    #[test]
    fn charset_designation_two_byte_escape() {
        // ESC ( B：中间字节 + final，B 不能泄漏为正文。
        assert_eq!(strip(&[b"\x1b(Bok\n"]), b"ok\n");
    }

    #[test]
    fn trailing_line_without_newline_kept() {
        assert_eq!(strip(&[b"tail"]), b"tail");
    }

    #[test]
    fn cap_marks_truncated() {
        let mut s = AnsiTranscriptStripper::new(8);
        s.push(b"12345678901234\n");
        assert!(s.is_truncated());
    }

    #[test]
    fn drop_without_finalize_saves_transcript_to_default_dir() {
        let tmp = std::env::temp_dir().join(format!(
            "nexshell-rec-drop-{}-{}",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("NEXSHELL_DB_PATH", tmp.join("nexshell.db"));

        {
            let mut rec = TerminalRecorder::start_labeled("drop-test");
            rec.push_bytes(b"lost output\n");
        }

        let files: Vec<_> = std::fs::read_dir(tmp.join("recordings"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);
        let text = std::fs::read_to_string(files[0].path()).unwrap();
        assert!(text.contains("lost output"));

        std::env::remove_var("NEXSHELL_DB_PATH");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn finalize_wraps_with_banners() {
        let mut rec = TerminalRecorder::start();
        rec.push_bytes(b"hello\nworld");
        let text = String::from_utf8(rec.finalize()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("===== "));
        assert!(lines[0].ends_with(" ====="));
        assert_eq!(lines[1], "hello");
        assert_eq!(lines[2], "world");
        assert!(lines[3].starts_with("===== "));
        assert!(lines[3].ends_with(" ====="));
    }
}
