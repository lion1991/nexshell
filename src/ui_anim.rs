//! eased 颜色过渡：render 驱动重定向 + 采样。
//! 语义：每帧 render 按当前目标色 retarget，tick 每 16ms 采样重绘，全部 settled 即停表。

use std::collections::HashMap;
use std::hash::Hash;
use std::time::Instant;

use warpui_core::color::ColorU;

/// 过渡时长（毫秒）。
pub const TRANSITION_MS: f32 = 120.0;

/// ease-out 三次：起步快、收尾缓。
pub fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t.clamp(0.0, 1.0);
    1.0 - inv * inv * inv
}

/// 四通道线性插值。
pub fn lerp_color(a: ColorU, b: ColorU, t: f32) -> ColorU {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    ColorU::new(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b), mix(a.a, b.a))
}

/// 单条颜色过渡。start=None 表示稳态无动画。
#[derive(Clone, Copy)]
pub struct ColorTransition {
    from: ColorU,
    to: ColorU,
    start: Option<Instant>,
}

impl ColorTransition {
    /// 稳态构造：无动画，直接停在 initial。
    pub fn new(initial: ColorU) -> Self {
        Self {
            from: initial,
            to: initial,
            start: None,
        }
    }

    fn progress(&self, now: Instant) -> f32 {
        match self.start {
            None => 1.0,
            Some(s) => (now.saturating_duration_since(s).as_millis() as f32 / TRANSITION_MS)
                .clamp(0.0, 1.0),
        }
    }

    pub fn sample(&self, now: Instant) -> ColorU {
        lerp_color(self.from, self.to, ease_out_cubic(self.progress(now)))
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.start.is_some() && self.progress(now) < 1.0
    }

    /// 重定向到新目标：目标变则从当前采样色起步翻转（动画中途不跳变）。
    pub fn retarget(&mut self, target: ColorU, now: Instant) {
        if self.to == target {
            return;
        }
        self.from = self.sample(now);
        self.to = target;
        self.start = Some(now);
    }
}

/// 动态集合的过渡表（多 tab / 多按钮）。key 为调用方稳定标识。
pub struct TransitionMap<K: Hash + Eq> {
    map: HashMap<K, ColorTransition>,
}

impl<K: Hash + Eq> TransitionMap<K> {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// 重定向 key 的过渡；首次出现以 target 稳态建（首帧不淡入）。
    pub fn retarget(&mut self, key: K, target: ColorU, now: Instant) {
        self.map
            .entry(key)
            .or_insert_with(|| ColorTransition::new(target))
            .retarget(target, now);
    }

    pub fn sample(&self, key: &K, now: Instant) -> Option<ColorU> {
        self.map.get(key).map(|t| t.sample(now))
    }

    pub fn any_animating(&self, now: Instant) -> bool {
        self.map.values().any(|t| t.is_animating(now))
    }

    /// 清理本帧不再出现的 key，防泄漏。
    pub fn retain(&mut self, keep: impl Fn(&K) -> bool) {
        self.map.retain(|k, _| keep(k));
    }
}

impl<K: Hash + Eq> Default for TransitionMap<K> {
    fn default() -> Self {
        Self::new()
    }
}
