//! CLIPRDR 剪贴板双向同步（见 docs/adr/0016）。
//! backend 回调跑在 RDP 线程（active_stage.process 内），不能重入 active_stage 借用，
//! 故经 async_channel 把"要发的 cliprdr PDU"回递事件循环统一编码发送。
//! Mac → 远端：轮询本地变化标记，变了就广播格式清单或文件清单；远端请求时由工作线程读取、转换后应答。
//! 远端 → Mac：目前只同步文本，远端一复制就立即拉取写入。

mod dib;
mod files;
#[cfg_attr(target_os = "macos", path = "local_mac.rs")]
#[cfg_attr(not(target_os = "macos"), path = "local_other.rs")]
mod local;
mod text;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use ironrdp_cliprdr::backend::{ClipboardMessage, CliprdrBackend};
use ironrdp_cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardFormatName, ClipboardGeneralCapabilityFlags,
    FileContentsFlags, FileContentsRequest, FileContentsResponse, FormatDataRequest,
    FormatDataResponse, LockDataId, OwnedFormatDataResponse,
};
use ironrdp_cliprdr::{Client, CliprdrClient, CliprdrSvcMessages};
use ironrdp_pdu::PduResult;

/// 本地剪贴板变化的轮询间隔。
pub(super) const POLL_INTERVAL: Duration = local::POLL_INTERVAL;

/// 本端注册格式的 ID，取私有区并避开 fork 文件清单用的 0xC0FD / 0xC0FE；服务端按名字识别。
const RTF_FORMAT_ID: ClipboardFormatId = ClipboardFormatId(0xC0F0);
const PNG_FORMAT_ID: ClipboardFormatId = ClipboardFormatId(0xC0F1);
const RTF_FORMAT_NAME: ClipboardFormatName = ClipboardFormatName::new_static("Rich Text Format");
const PNG_FORMAT_NAME: ClipboardFormatName = ClipboardFormatName::new_static("PNG");

/// 本地剪贴板里能广播给远端的内容。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Available {
    text: bool,
    rtf: bool,
    image: bool,
    files: bool,
}

/// 自上而下的 BGRA 位图。
struct Bitmap {
    width: usize,
    height: usize,
    bgra: Vec<u8>,
}

/// backend（RDP 线程回调）与轮询线程之间的共享状态。
#[derive(Clone, Debug)]
pub(super) struct ClipboardShared {
    /// 已处理过的本地变化标记。写入远端内容时持锁到记下新标记为止，
    /// 轮询就看不到"清空了还没写完"的中间态，也不会把自己写的内容发回远端。
    last_token: Arc<Mutex<u64>>,
    /// cliprdr 通道就绪（on_ready）后置 true；轮询在此之前不广播。
    ready: Arc<AtomicBool>,
    /// 服务端同意文件流（STREAM_FILECLIP_ENABLED）；否则 Finder 复制的文件只按文本广播。
    files_enabled: Arc<AtomicBool>,
    /// 远端剪贴板里现在是哪批本地文件（Finder 给的顶层路径）；远端换了内容就清空。
    offered_roots: Arc<Mutex<Vec<PathBuf>>>,
}

