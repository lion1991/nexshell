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

// ── 数值过渡（环弧 sweep 等）────────────────────────────────────

/// 数值过渡时长（毫秒）：比色过渡长，让弧线爬升可感。
pub const FLOAT_TRANSITION_MS: f32 = 350.0;

/// 单条数值过渡。start=None 表示稳态无动画。
#[derive(Clone, Copy)]
pub struct FloatTransition {
    from: f32,
    to: f32,
    start: Option<Instant>,
}

impl FloatTransition {
    /// 稳态构造：无动画，直接停在 initial。
    pub fn new(initial: f32) -> Self {
        Self {
            from: initial,
            to: initial,
            start: None,
        }
    }

    fn progress(&self, now: Instant) -> f32 {
        match self.start {
            None => 1.0,
            Some(s) => (now.saturating_duration_since(s).as_millis() as f32 / FLOAT_TRANSITION_MS)
                .clamp(0.0, 1.0),
        }
    }

    pub fn sample(&self, now: Instant) -> f32 {
        let t = ease_out_cubic(self.progress(now));
        self.from + (self.to - self.from) * t
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.start.is_some() && self.progress(now) < 1.0
    }

    /// 重定向到新目标：目标变则从当前采样值起步（动画中途不跳变）。
    pub fn retarget(&mut self, target: f32, now: Instant) {
        if self.to == target {
            return;
        }
        self.from = self.sample(now);
        self.to = target;
        self.start = Some(now);
    }
}

/// 动态集合的数值过渡表。key 为调用方稳定标识。
pub struct FloatTransitionMap<K: Hash + Eq> {
    map: HashMap<K, FloatTransition>,
}

impl<K: Hash + Eq> FloatTransitionMap<K> {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// 重定向 key 的过渡；首次出现以 target 稳态建（首帧不补间）。
    pub fn retarget(&mut self, key: K, target: f32, now: Instant) {
        self.map
            .entry(key)
            .or_insert_with(|| FloatTransition::new(target))
            .retarget(target, now);
    }

    pub fn sample(&self, key: &K, now: Instant) -> Option<f32> {
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

impl<K: Hash + Eq> Default for FloatTransitionMap<K> {
    fn default() -> Self {
        Self::new()
    }
}

// ── 一维弹簧（位移/宽度动画）────────────────────────────────────

/// 刚度与阻尼：微过阻尼（ζ≈1.05），~200ms 内视觉收敛；比临界略高以杀掉
/// 离散积分的亚像素微过冲（收敛尾部左右抖动的根源之一）。
const SPRING_STIFFNESS: f32 = 900.0;
const SPRING_DAMPING: f32 = 63.0;
/// 位置与速度双阈值判定收敛。带宽放到 1px/60px·s：指数尾巴在最后 1px 要爬
/// ~10 帧，绘制取整后表现为"先停稳再补 1px"（完成后的小抖）；提前落靶把
/// ≤1px 的落定融进运动末端，不可感。
const SPRING_SETTLE_DIST: f32 = 1.0;
const SPRING_SETTLE_VEL: f32 = 60.0;
/// 单帧推进的 dt 上限（长时间无渲染后不猛跳）。
const SPRING_MAX_DT: f32 = 0.05;
/// 积分子步长上限（半隐式欧拉的无条件稳定区）。
const SPRING_SUB_STEP: f32 = 0.010;

/// 一维弹簧动画：render 驱动 set_target + tick 驱动 step，自带 last_tick。
pub struct SpringAnim {
    value: f32,
    velocity: f32,
    target: f32,
    last_tick: Option<Instant>,
}

impl SpringAnim {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            last_tick: None,
        }
    }

    /// 瞬时到位并清速度（不该补间的场合）。
    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.velocity = 0.0;
        self.target = value;
        self.last_tick = None;
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn is_animating(&self) -> bool {
        (self.value - self.target).abs() > SPRING_SETTLE_DIST
            || self.velocity.abs() > SPRING_SETTLE_VEL
    }

    /// 推进到 now 并返回当前值（半隐式欧拉；收敛即精确落靶）。
    /// 长 dt 拆 ≤10ms 子步积分：k=900 时 ω·dt>1 单步会镜像过冲（事件驱动下
    /// 空闲后的首帧 dt 可达上限 50ms，曾导致底条"从屏幕边缘划入"）。
    pub fn sample(&mut self, now: Instant) -> f32 {
        let mut dt = self
            .last_tick
            .map(|t| now.saturating_duration_since(t).as_secs_f32())
            .unwrap_or(0.0)
            .min(SPRING_MAX_DT);
        self.last_tick = Some(now);
        while dt > 0.0 && self.is_animating() {
            let step = dt.min(SPRING_SUB_STEP);
            self.velocity += (SPRING_STIFFNESS * (self.target - self.value)
                - SPRING_DAMPING * self.velocity)
                * step;
            self.value += self.velocity * step;
            dt -= step;
        }
        if !self.is_animating() {
            self.value = self.target;
            self.velocity = 0.0;
        }
        self.value
    }
}

#[cfg(test)]
mod spring_tests {
    use super::*;
    use std::time::Duration;

    fn run(spring: &mut SpringAnim, ms: u64) -> (f32, f32) {
        let mut now = Instant::now();
        spring.sample(now); // 建立 last_tick
        let mut max_overshoot = 0.0_f32;
        for _ in 0..(ms / 16) {
            now += Duration::from_millis(16);
            let v = spring.sample(now);
            max_overshoot = max_overshoot.max(v - spring.target);
        }
        (spring.value, max_overshoot)
    }

    #[test]
    fn converges_and_settles_within_400ms() {
        let mut s = SpringAnim::new(0.0);
        s.set_target(240.0);
        let (v, _) = run(&mut s, 400);
        assert!((v - 240.0).abs() < 2.0, "value={v}");
        assert!(!s.is_animating());
    }

    #[test]
    fn overshoot_is_negligible() {
        let mut s = SpringAnim::new(0.0);
        s.set_target(240.0);
        let (_, overshoot) = run(&mut s, 400);
        assert!(overshoot < 2.0, "overshoot={overshoot}");
    }

    #[test]
    fn stale_tick_big_dt_never_overshoots() {
        // 还原 tab 底条 bug：空闲数秒后切 tab，首帧 dt 打到上限，
        // 单步欧拉曾镜像过冲到 old+2.25Δ（从屏幕边缘划入）。
        let mut s = SpringAnim::new(352.0);
        let t0 = Instant::now();
        s.sample(t0);
        s.set_target(152.0);
        let v = s.sample(t0 + Duration::from_secs(3));
        assert!((150.0..=352.0).contains(&v), "value={v} escaped [new, old]");
    }

    #[test]
    fn snap_clears_motion() {
        let mut s = SpringAnim::new(0.0);
        s.set_target(100.0);
        run(&mut s, 48);
        s.snap(100.0);
        assert!(!s.is_animating());
        assert_eq!(s.sample(Instant::now()), 100.0);
    }
}
