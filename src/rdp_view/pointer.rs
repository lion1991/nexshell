// rdp_view::pointer — RdpPointer（会话层）→ warpui Cursor 转换。
// Bitmap 分支把位图注册进 warp 全局光标表，只在收到新指针时做一次（非每帧）。

use nexshell::rdp_session::RdpPointer;
use warpui::platform::{register_custom_cursor, Cursor};

/// 远端指针形态 → 本地光标。Bitmap 注册位图取 id 存入 Cursor::CustomImage。
/// Default=系统箭头，Hidden=隐藏。`scale`=viewport 缩放（点/远端像素），
/// 光标点尺寸随画面等比，Retina 下位图 1:1 物理像素不发虚。
pub fn pointer_to_cursor(pointer: &RdpPointer, scale: f32) -> Cursor {
    match pointer {
        RdpPointer::Default => Cursor::Arrow,
        RdpPointer::Hidden => Cursor::Hidden,
        RdpPointer::Bitmap {
            rgba,
            width,
            height,
            hotspot_x,
            hotspot_y,
            ..
        } => {
            let id = register_custom_cursor(
                rgba.clone(),
                *width,
                *height,
                *hotspot_x,
                *hotspot_y,
                scale,
            );
            dump_pointer_png(rgba, *width, *height, id);
            Cursor::CustomImage(id)
        }
    }
}

/// 诊断：NEXSHELL_RDP_PTR_DUMP=<目录> 时把每个指针位图落盘原始 RGBA（尺寸在文件名），
/// 离线转 PNG 肉眼核对解码结果。不引 image 主依赖。
fn dump_pointer_png(rgba: &[u8], width: u32, height: u32, id: u64) {
    let Some(dir) = std::env::var_os("NEXSHELL_RDP_PTR_DUMP") else {
        return;
    };
    let path = std::path::Path::new(&dir).join(format!("ptr-{id}-{width}x{height}.rgba"));
    let _ = std::fs::write(path, rgba);
}
