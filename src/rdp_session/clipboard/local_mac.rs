//! macOS 本地剪贴板：直接调 NSPasteboard（ADR 0016 决策 1）。
//! 调用方都是后台线程（轮询、RDP、应答工作线程），每次调用包一层 autoreleasepool。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey, NSPasteboard,
    NSPasteboardType, NSPasteboardTypeFileURL, NSPasteboardTypePNG, NSPasteboardTypeRTF,
    NSPasteboardTypeString, NSPasteboardTypeTIFF,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo,
};
use objc2_foundation::{NSData, NSDictionary, NSString};

use super::{Available, Bitmap};

pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(250);

fn pasteboard() -> Retained<NSPasteboard> {
    NSPasteboard::generalPasteboard()
}

// AppKit 导出的常量字符串，进程内常驻。
fn string_type() -> &'static NSPasteboardType {
    unsafe { NSPasteboardTypeString }
}
fn rtf_type() -> &'static NSPasteboardType {
    unsafe { NSPasteboardTypeRTF }
}
fn png_type() -> &'static NSPasteboardType {
    unsafe { NSPasteboardTypePNG }
}
fn tiff_type() -> &'static NSPasteboardType {
    unsafe { NSPasteboardTypeTIFF }
}
fn file_url_type() -> &'static NSPasteboardType {
    unsafe { NSPasteboardTypeFileURL }
}

/// changeCount 加类型清单。截图等程序先清空剪贴板、过一阵才写入数据，
/// 写入不会再加 changeCount，只看它会把中途的空剪贴板当成最终内容。
pub(super) fn change_token() -> Option<u64> {
    autoreleasepool(|_| {
        let pb = pasteboard();
        let mut hasher = DefaultHasher::new();
        pb.changeCount().hash(&mut hasher);
        for t in pb.types().iter().flatten() {
            t.to_string().hash(&mut hasher);
        }
        Some(hasher.finish())
    })
}

pub(super) fn available() -> Available {
    autoreleasepool(|_| {
        let Some(types) = pasteboard().types() else {
            return Available::default();
        };
        let has = |t: &NSPasteboardType| types.containsObject(t);
        Available {
            text: has(string_type()),
            rtf: has(rtf_type()),
            // Finder 复制文件时附带图标 TIFF，不当图片广播。
            image: !has(file_url_type()) && (has(png_type()) || has(tiff_type())),
        }
    })
}

pub(super) fn read_text() -> Option<String> {
    autoreleasepool(|_| {
        pasteboard()
            .stringForType(string_type())
            .map(|s| s.to_string())
    })
}

pub(super) fn write_text(text: &str) -> bool {
    autoreleasepool(|_| {
        let pb = pasteboard();
        pb.clearContents();
        pb.setString_forType(&NSString::from_str(text), string_type())
    })
}

pub(super) fn read_rtf() -> Option<Vec<u8>> {
    autoreleasepool(|_| pasteboard().dataForType(rtf_type()).map(|d| d.to_vec()))
}

/// 有 PNG 原样给；只有 TIFF 时转成 PNG。
pub(super) fn read_png() -> Option<Vec<u8>> {
    autoreleasepool(|_| {
        let pb = pasteboard();
        if let Some(png) = pb.dataForType(png_type()) {
            return Some(png.to_vec());
        }
        let tiff = pb.dataForType(tiff_type())?;
        let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
        let props = NSDictionary::<NSBitmapImageRepPropertyKey, AnyObject>::new();
        unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props) }
            .map(|d| d.to_vec())
    })
}

/// 剪贴板图片 → sRGB、自上而下的 BGRA。
pub(super) fn read_bitmap() -> Option<Bitmap> {
    autoreleasepool(|_| {
        let pb = pasteboard();
        let data = pb
            .dataForType(png_type())
            .or_else(|| pb.dataForType(tiff_type()))?;
        decode_bitmap(&data)
    })
}

/// 解码并垫白底去掉透明：多数 Windows 程序不认 CF_DIB 的 alpha。
fn decode_bitmap(data: &NSData) -> Option<Bitmap> {
    let image = NSBitmapImageRep::imageRepWithData(data)?.CGImage()?;
    let width = CGImage::width(Some(&image));
    let height = CGImage::height(Some(&image));
    let mut bgra = vec![0u8; width.checked_mul(height)?.checked_mul(4)?];
    if bgra.is_empty() {
        return None;
    }
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
    // SAFETY: bgra 恰好 width*height*4 字节，context 在 bgra 之前释放。
    let context = unsafe {
        CGBitmapContextCreate(
            bgra.as_mut_ptr().cast(),
            width,
            height,
            8,
            width * 4,
            Some(&space),
            info,
        )
    }?;
    let rect = CGRect::new(
        CGPoint::new(0.0, 0.0),
        CGSize::new(width as f64, height as f64),
    );
    CGContext::set_rgb_fill_color(Some(&context), 1.0, 1.0, 1.0, 1.0);
    CGContext::fill_rect(Some(&context), rect);
    CGContext::draw_image(Some(&context), rect, Some(&image));
    drop(context);
    Some(Bitmap {
        width,
        height,
        bgra,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2×1 RGBA PNG：左像素不透明红，右像素全透明蓝。
    const RED_AND_CLEAR_PNG: [u8; 70] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0xf4,
        0x22, 0x7f, 0x8a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x46, 0x00, 0x0e, 0xfa, 0x02, 0xfe, 0xf5, 0x1e, 0x4c, 0xa9, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    fn close(actual: &[u8], expected: [u8; 4]) -> bool {
        actual
            .iter()
            .zip(expected)
            .all(|(&a, e)| a.abs_diff(e) <= 2)
    }

    #[test]
    fn decode_bitmap_outputs_opaque_bgra_on_white() {
        let data = NSData::with_bytes(&RED_AND_CLEAR_PNG);
        let bitmap = decode_bitmap(&data).expect("decode png");
        assert_eq!((bitmap.width, bitmap.height), (2, 1));
        assert!(
            close(&bitmap.bgra[0..4], [0, 0, 255, 255]),
            "{:?}",
            &bitmap.bgra[0..4]
        );
        assert!(
            close(&bitmap.bgra[4..8], [255, 255, 255, 255]),
            "{:?}",
            &bitmap.bgra[4..8]
        );
    }
}
