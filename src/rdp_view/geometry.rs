// rdp_view::geometry — 纯几何计算：letterbox 目标矩形 + 连接分辨率推导。
// 无 UI 依赖、全部可离线单测；渲染 Element 与 root_view 只调这里的自由函数。

use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::{vec2f, Vector2F, Vector2I};

/// letterbox 结果：远端桌面等比缩放后落在内容区里的目标矩形（相对内容区左上原点）
/// 加缩放比。`scale` = 逻辑像素 / 远端像素，供第 ④ 步鼠标坐标反算用。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RdpViewport {
    /// 画面绘制矩形（origin 相对内容区左上；调用方按需叠加绝对 origin）。
    pub content_rect: RectF,
    /// 逻辑像素 / 远端像素比例（等比，x/y 相同）。
    pub scale: f32,
}

/// 把远端桌面 `desktop`（像素）等比缩放进内容区 `area`（逻辑像素），居中留边（letterbox）。
/// 不裁剪不拉伸：取 min(宽比,高比)，画面完整可见，多余空间均分为黑边。
pub fn letterbox_rect(area: Vector2F, desktop: Vector2I) -> RdpViewport {
    let dw = desktop.x().max(1) as f32;
    let dh = desktop.y().max(1) as f32;
    let aw = area.x().max(0.0);
    let ah = area.y().max(0.0);

    let scale = (aw / dw).min(ah / dh).max(0.0);
    let w = dw * scale;
    let h = dh * scale;
    let x = (aw - w) / 2.0;
    let y = (ah - h) / 2.0;

    RdpViewport {
        content_rect: RectF::new(vec2f(x, y), vec2f(w, h)),
        scale,
    }
}

/// 鼠标绝对坐标 `mouse`（窗口逻辑像素，与 `vp.content_rect` 同坐标系）→ 远端桌面像素坐标。
/// 返回 `None` 表示落在 letterbox 黑边外（画面外），调用方据此不发送。
/// 命中画面内则按 `scale` 反算并 clamp 到 `[0, desktop-1]`（防边缘越界到无效像素）。
pub fn viewport_device_coords(
    vp: &RdpViewport,
    mouse: Vector2F,
    desktop: Vector2I,
) -> Option<(u16, u16)> {
    let rect = vp.content_rect;
    // 画面外（含黑边）不发送。用半开区间避免右/下边缘反算到 desktop（越界）。
    if mouse.x() < rect.min_x()
        || mouse.y() < rect.min_y()
        || mouse.x() >= rect.max_x()
        || mouse.y() >= rect.max_y()
    {
        return None;
    }
    if vp.scale <= 0.0 {
        return None;
    }
    let local_x = (mouse.x() - rect.min_x()) / vp.scale;
    let local_y = (mouse.y() - rect.min_y()) / vp.scale;
    let max_x = desktop.x().max(1) - 1;
    let max_y = desktop.y().max(1) - 1;
    let x = (local_x.floor() as i64).clamp(0, i64::from(max_x)) as u16;
    let y = (local_y.floor() as i64).clamp(0, i64::from(max_y)) as u16;
    Some((x, y))
}

/// 由内容区逻辑尺寸 + 窗口 scale factor 推导 RDP 连接分辨率（连接时定一次）。
/// `hidpi=false` 用逻辑像素；`hidpi=true` 用物理像素（逻辑×scale）。
/// 结果 clamp 到合理区间并对齐到偶数（部分 RDP 服务端要求宽为 2/4 的倍数）。
pub fn rdp_desktop_size(content_area: Vector2F, scale: f32, hidpi: bool) -> (u16, u16) {
    let factor = if hidpi { scale.max(1.0) } else { 1.0 };
    let w = (content_area.x().max(0.0) * factor).round() as i64;
    let h = (content_area.y().max(0.0) * factor).round() as i64;
    (clamp_even(w), clamp_even(h))
}

/// 请求远端主机 DPI 缩放百分比（对齐 Windows App）。HiDPI 下按物理/逻辑比例×100，
/// clamp 到 connector 有效区间 [100,500]（2.0→200，1.0→100）；非 HiDPI 返回 0=不请求。
pub fn rdp_desktop_scale_factor(scale: f32, hidpi: bool) -> u32 {
    if hidpi {
        ((scale.max(1.0) * 100.0).round() as u32).clamp(100, 500)
    } else {
        0
    }
}

