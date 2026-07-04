//! VideoToolbox H.264 硬解，实现 ironrdp_egfx 的 `H264Decoder`（docs/adr/0008 第②步）。
//! 输入 AVCC（4 字节大端长度前缀 NAL），输出 RGBA `DecodedFrame`；同步解码、SPS/PPS
//! 变化时重建会话。本文件集中全部 unsafe（VideoToolbox FFI），每处配安全性注释。

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr;
use std::ptr::NonNull;
use std::slice;

use ironrdp_egfx::decode::{DecodedFrame, DecoderError, DecoderResult, H264Decoder};
use objc2_core_foundation::{kCFAllocatorNull, kCFBooleanTrue, CFRetained, CFType};
use objc2_core_media::{
    CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMTime,
    CMVideoFormatDescriptionCreateFromH264ParameterSets,
};
use objc2_core_video::{
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVReturnSuccess, CVImageBuffer,
    CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType,
    CVPixelBufferGetPlaneCount, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_video_toolbox::{
    kVTDecompressionPropertyKey_RealTime, VTDecodeFrameFlags, VTDecodeInfoFlags,
    VTDecompressionOutputCallbackRecord, VTDecompressionSession, VTSessionSetProperty,
};

/// 已就绪的解码会话：格式描述 + 解压会话，随 SPS/PPS 一起持有以便比对。
struct VtSession {
    format: CFRetained<CMFormatDescription>,
    session: CFRetained<VTDecompressionSession>,
}

/// VideoToolbox H.264 硬解器。持久保存最近 SPS/PPS，仅在参数集变化时重建会话。
pub struct VtH264Decoder {
    sps: Vec<u8>,
    pps: Vec<u8>,
    session: Option<VtSession>,
    rebuilds: u64,
    first_frame_logged: bool,
    format_logged: bool,
    /// AVC dump（NEXSHELL_RDP_EGFX_DUMP=<dir>）：前 DUMP_MAX 个 AU 落原始 Annex B。
    dump_dir: Option<PathBuf>,
    dump_seq: u32,
}

/// 最多 dump 多少个 AU（供离线 vt_replay 迭代）。
const DUMP_MAX: u32 = 30;

// SAFETY: VTDecompressionSession/CMFormatDescription 均为线程安全的 CF 对象，
// 且本解码器只在单一 RDP 会话线程内被调用（DVC 同步处理），满足 Send 契约。
unsafe impl Send for VtH264Decoder {}

impl VtH264Decoder {
    pub fn new() -> Self {
        let dump_dir = std::env::var_os("NEXSHELL_RDP_EGFX_DUMP")
            .map(PathBuf::from)
            .filter(|d| !d.as_os_str().is_empty());
        if let Some(dir) = &dump_dir {
            let _ = std::fs::create_dir_all(dir);
            eprintln!("[egfx] AVC dump enabled → {}", dir.display());
        }
        Self {
            sps: Vec::new(),
            pps: Vec::new(),
            session: None,
            rebuilds: 0,
            first_frame_logged: false,
            format_logged: false,
            dump_dir,
            dump_seq: 0,
        }
    }

    /// 把原始 Annex B 输入落盘（前 DUMP_MAX 个 AU），命名 `<seq>_avc420_<bytes>B.bin`。
    fn maybe_dump(&mut self, raw: &[u8]) {
        let Some(dir) = &self.dump_dir else { return };
        if self.dump_seq >= DUMP_MAX {
            return;
        }
        let path = dir.join(format!("{:04}_avc420_{}B.bin", self.dump_seq, raw.len()));
        if let Err(e) = std::fs::write(&path, raw) {
            eprintln!("[egfx] AVC dump write failed {}: {e}", path.display());
        }
        self.dump_seq += 1;
    }

    /// 扫描本帧 NAL，若含新的 SPS/PPS 则（必要时）重建会话。
    fn ensure_session(&mut self, data: &[u8]) -> DecoderResult<()> {
        let (sps, pps) = extract_sps_pps(data);
        let mut changed = false;
        if let Some(sps) = sps {
            if sps != self.sps {
                self.sps = sps;
                changed = true;
            }
        }
        if let Some(pps) = pps {
            if pps != self.pps {
                self.pps = pps;
                changed = true;
            }
        }
        if self.session.is_some() && !changed {
            return Ok(());
        }
        self.build_current_session()
    }

    /// 用当前持有的 SPS/PPS 无条件重建会话（缺参数集则跳过，留待关键帧）。
    fn build_current_session(&mut self) -> DecoderResult<()> {
        if self.sps.is_empty() || self.pps.is_empty() {
            return Ok(());
        }
        // SAFETY: sps/pps 为本结构体持有的存活切片，长度非零；建 format desc + session
        // 均按 Create 规则取回 +1 引用，失败返回状态码。
        let session = unsafe { build_session(&self.sps, &self.pps) }.map_err(|status| {
            DecoderError::msg(format!("VT session create failed: OSStatus {status}"))
        })?;
        self.rebuilds += 1;
        eprintln!(
            "[egfx] VT decoder session rebuilt (#{}) sps={}B pps={}B",
            self.rebuilds,
            self.sps.len(),
            self.pps.len()
        );
        self.session = Some(session);
        Ok(())
    }
}

impl H264Decoder for VtH264Decoder {
    fn decode(&mut self, data: &[u8]) -> DecoderResult<DecodedFrame> {
        // MS-RDPEGFX 2.2.4.4：AVC420 载荷是 Annex B 字节流（start code 分隔），而
        // VideoToolbox 只吃 AVCC（4 字节大端长度前缀），故先归一化到 AVCC；已是 AVCC 的原样透传。
        // 上游 H264Decoder 契约误标为 AVCC，真机实为 Annex B，此处兼容两者。
        self.maybe_dump(data);
        let avcc = normalize_to_avcc(data);
        // 参数集只活在 format description 里（会话按 SPS/PPS 建）；样本缓冲只喂 VCL(±SEI)，
        // 剥离 AUD/SPS/PPS——否则 VT 检测到带内参数集与 format desc 并存 → -12916。
        let vcl = filter_nals_for_decode(&avcc);
        if !self.format_logged {
            self.format_logged = true;
            let kind = if is_annex_b(data) { "AnnexB" } else { "AVCC" };
            let head: Vec<String> = data.iter().take(8).map(|b| format!("{b:02x}")).collect();
            eprintln!(
                "[egfx] AVC420 wire format={kind} first_bytes=[{}] in={}B avcc={}B vcl={}B",
                head.join(" "),
                data.len(),
                avcc.len(),
                vcl.len()
            );
        }

        self.ensure_session(&avcc)?;
        if self.session.is_none() {
            return Err(DecoderError::msg(
                "no SPS/PPS available yet, cannot decode frame",
            ));
        }
        if vcl.is_empty() {
            return Err(DecoderError::msg("AVC420 AU has no VCL NAL to decode"));
        }

        // 首次尝试。SAFETY: vcl 在本调用期间存活；block buffer 用 kCFAllocatorNull 零拷贝
        // 引用它、不负责释放；sample buffer + 同步 decode_frame 全程持有引用，回调同步写出。
        let mut result = {
            let session = self.session.as_ref().expect("session present");
            unsafe { decode_sync(session, &vcl) }
        };
        // -12916(格式变更不支持)/-12911(会话无效)：重建会话重试一次。
        if let Err(status) = result {
            if status == -12916 || status == -12911 {
                eprintln!("[egfx] VT decode OSStatus {status}, rebuilding session and retrying");
                self.session = None;
                self.build_current_session()?;
                if let Some(session) = self.session.as_ref() {
                    // SAFETY: 同上，vcl/会话均存活。
                    result = unsafe { decode_sync(session, &vcl) };
                }
            }
        }

        match result {
            Ok(f) => {
                if !self.first_frame_logged {
                    self.first_frame_logged = true;
                    eprintln!(
                        "[egfx] first AVC420 frame decoded {}x{}",
                        f.width(),
                        f.height()
                    );
                }
                Ok(f)
            }
            Err(status) => {
                eprintln!(
                    "[egfx] VT decode failed: OSStatus {status}, in={}B vcl={}B",
                    data.len(),
                    vcl.len()
                );
                Err(DecoderError::msg(format!(
                    "VT decode failed: OSStatus {status}"
                )))
            }
        }
    }

    fn reset(&mut self) {
        // 丢弃会话与参数集；ResetGraphics 后服务端会重发关键帧（携带 SPS/PPS）。
        self.session = None;
        self.sps.clear();
        self.pps.clear();
    }
}

/// 回调收集槽：由 `decode_sync` 栈上分配，经 source_frame_ref_con 传入回调。
struct DecodeOutput {
    status: i32,
    image: *mut CVPixelBuffer,
}

/// VT 同步解码回调。同步模式（flags=0）下在 `decode_frame` 返回前被调一次。
unsafe extern "C-unwind" fn output_callback(
    _decomp_ref_con: *mut c_void,
    source_frame_ref_con: *mut c_void,
    status: i32,
    _info: VTDecodeInfoFlags,
    image_buffer: *mut CVImageBuffer,
    _pts: CMTime,
    _dur: CMTime,
) {
    if source_frame_ref_con.is_null() {
        return;
    }
    // SAFETY: 同步解码，source_frame_ref_con 指向 decode_sync 栈上仍存活的 DecodeOutput。
    let out = unsafe { &mut *(source_frame_ref_con as *mut DecodeOutput) };
    out.status = status;
    if status == 0 {
        if let Some(buf) = NonNull::new(image_buffer) {
            // SAFETY: 成功时 VT 传入有效 image buffer；retain +1 保活到 decode_sync 读取后释放。
            let retained = unsafe { CFRetained::retain(buf) };
            out.image = CFRetained::into_raw(retained).as_ptr();
        }
    }
}

/// 从 SPS/PPS 建格式描述 + 解压会话。返回 +1 引用；失败返回 OSStatus。
unsafe fn build_session(sps: &[u8], pps: &[u8]) -> Result<VtSession, i32> {
    // 参数集指针/长度数组（顺序：SPS, PPS），NAL 头长度 4（AVCC）。
    let ptrs: [NonNull<u8>; 2] = [
        NonNull::new(sps.as_ptr() as *mut u8).ok_or(-1)?,
        NonNull::new(pps.as_ptr() as *mut u8).ok_or(-1)?,
    ];
    let sizes: [usize; 2] = [sps.len(), pps.len()];

    let mut format_out: *const CMFormatDescription = ptr::null();
    // SAFETY: ptrs/sizes 为本函数栈上存活数组，长度 2；format_out 为有效可写指针。
    let status = unsafe {
        CMVideoFormatDescriptionCreateFromH264ParameterSets(
            None,
            2,
            NonNull::new(ptrs.as_ptr() as *mut NonNull<u8>).ok_or(-1)?,
            NonNull::new(sizes.as_ptr() as *mut usize).ok_or(-1)?,
            4,
            NonNull::new(&mut format_out).ok_or(-1)?,
        )
    };
    if status != 0 {
        return Err(status);
    }
    let format = NonNull::new(format_out as *mut CMFormatDescription).ok_or(-1)?;
    // SAFETY: Create 规则返回 +1，from_raw 接管所有权。
    let format = unsafe { CFRetained::from_raw(format) };

    let callback = VTDecompressionOutputCallbackRecord {
        decompressionOutputCallback: Some(output_callback),
        decompressionOutputRefCon: ptr::null_mut(),
    };
    let mut session_out: *mut VTDecompressionSession = ptr::null_mut();
    // SAFETY: format 存活；callback 指向本栈上记录；session_out 有效可写。
    // 目标像素属性传 None → 默认输出 NV12（bi-planar 4:2:0）。
    let status = unsafe {
        VTDecompressionSession::create(
            None,
            &format,
            None,
            None,
            &callback,
            NonNull::new(&mut session_out).ok_or(-1)?,
        )
    };
    if status != 0 {
        return Err(status);
    }
    let session = NonNull::new(session_out).ok_or(-1)?;
    // SAFETY: Create 规则返回 +1，from_raw 接管。
    let session = unsafe { CFRetained::from_raw(session) };

    // RealTime 提示（低延迟优先）；失败仅影响延迟，忽略。
    // SAFETY: session Deref 到 &CFType；key/value 为静态 CF 对象。
    unsafe {
        if let Some(bool_true) = kCFBooleanTrue {
            let value: &CFType = bool_true;
            let _ =
                VTSessionSetProperty(&session, kVTDecompressionPropertyKey_RealTime, Some(value));
        }
    }

    Ok(VtSession { format, session })
}

/// 同步解码单个 AU（AVCC 长度前缀流，仅 VCL±SEI）→ RGBA `DecodedFrame`。失败返回 OSStatus。
unsafe fn decode_sync(vt: &VtSession, data: &[u8]) -> Result<DecodedFrame, i32> {
    let session = &vt.session;
    // 1) block buffer 零拷贝引用 data（kCFAllocatorNull → 不释放我方内存）。
    let mut block_out: *mut CMBlockBuffer = ptr::null_mut();
    // SAFETY: data 存活于整个 decode_sync；block_out 可写。
    let status = unsafe {
        CMBlockBuffer::create_with_memory_block(
            None,
            data.as_ptr() as *mut c_void,
            data.len(),
            kCFAllocatorNull,
            ptr::null(),
            0,
            data.len(),
            0,
            NonNull::new(&mut block_out).ok_or(-1)?,
        )
    };
    if status != 0 {
        return Err(status);
    }
    let block = NonNull::new(block_out).ok_or(-1)?;
    // SAFETY: Create 规则 +1。
    let block = unsafe { CFRetained::from_raw(block) };

    // 2) 从 block buffer + 格式描述建 sample buffer。必须挂上会话的 format description
    //    （由 SPS/PPS 建），否则 VT 会回退去解析带内参数集，与会话格式冲突 → -12916。
    let sizes = [data.len()];
    let mut sample_out: *mut CMSampleBuffer = ptr::null_mut();
    // SAFETY: block/format 存活；sizes 为栈上数组；sample_out 可写。timing 传 0/null。
    let status = unsafe {
        CMSampleBuffer::create_ready(
            None,
            Some(&block),
            Some(&vt.format),
            1,
            0,
            ptr::null(),
            1,
            sizes.as_ptr(),
            NonNull::new(&mut sample_out).ok_or(-1)?,
        )
    };
    if status != 0 {
        return Err(status);
    }
    let sample = NonNull::new(sample_out).ok_or(-1)?;
    // SAFETY: Create 规则 +1。
    let sample = unsafe { CFRetained::from_raw(sample) };

    // 3) 同步解码：flags=0 → 回调在返回前触发。
    let mut output = DecodeOutput {
        status: 0,
        image: ptr::null_mut(),
    };
    // SAFETY: sample 存活；ref_con 指向本栈 output；info_out 传 null。
    let status = unsafe {
        session.decode_frame(
            &sample,
            VTDecodeFrameFlags(0),
            &mut output as *mut DecodeOutput as *mut c_void,
            ptr::null_mut(),
        )
    };
    if status != 0 {
        // 回调未触发或提前失败，无需释放 image。
        if let Some(img) = NonNull::new(output.image) {
            // SAFETY: 若回调已 retain，取回并释放。
            drop(unsafe { CFRetained::<CVPixelBuffer>::from_raw(img) });
        }
        return Err(status);
    }
    if output.status != 0 {
        return Err(output.status);
    }
    let image = NonNull::new(output.image).ok_or(-1)?;
    // SAFETY: 回调 retain 的 +1，from_raw 接管，函数结束释放。
    let pixel_buffer = unsafe { CFRetained::<CVPixelBuffer>::from_raw(image) };

    // SAFETY: pixel_buffer 存活；读平面前锁定基址、读后解锁。
    unsafe { read_pixel_buffer(&pixel_buffer) }
}

/// 锁定像素缓冲、按平面 stride 逐行读 NV12、转 RGBA。
unsafe fn read_pixel_buffer(pb: &CVPixelBuffer) -> Result<DecodedFrame, i32> {
    let fmt = CVPixelBufferGetPixelFormatType(pb);
    let full_range = if fmt == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange {
        true
    } else if fmt == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange {
        false
    } else {
        eprintln!("[egfx] unexpected VT pixel format fourcc=0x{fmt:08x}");
        return Err(-2);
    };
    if CVPixelBufferGetPlaneCount(pb) < 2 {
        return Err(-3);
    }

    let width = CVPixelBufferGetWidth(pb);
    let height = CVPixelBufferGetHeight(pb);
    if width == 0 || height == 0 {
        return Err(-4);
    }

    // 只读锁（flags=0 即可读写，用 0 简单稳妥）。
    // SAFETY: pb 有效；锁定后必配对解锁。
    let lock = unsafe { CVPixelBufferLockBaseAddress(pb, CVPixelBufferLockFlags(0)) };
    if lock != kCVReturnSuccess {
        return Err(lock);
    }

    // SAFETY: 已锁定，基址/stride 在解锁前有效；逐行按 stride 取，行内取 width。
    let result = unsafe {
        let y_base = CVPixelBufferGetBaseAddressOfPlane(pb, 0);
        let uv_base = CVPixelBufferGetBaseAddressOfPlane(pb, 1);
        if y_base.is_null() || uv_base.is_null() {
            CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags(0));
            return Err(-5);
        }
        let y_stride = CVPixelBufferGetBytesPerRowOfPlane(pb, 0);
        let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(pb, 1);
        let y_h = CVPixelBufferGetHeightOfPlane(pb, 0);
        let uv_h = CVPixelBufferGetHeightOfPlane(pb, 1);
        let y_plane = slice::from_raw_parts(y_base as *const u8, y_stride.saturating_mul(y_h));
        let uv_plane = slice::from_raw_parts(uv_base as *const u8, uv_stride.saturating_mul(uv_h));
        nv12_to_rgba(
            y_plane, y_stride, uv_plane, uv_stride, width, height, full_range,
        )
    };

    // SAFETY: 与上面的 lock 配对解锁。
    unsafe {
        CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags(0));
    }

    Ok(DecodedFrame::new(result, width as u32, height as u32))
}

