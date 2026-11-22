//! SSH 连接过渡层的纯函数：每个会话的淡入淡出状态机、进度条平滑、阶段切换过渡、计时文字。无 &self。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use nexshell::ssh_connect_progress::{ssh_connect_fraction, SshConnectProgress, SshConnectStage};
use warpui::color::ColorU;

const FADE_IN: Duration = Duration::from_millis(150);
const FADE_OUT: Duration = Duration::from_millis(220);
/// 超过这么久没画到（标签切走、离开终端页）就丢掉状态，切回来时重新淡入。
const STALE_AFTER: Duration = Duration::from_secs(1);
/// 最近画过的过渡层才驱动 UI tick；看不见的不空转。
const ANIMATING_WINDOW: Duration = Duration::from_millis(300);
/// 进度条追目标值的时间常数：切阶段时目标跳一段，显示值约 0.4 s 内滑过去。
const BAR_FOLLOW_SECS: f32 = 0.12;
/// 阶段切换时颜色、符号交叉淡变的时长。
const STAGE_FADE: Duration = Duration::from_millis(320);

pub(crate) struct ConnectOverlayAnim {
    /// 最近一次看到的连接进度；淡出时冻结在这一帧。
    progress: SshConnectProgress,
    shown_at: Instant,
    fade_from: Option<Instant>,
    last_rendered: Instant,
    /// 进度条当前显示的比例（平滑后），只增不减。
    bar: f32,
}

pub(crate) struct ConnectOverlayFrame {
    pub progress: SshConnectProgress,
    pub opacity: f32,
    pub bar: f32,
}

/// 阶段在清单里的样子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StageLook {
    Pending,
    Current,
    Done,
}

fn look_at(stage: SshConnectStage, current: usize) -> StageLook {
    match stage.index().cmp(&current) {
        std::cmp::Ordering::Less => StageLook::Done,
        std::cmp::Ordering::Equal => StageLook::Current,
        std::cmp::Ordering::Greater => StageLook::Pending,
    }
}

/// 这一帧某阶段从哪个样子过渡到哪个样子，及过渡进度 0..=1（已缓动）。
/// 阶段只会逐个前进，所以「上一刻」就是当前阶段的前一个。
pub(crate) fn stage_transition(
    stage: SshConnectStage,
    progress: &SshConnectProgress,
    now: Instant,
) -> (StageLook, StageLook, f32) {
    let current = progress.stage.index();
    let to = look_at(stage, current);
    let from = look_at(stage, current.saturating_sub(1));
    let k = ease_out(ratio(
        now.saturating_duration_since(progress.stage_started_at),
        STAGE_FADE,
    ));
    (from, to, k)
}

/// 两色按 k 线性混合（含透明度）。
pub(crate) fn mix_color(a: ColorU, b: ColorU, k: f32) -> ColorU {
    let k = k.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * k).round() as u8;
    ColorU::new(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b), mix(a.a, b.a))
}

/// 显示值按指数平滑追目标，`dt` 是距上一帧的时间；不倒退。
fn follow(shown: f32, target: f32, dt: Duration) -> f32 {
    if target <= shown {
        return shown;
    }
    let k = 1.0 - (-dt.as_secs_f32() / BAR_FOLLOW_SECS).exp();
    shown + (target - shown) * k
}

fn ease_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

fn ratio(elapsed: Duration, total: Duration) -> f32 {
    elapsed.as_secs_f32() / total.as_secs_f32()
}

/// 推进一个会话的过渡层：连接中则显示（首次出现淡入），进度消失则淡出，淡完返回 None。
pub(crate) fn step_connect_overlay(
    anims: &mut HashMap<String, ConnectOverlayAnim>,
    session_id: &str,
    progress: Option<&SshConnectProgress>,
    now: Instant,
) -> Option<ConnectOverlayFrame> {
    anims.retain(|_, anim| now.saturating_duration_since(anim.last_rendered) < STALE_AFTER);
    let anim = match (progress, anims.get_mut(session_id)) {
        (None, None) => return None,
        (Some(progress), None) => {
            anims
                .entry(session_id.to_string())
                .or_insert(ConnectOverlayAnim {
                    progress: progress.clone(),
                    shown_at: now,
                    fade_from: None,
                    last_rendered: now,
                    bar: ssh_connect_fraction(progress, now),
                })
        }
        (Some(progress), Some(anim)) => {
            anim.progress = progress.clone();
            anim.fade_from = None;
            anim
        }
        (None, Some(anim)) => {
            anim.fade_from.get_or_insert(now);
            anim
        }
    };
    let dt = now.saturating_duration_since(anim.last_rendered);
    anim.last_rendered = now;
    // 淡出时进度冻结，进度条也停在原处。
    if anim.fade_from.is_none() {
        let target = ssh_connect_fraction(&anim.progress, now).clamp(0.0, 1.0);
        anim.bar = follow(anim.bar, target, dt);
    }
    let fade_in = ease_out(ratio(now.saturating_duration_since(anim.shown_at), FADE_IN));
    let fade_out = match anim.fade_from {
        Some(from) => {
            let out = ratio(now.saturating_duration_since(from), FADE_OUT);
            if out >= 1.0 {
                anims.remove(session_id);
                return None;
            }
            1.0 - ease_out(out)
        }
        None => 1.0,
    };
    Some(ConnectOverlayFrame {
        progress: anim.progress.clone(),
        opacity: fade_in * fade_out,
        bar: anim.bar,
    })
}

