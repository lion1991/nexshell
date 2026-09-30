//! 剪贴板辅助进程（ADR 0016 决策 5）：替远端内容做 NSPasteboard 延迟提供。
//! provider 回调在主线程上阻塞等主进程从远端取回数据，放在独立进程里才不卡 NexShell 界面。
//! stdin 收主进程的消息，stdout 发读取请求；stdin 关闭（主进程退出）就退出。

use std::cell::RefCell;
use std::io::{self, BufReader, BufWriter, Stdout};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{process, ptr, thread};

use dispatch2::DispatchQueue;
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey, NSPasteboard,
    NSPasteboardItem, NSPasteboardItemDataProvider, NSPasteboardType, NSPasteboardWriting,
};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapContextCreate, CGBitmapContextCreateImage, CGBitmapContextGetData,
    CGColorSpace, CGImageAlphaInfo, CGImageByteOrderInfo,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSRunLoop, NSString, NSURL};

use super::wire::{self, Data, Format, Kind, Offer, Request, ToHelper};
use super::{dib, local, text, trace, Bitmap};

/// 文本、图片等主进程应答的上限。文件要等下载，不设上限，用户可在 NexShell 里取消。
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

struct ProviderIvars {
    session: u64,
    generation: u64,
    index: u32,
}

define_class!(
    // SAFETY: NSObject 没有子类化要求；Provider 不实现 Drop。
    #[unsafe(super(NSObject))]
    #[name = "NexShellPasteboardProvider"]
    #[ivars = ProviderIvars]
    struct Provider;

    unsafe impl NSObjectProtocol for Provider {}

    unsafe impl NSPasteboardItemDataProvider for Provider {
        #[unsafe(method(pasteboard:item:provideDataForType:))]
        fn provide_data_for_type(
            &self,
            _pasteboard: Option<&NSPasteboard>,
            item: &NSPasteboardItem,
            r#type: &NSPasteboardType,
        ) {
            provide(self.ivars(), item, r#type);
        }
    }
);

impl Provider {
    fn new(offer: &Offer, index: u32) -> Retained<Self> {
        let this = Self::alloc().set_ivars(ProviderIvars {
            session: offer.session,
            generation: offer.generation,
            index,
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// 最近一次写入的剪贴板内容。
struct Current {
    session: u64,
    change_count: isize,
    /// 还没给出数据的类型数。
    pending: usize,
    files: bool,
}

/// 主线程独占的状态。
struct Helper {
    out: BufWriter<Stdout>,
    replies: mpsc::Receiver<(u64, Option<Data>)>,
    next_id: u64,
    /// 条目被换掉前 provider 要一直活着。
    providers: Vec<Retained<Provider>>,
    current: Option<Current>,
}

thread_local! {
    static HELPER: RefCell<Option<Helper>> = const { RefCell::new(None) };
}

/// 辅助进程入口，不返回。
pub fn run() -> ! {
    let (tx, replies) = mpsc::channel();
    HELPER.set(Some(Helper {
        out: BufWriter::new(io::stdout()),
        replies,
        next_id: 0,
        providers: Vec::new(),
        current: None,
    }));
    thread::spawn(move || {
        let mut input = BufReader::new(io::stdin());
        loop {
            match wire::read_to_helper(&mut input) {
                // 应答直接交给正在主线程上等待的 provider。
                Ok(ToHelper::Reply { id, data }) => {
                    let _ = tx.send((id, data));
                }
                // 写剪贴板在主线程做；provider 正在等数据时排在它后面。
                Ok(ToHelper::Offer(offer)) => {
                    DispatchQueue::main().exec_async(move || write_offer(offer));
                }
                Ok(ToHelper::Clear { session }) => {
                    DispatchQueue::main().exec_async(move || clear(session));
                }
                Err(_) => process::exit(0),
            }
        }
    });
    loop {
        NSRunLoop::mainRunLoop().run();
        // 没有输入源时 run 会立即返回，别空转。
        thread::sleep(Duration::from_millis(100));
    }
}

fn write_offer(offer: Offer) {
    autoreleasepool(|_| {
        let pb = NSPasteboard::generalPasteboard();
        let mut providers = Vec::new();
        let mut promise = |index: u32, types: &[&NSPasteboardType]| {
            let item = NSPasteboardItem::new();
            let provider = Provider::new(&offer, index);
            item.setDataProvider_forTypes(
                ProtocolObject::from_ref(&*provider),
                &NSArray::from_slice(types),
            );
            providers.push(provider);
            item
        };
        let (items, pending): (Vec<_>, usize) = if offer.files > 0 {
            let items = (0..offer.files)
                .map(|i| promise(i, &[local::file_url_type()]))
                .collect();
            (items, offer.files as usize)
        } else {
            let mut types = Vec::new();
            if offer.text {
                types.push(local::string_type());
            }
            if offer.rtf {
                types.push(local::rtf_type());
            }
            if offer.image {
                types.extend([local::png_type(), local::tiff_type()]);
            }
            (vec![promise(0, &types)], types.len())
        };
        let marker = local::owner_marker(std::os::unix::process::parent_id(), offer.session);
        items[0].setString_forType(&NSString::from_str(&marker), &local::owner_type());
        let objects: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = items
            .into_iter()
            .map(ProtocolObject::from_retained)
            .collect();
        pb.clearContents();
        pb.writeObjects(&NSArray::from_retained_slice(&objects));
        trace(|| format!("helper offered {offer:?}"));
        with_helper(|h| {
            h.providers = providers;
            h.current = Some(Current {
                session: offer.session,
                change_count: pb.changeCount(),
                pending,
                files: offer.files > 0,
            });
        });
    });
}

/// 会话结束。剪贴板还是它写的、且还有没给出的数据（或是文件，临时目录会被删）就清空；
/// 数据都已给出（剪贴板管理器通常一写入就读完）则留着。
fn clear(session: u64) {
    autoreleasepool(|_| {
        let pb = NSPasteboard::generalPasteboard();
        with_helper(|h| {
            let Some(current) = h.current.take_if(|c| c.session == session) else {
                return;
            };
            h.providers.clear();
            if pb.changeCount() == current.change_count && (current.files || current.pending > 0) {
                pb.clearContents();
            }
        });
    });
}

fn provide(owner: &ProviderIvars, item: &NSPasteboardItem, ty: &NSPasteboardType) {
    let Some(kind) = kind_of(ty) else {
        return;
    };
    let request = Request {
        id: 0,
        session: owner.session,
        generation: owner.generation,
        kind,
        index: owner.index,
    };
    let started = Instant::now();
    let Some(data) = with_helper(|h| h.fetch(request)).flatten() else {
        trace(|| format!("helper got nothing for {kind:?}"));
        return;
    };
    let set = autoreleasepool(|_| set_data(item, ty, kind, &data));
    trace(|| format!("helper provided {kind:?}={set} in {:?}", started.elapsed()));
    if set {
        with_helper(|h| {
            if let Some(current) = &mut h.current {
                current.pending = current.pending.saturating_sub(1);
            }
        });
    }
}

impl Helper {
    fn fetch(&mut self, mut request: Request) -> Option<Data> {
        self.next_id += 1;
        request.id = self.next_id;
        wire::write_request(&mut self.out, &request).ok()?;
        let deadline = (request.kind != Kind::FileUrl).then(|| Instant::now() + REPLY_TIMEOUT);
        loop {
            let (id, data) = match deadline {
                Some(deadline) => self
                    .replies
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .ok()?,
                None => self.replies.recv().ok()?,
            };
            // 超时后才到的旧应答丢掉。
            if id == request.id {
                return data;
            }
        }
    }
}

fn with_helper<T>(f: impl FnOnce(&mut Helper) -> T) -> Option<T> {
    HELPER.with_borrow_mut(|h| h.as_mut().map(f))
}

fn kind_of(ty: &NSPasteboardType) -> Option<Kind> {
    [
        (local::string_type(), Kind::Text),
        (local::rtf_type(), Kind::Rtf),
        (local::png_type(), Kind::Png),
        (local::tiff_type(), Kind::Tiff),
        (local::file_url_type(), Kind::FileUrl),
    ]
    .into_iter()
    .find(|(t, _)| *t == ty)
    .map(|(_, kind)| kind)
}

/// 远端格式转成 Mac 类型写进条目。
fn set_data(item: &NSPasteboardItem, ty: &NSPasteboardType, kind: Kind, data: &Data) -> bool {
    let bytes = &data.bytes[..];
    let converted = match (kind, data.format) {
        (Kind::Text, Format::UnicodeText) => {
            let text = text::cf_unicode_to_mac_text(bytes);
            return item.setString_forType(&NSString::from_str(&text), ty);
        }
        (Kind::FileUrl, Format::Path) => {
            let Ok(path) = std::str::from_utf8(bytes) else {
                return false;
            };
            let url = NSURL::fileURLWithPath(&NSString::from_str(path));
            return url
                .absoluteString()
                .is_some_and(|s| item.setString_forType(&s, ty));
        }
        // Windows 剪贴板里的 RTF 以 NUL 结尾。
        (Kind::Rtf, Format::Rtf) => {
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            Some(NSData::with_bytes(&bytes[..end]))
        }
        (Kind::Png, Format::Png) => Some(NSData::with_bytes(bytes)),
        (Kind::Png, Format::Dib) => dib_rep(bytes).and_then(|rep| png(&rep)),
        (Kind::Tiff, Format::Png) => NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(bytes))
            .and_then(|rep| rep.TIFFRepresentation()),
        (Kind::Tiff, Format::Dib) => dib_rep(bytes).and_then(|rep| rep.TIFFRepresentation()),
        _ => None,
    };
    converted.is_some_and(|d| item.setData_forType(&d, ty))
}

fn png(rep: &NSBitmapImageRep) -> Option<Retained<NSData>> {
    let props = NSDictionary::<NSBitmapImageRepPropertyKey, AnyObject>::new();
    unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props) }
}

fn dib_rep(dib: &[u8]) -> Option<Retained<NSBitmapImageRep>> {
    bitmap_rep(&dib::dib_to_bitmap(dib)?)
}

/// 像素经 CGBitmapContext 转成 NSBitmapImageRep；CG 只收预乘 alpha。
fn bitmap_rep(bitmap: &Bitmap) -> Option<Retained<NSBitmapImageRep>> {
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
    // SAFETY: data 传空由 CG 分配并持有，行距 width*4。
    let context = unsafe {
        CGBitmapContextCreate(
            ptr::null_mut(),
            bitmap.width,
            bitmap.height,
            8,
            bitmap.width * 4,
            Some(&space),
            info,
        )
    }?;
    let dst = CGBitmapContextGetData(Some(&context)).cast::<u8>();
    if dst.is_null() {
        return None;
    }
    // SAFETY: 上下文按上面的尺寸分配了 width*height*4 字节，与 bgra 等长。
    let dst = unsafe { std::slice::from_raw_parts_mut(dst, bitmap.bgra.len()) };
    for (d, s) in dst.chunks_exact_mut(4).zip(bitmap.bgra.chunks_exact(4)) {
        let a = u16::from(s[3]);
        for c in 0..3 {
            d[c] = ((u16::from(s[c]) * a + 127) / 255) as u8;
        }
        d[3] = s[3];
    }
    let image = CGBitmapContextCreateImage(Some(&context))?;
    Some(NSBitmapImageRep::initWithCGImage(
        NSBitmapImageRep::alloc(),
        &image,
    ))
}
