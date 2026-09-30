//! CF_UNICODETEXT 与 Mac 文本互转。
//! CF_UNICODETEXT 规范：UTF-16LE + CRLF 换行 + 双字节 NUL 结尾；Mac 侧用 LF。

/// Mac 文本(LF) → CF_UNICODETEXT 字节：LF→CRLF + UTF-16LE + 双字节 NUL。
pub(super) fn mac_text_to_cf_unicode(text: &str) -> Vec<u8> {
    let crlf = lf_to_crlf(text);
    let mut bytes: Vec<u8> = crlf.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    bytes.push(0);
    bytes.push(0);
    bytes
}

/// CF_UNICODETEXT 字节 → Mac 文本(LF)：UTF-16LE 解码 + NUL 截断 + CRLF→LF。
pub(super) fn cf_unicode_to_mac_text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    let decoded = String::from_utf16_lossy(&units[..end]);
    crlf_to_lf(&decoded)
}

/// LF→CRLF。先把已有 CRLF/裸 CR 归一到 LF，再统一升 CRLF，避免混合换行被重复放大。
fn lf_to_crlf(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

/// CRLF→LF（含裸 CR）。
fn crlf_to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lf_to_crlf_upgrades_and_normalizes_mixed() {
        assert_eq!(lf_to_crlf("a\nb"), "a\r\nb");
        // 混合：已有 CRLF、裸 CR、LF 统一成 CRLF，不重复。
        assert_eq!(lf_to_crlf("a\r\nb\rc\nd"), "a\r\nb\r\nc\r\nd");
        assert_eq!(lf_to_crlf(""), "");
    }

    #[test]
    fn crlf_to_lf_downgrades() {
        assert_eq!(crlf_to_lf("a\r\nb"), "a\nb");
        assert_eq!(crlf_to_lf("a\rb"), "a\nb");
    }

    #[test]
    fn cf_unicode_bytes_end_with_double_nul_and_crlf() {
        let bytes = mac_text_to_cf_unicode("a\nb");
        // "a\r\nb" = 4 UTF-16 单元 + NUL 终止 = 5*2 = 10 字节。
        assert_eq!(bytes.len(), 10);
        assert_eq!(&bytes[bytes.len() - 2..], &[0, 0]);
        // 第 2 个单元应是 CR(0x0D)。
        assert_eq!(u16::from_le_bytes([bytes[2], bytes[3]]), 0x000D);
    }

    #[test]
    fn nul_truncates_trailing_garbage() {
        // "hi" + NUL + 垃圾数据，解码应只得 "hi"。
        let mut bytes: Vec<u8> = "hi".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&[0x42, 0x00, 0x43, 0x00]); // B C
        assert_eq!(cf_unicode_to_mac_text(&bytes), "hi");
    }

    #[test]
    fn utf16_roundtrip_non_ascii() {
        // CJK + emoji（含代理对），LF 换行。
        for text in ["你好\n世界", "emoji 😀 mix\nline", "a\nb\nc", ""] {
            let bytes = mac_text_to_cf_unicode(text);
            let back = cf_unicode_to_mac_text(&bytes);
            assert_eq!(back, text, "roundtrip failed for {text:?}");
        }
    }

    #[test]
    fn roundtrip_normalizes_crlf_to_lf() {
        // 送出 LF、收回 LF：远端拿到 CRLF，本地始终 LF。
        let bytes = mac_text_to_cf_unicode("line1\nline2");
        assert_eq!(cf_unicode_to_mac_text(&bytes), "line1\nline2");
    }

    #[test]
    fn odd_length_bytes_are_tolerated() {
        // 尾部落单字节被 chunks_exact 丢弃，不 panic。
        let decoded = cf_unicode_to_mac_text(&[0x41, 0x00, 0x42]);
        assert_eq!(decoded, "A");
    }
}
