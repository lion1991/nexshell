//! 远端 → Mac（ADR 0016 第 2、4 步）。
//! 远端复制时只记下格式，由辅助进程写入延迟提供的条目；Mac 上有程序读取时才向远端要数据。
//! 文件先取清单，路径被读取时整份下载到临时目录。辅助进程不可用时（非 macOS）退回立即拉取文本。

use std::collections::VecDeque;
use std::sync::Arc;

use ironrdp_cliprdr::backend::ClipboardMessage;
use ironrdp_cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardFormatName, FileContentsResponse, FileDescriptor,
    FormatDataResponse,
};

use super::super::RdpEvent;
use super::download::{self, Download};
use super::wire::{Data, Format, Kind, Offer, Request};
use super::{helper, local, lock, text, trace, ClipboardShared};

/// 远端剪贴板里本端认得的格式。
#[derive(Clone, Copy, Debug, Default)]
struct RemoteFormats {
    text: bool,
    rtf: Option<ClipboardFormatId>,
    png: Option<ClipboardFormatId>,
    dibv5: bool,
    dib: bool,
    files: Option<ClipboardFormatId>,
}

impl RemoteFormats {
    fn parse(formats: &[ClipboardFormat]) -> Self {
        let mut found = Self::default();
        for format in formats {
            let id = format.id();
            match format.name().map(|n| n.value()) {
                Some("Rich Text Format") => found.rtf = Some(id),
                Some("PNG") => found.png = Some(id),
                Some(name) if name == ClipboardFormatName::FILE_LIST.value() => {
                    found.files = Some(id)
                }
                _ => match id {
                    ClipboardFormatId::CF_UNICODETEXT => found.text = true,
                    ClipboardFormatId::CF_DIBV5 => found.dibv5 = true,
                    ClipboardFormatId::CF_DIB => found.dib = true,
                    _ => {}
                },
            }
        }
        found
    }

    /// 图片优先要 PNG（压缩过、带透明），其次 CF_DIBV5、CF_DIB。
    fn image(&self) -> Option<(ClipboardFormatId, Format)> {
        self.png
            .map(|id| (id, Format::Png))
            .or(self
                .dibv5
                .then_some((ClipboardFormatId::CF_DIBV5, Format::Dib)))
            .or(self.dib.then_some((ClipboardFormatId::CF_DIB, Format::Dib)))
    }

    fn source(&self, kind: Kind) -> Option<(ClipboardFormatId, Format)> {
        match kind {
            Kind::Text => self
                .text
                .then_some((ClipboardFormatId::CF_UNICODETEXT, Format::UnicodeText)),
            Kind::Rtf => self.rtf.map(|id| (id, Format::Rtf)),
            Kind::Png | Kind::Tiff => self.image(),
            Kind::FileUrl => None,
        }
    }
}

/// 数据取回来交给谁。
enum Waiter {
    Helper(u64),
    /// 没有辅助进程时直接写进本地剪贴板。
    LocalText,
}

enum Fetch {
    Data {
        generation: u64,
        format: ClipboardFormatId,
        as_format: Format,
        waiter: Waiter,
    },
    FileList {
        generation: u64,
        format: ClipboardFormatId,
    },
}

pub(super) struct Remote {
    proxy: async_channel::Sender<ClipboardMessage>,
    shared: ClipboardShared,
    events: async_channel::Sender<RdpEvent>,
    /// 远端每复制一次加一；辅助进程的请求带着它，对不上说明剪贴板已经换了。
    generation: u64,
    formats: RemoteFormats,
    /// fork 只能给一个在途的 FormatDataRequest 配对应答，其余排队。
    in_flight: Option<Fetch>,
    queue: VecDeque<Fetch>,
    /// 本轮最近取到的数据：PNG 和 TIFF 共用一次图片请求。
    cache: Option<(ClipboardFormatId, Arc<[u8]>)>,
    download: Option<Download>,
}

