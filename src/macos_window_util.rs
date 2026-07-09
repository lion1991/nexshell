//! macOS 原生窗口辅助：窗口 alpha / 层级 / 系统 About 面板。
//! 按 ADR step 10 从 main.rs 内联 mod 独立成文件；整体在 `#[cfg(target_os = "macos")]` 下编译。

use block::ConcreteBlock;
use cocoa::base::{id, nil, BOOL, NO};
use cocoa::foundation::NSString;
use objc::runtime::Class;
use objc::{msg_send, sel, sel_impl};
use std::sync::Once;

// NSFloatingWindowLevel = 3 (CGWindowLevelForKey(kCGFloatingWindowLevelKey))
const NS_FLOATING_WINDOW_LEVEL: i64 = 3;
const NS_NORMAL_WINDOW_LEVEL: i64 = 0;
const NS_WORKSPACE_ACCESSIBILITY_DISPLAY_OPTIONS_DID_CHANGE_NOTIFICATION: &str =
    "NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification";

static INSTALL_REDUCE_TRANSPARENCY_OBSERVER: Once = Once::new();

fn ns_workspace() -> Option<id> {
    unsafe {
        let cls = Class::get("NSWorkspace")?;
        let workspace: id = msg_send![cls, sharedWorkspace];
        if workspace == nil {
            None
        } else {
            Some(workspace)
        }
    }
}

pub fn accessibility_display_should_reduce_transparency() -> Option<bool> {
    unsafe {
        let workspace = ns_workspace()?;
        let responds: BOOL = msg_send![workspace, respondsToSelector: sel!(accessibilityDisplayShouldReduceTransparency)];
        if responds == NO {
            return None;
        }

        let should_reduce: BOOL =
            msg_send![workspace, accessibilityDisplayShouldReduceTransparency];
        Some(should_reduce != NO)
    }
}

fn sync_reduce_transparency_to_glass() {
    if let Some(enabled) = accessibility_display_should_reduce_transparency() {
        nexshell::glass_backdrop::set_reduce_transparency_enabled(enabled);
    }
}

pub fn install_reduce_transparency_observer() {
    INSTALL_REDUCE_TRANSPARENCY_OBSERVER.call_once(|| unsafe {
        sync_reduce_transparency_to_glass();

        let Some(workspace) = ns_workspace() else {
            return;
        };
        let center: id = msg_send![workspace, notificationCenter];
        if center == nil {
            return;
        }

        let name = NSString::alloc(nil)
            .init_str(NS_WORKSPACE_ACCESSIBILITY_DISPLAY_OPTIONS_DID_CHANGE_NOTIFICATION);
        let block = ConcreteBlock::new(|_: id| {
            sync_reduce_transparency_to_glass();
        })
        .copy();
        let _observer: id = msg_send![
            center,
            addObserverForName: name
            object: nil
            queue: nil
            usingBlock: &*block
        ];

        // The observer is process-lifetime; keep the copied block alive for it.
        std::mem::forget(block);
    });
}

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