/// data 是否以 Annex B start code（00 00 01 或 00 00 00 01）开头。
fn is_annex_b(data: &[u8]) -> bool {
    data.starts_with(&[0, 0, 0, 1]) || data.starts_with(&[0, 0, 1])
}

/// Annex B（start code 分隔）→ AVCC（4 字节大端长度前缀）。非 Annex B 视作已是 AVCC，原样克隆。
/// 精确记录每个 start code 起点，NAL 结束严格取下一个 start code 起点，避免 3/4 字节歧义误裁。
fn normalize_to_avcc(data: &[u8]) -> Vec<u8> {
    if !is_annex_b(data) {
        return data.to_vec();
    }
    // marks: (start_code_begin, nal_first_byte)
    let mut marks: Vec<(usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            marks.push((i, i + 3));
            i += 3;
            continue;
        }
        if i + 4 <= data.len()
            && data[i] == 0
            && data[i + 1] == 0
            && data[i + 2] == 0
            && data[i + 3] == 1
        {
            marks.push((i, i + 4));
            i += 4;
            continue;
        }
        i += 1;
    }
    let mut out = Vec::with_capacity(data.len());
    for (idx, &(_, nal_start)) in marks.iter().enumerate() {
        let end = if idx + 1 < marks.len() {
            marks[idx + 1].0
        } else {
            data.len()
        };
        if nal_start >= end {
            continue;
        }
        let nal = &data[nal_start..end];
        out.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        out.extend_from_slice(nal);
    }
    out
}

