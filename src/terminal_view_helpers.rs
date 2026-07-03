//! 终端视图纯函数 helper：标签 / 断连提示、串口、光标闪烁、分屏 header、overlay dispatch 等。
//! 另含按附录 B 暂置于此的通用 / 跨面板 helper：optional_text（通用）、close_button_element（tab_bar）、
//! find_match_label（find）——待各面板触发二次拆条件时再挪出。
//! 无 &self；按 ADR step 10（附录 B）从 main.rs 抽出。详见 docs/adr/0001-root-view-multi-file-impl.md。

use std::sync::{Arc, Mutex};
use std::time::Instant;

use warpui::color::ColorU;
use warpui::elements::{
    Align, ConstrainedBox, Container, CornerRadius, Empty, EventDispatchMode, Hoverable, Icon,
    MouseStateHandle, ParentOffsetBounds, Radius, SavePosition,
};
use warpui::{platform, Element};

use crate::terminal_grid_element::TerminalGridAction;
use crate::{
    CursorBlinkState, TerminalSessionKind, TerminalSessionTab, DEFAULT_WINDOW_TITLE,
    ICON_BUTTON_SIZE, ICON_PATH_CLOSE, ICON_PATH_CLOUD, ICON_PATH_FOLDER, TAB_CLOSE_BUTTON_WIDTH,
    TERMINAL_CURSOR_BLINK_INTERVAL,
};
use nexshell::host_management::HostConnectionConfig;
use nexshell::terminal_runtime::{LocalTerminalRuntime, TerminalPalette};

pub(crate) fn terminal_keyboard_input_enabled(
    file_panel_input_active: bool,
    overlay_editor_focused: bool,
) -> bool {
    !file_panel_input_active && !overlay_editor_focused
}

pub(crate) fn terminal_tab_original_label(
    kind: TerminalSessionKind,
    fallback_label: &str,
    runtime_title: Option<&str>,
) -> String {
    let has_fallback = !fallback_label.trim().is_empty();
    if matches!(
        kind,
        TerminalSessionKind::Remote | TerminalSessionKind::Serial
    ) && has_fallback
    {
        return fallback_label.to_string();
    }

    let title = runtime_title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(ToOwned::to_owned);
    title.unwrap_or_else(|| {
        if has_fallback {
            fallback_label.to_string()
        } else {
            kind.default_label().to_string()
        }
    })
}

pub(crate) fn terminal_disconnected_notice_text(
    kind: TerminalSessionKind,
    connected: bool,
    status: &str,
) -> Option<String> {
    if connected {
        return None;
    }

    let status = status.trim();
    let title = match kind {
        TerminalSessionKind::Remote => "远程连接已断开",
        TerminalSessionKind::Serial => "串口连接已断开",
        TerminalSessionKind::Local
        | TerminalSessionKind::Direct
        | TerminalSessionKind::ProcessList
        | TerminalSessionKind::NetworkList
        | TerminalSessionKind::SystemInfo
        | TerminalSessionKind::GitDiff
        | TerminalSessionKind::CodeViewer
        | TerminalSessionKind::Rdp => return None,
    };

    Some(if status.is_empty() {
        title.to_string()
    } else {
        format!("{title}\n{status}")
    })
}

pub(crate) fn inactive_terminal_runtime() -> Arc<Mutex<LocalTerminalRuntime>> {
    Arc::new(Mutex::new(LocalTerminalRuntime::failed(
        "inactive",
        "no active terminal",
    )))
}

fn normalized_serial_port(port: &str) -> Option<String> {
    let port = port.trim();
    (!port.is_empty()).then(|| port.to_string())
}

pub(crate) fn serial_port_from_host_config(config: &HostConnectionConfig) -> Option<String> {
    normalized_serial_port(
        config
            .serial_port
            .as_deref()
            .unwrap_or(config.host.as_str()),
    )
}

pub(crate) fn connected_serial_tab_port(tab: &TerminalSessionTab) -> Option<&str> {
    let connected = tab
        .terminal
        .lock()
        .map(|runtime| runtime.snapshot().connected)
        .unwrap_or(false);
    connected.then_some(tab.serial_port.as_deref()).flatten()
}

pub(crate) fn occupied_serial_port_index<'a>(
    open_ports: impl IntoIterator<Item = Option<&'a str>>,
    candidate_port: &str,
    skip_index: Option<usize>,
) -> Option<usize> {
    let candidate = normalized_serial_port(candidate_port)?;
    open_ports
        .into_iter()
        .enumerate()
        .find_map(|(index, port)| {
            if skip_index == Some(index) {
                return None;
            }
            let port = normalized_serial_port(port?)?;
            (port == candidate).then_some(index)
        })
}