/// 有过渡层正在显示（最近画过），UI tick 要继续跑。
pub(crate) fn connect_overlays_animating(
    anims: &HashMap<String, ConnectOverlayAnim>,
    now: Instant,
) -> bool {
    anims
        .values()
        .any(|anim| now.saturating_duration_since(anim.last_rendered) < ANIMATING_WINDOW)
}

/// 已耗时，一位小数秒：`1.2s`。
pub(crate) fn format_connect_elapsed(elapsed: Duration) -> String {
    format!("{:.1}s", elapsed.as_secs_f32())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(now: Instant) -> SshConnectProgress {
        SshConnectProgress::new("root@h:22".to_string(), now)
    }

    #[test]
    fn fades_in_then_out_and_forgets_the_session() {
        let t0 = Instant::now();
        let mut anims = HashMap::new();
        let p = progress(t0);
        assert!(step_connect_overlay(&mut anims, "s1", None, t0).is_none());
        let first = step_connect_overlay(&mut anims, "s1", Some(&p), t0).unwrap();
        assert_eq!(first.opacity, 0.0);
        let shown = step_connect_overlay(&mut anims, "s1", Some(&p), t0 + FADE_IN).unwrap();
        assert_eq!(shown.opacity, 1.0);
        // 进度消失：开始淡出，冻结在最后的进度。
        let t1 = t0 + Duration::from_millis(500);
        let fading = step_connect_overlay(&mut anims, "s1", None, t1).unwrap();
        assert_eq!(fading.opacity, 1.0);
        assert_eq!(fading.progress, p);
        let mid = step_connect_overlay(&mut anims, "s1", None, t1 + FADE_OUT / 2).unwrap();
        assert!(mid.opacity > 0.0 && mid.opacity < 1.0);
        assert!(step_connect_overlay(&mut anims, "s1", None, t1 + FADE_OUT).is_none());
        assert!(anims.is_empty());
    }

    #[test]
    fn stale_sessions_are_dropped_and_stop_the_tick() {
        let t0 = Instant::now();
        let mut anims = HashMap::new();
        let p = progress(t0);
        step_connect_overlay(&mut anims, "hidden", Some(&p), t0);
        assert!(connect_overlays_animating(&anims, t0));
        assert!(!connect_overlays_animating(&anims, t0 + ANIMATING_WINDOW));
        // 别的会话一画，过期的那个被清掉。
        step_connect_overlay(&mut anims, "visible", Some(&p), t0 + STALE_AFTER);
        assert!(!anims.contains_key("hidden"));
    }

    #[test]
    fn bar_slides_instead_of_jumping_on_stage_change() {
        let t0 = Instant::now();
        let mut anims = HashMap::new();
        let mut p = progress(t0);
        let mut t = t0;
        let mut last = 0.0;
        // 停在 TCP 1 秒，再进入身份验证：目标跳一段，显示值逐帧追上，不一步到位、不倒退。
        for frame in 0..120 {
            t = t0 + Duration::from_millis(16 * frame);
            if frame == 60 {
                p.advance(SshConnectStage::Auth, t);
            }
            let bar = step_connect_overlay(&mut anims, "s1", Some(&p), t)
                .unwrap()
                .bar;
            assert!(bar >= last);
            if frame == 61 {
                assert!(bar - last < 0.1, "切阶段后一帧跳了 {}", bar - last);
            }
            last = bar;
        }
        let target = ssh_connect_fraction(&p, t);
        assert!((target - last).abs() < 0.02);
    }

    #[test]
    fn stage_transition_hands_current_to_next() {
        let t0 = Instant::now();
        let mut p = progress(t0);
        let t1 = t0 + Duration::from_secs(1);
        p.advance(SshConnectStage::Auth, t1);
        let (from, to, k) = stage_transition(SshConnectStage::Tcp, &p, t1);
        assert_eq!((from, to, k), (StageLook::Current, StageLook::Done, 0.0));
        let (from, to, _) = stage_transition(SshConnectStage::Auth, &p, t1);
        assert_eq!((from, to), (StageLook::Pending, StageLook::Current));
        let (_, _, k) = stage_transition(SshConnectStage::Shell, &p, t1 + STAGE_FADE);
        assert_eq!(k, 1.0);
    }

    #[test]
    fn elapsed_has_one_decimal() {
        assert_eq!(format_connect_elapsed(Duration::from_millis(1234)), "1.2s");
    }
}