impl Remote {
    pub(super) fn new(
        proxy: async_channel::Sender<ClipboardMessage>,
        shared: ClipboardShared,
        events: async_channel::Sender<RdpEvent>,
    ) -> Self {
        Self {
            proxy,
            shared,
            events,
            generation: 0,
            formats: RemoteFormats::default(),
            in_flight: None,
            queue: VecDeque::new(),
            cache: None,
            download: None,
        }
    }

    pub(super) fn on_remote_copy(&mut self, formats: &[ClipboardFormat], files_enabled: bool) {
        self.generation += 1;
        self.formats = RemoteFormats::parse(formats);
        self.cache = None;
        // 在途的那个留着等应答，保证后续应答配对不错位。
        for fetch in self.queue.drain(..) {
            fetch.fail();
        }
        self.download = None;
        let f = self.formats;
        if let Some(format) = f.files.filter(|_| helper::LAZY && files_enabled) {
            // 先取清单（只有元数据），知道有几个顶层条目才能写剪贴板。
            self.enqueue(Fetch::FileList {
                generation: self.generation,
                format,
            });
            return;
        }
        let offer = Offer {
            session: self.shared.session,
            generation: self.generation,
            text: f.text,
            rtf: f.rtf.is_some(),
            image: f.image().is_some(),
            files: 0,
        };
        if !(offer.text || offer.rtf || offer.image) || helper::offer(offer) {
            return;
        }
        if f.text {
            self.enqueue(Fetch::Data {
                generation: self.generation,
                format: ClipboardFormatId::CF_UNICODETEXT,
                as_format: Format::UnicodeText,
                waiter: Waiter::LocalText,
            });
        }
    }

    /// Mac 上有程序读取延迟提供的数据。
    pub(super) fn on_local_paste(&mut self, request: Request) {
        if request.generation != self.generation {
            return helper::reply(request.id, None);
        }
        if request.kind == Kind::FileUrl {
            let next = match &mut self.download {
                Some(download) => download.request(request.id, request.index),
                None => {
                    helper::reply(request.id, None);
                    None
                }
            };
            if let Some(next) = next {
                self.send(ClipboardMessage::SendFileContentsRequest(next));
            }
            return;
        }
        let Some((format, as_format)) = self.formats.source(request.kind) else {
            return helper::reply(request.id, None);
        };
        if let Some((cached, bytes)) = &self.cache {
            if *cached == format {
                let bytes = Arc::clone(bytes);
                return helper::reply(
                    request.id,
                    Some(Data {
                        format: as_format,
                        bytes,
                    }),
                );
            }
        }
        self.enqueue(Fetch::Data {
            generation: self.generation,
            format,
            as_format,
            waiter: Waiter::Helper(request.id),
        });
    }

    pub(super) fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        trace(|| {
            format!(
                "remote data error={} {} bytes",
                response.is_error(),
                response.data().len()
            )
        });
        match self.in_flight.take() {
            Some(Fetch::Data {
                generation,
                format,
                as_format,
                waiter,
            }) => {
                let bytes = (!response.is_error()).then(|| Arc::<[u8]>::from(response.data()));
                if let Some(bytes) = bytes.as_ref().filter(|_| generation == self.generation) {
                    self.cache = Some((format, Arc::clone(bytes)));
                }
                waiter.deliver(
                    bytes.map(|bytes| Data {
                        format: as_format,
                        bytes,
                    }),
                    &self.shared,
                );
            }
            Some(Fetch::FileList { .. }) => trace(|| "remote file list unavailable".into()),
            None => {}
        }
        self.pump();
    }

    pub(super) fn on_remote_file_list(&mut self, files: &[FileDescriptor], lock: Option<u32>) {
        trace(|| format!("remote file list: {} entries, lock {lock:?}", files.len()));
        match self.in_flight.take() {
            Some(Fetch::FileList { generation, .. }) if generation == self.generation => {
                let download = Download::new(
                    self.shared.session,
                    generation,
                    files,
                    lock,
                    self.events.clone(),
                );
                if let Some(download) = download {
                    helper::offer(Offer {
                        session: self.shared.session,
                        generation,
                        files: download.roots(),
                        ..Offer::default()
                    });
                    self.download = Some(download);
                }
            }
            Some(other) => other.fail(),
            None => {}
        }
        self.pump();
    }

    pub(super) fn on_file_contents_response(&mut self, response: &FileContentsResponse<'_>) {
        let next = self
            .download
            .as_mut()
            .and_then(|download| download.on_response(response));
        if let Some(next) = next {
            self.send(ClipboardMessage::SendFileContentsRequest(next));
        }
    }

    pub(super) fn cancel_download(&mut self) {
        if let Some(download) = &mut self.download {
            download.cancel();
        }
    }

    fn enqueue(&mut self, fetch: Fetch) {
        self.queue.push_back(fetch);
        self.pump();
    }

    fn pump(&mut self) {
        if self.in_flight.is_some() {
            return;
        }
        let Some(fetch) = self.queue.pop_front() else {
            return;
        };
        let format = match &fetch {
            Fetch::Data { format, .. } | Fetch::FileList { format, .. } => *format,
        };
        trace(|| format!("paste request {}", format.value()));
        self.send(ClipboardMessage::SendInitiatePaste(format));
        self.in_flight = Some(fetch);
    }

    fn send(&self, message: ClipboardMessage) {
        let _ = self.proxy.try_send(message);
    }
}