pub(crate) fn terminal_palette_ansi_color(palette: &TerminalPalette, index: usize) -> ColorU {
    let rgb = palette.ansi[index];
    ColorU::new(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
        0xff,
    )
}

pub(crate) fn optional_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

// Close button(× icon),hover 时背景提亮 ── 对照 warp tab.rs:984-1062 / icon_button。
pub(crate) fn close_button_element(
    state: MouseStateHandle,
    tab_index: usize,
    tab_position_id: String,
    is_visible: bool,
    close_action: TerminalGridAction,
    close_bg_default: ColorU,
    close_bg_hover: ColorU,
    icon_active: ColorU,
) -> Box<dyn Element> {
    if !is_visible {
        return ConstrainedBox::new(Empty::new().finish())
            .with_width(ICON_BUTTON_SIZE)
            .with_height(ICON_BUTTON_SIZE)
            .finish();
    }

    let close_position_id = format!("nexshell_close_tab_button:{tab_index}");
    Align::new(
        SavePosition::new(
            Hoverable::new(state, move |hover| {
                let bg = if hover.is_hovered() {
                    close_bg_hover
                } else {
                    close_bg_default
                };
                Container::new(
                    ConstrainedBox::new(
                        Align::new(
                            ConstrainedBox::new(Icon::new(ICON_PATH_CLOSE, icon_active).finish())
                                .with_width(12.0)
                                .with_height(12.0)
                                .finish(),
                        )
                        .finish(),
                    )
                    .with_width(TAB_CLOSE_BUTTON_WIDTH)
                    .with_height(TAB_CLOSE_BUTTON_WIDTH)
                    .finish(),
                )
                .with_background_color(bg)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(2.0)))
                .finish()
            })
            .on_hover(move |is_hover, ctx, _, _| {
                if is_hover {
                    if let Some(rect) = ctx.element_position_by_id(&tab_position_id) {
                        ctx.dispatch_typed_action(TerminalGridAction::TabHoverWidthStart {
                            width: rect.width(),
                        });
                    }
                } else {
                    ctx.dispatch_typed_action(TerminalGridAction::TabHoverWidthEnd);
                }
            })
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::TabHoverWidthEnd);
                ctx.dispatch_typed_action(close_action.clone());
            })
            .finish(),
            &close_position_id,
        )
        .finish(),
    )
    .finish()
}

pub(crate) fn find_match_label(match_count: usize, current: Option<usize>) -> String {
    if match_count == 0 {
        "0 / 0".to_string()
    } else {
        format!(
            "{} / {}",
            current.map(|idx| idx + 1).unwrap_or(0),
            match_count
        )
    }
}

pub(crate) fn update_cursor_blink(
    state: &mut CursorBlinkState,
    cursor_blinking: bool,
    now: Instant,
) -> bool {
    if !cursor_blinking {
        let changed = !state.phase_visible || state.last_toggled_at.is_some();
        state.phase_visible = true;
        state.last_toggled_at = None;
        return changed;
    }

    let Some(last_toggled_at) = state.last_toggled_at else {
        state.last_toggled_at = Some(now);
        return false;
    };

    if now.duration_since(last_toggled_at) < TERMINAL_CURSOR_BLINK_INTERVAL {
        return false;
    }

    state.phase_visible = !state.phase_visible;
    state.last_toggled_at = Some(now);
    true
}

pub(crate) fn terminal_window_title(title: Option<&str>) -> &str {
    title
        .filter(|title| !title.is_empty())
        .unwrap_or(DEFAULT_WINDOW_TITLE)
}

pub(crate) fn terminal_context_menu_offset_bounds() -> ParentOffsetBounds {
    ParentOffsetBounds::WindowByPosition
}

pub(crate) fn root_overlay_event_dispatch_mode() -> EventDispatchMode {
    EventDispatchMode::Waterfall
}

pub(crate) fn terminal_overlay_event_dispatch_mode() -> EventDispatchMode {
    EventDispatchMode::Waterfall
}

pub(crate) fn root_debug_key_log(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("NEXSHELL_DEBUG_KEYS").is_some() {
        eprintln!("[nexshell key-debug] {args}");
    }
}

fn shorten_path_for_badge(title: &str) -> String {
    let path = title.trim();
    if path.is_empty() {
        return "~".to_string();
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = home.to_string_lossy().into_owned();
        if let Some(rest) = path.strip_prefix(&home) {
            let short = format!("~{rest}");
            return truncate_path_display(&short, 30);
        }
    }
    truncate_path_display(path, 30)
}

