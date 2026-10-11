// rdp_view::live_image — 会话 framebuffer 作为 warpui 常驻图片的像素源。
// 渲染器非阻塞地读，按帧代号只上传脏区；UI 线程不再整帧拷贝。

use std::sync::Arc;

use parking_lot::Mutex;
use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::vector::Vector2I;
use warpui_core::rendering::live_image::{LiveImageFrame, LiveImageSource};

use nexshell::rdp_session::{DirtyRect, RdpFramebuffer};

pub struct RdpLiveImage {
    framebuffer: Arc<Mutex<RdpFramebuffer>>,
    /// 读不到时请求补一次重绘。
    redraw_tx: async_channel::Sender<()>,
}

impl RdpLiveImage {
    pub fn new(
        framebuffer: Arc<Mutex<RdpFramebuffer>>,
        redraw_tx: async_channel::Sender<()>,
    ) -> Self {
        Self {
            framebuffer,
            redraw_tx,
        }
    }
}

impl LiveImageSource for RdpLiveImage {
    fn try_read(&self, since: u64, read: &mut dyn FnMut(&LiveImageFrame<'_>)) -> bool {
        let Some(fb) = self.framebuffer.try_lock() else {
            // 写帧的一方解锁后会发帧事件触发重绘，但只读的一方（取尺寸等）不会，所以自己补一次。
            let _ = self.redraw_tx.try_send(());
            return false;
        };
        read(&LiveImageFrame {
            width: u32::from(fb.width),
            height: u32::from(fb.height),
            rgba: &fb.rgba,
            generation: fb.generation(),
            dirty: fb
                .dirty_since(since)
                .map(|rects| rects.into_iter().map(to_rect).collect()),
        });
        true
    }
}

fn to_rect(rect: DirtyRect) -> RectI {
    RectI::new(
        Vector2I::new(i32::from(rect.x), i32::from(rect.y)),
        Vector2I::new(i32::from(rect.width), i32::from(rect.height)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(image: &RdpLiveImage, since: u64) -> Option<(u64, Option<Vec<RectI>>)> {
        let mut seen = None;
        image
            .try_read(since, &mut |frame| {
                seen = Some((frame.generation, frame.dirty.clone()));
            })
            .then_some(())?;
        seen
    }

    #[test]
    fn reports_damage_since_the_given_generation() {
        let fb = Arc::new(Mutex::new(RdpFramebuffer::new(4, 4)));
        let (tx, _rx) = async_channel::bounded(1);
        let image = RdpLiveImage::new(fb.clone(), tx);
        let rect = DirtyRect {
            x: 1,
            y: 2,
            width: 3,
            height: 1,
        };
        fb.lock().mark_dirty(rect);

        assert_eq!(read(&image, 0), Some((1, Some(vec![to_rect(rect)]))));
        assert_eq!(read(&image, 1), Some((1, Some(vec![]))));
    }

    #[test]
    fn busy_framebuffer_requests_a_redraw() {
        let fb = Arc::new(Mutex::new(RdpFramebuffer::new(4, 4)));
        let (tx, rx) = async_channel::bounded(1);
        let image = RdpLiveImage::new(fb.clone(), tx);

        let guard = fb.lock();
        assert_eq!(read(&image, 0), None);
        assert_eq!(read(&image, 0), None);
        drop(guard);
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }
}
