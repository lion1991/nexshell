//! 光标拖影（Neovide 风格）：四角独立指数趋近目标格，运动前侧角快、尾侧角慢，
//! 中间态是变形四边形（scene::Quad 绘制）。调用方需传入最终绘制坐标系下的逻辑 px。

use std::time::Instant;

use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::Vector2F;

/// 前导角 / 拖尾角的指数趋近速率（1/s），视觉时长约 3/rate 秒。
const LEAD_RATE: f32 = 45.0;
const TRAIL_RATE: f32 = 16.0;
/// 全部角距目标小于该值（px）即收敛落格。
const SETTLE_EPS: f32 = 0.5;
/// dt 上限：长时间无 tick 后直接近似收敛，避免超大步长。
const MAX_DT: f32 = 0.05;
/// 角滞后目标的最大距离（px）：大跨行跳跃时截断拖尾，不横贯整屏。
const MAX_TRAIL_PX: f32 = 200.0;
/// 透明度随滞后距离衰减：滞后 0 → 全实，滞后达此值 → 降到 TAIL_MIN_ALPHA。
const TRAIL_FADE_PX: f32 = 150.0;
const TAIL_MIN_ALPHA: f32 = 0.25;

/// 一帧拖影：四角位置 + 各角透明度（拖尾渐隐）。
pub struct SmearQuad {
    pub corners: [Vector2F; 4],
    pub alphas: [f32; 4],
}

pub struct CursorSmear {
    /// 左上/右上/右下/左下（与 scene::Quad 角序一致）。
    corners: [Vector2F; 4],
    target: Option<RectF>,
    last_tick: Option<Instant>,
}

fn rect_corners(rect: RectF) -> [Vector2F; 4] {
    [
        rect.origin(),
        Vector2F::new(rect.max_x(), rect.min_y()),
        rect.lower_right(),
        Vector2F::new(rect.min_x(), rect.max_y()),
    ]
}

impl CursorSmear {
    pub fn new() -> Self {
        Self {
            corners: [Vector2F::zero(); 4],
            target: None,
            last_tick: None,
        }
    }

    /// 一切不该补间的场合（光标隐藏 / 翻滚动历史 / 切 tab-pane）：清状态，下次直接落格。
    pub fn reset(&mut self) {
        self.target = None;
        self.last_tick = None;
    }

    pub fn is_animating(&self) -> bool {
        self.target.is_some_and(|t| {
            self.corners
                .iter()
                .zip(rect_corners(t))
                .any(|(c, g)| (*c - g).length() > SETTLE_EPS)
        })
    }

    /// 每帧推进。Some = 画变形四边形（含各角透明度）；None = 首现或已收敛，按普通矩形画。
    pub fn update(&mut self, target: RectF, now: Instant) -> Option<SmearQuad> {
        let goals = rect_corners(target);
        if self.target.is_none() {
            // 首次出现：直接落格不补间。
            self.corners = goals;
            self.target = Some(target);
            self.last_tick = Some(now);
            return None;
        }
        self.target = Some(target);
        let dt = self
            .last_tick
            .map(|t| now.saturating_duration_since(t).as_secs_f32())
            .unwrap_or(0.0)
            .min(MAX_DT);
        self.last_tick = Some(now);

        if !self.is_animating() {
            self.corners = goals;
            return None;
        }

        // 运动方向 = 几何中心差；目标角在方向前侧用快速率，后侧用慢速率 → 变形拖尾。
        let center = (self.corners[0] + self.corners[1] + self.corners[2] + self.corners[3]) * 0.25;
        let goal_center = target.center();
        let dir = goal_center - center;
        let mut alphas = [1.0_f32; 4];
        for (i, (corner, goal)) in self.corners.iter_mut().zip(goals).enumerate() {
            let lead = dir.square_length() < 1e-3 || (goal - goal_center).dot(dir) > 0.0;
            let rate = if lead { LEAD_RATE } else { TRAIL_RATE };
            let step = 1.0 - (-rate * dt).exp();
            *corner = *corner + (goal - *corner) * step;
            // 大跨行：滞后超上限截断拖尾，不横贯整屏。
            let lag = *corner - goal;
            let lag_len = lag.length();
            if lag_len > MAX_TRAIL_PX {
                *corner = goal + lag * (MAX_TRAIL_PX / lag_len);
            }
            // 滞后越远越透明 → 彗星式渐隐尾。
            alphas[i] = (1.0 - lag_len.min(MAX_TRAIL_PX) / TRAIL_FADE_PX).max(TAIL_MIN_ALPHA);
        }

        if self
            .corners
            .iter()
            .zip(goals)
            .all(|(c, g)| (*c - g).length() <= SETTLE_EPS)
        {
            self.corners = goals;
            return None;
        }
        Some(SmearQuad {
            corners: self.corners,
            alphas,
        })
    }
}

impl Default for CursorSmear {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn rect(x: f32, y: f32) -> RectF {
        RectF::new(Vector2F::new(x, y), Vector2F::new(10.0, 20.0))
    }

    #[test]
    fn first_update_snaps_without_animation() {
        let mut smear = CursorSmear::new();
        let t0 = Instant::now();
        assert!(smear.update(rect(0.0, 0.0), t0).is_none());
        assert!(!smear.is_animating());
    }

    #[test]
    fn jump_animates_lead_faster_than_trail() {
        let mut smear = CursorSmear::new();
        let t0 = Instant::now();
        smear.update(rect(0.0, 0.0), t0);
        let sq = smear
            .update(rect(100.0, 0.0), t0 + Duration::from_millis(16))
            .expect("jump should animate");
        // 向右跳：右侧角（前导）应比左侧角（拖尾）走完更大比例。
        let lead_progress = sq.corners[1].x() - 10.0; // 右上角从 x=10 → 110
        let trail_progress = sq.corners[0].x(); // 左上角从 x=0 → 100
        assert!(lead_progress / 100.0 > trail_progress / 100.0);
        // 拖尾角滞后更远 → 更透明。
        assert!(sq.alphas[0] < sq.alphas[1]);
        assert!(smear.is_animating());
    }

    #[test]
    fn huge_jump_clamps_trail_and_fades_tail() {
        let mut smear = CursorSmear::new();
        let t0 = Instant::now();
        smear.update(rect(0.0, 0.0), t0);
        let sq = smear
            .update(rect(0.0, 2000.0), t0 + Duration::from_millis(16))
            .expect("jump should animate");
        let goals = rect_corners(rect(0.0, 2000.0));
        for (corner, goal) in sq.corners.iter().zip(goals) {
            assert!((*corner - goal).length() <= MAX_TRAIL_PX + 0.01);
        }
        // 拖尾角贴着钳制上限 → 透明度落到下限。
        assert!((sq.alphas[0] - TAIL_MIN_ALPHA).abs() < 1e-3);
    }

    #[test]
    fn converges_and_settles() {
        let mut smear = CursorSmear::new();
        let mut now = Instant::now();
        smear.update(rect(0.0, 0.0), now);
        let target = rect(100.0, 40.0);
        for _ in 0..120 {
            now += Duration::from_millis(16);
            if smear.update(target, now).is_none() {
                assert!(!smear.is_animating());
                return;
            }
        }
        panic!("smear never settled");
    }

    #[test]
    fn reset_snaps_next_update() {
        let mut smear = CursorSmear::new();
        let t0 = Instant::now();
        smear.update(rect(0.0, 0.0), t0);
        smear.reset();
        assert!(smear
            .update(rect(100.0, 0.0), t0 + Duration::from_millis(16))
            .is_none());
    }
}