pub(crate) fn split_pane_header_badge_title(
    runtime_title: Option<&str>,
    fallback_label: &str,
    kind: TerminalSessionKind,
) -> String {
    if let Some(title) = runtime_title
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        return shorten_path_for_badge(title);
    }

    let fallback = fallback_label.trim();
    if matches!(
        kind,
        TerminalSessionKind::Remote | TerminalSessionKind::Serial
    ) && !fallback.is_empty()
    {
        return truncate_path_display(fallback, 30);
    }

    shorten_path_for_badge("")
}

pub(crate) fn split_pane_header_badge_icon(kind: TerminalSessionKind) -> &'static str {
    match kind {
        // RDP 亦为远程主机，与 Remote 同用云图标（RDP 整页不参与分屏，取值仅备用）。
        TerminalSessionKind::Remote | TerminalSessionKind::Rdp => ICON_PATH_CLOUD,
        TerminalSessionKind::Local
        | TerminalSessionKind::Serial
        | TerminalSessionKind::Direct
        | TerminalSessionKind::ProcessList
        | TerminalSessionKind::NetworkList
        | TerminalSessionKind::SystemInfo
        | TerminalSessionKind::GitDiff
        | TerminalSessionKind::CodeViewer => ICON_PATH_FOLDER,
    }
}

pub(crate) fn terminal_tab_kind_uses_side_panel_layout(kind: TerminalSessionKind) -> bool {
    matches!(
        kind,
        TerminalSessionKind::Local
            | TerminalSessionKind::Remote
            | TerminalSessionKind::Serial
            | TerminalSessionKind::Direct
            | TerminalSessionKind::GitDiff
            | TerminalSessionKind::CodeViewer
    )
}

pub(crate) fn split_pane_header_background_color(
    theme: &warp_core::ui::theme::WarpTheme,
) -> ColorU {
    let color = theme.background().into_solid();
    ColorU::new(color.r, color.g, color.b, 0xff)
}

fn truncate_path_display(path: &str, max_len: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_len {
        return path.to_string();
    }
    let suffix: String = path.chars().skip(char_count - (max_len - 1)).collect();
    format!("…{suffix}")
}

pub(crate) fn terminal_clear_key_binding() -> &'static str {
    if platform::OperatingSystem::get().is_mac() {
        "cmd-k"
    } else {
        "ctrl-shift-K"
    }
}

#[cfg(test)]
mod tests {
    use super::{
        split_pane_header_background_color, split_pane_header_badge_icon,
        split_pane_header_badge_title, terminal_context_menu_offset_bounds,
        terminal_disconnected_notice_text, terminal_tab_original_label, terminal_window_title,
        update_cursor_blink, CursorBlinkState, EventDispatchMode, ParentOffsetBounds,
        TerminalSessionKind, DEFAULT_WINDOW_TITLE, ICON_PATH_CLOUD, ICON_PATH_FOLDER,
        TERMINAL_CURSOR_BLINK_INTERVAL,
    };
    use crate::terminal_grid_element::ThemeChoice;
    use std::time::{Duration, Instant};

    #[test]
    fn cursor_blink_toggles_at_interval_and_resets_when_disabled() {
        let now = Instant::now();
        let mut state = CursorBlinkState::default();

        assert!(!update_cursor_blink(&mut state, true, now));
        assert!(!update_cursor_blink(
            &mut state,
            true,
            now + TERMINAL_CURSOR_BLINK_INTERVAL / 2
        ));

        assert!(update_cursor_blink(
            &mut state,
            true,
            now + TERMINAL_CURSOR_BLINK_INTERVAL + Duration::from_millis(1)
        ));

        assert!(update_cursor_blink(
            &mut state,
            false,
            now + TERMINAL_CURSOR_BLINK_INTERVAL + Duration::from_millis(2)
        ));
    }

    #[test]
    fn terminal_window_title_uses_runtime_title_or_default() {
        assert_eq!(terminal_window_title(Some("vim main.rs")), "vim main.rs");
        assert_eq!(terminal_window_title(Some("")), DEFAULT_WINDOW_TITLE);
        assert_eq!(terminal_window_title(None), DEFAULT_WINDOW_TITLE);
    }

    #[test]
    fn remote_terminal_tab_label_prefers_connection_label_over_runtime_title() {
        assert_eq!(
            terminal_tab_original_label(
                TerminalSessionKind::Remote,
                "Production SSH",
                Some("vm-b69q0h1e3eq5")
            ),
            "Production SSH"
        );
    }

    #[test]
    fn serial_terminal_tab_label_prefers_connection_label_over_runtime_title() {
        assert_eq!(
            terminal_tab_original_label(
                TerminalSessionKind::Serial,
                "USB Console",
                Some("opening serial: /dev/cu.usbserial @ 115200")
            ),
            "USB Console"
        );
    }

    #[test]
    fn local_terminal_tab_label_still_prefers_runtime_title() {
        assert_eq!(
            terminal_tab_original_label(TerminalSessionKind::Local, "Local", Some("vim main.rs")),
            "vim main.rs"
        );
    }

