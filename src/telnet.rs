//! Telnet 协议编解码（RFC 854/855），只协商 BINARY / ECHO / SGA / TTYPE / NAWS，
//! 其余选项一律拒绝。纯逻辑，不碰 IO；传输循环在 `terminal_runtime::telnet`。

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const NOP: u8 = 241;
const SE: u8 = 240;

const OPT_BINARY: u8 = 0;
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_TTYPE: u8 = 24;
const OPT_NAWS: u8 = 31;

const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;

/// 保活用的 IAC NOP，服务端按规范直接忽略。
pub const KEEPALIVE: [u8; 2] = [IAC, NOP];

/// 一次 `receive` 的结果：给终端的数据 + 要回给服务端的协商字节。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TelnetInput {
    pub data: Vec<u8>,
    pub reply: Vec<u8>,
}

/// 单个选项一侧的状态（RFC 1143 的简化版：我们从不主动关闭选项，不需要 WantNo）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Opt {
    No,
    Yes,
    /// 我们发了请求、在等对方回应；回应到达时不再确认，避免 DO/WILL 回环。
    WantYes,
}

#[derive(Clone, Copy)]
enum Parse {
    Data,
    Iac,
    Negotiate(u8),
    Sub,
    SubIac,
}

pub struct TelnetCodec {
    term: String,
    cols: u16,
    rows: u16,
    parse: Parse,
    sub: Vec<u8>,
    /// 上一个数据字节是 CR：NVT 下紧随的 NUL 要丢掉。
    after_cr: bool,
    /// 我方（WILL/WONT）与对方（DO/DONT 所指）各选项状态。
    us: [Opt; 256],
    him: [Opt; 256],
}

impl TelnetCodec {
    pub fn new(term: &str, cols: u16, rows: u16) -> Self {
        Self {
            term: term.to_string(),
            cols,
            rows,
            parse: Parse::Data,
            sub: Vec::new(),
            after_cr: false,
            us: [Opt::No; 256],
            him: [Opt::No; 256],
        }
    }

    /// 连上后主动发出的协商（同 PuTTY 的主动模式）。
    pub fn start(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        for opt in [OPT_NAWS, OPT_TTYPE, OPT_SGA] {
            self.us[opt as usize] = Opt::WantYes;
            out.extend_from_slice(&[IAC, WILL, opt]);
        }
        for opt in [OPT_SGA, OPT_ECHO] {
            self.him[opt as usize] = Opt::WantYes;
            out.extend_from_slice(&[IAC, DO, opt]);
        }
        out
    }

    pub fn receive(&mut self, bytes: &[u8]) -> TelnetInput {
        let mut out = TelnetInput::default();
        for &byte in bytes {
            self.parse = match self.parse {
                Parse::Data if byte == IAC => Parse::Iac,
                Parse::Data => {
                    self.push_data(byte, &mut out.data);
                    Parse::Data
                }
                Parse::Iac => match byte {
                    IAC => {
                        self.push_data(IAC, &mut out.data);
                        Parse::Data
                    }
                    WILL | WONT | DO | DONT => Parse::Negotiate(byte),
                    SB => {
                        self.sub.clear();
                        Parse::Sub
                    }
                    _ => Parse::Data,
                },
                Parse::Negotiate(cmd) => {
                    self.negotiate(cmd, byte, &mut out.reply);
                    Parse::Data
                }
                Parse::Sub if byte == IAC => Parse::SubIac,
                Parse::Sub => {
                    self.sub.push(byte);
                    Parse::Sub
                }
                Parse::SubIac => match byte {
                    SE => {
                        self.subnegotiate(&mut out.reply);
                        Parse::Data
                    }
                    IAC => {
                        self.sub.push(IAC);
                        Parse::Sub
                    }
                    _ => Parse::Sub,
                },
            };
        }
        out
    }

    /// 终端输入 → 线上字节：转义 0xFF，非 BINARY 模式下裸 CR 发成 CR NUL。
    pub fn send(&self, bytes: &[u8]) -> Vec<u8> {
        let binary = self.us[OPT_BINARY as usize] == Opt::Yes;
        let mut out = Vec::with_capacity(bytes.len() + 4);
        for (i, &byte) in bytes.iter().enumerate() {
            out.push(byte);
            if byte == IAC {
                out.push(IAC);
            } else if byte == b'\r' && !binary && bytes.get(i + 1) != Some(&b'\n') {
                out.push(0);
            }
        }
        out
    }

    /// 窗口变化；NAWS 已协商成功才返回要发的子协商。
    pub fn resize(&mut self, cols: u16, rows: u16) -> Option<Vec<u8>> {
        self.cols = cols;
        self.rows = rows;
        (self.us[OPT_NAWS as usize] == Opt::Yes).then(|| self.naws())
    }

