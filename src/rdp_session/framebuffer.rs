//! 会话共享的 RGBA framebuffer：会话线程写，UI 渲染器按帧代号取脏区上传。

use std::collections::VecDeque;

use super::DirtyRect;

/// 渲染侧最多落后这么多帧还能只传脏区，再多就整帧上传。
const DAMAGE_LOG_LEN: usize = 32;

/// RGBA framebuffer（R,G,B,A，非预乘）。会话线程写，UI 渲染器经 LiveImageSource 非阻塞地读。
pub struct RdpFramebuffer {
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    /// 帧代号：每次内容变更 +1，换分辨率也不归零。渲染侧据此判断有无新帧、该上传哪些区域。
    generation: u64,
    /// 最近几次变更的 (帧代号, 脏区)，代号递增。
    damage: VecDeque<(u64, DirtyRect)>,
}

impl RdpFramebuffer {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            rgba: vec![0; usize::from(width) * usize::from(height) * 4],
            generation: 0,
            damage: VecDeque::new(),
        }
    }

    /// 当前帧代号（0 = 尚无任何帧）。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 换分辨率：按新尺寸清成黑屏，帧代号继续递增；旧脏区作废，渲染侧整帧重传。
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.rgba = vec![0; usize::from(width) * usize::from(height) * 4];
        self.generation = self.generation.wrapping_add(1);
        self.damage.clear();
    }

    /// 内容已变：帧代号 +1 并记下脏区。
    pub fn mark_dirty(&mut self, rect: DirtyRect) {
        self.generation = self.generation.wrapping_add(1);
        if self.damage.len() == DAMAGE_LOG_LEN {
            self.damage.pop_front();
        }
        self.damage.push_back((self.generation, rect));
    }

    /// 帧代号 `since` 之后变过的区域：`since` 已是最新给空列表，记录不全给 None（需整帧）。
    pub fn dirty_since(&self, since: u64) -> Option<Vec<DirtyRect>> {
        if since == self.generation {
            return Some(Vec::new());
        }
        let first = self.damage.front()?.0;
        if since > self.generation || first > since + 1 {
            return None;
        }
        Some(
            self.damage
                .iter()
                .filter(|(generation, _)| *generation > since)
                .map(|(_, rect)| *rect)
                .collect(),
        )
    }

    /// 整帧覆盖（src 必须是同尺寸 RGBA）。返回整帧脏矩形。
    pub fn apply_full(&mut self, src: &[u8]) -> DirtyRect {
        let len = self.rgba.len().min(src.len());
        self.rgba[..len].copy_from_slice(&src[..len]);
        let full = DirtyRect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        };
        self.mark_dirty(full);
        full
    }

    /// 把 src（整帧 RGBA）中 rect 覆盖的行拷进本 framebuffer 对应位置。
    pub fn apply_region(&mut self, src: &[u8], rect: DirtyRect) {
        let stride = usize::from(self.width) * 4;
        let x0 = usize::from(rect.x);
        let x1 = usize::from(rect.x + rect.width).min(usize::from(self.width));
        let y1 = rect.y.saturating_add(rect.height).min(self.height);
        if x1 <= x0 || y1 <= rect.y {
            return;
        }
        self.mark_dirty(DirtyRect {
            x: rect.x,
            y: rect.y,
            width: (x1 - x0) as u16,
            height: y1 - rect.y,
        });
        let row_bytes = (x1 - x0) * 4;
        for row in rect.y..y1 {
            let off = usize::from(row) * stride + x0 * 4;
            if off + row_bytes > self.rgba.len() || off + row_bytes > src.len() {
                break;
            }
            self.rgba[off..off + row_bytes].copy_from_slice(&src[off..off + row_bytes]);
        }
    }

    /// EGFX 合成：把已映射 surface（src=src_w×src_h RGBA，映射原点 origin）落进 framebuffer，
    /// 只写 `clip`（output 坐标）范围内、且被 surface 覆盖的行。不记脏区（发布前统一 mark_dirty）。
    pub fn compose_surface(
        &mut self,
        origin_x: i32,
        origin_y: i32,
        src: &[u8],
        src_w: u16,
        src_h: u16,
        clip: DirtyRect,
    ) {
        let fb_w = i32::from(self.width);
        let fb_h = i32::from(self.height);
        let sw = i32::from(src_w);
        let sh = i32::from(src_h);
        // clip ∩ surface-output-rect ∩ framebuffer。
        let cx0 = i32::from(clip.x).max(origin_x).max(0);
        let cy0 = i32::from(clip.y).max(origin_y).max(0);
        let cx1 = (i32::from(clip.x) + i32::from(clip.width))
            .min(origin_x + sw)
            .min(fb_w);
        let cy1 = (i32::from(clip.y) + i32::from(clip.height))
            .min(origin_y + sh)
            .min(fb_h);
        if cx1 <= cx0 || cy1 <= cy0 {
            return;
        }
        let fb_stride = usize::from(self.width) * 4;
        let src_stride = usize::from(src_w) * 4;
        let n = (cx1 - cx0) as usize * 4;
        for row in cy0..cy1 {
            let sy = (row - origin_y) as usize;
            let sx = (cx0 - origin_x) as usize;
            let so = sy * src_stride + sx * 4;
            let dofs = row as usize * fb_stride + cx0 as usize * 4;
            if so + n > src.len() || dofs + n > self.rgba.len() {
                continue;
            }
            self.rgba[dofs..dofs + n].copy_from_slice(&src[so..so + n]);
        }
    }
}