/// 从 AVCC（4 字节大端长度前缀）NAL 流里提取最后出现的 SPS(type7)/PPS(type8)。
fn extract_sps_pps(data: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let mut sps = None;
    let mut pps = None;
    let mut off = 0usize;
    while off + 4 <= data.len() {
        let len =
            u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]) as usize;
        off += 4;
        if len == 0 || off + len > data.len() {
            break;
        }
        let nal = &data[off..off + len];
        match nal[0] & 0x1F {
            7 => sps = Some(nal.to_vec()),
            8 => pps = Some(nal.to_vec()),
            _ => {}
        }
        off += len;
    }
    (sps, pps)
}

/// 从 AVCC NAL 流里滤出可解码的 NAL，只保留 VCL（type 1..=5）与 SEI（type 6），
/// 剥离 AUD(9)/SPS(7)/PPS(8)/filler 等——参数集只经 format description 生效。
/// 返回同为 AVCC（4 字节长度前缀）的紧凑缓冲；无 VCL 时可能为空。
fn filter_nals_for_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut off = 0usize;
    while off + 4 <= data.len() {
        let len =
            u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]) as usize;
        off += 4;
        if len == 0 || off + len > data.len() {
            break;
        }
        let nal = &data[off..off + len];
        let keep = matches!(nal[0] & 0x1F, 1..=6);
        if keep {
            out.extend_from_slice(&(len as u32).to_be_bytes());
            out.extend_from_slice(nal);
        }
        off += len;
    }
    out
}

