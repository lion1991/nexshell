//! SSH 连接过渡层的纯函数：每个会话的淡入淡出状态机、计时文字。无 &self。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use nexshell::ssh_connect_progress::SshConnectProgress;

const FADE_IN: Duration = Duration::from_millis(150);
const FADE_OUT: Duration = Duration::from_millis(220);
/// 超过这么久没画到（标签切走、离开终端页）就丢掉状态，切回来时重新淡入。
const STALE_AFTER: Duration = Duration::from_secs(1);
/// 最近画过的过渡层才驱动 UI tick；看不见的不空转。
const ANIMATING_WINDOW: Duration = Duration::from_millis(300);

pub(crate) struct ConnectOverlayAnim {
    /// 最近一次看到的连接进度；淡出时冻结在这一帧。
    progress: SshConnectProgress,
    shown_at: Instant,
    fade_from: Option<Instant>,
    last_rendered: Instant,
}

pub(crate) struct ConnectOverlayFrame {
    pub progress: SshConnectProgress,
    pub opacity: f32,
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
    anim.last_rendered = now;
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
    fn elapsed_has_one_decimal() {
        assert_eq!(format_connect_elapsed(Duration::from_millis(1234)), "1.2s");
    }
}
