//! bridge worker 的纯函数状态机：事件 → 标脏、合并拉取的时机、重连退避。
//! 全是无 I/O 的判定，单测直接驱动这些函数。

use std::time::{Duration, Instant};

use super::protocol::{EventEnvelope, SUBSCRIPTIONS};

pub const RECONNECT_MIN: Duration = Duration::from_secs(1);
pub const RECONNECT_MAX: Duration = Duration::from_secs(30);
/// 会话活过这么久才算「稳定」，退避才归零；否则服务端 flap 会被 1Hz 猛敲。
pub const STABLE_SESSION: Duration = Duration::from_secs(10);
/// 两次 `session.snapshot` 之间的硬下限。herdr 事件很吵（pane_updated 连 echo
/// 都触发），缓冲合并挡不住「稀稀拉拉持续到达」的情况，再加一道频率闸。
pub const MIN_FETCH_INTERVAL: Duration = Duration::from_millis(100);

/// 事件是不是我们订的那些。订阅名是点号（`workspace.focused`），推送信封是
/// 下划线（`workspace_focused`），这里按下划线比。
///
/// 不解析事件负载：herdr 焦点是 per-client 的，事件里的 `focused_*` 未必是
/// 本 client 的，一律只当「session 可能变了」的信号，然后重拉 snapshot。
pub fn event_marks_dirty(event: &EventEnvelope) -> bool {
    let kind = event.kind();
    SUBSCRIPTIONS
        .iter()
        .any(|subscription| subscription.replace('.', "_") == kind)
}

/// 现在该不该拉 snapshot。三个条件：有脏标记、这一批事件已读空、且距上次拉取
/// 已过 `MIN_FETCH_INTERVAL`。
///
/// 缓冲合并管的是「同一批连发」（切 workspace 实测 workspace_focused +
/// tab_focused 同批到达）；频率闸管的是「持续稀疏到达」。未到间隔时调用方会
/// 睡掉剩余时间再拉，期间到达的事件继续并进同一次拉取。
pub fn should_fetch_now(dirty: bool, more_buffered: bool, since_last_fetch: Duration) -> bool {
    dirty && !more_buffered && since_last_fetch >= MIN_FETCH_INTERVAL
}

/// 算下一次重连前要等多久。`prev` 是上一次真正睡过的时长（`None` = 还没退避过），
/// `session_opened_at` 是刚结束那次会话建立订阅的时刻（`None` = 压根没连上）。
///
/// 只有连上并稳定活过 `STABLE_SESSION` 才归零，否则翻倍到 `RECONNECT_MAX` 封顶：
/// 1s → 2s → 4s → 8s → 16s → 30s → 30s …
pub fn next_backoff(
    prev: Option<Duration>,
    session_opened_at: Option<Instant>,
    now: Instant,
) -> Duration {
    let stable = session_opened_at.is_some_and(|at| now.duration_since(at) >= STABLE_SESSION);
    if stable {
        return RECONNECT_MIN;
    }
    match prev {
        None => RECONNECT_MIN,
        Some(prev) => (prev * 2).min(RECONNECT_MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str) -> EventEnvelope {
        EventEnvelope {
            event: kind.to_string(),
            data_type: kind.to_string(),
        }
    }

    fn ago(secs: u64) -> Instant {
        Instant::now()
            .checked_sub(Duration::from_secs(secs))
            .expect("instant in range")
    }

    /// 关键回归：TUI 切 workspace 只发 workspace_focused + tab_focused，
    /// 不发 pane_focused。这两条必须能标脏。
    #[test]
    fn subscribed_events_mark_the_session_dirty() {
        for kind in [
            "workspace_focused",
            "workspace_created",
            "workspace_updated",
            "workspace_renamed",
            "workspace_closed",
            "tab_focused",
            "tab_created",
            "tab_closed",
            "pane_focused",
            "pane_updated",
            "pane_created",
            "pane_closed",
            "layout_updated",
        ] {
            assert!(event_marks_dirty(&event(kind)), "{kind} should mark dirty");
        }
    }

    #[test]
    fn unrelated_events_do_not_mark_dirty() {
        for kind in [
            "pane_agent_status_changed",
            "pane_output_matched",
            "subscription_started",
            "",
        ] {
            assert!(!event_marks_dirty(&event(kind)), "{kind} should be ignored");
        }
    }

    /// data.type 缺失时按信封 event 字段判定。
    #[test]
    fn dirty_check_falls_back_to_the_envelope_event_name() {
        let ev = EventEnvelope {
            event: "workspace_focused".to_string(),
            data_type: String::new(),
        };
        assert!(event_marks_dirty(&ev));
    }

    /// 间隔早已满足时，只看脏标记与缓冲。
    const LONG_AGO: Duration = Duration::from_secs(5);

    #[test]
    fn fetch_is_coalesced_while_more_lines_are_buffered() {
        // 第一条焦点事件到达，缓冲里还压着后续事件 → 先不拉。
        assert!(!should_fetch_now(true, true, LONG_AGO));
        // 读空了才拉，这一批只拉一次。
        assert!(should_fetch_now(true, false, LONG_AGO));
        // 没有脏标记就永远不拉。
        assert!(!should_fetch_now(false, false, LONG_AGO));
        assert!(!should_fetch_now(false, true, LONG_AGO));
    }

    #[test]
    fn fetch_respects_the_minimum_interval() {
        // 刚拉过 → 即使脏且缓冲读空也先等。
        assert!(!should_fetch_now(true, false, Duration::ZERO));
        assert!(!should_fetch_now(
            true,
            false,
            MIN_FETCH_INTERVAL - Duration::from_millis(1)
        ));
        // 正好到点即可拉。
        assert!(should_fetch_now(true, false, MIN_FETCH_INTERVAL));
        assert!(should_fetch_now(
            true,
            false,
            MIN_FETCH_INTERVAL + Duration::from_millis(1)
        ));
        // 间隔到了也救不了「没脏」或「还有缓冲」。
        assert!(!should_fetch_now(false, false, LONG_AGO));
        assert!(!should_fetch_now(true, true, LONG_AGO));
    }

    #[test]
    fn backoff_doubles_from_one_second_to_the_cap() {
        let now = Instant::now();
        let mut prev = None;
        let mut seen = Vec::new();
        for _ in 0..8 {
            let delay = next_backoff(prev, None, now);
            seen.push(delay.as_secs());
            prev = Some(delay);
        }
        assert_eq!(seen, vec![1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[test]
    fn stable_session_resets_backoff_to_one_second() {
        assert_eq!(
            next_backoff(Some(Duration::from_secs(16)), Some(ago(11)), Instant::now()),
            RECONNECT_MIN
        );
        assert_eq!(
            next_backoff(Some(RECONNECT_MAX), Some(ago(600)), Instant::now()),
            RECONNECT_MIN
        );
    }

    #[test]
    fn flapping_session_keeps_growing_backoff() {
        assert_eq!(
            next_backoff(Some(Duration::from_secs(4)), Some(ago(2)), Instant::now()),
            Duration::from_secs(8)
        );
        assert_eq!(
            next_backoff(Some(Duration::from_secs(8)), Some(ago(9)), Instant::now()),
            Duration::from_secs(16)
        );
        // 正好 10s 才算稳定。
        assert_eq!(
            next_backoff(Some(Duration::from_secs(8)), Some(ago(10)), Instant::now()),
            RECONNECT_MIN
        );
    }
}