/// 离线回放：按名字顺序读目录里 `*_avc420_*.bin`（原始 Annex B），喂同一
/// `VtH264Decoder`，逐个打印 OK(宽x高)/FAIL(错误含 OSStatus)。供 examples/vt_replay 使用。
/// `ppm`：可选，把**第一帧**解码 RGBA 落成 P6 PPM，供人工目检 NV12→RGBA 颜色/UV 次序。
#[doc(hidden)]
pub fn vt_replay_dir(dir: &std::path::Path, ppm: Option<&std::path::Path>) -> std::io::Result<()> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains("avc420") && n.ends_with(".bin"))
        })
        .collect();
    files.sort();
    if files.is_empty() {
        println!("[replay] no *_avc420_*.bin in {}", dir.display());
        return Ok(());
    }
    let mut dec = VtH264Decoder::new();
    let mut ppm_done = false;
    for path in &files {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let bytes = std::fs::read(path)?;
        match dec.decode(&bytes) {
            Ok(f) => {
                println!("[replay] {name}: OK {}x{}", f.width(), f.height());
                // 首个成功解码帧写 PPM（AVC 首帧可能为纯参数集无 VCL，故取首个成功者）。
                if let (Some(out), false) = (ppm, ppm_done) {
                    write_ppm(out, f.data(), f.width(), f.height())?;
                    println!(
                        "[replay] wrote PPM {} ({}x{})",
                        out.display(),
                        f.width(),
                        f.height()
                    );
                    ppm_done = true;
                }
            }
            Err(e) => println!("[replay] {name}: FAIL {e}"),
        }
    }
    if ppm.is_some() && !ppm_done {
        println!("[replay] no frame decoded; PPM not written");
    }
    Ok(())
}

