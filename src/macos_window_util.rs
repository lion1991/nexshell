//! macOS 原生窗口辅助：窗口 alpha / 层级 / 系统 About 面板。
//! 按 ADR step 10 从 main.rs 内联 mod 独立成文件；整体在 `#[cfg(target_os = "macos")]` 下编译。

use cocoa::base::{id, nil};
use objc::{msg_send, sel, sel_impl};

// NSFloatingWindowLevel = 3 (CGWindowLevelForKey(kCGFloatingWindowLevelKey))
const NS_FLOATING_WINDOW_LEVEL: i64 = 3;
const NS_NORMAL_WINDOW_LEVEL: i64 = 0;

pub fn set_window_alpha(alpha: f64) {
    unsafe {
        let app: id = cocoa::appkit::NSApp();
        let key_win: id = msg_send![app, keyWindow];
        if key_win != nil {
            let _: () = msg_send![key_win, setAlphaValue: alpha];
        }
    }
}

/// 降回普通窗口层级
pub fn reset_window_level() {
    unsafe {
        let app: id = cocoa::appkit::NSApp();
        let key_win: id = msg_send![app, keyWindow];
        if key_win != nil {
            let _: () = msg_send![key_win, setLevel: NS_NORMAL_WINDOW_LEVEL];
        }
    }
}

/// 提升 keyWindow 层级，使其始终在普通窗口之上
pub fn raise_window_level() {
    unsafe {
        let app: id = cocoa::appkit::NSApp();
        let key_win: id = msg_send![app, keyWindow];
        if key_win != nil {
            let _: () = msg_send![key_win, setLevel: NS_FLOATING_WINDOW_LEVEL];
        }
    }
}

/// 显示 macOS 原生 About 面板
pub fn show_about_panel() {
    use cocoa::foundation::{NSDictionary, NSString};
    unsafe {
        let app: id = cocoa::appkit::NSApp();
        let keys = vec![
            NSString::alloc(nil).init_str("ApplicationName"),
            NSString::alloc(nil).init_str("ApplicationVersion"),
            NSString::alloc(nil).init_str("Copyright"),
        ];
        let vals = vec![
            NSString::alloc(nil).init_str("NexShell"),
            NSString::alloc(nil).init_str(env!("CARGO_PKG_VERSION")),
            NSString::alloc(nil).init_str("© 2025-2026 Matt"),
        ];
        let options = NSDictionary::dictionaryWithObjects_forKeys_(
            nil,
            cocoa::foundation::NSArray::arrayWithObjects(nil, &vals),
            cocoa::foundation::NSArray::arrayWithObjects(nil, &keys),
        );
        let _: () = msg_send![app, orderFrontStandardAboutPanelWithOptions: options];
    }
}