impl ClipboardShared {
    pub(super) fn new() -> Self {
        Self {
            last_token: Arc::new(Mutex::new(0)),
            ready: Arc::new(AtomicBool::new(false)),
            files_enabled: Arc::new(AtomicBool::new(false)),
            offered_roots: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

/// 轮询发现本地变化后要广播的内容。
#[derive(Debug)]
pub(super) enum LocalOffer {
    Formats(Vec<ClipboardFormat>),
    Files(files::FileOffer),
}

/// 远端要本端数据的请求，交给工作线程按序读取后应答。
enum Job {
    Format(ClipboardFormatId),
    File {
        list: files::FileList,
        request: FileContentsRequest,
    },
}

/// 把 Mac 剪贴板接进 IronRDP cliprdr 通道。
#[derive(Debug)]
pub(super) struct RdpCliprdrBackend {
    proxy: async_channel::Sender<ClipboardMessage>,
    shared: ClipboardShared,
    jobs: mpsc::Sender<Job>,
    /// 最近一次广播的文件清单。远端锁住剪贴板时另存快照，带锁 ID 的请求按快照取文件。
    offered: Option<files::FileList>,
    locked: HashMap<u32, files::FileList>,
    // trait 要求返回 &str，需自持一份（文件传输才用）。
    temp_dir: String,
}

impl RdpCliprdrBackend {
    pub(super) fn new(
        proxy: async_channel::Sender<ClipboardMessage>,
        shared: &ClipboardShared,
    ) -> Self {
        let (jobs, pending) = mpsc::channel::<Job>();
        let reply = proxy.clone();
        // backend 释放后 jobs 断开，线程随之退出。
        thread::spawn(move || {
            let mut reader = files::RangeReader::default();
            for job in pending {
                let message = match job {
                    Job::Format(format) => answer_format(format),
                    Job::File { list, request } => answer_file(&list, &request, &mut reader),
                };
                if reply.send_blocking(message).is_err() {
                    break;
                }
            }
        });
        Self {
            proxy,
            shared: shared.clone(),
            jobs,
            offered: None,
            locked: HashMap::new(),
            temp_dir: ".".to_string(),
        }
    }

    fn send(&self, msg: ClipboardMessage) {
        let _ = self.proxy.try_send(msg);
    }
}

ironrdp_core::impl_as_any!(RdpCliprdrBackend);

impl CliprdrBackend for RdpCliprdrBackend {
    fn temporary_directory(&self) -> &str {
        &self.temp_dir
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
            | ClipboardGeneralCapabilityFlags::STREAM_FILECLIP_ENABLED
            | ClipboardGeneralCapabilityFlags::FILECLIP_NO_FILE_PATHS
            | ClipboardGeneralCapabilityFlags::CAN_LOCK_CLIPDATA
            | ClipboardGeneralCapabilityFlags::HUGE_FILE_SUPPORT_ENABLED
    }

    fn on_ready(&mut self) {
        self.shared.ready.store(true, Ordering::Relaxed);
    }

    fn on_request_format_list(&mut self) {
        // 初始化期必须发一次格式清单（可以为空），同步当前 Mac 剪贴板。
        let mut last = lock(&self.shared.last_token);
        let available = local::available();
        // 通道就绪前发不了文件清单：不记标记，就绪后由轮询补发。
        if !available.files {
            if let Some(token) = local::change_token() {
                *last = token;
            }
        }
        let formats = advertised_formats(available);
        drop(last);
        trace(|| format!("initial advertise [{}]", describe(&formats)));
        self.send(ClipboardMessage::SendInitiateCopy(formats));
    }

    fn on_format_list_response(&mut self, ok: bool) {
        // 远端拒收时 fork 已丢掉它那份文件清单，这里跟着丢。
        if !ok {
            self.offered = None;
            lock(&self.shared.offered_roots).clear();
        }
    }

    fn on_process_negotiated_capabilities(
        &mut self,
        capabilities: ClipboardGeneralCapabilityFlags,
    ) {
        trace(|| format!("negotiated {capabilities:?}"));
        self.shared.files_enabled.store(
            capabilities.contains(ClipboardGeneralCapabilityFlags::STREAM_FILECLIP_ENABLED),
            Ordering::Relaxed,
        );
    }

    fn on_remote_copy(&mut self, available_formats: &[ClipboardFormat]) {
        trace(|| format!("remote copy [{}]", describe(available_formats)));
        lock(&self.shared.offered_roots).clear();
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
        if self.jobs.send(Job::Format(request.format)).is_err() {
            self.send(ClipboardMessage::SendFormatData(
                OwnedFormatDataResponse::new_error(),
            ));
        }
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        trace(|| {
            format!(
                "remote data error={} {} bytes",
                response.is_error(),
                response.data().len()
            )
        });
        if response.is_error() {
            return;
        }
        let text = text::cf_unicode_to_mac_text(response.data());
        let mut last = lock(&self.shared.last_token);
        if local::write_text(&text) {
            if let Some(token) = local::change_token() {
                *last = token;
            }
        }
    }

    fn on_file_contents_request(&mut self, request: FileContentsRequest) {
        let stream_id = request.stream_id;
        // 锁的快照找不到时退回当前清单，与 fork 的校验一致。
        let list = request
            .data_id
            .and_then(|id| self.locked.get(&id))
            .or(self.offered.as_ref())
            .cloned();
        let queued = list.is_some_and(|list| self.jobs.send(Job::File { list, request }).is_ok());
        if !queued {
            self.send(ClipboardMessage::SendFileContentsResponse(
                FileContentsResponse::new_error(stream_id),
            ));
        }
    }

    // 远端 → Finder 留给第 4 步。
    fn on_file_contents_response(&mut self, _response: FileContentsResponse<'_>) {}

    fn on_lock(&mut self, data_id: LockDataId) {
        trace(|| format!("lock {}", data_id.0));
        if let Some(list) = &self.offered {
            self.locked.insert(data_id.0, Arc::clone(list));
        }
    }

    fn on_unlock(&mut self, data_id: LockDataId) {
        trace(|| format!("unlock {}", data_id.0));
        self.locked.remove(&data_id.0);
    }
}

/// 事件循环收到轮询结果时调用：先让 backend 记下这次的文件清单，再广播。
pub(super) fn initiate_offer(
    cliprdr: &mut CliprdrClient,
    offer: LocalOffer,
) -> PduResult<CliprdrSvcMessages<Client>> {
    let files = match &offer {
        LocalOffer::Files(offer) => Some(Arc::clone(&offer.files)),
        LocalOffer::Formats(_) => None,
    };
    if let Some(backend) = cliprdr.downcast_backend_mut::<RdpCliprdrBackend>() {
        backend.offered = files;
    }
    match offer {
        LocalOffer::Formats(formats) => cliprdr.initiate_copy(&formats),
        LocalOffer::Files(offer) => cliprdr.initiate_file_copy(offer.descriptors),
    }
}

/// 轮询一次：通道就绪且本地剪贴板有新变化时，返回要广播的内容（格式清单可以为空，表示清空远端）。
pub(super) fn poll_local_change(shared: &ClipboardShared) -> Option<LocalOffer> {
    if !shared.ready.load(Ordering::Relaxed) {
        return None;
    }
    let mut last = lock(&shared.last_token);
    let token = local::change_token()?;
    if *last == token {
        return None;
    }
    *last = token;
    let available = local::available();
    if !(available.files && shared.files_enabled.load(Ordering::Relaxed)) {
        lock(&shared.offered_roots).clear();
        let formats = advertised_formats(available);
        trace(|| format!("local change {token} → advertise [{}]", describe(&formats)));
        return Some(LocalOffer::Formats(formats));
    }
    // 展开大目录可能要一阵，不能一直持锁挡住远端内容的写入。
    drop(last);
    let roots = local::file_paths();
    // Finder 复制一次会分几批写剪贴板，同一批文件远端已经有了就不重发。
    if *lock(&shared.offered_roots) == roots {
        trace(|| format!("local change {token} → same files, skip"));
        return None;
    }
    let started = Instant::now();
    let offer = files::collect(&roots);
    trace(|| {
        format!(
            "local change {token} → {:?} files in {:?}",
            offer.as_ref().map(|o| o.descriptors.len()),
            started.elapsed()
        )
    });
    // 展开期间远端又写了剪贴板，这份清单已经过时。
    if *lock(&shared.last_token) != token {
        return None;
    }
    let mut offered = lock(&shared.offered_roots);
    Some(match offer {
        Some(offer) => {
            *offered = roots;
            LocalOffer::Files(offer)
        }
        None => {
            offered.clear();
            LocalOffer::Formats(advertised_formats(available))
        }
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// 设了 NEXSHELL_RDP_CLIPBOARD_TRACE 就把格式清单、请求与应答打到 stderr。
fn trace(message: impl FnOnce() -> String) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("NEXSHELL_RDP_CLIPBOARD_TRACE").is_some()) {
        eprintln!("[rdp-clip] {}", message());
    }
}

fn describe(formats: &[ClipboardFormat]) -> String {
    formats
        .iter()
        .map(|f| match f.name() {
            Some(name) => format!("{}:{}", f.id().value(), name.value()),
            None => f.id().value().to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn advertised_formats(available: Available) -> Vec<ClipboardFormat> {
    let mut formats = Vec::new();
    if available.text {
        formats.push(ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT));
    }
    if available.rtf {
        formats.push(ClipboardFormat::new(RTF_FORMAT_ID).with_name(RTF_FORMAT_NAME));
    }
    if available.image {
        formats.push(ClipboardFormat::new(PNG_FORMAT_ID).with_name(PNG_FORMAT_NAME));
        formats.push(ClipboardFormat::new(ClipboardFormatId::CF_DIB));
    }
    formats
}

fn answer_format(format: ClipboardFormatId) -> ClipboardMessage {
    let started = Instant::now();
    let data = read_format(format);
    trace(|| {
        format!(
            "request {} → {:?} bytes in {:?}",
            format.value(),
            data.as_ref().map(Vec::len),
            started.elapsed()
        )
    });
    ClipboardMessage::SendFormatData(match data {
        Some(data) => OwnedFormatDataResponse::new_data(data),
        None => OwnedFormatDataResponse::new_error(),
    })
}

fn answer_file(
    list: &files::FileList,
    request: &FileContentsRequest,
    reader: &mut files::RangeReader,
) -> ClipboardMessage {
    let id = request.stream_id;
    let file = usize::try_from(request.index)
        .ok()
        .and_then(|i| list.get(i));
    let response = file.and_then(|file| {
        if request.flags.contains(FileContentsFlags::SIZE) {
            let size = files::size(file);
            trace(|| format!("file {} size → {size:?}", request.index));
            size.map(|size| FileContentsResponse::new_size_response(id, size))
        } else {
            reader
                .read(file, request.position, request.requested_size)
                .map(|data| FileContentsResponse::new_data_response(id, data))
        }
    });
    ClipboardMessage::SendFileContentsResponse(response.unwrap_or_else(|| {
        trace(|| format!("file {} request failed", request.index));
        FileContentsResponse::new_error(id)
    }))
}

/// 按远端请求的格式读 Mac 剪贴板并转换；读不到或转不了返回 None（应答失败）。
fn read_format(format: ClipboardFormatId) -> Option<Vec<u8>> {
    match format {
        ClipboardFormatId::CF_UNICODETEXT => {
            local::read_text().map(|t| text::mac_text_to_cf_unicode(&t))
        }
        // Windows 剪贴板里的 RTF 以 NUL 结尾。
        RTF_FORMAT_ID => local::read_rtf().map(|mut rtf| {
            rtf.push(0);
            rtf
        }),
        PNG_FORMAT_ID => local::read_png(),
        ClipboardFormatId::CF_DIB => local::read_bitmap().and_then(|b| dib::bitmap_to_dib(&b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(available: Available) -> Vec<u32> {
        advertised_formats(available)
            .iter()
            .map(|f| f.id().value())
            .collect()
    }

    #[test]
    fn advertised_formats_follow_available_content() {
        assert!(ids(Available::default()).is_empty());
        let all = Available {
            text: true,
            rtf: true,
            image: true,
            files: false,
        };
        assert_eq!(ids(all), vec![13, 0xC0F0, 0xC0F1, 8]);
        let image_only = Available {
            image: true,
            ..Available::default()
        };
        assert_eq!(ids(image_only), vec![0xC0F1, 8]);
    }

    #[test]
    fn registered_formats_carry_windows_names() {
        let formats = advertised_formats(Available {
            text: true,
            rtf: true,
            image: true,
            files: false,
        });
        let names: Vec<_> = formats
            .iter()
            .map(|f| f.name().map(|n| n.value().to_owned()))
            .collect();
        assert_eq!(
            names,
            vec![
                None,
                Some("Rich Text Format".to_owned()),
                Some("PNG".to_owned()),
                None
            ]
        );
    }
}