/// 把 RGBA 帧写成 P6 PPM（RGB，丢弃 alpha）。无新依赖，供离线目检解码色彩。
fn write_ppm(path: &std::path::Path, rgba: &[u8], w: u32, h: u32) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(rgba.len() / 4 * 3 + 32);
    buf.extend_from_slice(format!("P6\n{w} {h}\n255\n").as_bytes());
    for px in rgba.chunks_exact(4) {
        buf.extend_from_slice(&px[..3]);
    }
    std::fs::write(path, &buf)
}

/// NV12（Y 平面 + 交错 CbCr 平面）→ RGBA8888。逐行按 stride 取（容忍 padding），
/// 按 BT.601 转色（full/video range）。纯函数，供单测覆盖 stride padding 情形。
fn nv12_to_rgba(
    y: &[u8],
    y_stride: usize,
    uv: &[u8],
    uv_stride: usize,
    width: usize,
    height: usize,
    full_range: bool,
) -> Vec<u8> {
    let mut out = vec![0u8; width * height * 4];
    for row in 0..height {
        let y_row = row * y_stride;
        let uv_row = (row / 2) * uv_stride;
        for col in 0..width {
            let yb = y.get(y_row + col).copied().unwrap_or(0);
            let uv_col = (col & !1) + uv_row; // 每 2 像素共享一组 CbCr
            let cb = uv.get(uv_col).copied().unwrap_or(128);
            let cr = uv.get(uv_col + 1).copied().unwrap_or(128);
            let (r, g, b) = ycbcr_to_rgb(yb, cb, cr, full_range);
            let o = (row * width + col) * 4;
            out[o] = r;
            out[o + 1] = g;
            out[o + 2] = b;
            out[o + 3] = 0xFF;
        }
    }
    out
}