impl std::fmt::Debug for Remote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Remote")
            .field("generation", &self.generation)
            .field("formats", &self.formats)
            .field("queued", &self.queue.len())
            .finish_non_exhaustive()
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        if let Some(fetch) = self.in_flight.take() {
            fetch.fail();
        }
        for fetch in self.queue.drain(..) {
            fetch.fail();
        }
        self.download = None;
        download::remove_session_dirs(self.shared.session, None);
    }
}

impl Fetch {
    fn fail(self) {
        if let Fetch::Data {
            waiter: Waiter::Helper(id),
            ..
        } = self
        {
            helper::reply(id, None);
        }
    }
}

impl Waiter {
    fn deliver(self, data: Option<Data>, shared: &ClipboardShared) {
        match self {
            Waiter::Helper(id) => helper::reply(id, data),
            Waiter::LocalText => {
                let Some(data) = data else {
                    return;
                };
                let text = text::cf_unicode_to_mac_text(&data.bytes);
                // 持锁到记下新标记为止，轮询不会把刚写入的内容发回远端。
                let mut last = lock(&shared.last_token);
                if local::write_text(&text) {
                    if let Some(token) = local::change_token() {
                        *last = token;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(id: u32, name: &'static str) -> ClipboardFormat {
        ClipboardFormat::new(ClipboardFormatId(id)).with_name(ClipboardFormatName::new_static(name))
    }

    #[test]
    fn remote_formats_prefer_png_then_dibv5() {
        let formats = [
            ClipboardFormat::new(ClipboardFormatId::CF_DIB),
            ClipboardFormat::new(ClipboardFormatId::CF_DIBV5),
            named(0xC123, "PNG"),
            named(0xC124, "Rich Text Format"),
            ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT),
        ];
        let found = RemoteFormats::parse(&formats);
        assert_eq!(
            found.source(Kind::Tiff),
            Some((ClipboardFormatId(0xC123), Format::Png))
        );
        assert_eq!(
            found.source(Kind::Rtf),
            Some((ClipboardFormatId(0xC124), Format::Rtf))
        );
        assert!(found.source(Kind::Text).is_some());

        let dib_only = RemoteFormats::parse(&formats[..2]);
        assert_eq!(
            dib_only.source(Kind::Png),
            Some((ClipboardFormatId::CF_DIBV5, Format::Dib))
        );
        assert_eq!(dib_only.source(Kind::Text), None);
    }

    #[test]
    fn remote_formats_detect_file_list_by_name() {
        let found = RemoteFormats::parse(&[named(0xC0AB, "FileGroupDescriptorW")]);
        assert_eq!(found.files, Some(ClipboardFormatId(0xC0AB)));
        assert_eq!(found.image(), None);
    }
}