    fn push_data(&mut self, byte: u8, data: &mut Vec<u8>) {
        let drop_nul = self.after_cr && byte == 0 && self.him[OPT_BINARY as usize] != Opt::Yes;
        self.after_cr = byte == b'\r';
        if !drop_nul {
            data.push(byte);
        }
    }

    fn negotiate(&mut self, cmd: u8, opt: u8, reply: &mut Vec<u8>) {
        let i = opt as usize;
        match cmd {
            DO => match self.us[i] {
                Opt::Yes => {}
                state if supports_local(opt) => {
                    self.us[i] = Opt::Yes;
                    if state == Opt::No {
                        reply.extend_from_slice(&[IAC, WILL, opt]);
                    }
                    if opt == OPT_NAWS {
                        reply.extend_from_slice(&self.naws());
                    }
                }
                _ => reply.extend_from_slice(&[IAC, WONT, opt]),
            },
            DONT => {
                if self.us[i] == Opt::Yes {
                    reply.extend_from_slice(&[IAC, WONT, opt]);
                }
                self.us[i] = Opt::No;
            }
            WILL => match self.him[i] {
                Opt::Yes => {}
                state if supports_remote(opt) => {
                    self.him[i] = Opt::Yes;
                    if state == Opt::No {
                        reply.extend_from_slice(&[IAC, DO, opt]);
                    }
                }
                _ => reply.extend_from_slice(&[IAC, DONT, opt]),
            },
            WONT => {
                if self.him[i] == Opt::Yes {
                    reply.extend_from_slice(&[IAC, DONT, opt]);
                }
                self.him[i] = Opt::No;
            }
            _ => {}
        }
    }

    fn subnegotiate(&mut self, reply: &mut Vec<u8>) {
        if self.sub.as_slice() == [OPT_TTYPE, TTYPE_SEND] && self.us[OPT_TTYPE as usize] == Opt::Yes
        {
            reply.extend_from_slice(&[IAC, SB, OPT_TTYPE, TTYPE_IS]);
            push_escaped(reply, self.term.as_bytes());
            reply.extend_from_slice(&[IAC, SE]);
        }
    }

    fn naws(&self) -> Vec<u8> {
        let mut out = vec![IAC, SB, OPT_NAWS];
        let [c0, c1] = self.cols.to_be_bytes();
        let [r0, r1] = self.rows.to_be_bytes();
        push_escaped(&mut out, &[c0, c1, r0, r1]);
        out.extend_from_slice(&[IAC, SE]);
        out
    }
}

fn supports_local(opt: u8) -> bool {
    matches!(opt, OPT_BINARY | OPT_SGA | OPT_TTYPE | OPT_NAWS)
}

fn supports_remote(opt: u8) -> bool {
    matches!(opt, OPT_BINARY | OPT_ECHO | OPT_SGA)
}

fn push_escaped(out: &mut Vec<u8>, bytes: &[u8]) {
    for &byte in bytes {
        out.push(byte);
        if byte == IAC {
            out.push(IAC);
        }
    }
}

/// 自动应答登录提示：用户名 / 密码各最多答一次；答完密码或输出超出预算即停用，
/// 免得会话里再出现 `login:`（如在远端再 telnet 别处）时误发。
pub struct TelnetAutoLogin {
    username: Option<String>,
    password: Option<String>,
    /// 最近输出的尾巴（小写），用于跨 chunk 匹配提示。
    tail: String,
    /// 剩余可等待的输出字节数，耗尽仍未见提示就停用。
    budget: usize,
}

const OUTPUT_BUDGET_BYTES: usize = 16 * 1024;

const PROMPT_TAIL_CHARS: usize = 64;
const USERNAME_PROMPTS: [&str; 4] = ["login:", "username:", "user name:", "用户名:"];
const PASSWORD_PROMPTS: [&str; 2] = ["password:", "密码:"];

impl TelnetAutoLogin {
    pub fn new(username: &str, password: &str) -> Self {
        Self {
            username: Some(username.to_string()).filter(|v| !v.is_empty()),
            password: Some(password.to_string()).filter(|v| !v.is_empty()),
            tail: String::new(),
            budget: OUTPUT_BUDGET_BYTES,
        }
    }

