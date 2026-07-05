//! 本地化自 warp::font_fallback —— 给 warpui 文本系统注册 CJK fallback 字体。
//!
//! 解耦删 warp app 后漏搬此注册：code_viewer（warp_editor/warpui 文本系统）的 base 字体
//! Hack 无 CJK 字形，缺 `fallback_font_fn` 时中文渲染乱码（终端走自有 grid 渲染故不受影响）。
//! 这里用各平台系统中文字体补 fallback —— 跨平台、不依赖网络（区别于 warp 原版从 CDN 拉 Noto）。

use std::sync::Arc;

use lazy_static::lazy_static;
use warpui::assets::asset_cache::AssetSource;
use warpui::fonts::ExternalFontFamily;

/// fallback 字体标识；source provider 据此返回平台系统 CJK 字体路径。
const CJK_FONT_ID: &str = "nexshell-cjk-fallback";

lazy_static! {
    static ref CJK_FAMILY: ExternalFontFamily = ExternalFontFamily {
        name: "NexshellCJKFallback",
        font_urls: Arc::new(vec![CJK_FONT_ID.to_string()]),
    };
}

/// base 字体缺字形的字符 → fallback 字体族。覆盖 CJK 表意/假名/标点/全角。
/// 仅在 warpui 判定字符在当前字体缺字形时被查，故范围只列 base 字体（Hack/Roboto）必然没有的 CJK 区段。
pub fn fallback_font_fn(ch: char) -> Option<ExternalFontFamily> {
    match ch {
        '\u{2E80}'..='\u{9FFF}'      // CJK 部首补充 / 假名 / 标点 / 扩展 A / 统一表意
        | '\u{F900}'..='\u{FAFF}'    // CJK 兼容表意
        | '\u{FE30}'..='\u{FE4F}'    // CJK 兼容形式
        | '\u{FF00}'..='\u{FFEF}'    // 全角 / 半角形式
        | '\u{20000}'..='\u{2FA1F}'  // CJK 扩展 B–F + 兼容补充
        => Some(CJK_FAMILY.clone()),
        _ => None,
    }
}

/// fallback 字体字节源：平台系统中文字体（取第一个存在的路径），无网络依赖。
pub fn fallback_source(_id: &str) -> AssetSource {
    AssetSource::LocalFile {
        path: system_cjk_font_path().to_string(),
    }
}

fn system_cjk_font_path() -> &'static str {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/STHeiti Medium.ttc",
        ]
    } else if cfg!(target_os = "windows") {
        &[
            "C:\\Windows\\Fonts\\msyh.ttc", // 微软雅黑
            "C:\\Windows\\Fonts\\msyh.ttf",
            "C:\\Windows\\Fonts\\simsun.ttc", // 宋体
            "C:\\Windows\\Fonts\\simhei.ttf", // 黑体
        ]
    } else {
        &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ]
    };
    candidates
        .iter()
        .copied()
        .find(|p| std::path::Path::new(p).exists())
        .unwrap_or(candidates[0])
}
