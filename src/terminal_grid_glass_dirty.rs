use std::collections::HashMap;

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminalGlassContentFingerprint {
    cols: usize,
    rows: usize,
    display_offset: usize,
    history_size: usize,
    line_hash: u64,
}

impl TerminalGlassContentFingerprint {
    pub(crate) fn from_visible_lines<'a>(
        cols: usize,
        rows: usize,
        display_offset: usize,
        history_size: usize,
        lines: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let mut line_hash = FNV_OFFSET;
        for line in lines {
            line_hash = hash_usize(line_hash, line.len());
            for byte in line.as_bytes() {
                line_hash = hash_byte(line_hash, *byte);
            }
            line_hash = hash_byte(line_hash, 0xff);
        }
        Self {
            cols,
            rows,
            display_offset,
            history_size,
            line_hash,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct TerminalGlassDirtyTracker {
    fingerprints: HashMap<String, TerminalGlassContentFingerprint>,
}

impl TerminalGlassDirtyTracker {
    pub(crate) fn did_content_change(
        &mut self,
        key: &str,
        fingerprint: TerminalGlassContentFingerprint,
    ) -> bool {
        self.fingerprints
            .insert(key.to_string(), fingerprint)
            .map_or(false, |previous| previous != fingerprint)
    }
}

fn hash_byte(hash: u64, byte: u8) -> u64 {
    (hash ^ byte as u64).wrapping_mul(FNV_PRIME)
}

fn hash_usize(mut hash: u64, value: usize) -> u64 {
    for byte in (value as u64).to_le_bytes() {
        hash = hash_byte(hash, byte);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{TerminalGlassContentFingerprint, TerminalGlassDirtyTracker};

    #[derive(Clone)]
    struct TestPaintState<'a> {
        cols: usize,
        rows: usize,
        display_offset: usize,
        history_size: usize,
        lines: Vec<&'a str>,
        cursor_blink_visible: bool,
        selection_active: bool,
        ime_marked_text: Option<&'a str>,
    }

    impl<'a> TestPaintState<'a> {
        fn fingerprint(&self) -> TerminalGlassContentFingerprint {
            TerminalGlassContentFingerprint::from_visible_lines(
                self.cols,
                self.rows,
                self.display_offset,
                self.history_size,
                self.lines.iter().copied(),
            )
        }
    }

    fn state<'a>(lines: Vec<&'a str>) -> TestPaintState<'a> {
        TestPaintState {
            cols: 4,
            rows: 2,
            display_offset: 0,
            history_size: 10,
            lines,
            cursor_blink_visible: true,
            selection_active: false,
            ime_marked_text: None,
        }
    }

    #[test]
    fn terminal_glass_dirty_tracker_marks_content_changes_dirty() {
        let mut tracker = TerminalGlassDirtyTracker::default();
        let first = state(vec!["abcd", "efgh"]);
        let second = state(vec!["abcd", "zzzz"]);

        assert!(!tracker.did_content_change("pane-1", first.fingerprint()));
        assert!(tracker.did_content_change("pane-1", second.fingerprint()));
    }

    #[test]
    fn terminal_glass_dirty_tracker_ignores_cursor_selection_and_ime_overlays() {
        let mut tracker = TerminalGlassDirtyTracker::default();
        let first = state(vec!["prompt", ""]);
        let mut second = first.clone();
        second.cursor_blink_visible = false;
        second.selection_active = true;
        second.ime_marked_text = Some("pin");

        assert!(!tracker.did_content_change("pane-1", first.fingerprint()));
        assert!(!tracker.did_content_change("pane-1", second.fingerprint()));
    }

    #[test]
    fn terminal_glass_dirty_tracker_marks_scroll_changes_dirty() {
        let mut tracker = TerminalGlassDirtyTracker::default();
        let first = state(vec!["line1", "line2"]);
        let mut second = first.clone();
        second.display_offset = 1;

        assert!(!tracker.did_content_change("pane-1", first.fingerprint()));
        assert!(tracker.did_content_change("pane-1", second.fingerprint()));
    }

    #[test]
    fn terminal_glass_dirty_tracker_marks_resize_dirty() {
        let mut tracker = TerminalGlassDirtyTracker::default();
        let first = state(vec!["line1", "line2"]);
        let mut second = first.clone();
        second.cols = 8;
        second.rows = 3;

        assert!(!tracker.did_content_change("pane-1", first.fingerprint()));
        assert!(tracker.did_content_change("pane-1", second.fingerprint()));
    }

    #[test]
    fn terminal_glass_dirty_tracker_keeps_unchanged_frames_clean() {
        let mut tracker = TerminalGlassDirtyTracker::default();
        let first = state(vec!["same", "content"]);
        let second = first.clone();

        assert!(!tracker.did_content_change("pane-1", first.fingerprint()));
        assert!(!tracker.did_content_change("pane-1", second.fingerprint()));
    }
}
