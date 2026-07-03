//! CLIPRDR 文本剪贴板双向同步（见 docs/adr/0007 第⑤步）。v1 仅 CF_UNICODETEXT。
//! backend 回调跑在 RDP 线程（active_stage.process 内），不能重入 active_stage 借用，
//! 故经 async_channel 把"要发的 cliprdr PDU"回递事件循环统一编码发送。
//! Mac→远端靠 1s 轮询剪贴板文本 hash 感知变化，广播 format list；远端按需 request 时再读内容应答。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use arboard::Clipboard;
use ironrdp_cliprdr::backend::{ClipboardMessage, CliprdrBackend};
use ironrdp_cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags, FileContentsRequest,
    FileContentsResponse, FormatDataRequest, FormatDataResponse, LockDataId,
    OwnedFormatDataResponse,
};

/// backend（RDP 线程回调）与事件循环轮询之间的共享状态。
#[derive(Clone)]
pub(super) struct ClipboardShared {
    /// 已知本地剪贴板文本 hash。轮询据此判断"本地是否有新复制"；
    /// backend 写入远端来的文本后也更新它，避免把自己的写当成本地复制回传（消回环）。
    last_local_hash: Arc<AtomicU64>,
    /// cliprdr 通道就绪（on_ready）后置 true；轮询在此之前不广播。
    ready: Arc<AtomicBool>,
}

impl ClipboardShared {
    pub(super) fn new() -> Self {
        Self {
            last_local_hash: Arc::new(AtomicU64::new(0)),
            ready: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// CF_UNICODETEXT 文本 backend：把 Mac 剪贴板接进 IronRDP cliprdr 通道。
#[derive(Debug)]
pub(super) struct TextCliprdrBackend {
    proxy: async_channel::Sender<ClipboardMessage>,
    last_local_hash: Arc<AtomicU64>,
    ready: Arc<AtomicBool>,
    // trait 要求返回 &str，需自持一份（文件传输才用，文本路径无意义）。
    temp_dir: String,
}

impl TextCliprdrBackend {
    pub(super) fn new(
        proxy: async_channel::Sender<ClipboardMessage>,
        shared: &ClipboardShared,
    ) -> Self {
        Self {
            proxy,
            last_local_hash: Arc::clone(&shared.last_local_hash),
            ready: Arc::clone(&shared.ready),
            temp_dir: ".".to_string(),
        }
    }

    fn send(&self, msg: ClipboardMessage) {
        let _ = self.proxy.try_send(msg);
    }
}

ironrdp_core::impl_as_any!(TextCliprdrBackend);

impl CliprdrBackend for TextCliprdrBackend {
    fn temporary_directory(&self) -> &str {
        &self.temp_dir
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        // 长格式名：现代 mstsc/Windows 默认用之，标准格式(CF_UNICODETEXT)也兼容短名。
        ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
    }

    fn on_ready(&mut self) {
        self.ready.store(true, Ordering::Relaxed);
    }

    fn on_request_format_list(&mut self) {
        // 初始化期请求广播本地格式：Mac 剪贴板有文本就登告 CF_UNICODETEXT。
        if let Some(text) = read_clipboard_text() {
            if !text.is_empty() {
                self.last_local_hash
                    .store(hash_text(&text), Ordering::Relaxed);
                self.send(ClipboardMessage::SendInitiateCopy(vec![
                    unicode_text_format(),
                ]));
            }
        }
    }

    fn on_process_negotiated_capabilities(
        &mut self,
        _capabilities: ClipboardGeneralCapabilityFlags,
    ) {
    }

    fn on_remote_copy(&mut self, available_formats: &[ClipboardFormat]) {
        // 远端复制含文本格式：立即 request 数据（eager 策略）。
        if available_formats
            .iter()
            .any(|f| f.id() == ClipboardFormatId::CF_UNICODETEXT)
        {
            self.send(ClipboardMessage::SendInitiatePaste(
                ClipboardFormatId::CF_UNICODETEXT,
            ));
        }
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        // 远端要粘我方数据：读 Mac 剪贴板 → CF_UNICODETEXT 应答。
        let response = if request.format == ClipboardFormatId::CF_UNICODETEXT {
            match read_clipboard_text() {
                Some(text) => OwnedFormatDataResponse::new_data(mac_text_to_cf_unicode(&text)),
                None => OwnedFormatDataResponse::new_error(),
            }
        } else {
            OwnedFormatDataResponse::new_error()
        };
        self.send(ClipboardMessage::SendFormatData(response));
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        // 远端发来数据：解码写入 Mac 剪贴板，同步 hash 抑制回环。
        if response.is_error() {
            return;
        }
        let text = cf_unicode_to_mac_text(response.data());
        self.last_local_hash
            .store(hash_text(&text), Ordering::Relaxed);
        write_clipboard_text(&text);
    }

    // v1 不做文件/锁，全部空实现。
    fn on_file_contents_request(&mut self, _request: FileContentsRequest) {}
    fn on_file_contents_response(&mut self, _response: FileContentsResponse<'_>) {}
    fn on_lock(&mut self, _data_id: LockDataId) {}
    fn on_unlock(&mut self, _data_id: LockDataId) {}
}

/// 轮询一次：cliprdr 就绪且 Mac 剪贴板文本较上次有变化时，返回要广播的格式；否则 None。
/// 更新共享 hash，使远端后续 request 与再次轮询自洽。
pub(super) fn poll_local_change(shared: &ClipboardShared) -> Option<Vec<ClipboardFormat>> {
    if !shared.ready.load(Ordering::Relaxed) {
        return None;
    }
    let text = read_clipboard_text()?;
    if text.is_empty() {
        return None;
    }
    let hash = hash_text(&text);
    if hash == shared.last_local_hash.load(Ordering::Relaxed) {
        return None;
    }
    shared.last_local_hash.store(hash, Ordering::Relaxed);
    Some(vec![unicode_text_format()])
}

fn unicode_text_format() -> ClipboardFormat {
    ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)
}

// ---- Mac 剪贴板访问（仅 RDP 线程调用，无并发）。arboard 失败一律降级为无操作 ----

fn read_clipboard_text() -> Option<String> {
    Clipboard::new().ok()?.get_text().ok()
}

fn write_clipboard_text(text: &str) {
    if let Ok(mut clipboard) = Clipboard::new() {
        let _ = clipboard.set_text(text.to_owned());
    }
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

// ---- 换行 + 编码转换纯函数 ----
// CF_UNICODETEXT 规范：UTF-16LE + CRLF 换行 + 双字节 NUL 结尾；Mac 侧用 LF。

/// Mac 文本(LF) → CF_UNICODETEXT 字节：LF→CRLF + UTF-16LE + 双字节 NUL。
fn mac_text_to_cf_unicode(text: &str) -> Vec<u8> {
    let crlf = lf_to_crlf(text);
    let mut bytes: Vec<u8> = crlf.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    bytes.push(0);
    bytes.push(0);
    bytes
}

/// CF_UNICODETEXT 字节 → Mac 文本(LF)：UTF-16LE 解码 + NUL 截断 + CRLF→LF。
fn cf_unicode_to_mac_text(bytes: &[u8]) -> String {
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
