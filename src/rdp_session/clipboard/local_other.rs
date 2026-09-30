//! 非 macOS 本地剪贴板：arboard 只同步文本，变化标记取文本 hash。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use arboard::Clipboard;

use super::{Available, Bitmap};

pub(super) const POLL_INTERVAL: Duration = Duration::from_secs(1);

pub(super) fn change_token() -> Option<u64> {
    let text = read_text()?;
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    Some(hasher.finish())
}

pub(super) fn available() -> Available {
    Available {
        text: read_text().is_some_and(|t| !t.is_empty()),
        ..Available::default()
    }
}

pub(super) fn read_text() -> Option<String> {
    Clipboard::new().ok()?.get_text().ok()
}

pub(super) fn write_text(text: &str) -> bool {
    Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_owned()))
        .is_ok()
}

pub(super) fn read_rtf() -> Option<Vec<u8>> {
    None
}

pub(super) fn read_png() -> Option<Vec<u8>> {
    None
}

pub(super) fn read_bitmap() -> Option<Bitmap> {
    None
}