    #[test]
    fn remote_terminal_disconnect_notice_uses_runtime_status() {
        assert_eq!(
            terminal_disconnected_notice_text(
                TerminalSessionKind::Remote,
                false,
                "SSH session closed"
            ),
            Some("远程连接已断开\nSSH session closed".to_string())
        );
        assert_eq!(
            terminal_disconnected_notice_text(TerminalSessionKind::Remote, true, "connected"),
            None
        );
        assert_eq!(
            terminal_disconnected_notice_text(
                TerminalSessionKind::Local,
                false,
                "shell process exited"
            ),
            None
        );
    }

    #[test]
    fn serial_terminal_disconnect_notice_uses_runtime_status() {
        assert_eq!(
            terminal_disconnected_notice_text(
                TerminalSessionKind::Serial,
                false,
                "failed to open serial port"
            ),
            Some("串口连接已断开\nfailed to open serial port".to_string())
        );
    }

    #[test]
    fn inactive_terminal_runtime_replaces_previous_runtime_after_all_tabs_close() {
        let previous = std::sync::Arc::new(std::sync::Mutex::new(
            super::LocalTerminalRuntime::failed("old-serial", "connected serial"),
        ));
        let replacement = super::inactive_terminal_runtime();

        assert!(!std::sync::Arc::ptr_eq(&previous, &replacement));
        let snapshot = replacement.lock().unwrap().snapshot();
        assert_eq!(snapshot.session_id, "inactive");
        assert!(!snapshot.connected);
    }

    #[test]
    fn occupied_serial_port_index_matches_trimmed_ports_and_can_skip_current_tab() {
        let open_ports = [
            Some(" /dev/cu.usbserial-1420 ".to_string()),
            None,
            Some("/dev/cu.usbserial-1430".to_string()),
        ];

        assert_eq!(
            super::occupied_serial_port_index(
                open_ports.iter().map(|port| port.as_deref()),
                "/dev/cu.usbserial-1420",
                None,
            ),
            Some(0)
        );
        assert_eq!(
            super::occupied_serial_port_index(
                open_ports.iter().map(|port| port.as_deref()),
                " /dev/cu.usbserial-1430 ",
                Some(2),
            ),
            None
        );
        assert_eq!(
            super::occupied_serial_port_index(
                open_ports.iter().map(|port| port.as_deref()),
                "   ",
                None,
            ),
            None
        );
    }

    #[test]
    fn remote_split_pane_badge_uses_connection_label_when_runtime_title_is_empty() {
        assert_eq!(
            split_pane_header_badge_title(None, "Production SSH", TerminalSessionKind::Remote),
            "Production SSH"
        );
    }

    #[test]
    fn remote_split_pane_badge_uses_remote_icon() {
        assert_eq!(
            split_pane_header_badge_icon(TerminalSessionKind::Remote),
            ICON_PATH_CLOUD
        );
        assert_eq!(
            split_pane_header_badge_icon(TerminalSessionKind::Local),
            ICON_PATH_FOLDER
        );
    }

    #[test]
    fn split_pane_header_background_is_opaque() {
        for theme in ThemeChoice::ALL {
            let color = split_pane_header_background_color(&theme.to_warp_theme());
            assert_eq!(
                color.a, 0xff,
                "theme {:?} header color must be opaque",
                theme
            );
        }
    }

    #[test]
    fn terminal_keyboard_input_pauses_for_overlay_editors() {
        assert!(super::terminal_keyboard_input_enabled(false, false));
        assert!(!super::terminal_keyboard_input_enabled(true, false));
        assert!(!super::terminal_keyboard_input_enabled(false, true));
    }

    #[test]
    fn terminal_context_menu_position_is_window_bounded() {
        assert!(matches!(
            terminal_context_menu_offset_bounds(),
            ParentOffsetBounds::WindowByPosition
        ));
    }

    #[test]
    fn root_overlay_stack_uses_waterfall_event_dispatch() {
        assert!(matches!(
            super::root_overlay_event_dispatch_mode(),
            EventDispatchMode::Waterfall
        ));
    }

    #[test]
    fn terminal_overlay_stack_uses_waterfall_event_dispatch() {
        assert!(matches!(
            super::terminal_overlay_event_dispatch_mode(),
            EventDispatchMode::Waterfall
        ));
    }

    #[test]
    fn terminal_clear_key_binding_matches_warp_platform_policy() {
        if super::platform::OperatingSystem::get().is_mac() {
            assert_eq!(super::terminal_clear_key_binding(), "cmd-k");
        } else {
            assert_eq!(super::terminal_clear_key_binding(), "ctrl-shift-K");
        }
    }
}