/// BT.601 YCbCr → RGB，输出 clamp 到 0..=255。
#[inline]
fn ycbcr_to_rgb(y: u8, cb: u8, cr: u8, full_range: bool) -> (u8, u8, u8) {
    let (yf, cbf, crf) = (y as f32, cb as f32 - 128.0, cr as f32 - 128.0);
    let (r, g, b) = if full_range {
        (
            yf + 1.402 * crf,
            yf - 0.344136 * cbf - 0.714136 * crf,
            yf + 1.772 * cbf,
        )
    } else {
        let yl = 1.164 * (yf - 16.0);
        (
            yl + 1.596 * crf,
            yl - 0.391 * cbf - 0.813 * crf,
            yl + 2.018 * cbf,
        )
    };
    (clamp_u8(r), clamp_u8(g), clamp_u8(b))
}

#[inline]
fn clamp_u8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annexb_to_avcc_mixed_start_codes() {
        // NAL1 4字节起始码 [67 AA]，NAL2 3字节起始码 [68 CC BB]。
        let annexb = [
            0, 0, 0, 1, 0x67, 0xAA, // SPS
            0, 0, 1, 0x68, 0xCC, 0xBB, // PPS
        ];
        assert!(is_annex_b(&annexb));
        let avcc = normalize_to_avcc(&annexb);
        assert_eq!(
            avcc,
            vec![0, 0, 0, 2, 0x67, 0xAA, 0, 0, 0, 3, 0x68, 0xCC, 0xBB]
        );
        // 归一化后可被 AVCC 解析器提取到 SPS/PPS。
        let (sps, pps) = extract_sps_pps(&avcc);
        assert_eq!(sps, Some(vec![0x67, 0xAA]));
        assert_eq!(pps, Some(vec![0x68, 0xCC, 0xBB]));
    }

    #[test]
    fn avcc_passthrough_unchanged() {
        // 已是 AVCC（首4字节是长度3，非起始码）→ 原样。
        let avcc = [0, 0, 0, 3, 0x67, 0xAA, 0xBB];
        assert!(!is_annex_b(&avcc));
        assert_eq!(normalize_to_avcc(&avcc), avcc.to_vec());
    }

    #[test]
    fn extract_sps_pps_reads_both() {
        // 两个 NAL：len=3 type7(SPS)，len=2 type8(PPS)。
        let data = [
            0, 0, 0, 3, 0x67, 0xAA, 0xBB, // SPS (0x67 & 0x1F = 7)
            0, 0, 0, 2, 0x68, 0xCC, // PPS (0x68 & 0x1F = 8)
        ];
        let (sps, pps) = extract_sps_pps(&data);
        assert_eq!(sps, Some(vec![0x67, 0xAA, 0xBB]));
        assert_eq!(pps, Some(vec![0x68, 0xCC]));
    }

    #[test]
    fn extract_sps_pps_none_when_only_slices() {
        // type5 IDR slice，无参数集。
        let data = [0, 0, 0, 2, 0x65, 0x11];
        let (sps, pps) = extract_sps_pps(&data);
        assert!(sps.is_none() && pps.is_none());
    }

    #[test]
    fn extract_sps_pps_truncated_tail_ignored() {
        // 声明 len=100 但缓冲不足 → 停止，不 panic。
        let data = [0, 0, 0, 100, 0x67];
        let (sps, _) = extract_sps_pps(&data);
        assert!(sps.is_none());
    }

    #[test]
    fn filter_keeps_vcl_and_sei_drops_params_and_aud() {
        // AU：AUD(9) + SEI(6) + SPS(7) + PPS(8) + IDR(5)。仅 SEI+IDR 应保留。
        let data = [
            0, 0, 0, 1, 0x09, // AUD
            0, 0, 0, 2, 0x06, 0x01, // SEI
            0, 0, 0, 1, 0x67, // SPS
            0, 0, 0, 1, 0x68, // PPS
            0, 0, 0, 3, 0x65, 0xAA, 0xBB, // IDR slice
        ];
        let vcl = filter_nals_for_decode(&data);
        assert_eq!(
            vcl,
            vec![
                0, 0, 0, 2, 0x06, 0x01, // SEI
                0, 0, 0, 3, 0x65, 0xAA, 0xBB, // IDR
            ]
        );
        // 过滤结果里已无 SPS/PPS。
        let (sps, pps) = extract_sps_pps(&vcl);
        assert!(sps.is_none() && pps.is_none());
    }

    #[test]
    fn filter_empty_when_only_params() {
        // 仅 SPS+PPS+AUD（无 VCL/SEI）→ 空。
        let data = [
            0, 0, 0, 1, 0x09, // AUD
            0, 0, 0, 1, 0x67, // SPS
            0, 0, 0, 1, 0x68, // PPS
        ];
        assert!(filter_nals_for_decode(&data).is_empty());
    }

    #[test]
    fn nv12_gray_full_range_is_neutral() {
        // Y=128, Cb=Cr=128 → 灰(128,128,128)（full range）。
        let w = 2;
        let h = 2;
        let y = vec![128u8; w * h];
        let uv = vec![128u8; w]; // 1 行 CbCr（h/2=1），2 字节/组
        let rgba = nv12_to_rgba(&y, w, &uv, w, w, h, true);
        for px in rgba.chunks_exact(4) {
            assert_eq!(px[0], 128);
            assert_eq!(px[1], 128);
            assert_eq!(px[2], 128);
            assert_eq!(px[3], 255);
        }
    }

    #[test]
    fn nv12_honors_stride_padding() {
        // Y/UV 平面每行尾部有 padding；正确实现应跳过 padding 只读前 width 列。
        let w = 2;
        let h = 2;
        let y_stride = 5; // 2 有效 + 3 padding
        let uv_stride = 6;
        let mut y = vec![0u8; y_stride * h];
        // 行 0/1 前两列都填 128（有效），padding 填 0xFF（应被忽略）。
        for row in 0..h {
            y[row * y_stride] = 128;
            y[row * y_stride + 1] = 128;
            y[row * y_stride + 2] = 0xFF;
            y[row * y_stride + 3] = 0xFF;
            y[row * y_stride + 4] = 0xFF;
        }
        let mut uv = vec![0xFFu8; uv_stride]; // 1 行
        uv[0] = 128;
        uv[1] = 128;
        uv[2] = 128;
        uv[3] = 128;
        let rgba = nv12_to_rgba(&y, y_stride, &uv, uv_stride, w, h, true);
        // 全部应为中性灰，证明 padding 的 0xFF 未被误读。
        for px in rgba.chunks_exact(4) {
            assert_eq!((px[0], px[1], px[2]), (128, 128, 128));
        }
    }

    #[test]
    fn nv12_red_video_range() {
        // video-range 红：Y≈81, Cb≈90, Cr≈240（BT.601 limited）。转换应偏红。
        let w = 2;
        let h = 2;
        let y = vec![81u8; w * h];
        let mut uv = vec![0u8; w];
        uv[0] = 90;
        uv[1] = 240;
        let rgba = nv12_to_rgba(&y, w, &uv, w, w, h, false);
        let px = &rgba[..4];
        assert!(px[0] > 200, "R should be high, got {}", px[0]);
        assert!(px[1] < 80, "G should be low, got {}", px[1]);
        assert!(px[2] < 80, "B should be low, got {}", px[2]);
    }
}