    /// 喂入解码后的终端输出；命中提示时返回要发送的一行（含 CR）。
    pub fn feed(&mut self, text: &str) -> Option<String> {
        if self.username.is_none() && self.password.is_none() {
            return None;
        }
        self.tail.push_str(&text.to_lowercase().replace('：', ":"));
        let excess = self.tail.chars().count().saturating_sub(PROMPT_TAIL_CHARS);
        if excess > 0 {
            self.tail = self.tail.chars().skip(excess).collect();
        }
        let tail = self.tail.trim_end();
        let line = if PASSWORD_PROMPTS.iter().any(|p| tail.ends_with(p)) {
            let password = self.password.take()?;
            self.username = None;
            password
        } else if USERNAME_PROMPTS.iter().any(|p| tail.ends_with(p)) {
            self.username.take()?
        } else {
            self.budget = self.budget.saturating_sub(text.len());
            if self.budget == 0 {
                self.username = None;
                self.password = None;
            }
            return None;
        };
        self.tail.clear();
        Some(line + "\r")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codec() -> TelnetCodec {
        TelnetCodec::new("xterm-256color", 80, 24)
    }

    #[test]
    fn plain_data_passes_through() {
        let mut c = codec();
        let out = c.receive(b"hello");
        assert_eq!(out.data, b"hello");
        assert!(out.reply.is_empty());
    }

    #[test]
    fn escaped_iac_in_data_becomes_single_ff() {
        let mut c = codec();
        assert_eq!(
            c.receive(&[b'a', 255, 255, b'b']).data,
            vec![b'a', 255, b'b']
        );
    }

    #[test]
    fn command_split_across_reads_is_reassembled() {
        let mut c = codec();
        let first = c.receive(&[b'x', 255]);
        assert_eq!(first.data, b"x");
        assert!(first.reply.is_empty());
        let second = c.receive(&[251]);
        assert!(second.data.is_empty());
        assert!(second.reply.is_empty());
        // 服务端 WILL ECHO → 回 DO ECHO
        let third = c.receive(&[1, b'y']);
        assert_eq!(third.data, b"y");
        assert_eq!(third.reply, vec![255, 253, 1]);
    }

    #[test]
    fn subnegotiation_split_across_reads_answers_ttype() {
        let mut c = codec();
        // 先让 TTYPE 在我方启用：服务端 DO TTYPE → WILL TTYPE
        assert_eq!(c.receive(&[255, 253, 24]).reply, vec![255, 251, 24]);
        let a = c.receive(&[255, 250, 24]);
        assert!(a.reply.is_empty());
        let b = c.receive(&[1, 255]);
        assert!(b.reply.is_empty());
        let done = c.receive(&[240, b'$']);
        assert_eq!(done.data, b"$");
        let mut want = vec![255, 250, 24, 0];
        want.extend_from_slice(b"xterm-256color");
        want.extend_from_slice(&[255, 240]);
        assert_eq!(done.reply, want);
    }

    #[test]
    fn subnegotiation_payload_never_leaks_into_data() {
        let mut c = codec();
        let out = c.receive(&[255, 250, 99, b'j', b'u', b'n', b'k', 255, 240, b'o', b'k']);
        assert_eq!(out.data, b"ok");
    }

    #[test]
    fn unsupported_options_are_refused() {
        let mut c = codec();
        // DO LINEMODE(34) → WONT；WILL STATUS(5) → DONT
        assert_eq!(c.receive(&[255, 253, 34]).reply, vec![255, 252, 34]);
        assert_eq!(c.receive(&[255, 251, 5]).reply, vec![255, 254, 5]);
    }

    #[test]
    fn repeated_request_for_enabled_option_is_not_acknowledged_again() {
        let mut c = codec();
        assert_eq!(c.receive(&[255, 251, 1]).reply, vec![255, 253, 1]);
        assert!(c.receive(&[255, 251, 1]).reply.is_empty());
        assert_eq!(c.receive(&[255, 253, 3]).reply, vec![255, 251, 3]);
        assert!(c.receive(&[255, 253, 3]).reply.is_empty());
    }

    #[test]
    fn reply_to_our_own_request_is_not_acknowledged() {
        let mut c = codec();
        let start = c.start();
        assert!(
            start.windows(3).any(|w| w == [255, 253, 1]),
            "start 应含 DO ECHO"
        );
        // 服务端对我们的 DO ECHO 回 WILL ECHO：不再回 DO
        assert!(c.receive(&[255, 251, 1]).reply.is_empty());
        // 服务端拒绝我们的 WILL SGA：不回任何东西
        assert!(c.receive(&[255, 254, 3]).reply.is_empty());
    }

    #[test]
    fn disabling_enabled_option_is_confirmed_once() {
        let mut c = codec();
        c.receive(&[255, 251, 1]);
        assert_eq!(c.receive(&[255, 252, 1]).reply, vec![255, 254, 1]);
        assert!(c.receive(&[255, 252, 1]).reply.is_empty());
    }

    #[test]
    fn naws_sent_when_agreed_and_on_resize() {
        let mut c = codec();
        // 未协商时 resize 不发
        assert_eq!(c.resize(100, 30), None);
        let out = c.receive(&[255, 253, 31]);
        assert_eq!(
            out.reply,
            vec![255, 251, 31, 255, 250, 31, 0, 100, 0, 30, 255, 240]
        );
        assert_eq!(
            c.resize(300, 50),
            Some(vec![255, 250, 31, 1, 44, 0, 50, 255, 240])
        );
    }

    #[test]
    fn naws_sent_when_server_accepts_our_will() {
        let mut c = codec();
        c.start();
        assert_eq!(
            c.receive(&[255, 253, 31]).reply,
            vec![255, 250, 31, 0, 80, 0, 24, 255, 240]
        );
    }

    #[test]
    fn naws_doubles_ff_bytes() {
        let mut c = codec();
        c.receive(&[255, 253, 31]);
        assert_eq!(
            c.resize(255, 24),
            Some(vec![255, 250, 31, 0, 255, 255, 0, 24, 255, 240])
        );
    }

    #[test]
    fn send_escapes_ff_and_turns_bare_cr_into_cr_nul() {
        let c = codec();
        assert_eq!(c.send(&[b'a', 255]), vec![b'a', 255, 255]);
        assert_eq!(c.send(b"ls\r"), b"ls\r\0".to_vec());
        assert_eq!(c.send(b"a\r\nb"), b"a\r\nb".to_vec());
    }

    #[test]
    fn send_keeps_bare_cr_in_binary_mode() {
        let mut c = codec();
        assert_eq!(c.receive(&[255, 253, 0]).reply, vec![255, 251, 0]);
        assert_eq!(c.send(b"ls\r"), b"ls\r".to_vec());
    }

    #[test]
    fn inbound_cr_nul_drops_nul_even_across_reads() {
        let mut c = codec();
        assert_eq!(c.receive(b"a\r\0b").data, b"a\rb");
        assert_eq!(c.receive(b"c\r").data, b"c\r");
        assert_eq!(c.receive(b"\0d").data, b"d");
    }

    #[test]
    fn inbound_nul_kept_when_server_sends_binary() {
        let mut c = codec();
        c.receive(&[255, 251, 0]);
        assert_eq!(c.receive(b"a\r\0b").data, b"a\r\0b");
    }

    #[test]
    fn other_commands_are_swallowed() {
        let mut c = codec();
        // NOP / GA 不进数据
        assert_eq!(c.receive(&[b'a', 255, 241, 255, 249, b'b']).data, b"ab");
    }

    #[test]
    fn auto_login_answers_username_then_password_once() {
        let mut a = TelnetAutoLogin::new("admin", "s3cret");
        assert_eq!(a.feed("Welcome\r\n"), None);
        assert_eq!(a.feed("Username: "), Some("admin\r".to_string()));
        assert_eq!(a.feed("\r\nPassword:"), Some("s3cret\r".to_string()));
        // 登录失败再次提示：交给用户
        assert_eq!(a.feed("\r\nUsername: "), None);
        assert_eq!(a.feed("Password: "), None);
    }

    #[test]
    fn auto_login_matches_prompt_split_across_chunks_and_case() {
        let mut a = TelnetAutoLogin::new("root", "");
        assert_eq!(a.feed("router LOG"), None);
        assert_eq!(a.feed("IN: "), Some("root\r".to_string()));
    }

    #[test]
    fn auto_login_ignores_prompt_words_not_at_end() {
        let mut a = TelnetAutoLogin::new("root", "pw");
        assert_eq!(a.feed("Last login: Tue Oct  1 10:00\r\n$ "), None);
    }

    #[test]
    fn auto_login_password_only_device() {
        let mut a = TelnetAutoLogin::new("", "cisco");
        assert_eq!(a.feed("User Access Verification\r\n\r\nUsername: "), None);
        assert_eq!(a.feed("Password: "), Some("cisco\r".to_string()));
    }

    #[test]
    fn auto_login_chinese_prompts() {
        let mut a = TelnetAutoLogin::new("admin", "pw");
        assert_eq!(a.feed("用户名："), Some("admin\r".to_string()));
        assert_eq!(a.feed("密码:"), Some("pw\r".to_string()));
    }

    #[test]
    fn auto_login_disabled_after_password_even_if_username_unused() {
        let mut a = TelnetAutoLogin::new("admin", "pw");
        assert_eq!(a.feed("Password:"), Some("pw\r".to_string()));
        assert_eq!(a.feed("\r\nlogin: "), None);
    }

    #[test]
    fn auto_login_gives_up_after_long_output_without_prompt() {
        let mut a = TelnetAutoLogin::new("admin", "pw");
        let banner = "x".repeat(20 * 1024);
        assert_eq!(a.feed(&banner), None);
        assert_eq!(a.feed("\r\nlogin: "), None);
    }
}