fn clamp_even(value: i64) -> u16 {
    let clamped = value.clamp(640, 8192) as u16;
    clamped & !1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterbox_pillarboxes_when_width_constrained() {
        // 内容区更宽（16:9 桌面塞进 2:1 内容区）→ 高受限，左右留边。
        let vp = letterbox_rect(vec2f(2000.0, 1000.0), Vector2I::new(1600, 900));
        assert!((vp.scale - 1000.0 / 900.0).abs() < 1e-4);
        // 高度撑满，宽度 = 1600 * scale < 2000，水平居中。
        assert!((vp.content_rect.size().y() - 1000.0).abs() < 1e-3);
        let w = 1600.0 * vp.scale;
        assert!((vp.content_rect.size().x() - w).abs() < 1e-3);
        assert!((vp.content_rect.origin().x() - (2000.0 - w) / 2.0).abs() < 1e-3);
        assert!(vp.content_rect.origin().y().abs() < 1e-3);
    }

    #[test]
    fn letterbox_letterboxes_when_height_constrained() {
        // 内容区更高（16:9 桌面塞进 1:1 内容区）→ 宽受限，上下留边。
        let vp = letterbox_rect(vec2f(1000.0, 1000.0), Vector2I::new(1920, 1080));
        assert!((vp.scale - 1000.0 / 1920.0).abs() < 1e-4);
        assert!((vp.content_rect.size().x() - 1000.0).abs() < 1e-3);
        let h = 1080.0 * vp.scale;
        assert!((vp.content_rect.size().y() - h).abs() < 1e-3);
        assert!(vp.content_rect.origin().x().abs() < 1e-3);
        assert!((vp.content_rect.origin().y() - (1000.0 - h) / 2.0).abs() < 1e-3);
    }

    #[test]
    fn letterbox_exact_aspect_fills_without_border() {
        let vp = letterbox_rect(vec2f(1920.0, 1080.0), Vector2I::new(1920, 1080));
        assert!((vp.scale - 1.0).abs() < 1e-4);
        assert!(vp.content_rect.origin().x().abs() < 1e-3);
        assert!(vp.content_rect.origin().y().abs() < 1e-3);
        assert!((vp.content_rect.size().x() - 1920.0).abs() < 1e-3);
    }

    #[test]
    fn letterbox_content_smaller_than_desktop_scales_down() {
        // 内容区小于桌面：scale < 1，画面缩小仍完整可见。
        let vp = letterbox_rect(vec2f(800.0, 600.0), Vector2I::new(1920, 1080));
        assert!(vp.scale < 1.0);
        assert!(vp.content_rect.size().x() <= 800.0 + 1e-3);
        assert!(vp.content_rect.size().y() <= 600.0 + 1e-3);
    }

    #[test]
    fn device_coords_center_and_scale() {
        // 桌面 1000x500 缩放到 content_rect origin(100,50) size(500,250)，scale=0.5。
        let vp = RdpViewport {
            content_rect: RectF::new(vec2f(100.0, 50.0), vec2f(500.0, 250.0)),
            scale: 0.5,
        };
        let desktop = Vector2I::new(1000, 500);
        // 画面左上角 → 远端(0,0)。
        assert_eq!(
            viewport_device_coords(&vp, vec2f(100.0, 50.0), desktop),
            Some((0, 0))
        );
        // 画面中心 → 远端约 (500,250) → clamp 内。
        let (x, y) = viewport_device_coords(&vp, vec2f(350.0, 175.0), desktop).unwrap();
        assert_eq!((x, y), (500, 250));
    }

    #[test]
    fn device_coords_outside_letterbox_returns_none() {
        let vp = RdpViewport {
            content_rect: RectF::new(vec2f(100.0, 50.0), vec2f(500.0, 250.0)),
            scale: 0.5,
        };
        let desktop = Vector2I::new(1000, 500);
        // 左上黑边外。
        assert_eq!(
            viewport_device_coords(&vp, vec2f(50.0, 20.0), desktop),
            None
        );
        // 右下黑边外（超过 max）。
        assert_eq!(
            viewport_device_coords(&vp, vec2f(700.0, 400.0), desktop),
            None
        );
    }

    #[test]
    fn device_coords_edge_clamps_within_desktop() {
        let vp = RdpViewport {
            content_rect: RectF::new(vec2f(0.0, 0.0), vec2f(100.0, 100.0)),
            scale: 1.0,
        };
        let desktop = Vector2I::new(100, 100);
        // 逼近右下边缘（99.9,99.9）→ 反算 99，不越界到 100。
        let (x, y) = viewport_device_coords(&vp, vec2f(99.9, 99.9), desktop).unwrap();
        assert_eq!((x, y), (99, 99));
        // 恰在右/下边缘 (100,100) 属半开区间外 → None。
        assert_eq!(
            viewport_device_coords(&vp, vec2f(100.0, 100.0), desktop),
            None
        );
    }

    #[test]
    fn desktop_size_standard_uses_logical_pixels() {
        let (w, h) = rdp_desktop_size(vec2f(1440.0, 900.0), 2.0, false);
        assert_eq!((w, h), (1440, 900));
    }

    #[test]
    fn desktop_size_hidpi_scales_by_factor() {
        let (w, h) = rdp_desktop_size(vec2f(1440.0, 900.0), 2.0, true);
        assert_eq!((w, h), (2880, 1800));
    }

    #[test]
    fn desktop_size_clamps_and_rounds_even() {
        // 过小 → 每维 clamp 到 640 下限；奇数 → 对齐偶数（1001 → 1000）。
        let (w, h) = rdp_desktop_size(vec2f(100.0, 100.0), 1.0, false);
        assert_eq!((w, h), (640, 640));
        let (w, _) = rdp_desktop_size(vec2f(1001.0, 700.0), 1.0, false);
        assert_eq!((w, w % 2), (1000, 0));
    }

    #[test]
    fn scale_factor_hidpi_retina_is_200() {
        assert_eq!(rdp_desktop_scale_factor(2.0, true), 200);
    }

    #[test]
    fn scale_factor_hidpi_unity_is_100() {
        assert_eq!(rdp_desktop_scale_factor(1.0, true), 100);
    }

    #[test]
    fn scale_factor_standard_is_zero() {
        assert_eq!(rdp_desktop_scale_factor(2.0, false), 0);
    }
}
