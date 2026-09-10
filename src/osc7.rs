//! OSC 7 payload 解析：`file://host/path` → 本地 PathBuf。
//!
//! 参考 Warp `warp_terminal/src/model/ansi/mod.rs`（parse_osc_7_cwd /
//! osc_7_host_is_local / percent_decode_utf8）。差异：Warp 拒绝空 host 与
//! localhost，我们放宽接受——macOS hostname 随网络变化，shell 启动时的 $HOST
//! 与进程 gethostname 可能不一致；我们自己的注入脚本发空 host。
//! 目的只是挡掉 SSH 远端 shell 的 OSC 7 污染本地 cwd。

use std::{path::PathBuf, sync::OnceLock};

/// 本机 hostname（gethostname），首次调用后缓存。取不到时回退空串。
pub fn local_hostname() -> &'static str {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut buf = [0u8; 256];
        // SAFETY: buf 长度传给 libc，返回后按 NUL 截断
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len() - 1) };
        if rc != 0 {
            return String::new();
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8(buf[..end].to_vec()).unwrap_or_default()
    })
}

/// host 段是否算本机：空 / localhost / 与本机名的第一个 DNS label 相等
/// （忽略大小写）。比 label 而非整串，覆盖 `.local` / `.lan` / FQDN 差异；
/// 本机名取不到（空串）时拒绝一切非空 host。
fn host_is_local(host: &str, local: &str) -> bool {
    if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    fn label(s: &str) -> &str {
        s.split('.').next().unwrap_or("")
    }
    let (h, l) = (label(host), label(local));
    !l.is_empty() && h.eq_ignore_ascii_case(l)
}

/// 解析 OSC 7 payload，提取 PathBuf。支持两种形式：
/// - `file://host/path`：host 非本机则丢弃（挡 SSH 远端污染），否则 url-decode `/path`。
/// - 退化形式：裸 `/path`，整体 url-decode（本地工具用）。
pub fn parse_osc7_payload(payload: &[u8], local_hostname: &str) -> Option<PathBuf> {
    let text = std::str::from_utf8(payload).ok()?;
    let path_part = if let Some(rest) = text.strip_prefix("file://") {
        let slash = rest.find('/')?;
        let host = &rest[..slash];
        if !host_is_local(host, local_hostname) {
            log::debug!("忽略非本机 OSC 7，host={host:?}");
            return None;
        }
        &rest[slash..]
    } else {
        text
    };
    let decoded = url_decode_percent(path_part);
    if decoded.is_empty() {
        None
    } else {
        Some(PathBuf::from(decoded))
    }
}

/// URL percent-decode：`%XX` → 单字节；非法 / 残缺 `%` 按字面透传
/// （我们自己的脚本发裸 $PWD 不编码，目录名里的 `%` 必须原样保留）。
/// 解码结果非 UTF-8 时按 lossy 处理，不整条丢弃。
fn url_decode_percent(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let (Some(hi), Some(lo)) = (
                bytes.get(i + 1).copied().and_then(hex_digit),
                bytes.get(i + 2).copied().and_then(hex_digit),
            ) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL: &str = "Matts-MacBook-Pro";

    fn parse(s: &str) -> Option<PathBuf> {
        parse_osc7_payload(s.as_bytes(), LOCAL)
    }

    #[test]
    fn accepts_empty_host() {
        assert_eq!(
            parse("file:///Users/matt"),
            Some(PathBuf::from("/Users/matt"))
        );
    }

    #[test]
    fn accepts_localhost_any_case() {
        assert_eq!(parse("file://localhost/tmp"), Some(PathBuf::from("/tmp")));
        assert_eq!(parse("file://LocalHost/tmp"), Some(PathBuf::from("/tmp")));
    }

    #[test]
    fn accepts_exact_local_hostname() {
        assert_eq!(
            parse("file://Matts-MacBook-Pro/tmp"),
            Some(PathBuf::from("/tmp"))
        );
    }

    #[test]
    fn accepts_local_hostname_case_insensitive() {
        assert_eq!(
            parse("file://matts-macbook-pro/tmp"),
            Some(PathBuf::from("/tmp"))
        );
    }

    #[test]
    fn accepts_dot_suffix_either_side() {
        // 只比第一个 DNS label：.local / .lan / FQDN 差异都不影响
        for p in [
            "file://Matts-MacBook-Pro.LOCAL/tmp",
            "file://Matts-MacBook-Pro.lan/tmp",
            "file://Matts-MacBook-Pro.corp.example.com/tmp",
        ] {
            assert_eq!(parse(p), Some(PathBuf::from("/tmp")), "{p}");
        }
        assert_eq!(
            parse_osc7_payload(b"file://host/tmp", "host.local"),
            Some(PathBuf::from("/tmp"))
        );
    }

    #[test]
    fn rejects_all_hosts_when_local_hostname_empty() {
        assert_eq!(parse_osc7_payload(b"file://any/tmp", ""), None);
        // 空 host / localhost 仍放行
        assert_eq!(
            parse_osc7_payload(b"file:///tmp", ""),
            Some(PathBuf::from("/tmp"))
        );
    }

    /// host 含多字节 UTF-8 时不能在 PTY 读线程 panic（字节切片越界）。
    #[test]
    fn multibyte_host_is_rejected_without_panic() {
        assert_eq!(parse("file://中abcde/tmp/x"), None);
        assert_eq!(parse("file://中/tmp/x"), None);
        assert_eq!(
            parse_osc7_payload("file://中abcde/tmp".as_bytes(), "中abcde"),
            Some(PathBuf::from("/tmp"))
        );
    }

    #[test]
    fn accepts_bare_path() {
        assert_eq!(parse("/tmp/foo"), Some(PathBuf::from("/tmp/foo")));
    }

    #[test]
    fn keeps_semicolon_in_path() {
        assert_eq!(parse("file:///tmp/a;b"), Some(PathBuf::from("/tmp/a;b")));
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(
            parse("file:///tmp/my%20dir"),
            Some(PathBuf::from("/tmp/my dir"))
        );
    }

    #[test]
    fn keeps_literal_percent_in_path() {
        // 脚本发裸 $PWD 不做编码，目录名里的 % 必须原样保留
        assert_eq!(
            parse("file:///tmp/50%pct"),
            Some(PathBuf::from("/tmp/50%pct"))
        );
        assert_eq!(parse("file:///tmp/a%2"), Some(PathBuf::from("/tmp/a%2")));
        assert_eq!(parse("file:///tmp/a%"), Some(PathBuf::from("/tmp/a%")));
        assert_eq!(parse("file:///tmp/a%zz"), Some(PathBuf::from("/tmp/a%zz")));
    }

    #[test]
    fn non_utf8_decoded_bytes_go_lossy() {
        assert!(parse("file:///tmp/%ff%fe").is_some());
    }

    #[test]
    fn rejects_empty_and_hostless_forms() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("file://host-only"), None);
    }

    #[test]
    fn local_hostname_is_non_empty_and_cached() {
        let a = local_hostname();
        assert!(!a.is_empty());
        assert_eq!(a, local_hostname());
    }
}
