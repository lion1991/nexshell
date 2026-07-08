use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    env, fs,
    io::{Read, Write},
    ops::{BitOr, BitOrAssign, Range},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use parking_lot::FairMutex;
use portable_pty::PtySize;

use crate::pty_event_loop;
use crate::pty_event_loop::{EventLoopHandle, Message, PtyEvent, PtySink};
use crate::ssh_session::{ChannelRequest, SshConnectOptions, SshHandle, SshSession};
use crate::terminal_recorder::TerminalRecorder;

use alacritty_terminal::{
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, GridCell, Scroll},
    index::{Boundary, Column, Line, Point, Side},
    selection::{Selection, SelectionRange, SelectionType},
    term::{
        cell::{Cell as AlacrittyCell, Flags, LineLength},
        color::{Colors, COUNT as TERMINAL_COLOR_COUNT},
        search::{Match as RegexMatch, RegexSearch},
        Config, TermMode,
    },
    vte::ansi::{self, ClearMode, Color, CursorShape as AnsiCursorShape, Handler, NamedColor, Rgb},
    Term,
};
use encoding_rs::{CoderResult, Decoder, Encoder, Encoding, UTF_8};
#[cfg(feature = "warpui-app")]
use warp_terminal::model::{
    escape_sequences::{
        maybe_kitty_keyboard_escape_sequence, EscCodes, KeystrokeWithDetails, ModeProvider,
        ToEscapeSequence, C1,
    },
    TermMode as WarpTermMode,
};
#[cfg(feature = "warpui-app")]
use warpui::keymap::Keystroke;
#[cfg(feature = "warpui-app")]
use warpui::platform::keyboard::KeyCode;

pub const BRACKETED_PASTE_PREFIX: &[u8] = b"\x1b[200~";
pub const BRACKETED_PASTE_SUFFIX: &[u8] = b"\x1b[201~";
const DEFAULT_CELL_PIXEL_WIDTH: u16 = 8;
const DEFAULT_CELL_PIXEL_HEIGHT: u16 = 18;

#[cfg(unix)]
type LocalPtyDescriptor = std::os::unix::io::RawFd;
#[cfg(not(unix))]
type LocalPtyDescriptor = ();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseReportButton {
    Left,
    Middle,
    Right,
    Move,
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseReportAction {
    Press,
    Release,
    Drag,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MouseReportModifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// Build an SGR-format mouse report (`CSI < b ; col ; row M|m`). The button
/// codes / drag offset / modifier offsets follow the same xterm convention
/// Warp encodes in `crates/warp_terminal/src/model/escape_sequences.rs:255`.
pub fn encode_sgr_mouse_report(
    button: MouseReportButton,
    action: MouseReportAction,
    col: u16,
    row: u16,
    modifiers: MouseReportModifiers,
) -> Vec<u8> {
    let mut code: u32 = match button {
        MouseReportButton::Left => 0,
        MouseReportButton::Middle => 1,
        MouseReportButton::Right => 2,
        MouseReportButton::Move => 35,
        MouseReportButton::WheelUp => 64,
        MouseReportButton::WheelDown => 65,
    };
    if matches!(action, MouseReportAction::Drag) {
        code += 32;
    }
    if modifiers.shift {
        code += 4;
    }
    if modifiers.alt {
        code += 8;
    }
    if modifiers.ctrl {
        code += 16;
    }

    let suffix = match action {
        MouseReportAction::Release => 'm',
        _ => 'M',
    };

    format!(
        "\x1b[<{code};{col};{row}{suffix}",
        col = col.max(1),
        row = row.max(1),
    )
    .into_bytes()
}

// 鼠标上报模式的原子镜像位。UI 线程发鼠标报告前用它免锁查实时状态，
// 不能依赖渲染期快照——TUI 退出瞬间快照滞后会把 \e[<35;x;yM 漏进 shell。
pub const MOUSE_MODE_SGR: u8 = 1 << 0;
pub const MOUSE_MODE_CLICK: u8 = 1 << 1;
pub const MOUSE_MODE_MOTION: u8 = 1 << 2;
pub const MOUSE_MODE_DRAG: u8 = 1 << 3;

pub fn mouse_mode_bits_app_active(bits: u8) -> bool {
    bits & MOUSE_MODE_SGR != 0
        && bits & (MOUSE_MODE_CLICK | MOUSE_MODE_MOTION | MOUSE_MODE_DRAG) != 0
}

pub fn mouse_mode_bits_drag_active(bits: u8) -> bool {
    bits & MOUSE_MODE_SGR != 0 && bits & (MOUSE_MODE_DRAG | MOUSE_MODE_MOTION) != 0
}

pub fn mouse_mode_bits_motion_active(bits: u8) -> bool {
    bits & MOUSE_MODE_SGR != 0 && bits & MOUSE_MODE_MOTION != 0
}
use portable_pty::native_pty_system;
use portable_pty::CommandBuilder;
use russh::ChannelMsg;

const GRID_EVENT_CHANNEL_CAPACITY: usize = 1_024;

/// IME 合成期间的 marked text 状态。Warp 把它存在 `TerminalModel` 上、
/// 随 grid 渲染时逐字 overlay 到光标后；我们走的是 GPUI 输入栈，结构一致，
/// 但渲染端做成简单的覆盖层即可。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkedText {
    /// IME 当前正在合成的字符串（UTF-8）。
    pub text: String,
    /// IME 反白选区（UTF-16 偏移，对应 NSTextInputClient 协议 / GPUI 的
    /// `replace_and_mark_text_in_range` 签名），渲染端用它强调候选段。
    pub selected_range_utf16: Range<usize>,
}

impl MarkedText {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn cell_len(&self) -> usize {
        use unicode_width::UnicodeWidthChar;
        self.text.chars().map(|ch| ch.width().unwrap_or(0)).sum()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRuntimeSnapshot {
    pub session_id: String,
    pub connected: bool,
    pub status: String,
    pub title: Option<String>,
    /// shell integration marker 触发后翻 true，UI 由占位切到 grid。
    pub bootstrapped: bool,
    /// 占位 "Starting {name}..." 显示名。
    pub shell_display_name: Option<String>,
    /// Monotonic counter incremented every time the alacritty grid emits
    /// `Event::Bell`. The UI watches it for visual flash; using a counter
    /// avoids `Instant`'s lack of `Eq`/`Hash` and lets us survive snapshot
    /// cloning + diffing.
    pub bell_pulse: u64,
    /// `find_pulse` increments whenever the find state (query / matches /
    /// current index / display position) changes so the UI can re-read the
    /// match list without polling. Unlike `bell_pulse` this also bumps when
    /// the query is cleared.
    pub find_pulse: u64,
    pub find_query: Option<String>,
    pub find_match_count: usize,
    pub find_current_match: Option<usize>,
    /// IME marked text 当前状态。无合成时为 `None`。
    pub marked_text: Option<MarkedText>,
    pub lines: Vec<String>,
    pub grid: TerminalGridSnapshot,
    /// shell 通过 OSC 7 上报的本地 cwd；远程 / 串口 tab 恒为 None。
    pub local_cwd: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalInputEditor {
    buffer: String,
    cursor_byte_index: usize,
    marked_text: Option<MarkedText>,
    revision: u64,
    /// Tab 补全后 shell 接管行编辑，按键直通 PTY
    shell_owns_line: bool,
    /// 基于历史的 autosuggestion（类 zsh-autosuggestions）
    suggestion: Option<String>,
}

impl TerminalInputEditor {
    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn cursor_byte_index(&self) -> usize {
        self.cursor_byte_index
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty() && self.marked_text().is_none()
    }

    pub fn marked_text(&self) -> Option<&MarkedText> {
        self.marked_text
            .as_ref()
            .filter(|marked| !marked.is_empty())
    }

    pub fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.marked_text = None;
        self.buffer.insert_str(self.cursor_byte_index, text);
        self.cursor_byte_index += text.len();
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn backspace(&mut self) -> bool {
        if self.marked_text.is_some() {
            return self.clear_marked_text();
        }

        let Some(previous) = self.previous_cursor_boundary() else {
            return false;
        };
        self.buffer.drain(previous..self.cursor_byte_index);
        self.cursor_byte_index = previous;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn move_left(&mut self) -> bool {
        if self.marked_text.is_some() {
            return true;
        }

        let Some(previous) = self.previous_cursor_boundary() else {
            return !self.buffer.is_empty();
        };
        self.cursor_byte_index = previous;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn move_right(&mut self) -> bool {
        if self.marked_text.is_some() {
            return true;
        }

        let Some(next) = self.next_cursor_boundary() else {
            return !self.buffer.is_empty();
        };
        self.cursor_byte_index = next;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    fn previous_cursor_boundary(&self) -> Option<usize> {
        if self.cursor_byte_index == 0 {
            return None;
        }

        self.buffer[..self.cursor_byte_index]
            .char_indices()
            .last()
            .map(|(index, _)| index)
    }

    fn next_cursor_boundary(&self) -> Option<usize> {
        if self.cursor_byte_index >= self.buffer.len() {
            return None;
        }

        let remainder = &self.buffer[self.cursor_byte_index..];
        let mut chars = remainder.char_indices();
        chars.next()?;
        Some(
            chars
                .next()
                .map(|(index, _)| self.cursor_byte_index + index)
                .unwrap_or(self.buffer.len()),
        )
    }

    pub fn clear(&mut self) {
        if self.buffer.is_empty()
            && self.cursor_byte_index == 0
            && self.marked_text.is_none()
            && !self.shell_owns_line
        {
            return;
        }
        self.buffer.clear();
        self.cursor_byte_index = 0;
        self.marked_text = None;
        self.suggestion = None;
        self.shell_owns_line = false;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn set_marked_text(&mut self, text: String, selected_range_utf16: Range<usize>) {
        let next = if text.is_empty() {
            None
        } else {
            Some(MarkedText {
                text,
                selected_range_utf16,
            })
        };
        if self.marked_text == next {
            return;
        }
        self.marked_text = next;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn clear_marked_text(&mut self) -> bool {
        let had_marked_text = self.marked_text.take().is_some();
        if had_marked_text {
            self.revision = self.revision.wrapping_add(1);
        }
        had_marked_text
    }

    pub fn submit_bytes(&mut self) -> Option<Vec<u8>> {
        self.suggestion = None;
        let mut bytes = Vec::with_capacity(self.buffer.len() + 1);
        bytes.extend_from_slice(self.buffer.as_bytes());
        bytes.push(b'\r');
        self.clear();
        Some(bytes)
    }

    pub fn shell_owns_line(&self) -> bool {
        self.shell_owns_line
    }

    pub fn relinquish_line(&mut self) {
        self.shell_owns_line = false;
    }

    /// Flush buffer + \t 给 shell 触发补全，之后按键直通 PTY
    pub fn flush_for_completion(&mut self) -> Option<Vec<u8>> {
        let mut bytes = Vec::with_capacity(self.buffer.len() + 1);
        bytes.extend_from_slice(self.buffer.as_bytes());
        bytes.push(b'\t');
        self.buffer.clear();
        self.cursor_byte_index = 0;
        self.marked_text = None;
        self.suggestion = None;
        self.revision = self.revision.wrapping_add(1);
        self.shell_owns_line = true;
        Some(bytes)
    }

    /// Flush buffer + 方向键转义序列，用于 shell 历史导航
    pub fn flush_for_history(&mut self, arrow_escape: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.buffer.len() + arrow_escape.len());
        bytes.extend_from_slice(self.buffer.as_bytes());
        bytes.extend_from_slice(arrow_escape);
        self.buffer.clear();
        self.cursor_byte_index = 0;
        self.marked_text = None;
        self.suggestion = None;
        self.revision = self.revision.wrapping_add(1);
        self.shell_owns_line = true;
        bytes
    }

    pub fn suggestion(&self) -> Option<&str> {
        self.suggestion.as_deref()
    }

    pub fn set_suggestion(&mut self, suggestion: Option<String>) {
        self.suggestion = suggestion;
    }
}

fn terminal_runtime_debug_log(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("NEXSHELL_DEBUG_KEYS").is_some() {
        eprintln!("[nexshell key-debug] {args}");
    }
}

fn terminal_runtime_debug_bytes(bytes: &[u8]) -> String {
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    let text = String::from_utf8_lossy(bytes);
    format!(
        "len={} hex=[{hex}] text=\"{}\"",
        bytes.len(),
        text.escape_debug()
    )
}

pub fn terminal_input_editor_should_capture(snapshot: &TerminalGridSnapshot) -> bool {
    !snapshot.input_modes.alt_screen && !snapshot.mouse_app_active() && snapshot.display_offset == 0
}

pub fn terminal_snapshot_with_input_editor(
    snapshot: &TerminalGridSnapshot,
    editor: &TerminalInputEditor,
) -> TerminalGridSnapshot {
    let has_content = !editor.is_empty();
    let has_suggestion = editor.suggestion().is_some() && !editor.buffer().is_empty();
    if (!has_content && !has_suggestion) || !terminal_input_editor_should_capture(snapshot) {
        return snapshot.clone();
    }

    let mut projected = snapshot.clone();
    let cols = projected.cols.max(1);
    let rows = projected.rows.max(1);
    let start_cursor_index = projected.cursor_row * cols + projected.cursor_col;

    // 注册 ghost text style（灰色前景）
    let ghost_style_id = projected.styles.len() as u16;
    projected.styles.push(TerminalCellStyleSnapshot {
        fg: TerminalColorSnapshot::Rgb {
            r: 100,
            g: 100,
            b: 100,
        },
        bg: TerminalColorSnapshot::Named("background"),
        underline_color: None,
    });

    let (buffer_before_cursor, buffer_after_cursor) =
        editor.buffer().split_at(editor.cursor_byte_index());

    let input_cursor_index = project_input_editor_text(
        &mut projected,
        buffer_before_cursor,
        false,
        start_cursor_index,
        cols,
        rows,
    );
    let render_after_cursor_index = if let Some(marked) = editor.marked_text() {
        project_input_editor_text(
            &mut projected,
            marked.text.as_str(),
            true,
            input_cursor_index,
            cols,
            rows,
        )
    } else {
        input_cursor_index
    };
    let end_index = project_input_editor_text(
        &mut projected,
        buffer_after_cursor,
        false,
        render_after_cursor_index,
        cols,
        rows,
    );

    // ghost text：将 suggestion 中超出 buffer 的后缀以灰色投射
    let ghost_end = if let Some(suggestion) = editor.suggestion() {
        if let Some(suffix) = suggestion.strip_prefix(editor.buffer()) {
            project_ghost_text(
                &mut projected,
                suffix,
                ghost_style_id,
                end_index,
                cols,
                rows,
            )
        } else {
            end_index
        }
    } else {
        end_index
    };

    // 清除投射区域之后到行尾的残留内容
    let clear_from = ghost_end;
    let clear_row = clear_from / cols;
    let clear_col = clear_from % cols;
    if clear_row < rows && clear_col < cols {
        ensure_projected_line_cells(&mut projected.lines, clear_row, cols);
        for col in clear_col..cols {
            projected.lines[clear_row].cells[col] = empty_input_editor_cell();
        }
    }

    for row in 0..projected.lines.len().min(rows) {
        projected.lines[row].text = terminal_line_text_from_cells(&projected.lines[row].cells);
    }
    projected.dirty_rows.resize(rows, false);
    projected.dirty_rows.fill(true);

    let max_cursor_index = rows * cols - 1;
    let cursor_index = render_after_cursor_index.min(max_cursor_index);
    projected.cursor_row = cursor_index / cols;
    projected.cursor_col = cursor_index % cols;
    projected.marked_text_active = editor.marked_text().is_some();
    projected
}

fn project_input_editor_text(
    projected: &mut TerminalGridSnapshot,
    text: &str,
    marked: bool,
    mut cursor_index: usize,
    cols: usize,
    rows: usize,
) -> usize {
    for ch in text.chars() {
        let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width == 0 {
            continue;
        }

        let row = cursor_index / cols;
        let col = cursor_index % cols;
        if row >= rows {
            break;
        }
        ensure_projected_line_cells(&mut projected.lines, row, cols);
        projected.lines[row].cells[col] = input_editor_cell(ch, width > 1, marked);
        cursor_index += 1;

        if width > 1 {
            let spacer_row = cursor_index / cols;
            let spacer_col = cursor_index % cols;
            if spacer_row < rows {
                ensure_projected_line_cells(&mut projected.lines, spacer_row, cols);
                projected.lines[spacer_row].cells[spacer_col] =
                    input_editor_wide_spacer_cell(marked);
            }
            cursor_index += 1;
        }
    }
    cursor_index
}

fn ensure_projected_line_cells(lines: &mut Vec<TerminalGridLineSnapshot>, row: usize, cols: usize) {
    while lines.len() <= row {
        lines.push(TerminalGridLineSnapshot {
            text: String::new(),
            cells: vec![empty_input_editor_cell(); cols],
        });
    }
    if lines[row].cells.len() < cols {
        lines[row].cells.resize_with(cols, empty_input_editor_cell);
    }
}

fn input_editor_cell(ch: char, wide: bool, marked: bool) -> TerminalGridCellSnapshot {
    let mut flags = TerminalCellFlags::empty();
    flags.set(TerminalCellFlags::WIDE_CHAR, wide);
    flags.set(TerminalCellFlags::UNDERLINE, marked);
    TerminalGridCellSnapshot {
        ch,
        content: Arc::from(ch.to_string()),
        style_id: 0,
        flags,
        hyperlink: None,
    }
}

fn input_editor_wide_spacer_cell(marked: bool) -> TerminalGridCellSnapshot {
    let mut flags = TerminalCellFlags::empty();
    flags.insert(TerminalCellFlags::WIDE_CHAR_SPACER);
    flags.set(TerminalCellFlags::UNDERLINE, marked);
    TerminalGridCellSnapshot {
        ch: ' ',
        content: Arc::from(" "),
        style_id: 0,
        flags,
        hyperlink: None,
    }
}

fn empty_input_editor_cell() -> TerminalGridCellSnapshot {
    TerminalGridCellSnapshot {
        ch: ' ',
        content: Arc::from(" "),
        style_id: 0,
        flags: TerminalCellFlags::empty(),
        hyperlink: None,
    }
}

fn ghost_text_cell(ch: char, wide: bool, style_id: u16) -> TerminalGridCellSnapshot {
    let mut flags = TerminalCellFlags::empty();
    flags.set(TerminalCellFlags::WIDE_CHAR, wide);
    TerminalGridCellSnapshot {
        ch,
        content: Arc::from(ch.to_string()),
        style_id,
        flags,
        hyperlink: None,
    }
}

fn project_ghost_text(
    projected: &mut TerminalGridSnapshot,
    text: &str,
    style_id: u16,
    mut cursor_index: usize,
    cols: usize,
    rows: usize,
) -> usize {
    for ch in text.chars() {
        let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width == 0 {
            continue;
        }
        let row = cursor_index / cols;
        let col = cursor_index % cols;
        if row >= rows {
            break;
        }
        ensure_projected_line_cells(&mut projected.lines, row, cols);
        projected.lines[row].cells[col] = ghost_text_cell(ch, width > 1, style_id);
        cursor_index += 1;
        if width > 1 {
            let sr = cursor_index / cols;
            let sc = cursor_index % cols;
            if sr < rows {
                ensure_projected_line_cells(&mut projected.lines, sr, cols);
                let mut flags = TerminalCellFlags::empty();
                flags.insert(TerminalCellFlags::WIDE_CHAR_SPACER);
                projected.lines[sr].cells[sc] = TerminalGridCellSnapshot {
                    ch: ' ',
                    content: Arc::from(" "),
                    style_id,
                    flags,
                    hyperlink: None,
                };
            }
            cursor_index += 1;
        }
    }
    cursor_index
}

fn terminal_line_text_from_cells(cells: &[TerminalGridCellSnapshot]) -> String {
    let mut text = String::new();
    for cell in cells {
        if cell.wide_spacer() {
            continue;
        }
        text.push_str(cell.content.as_ref());
    }
    text.trim_end().to_string()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalCursorShape {
    #[default]
    Block,
    Underline,
    Beam,
    HollowBlock,
    Hidden,
}

impl From<AnsiCursorShape> for TerminalCursorShape {
    fn from(shape: AnsiCursorShape) -> Self {
        match shape {
            AnsiCursorShape::Block => Self::Block,
            AnsiCursorShape::Underline => Self::Underline,
            AnsiCursorShape::Beam => Self::Beam,
            AnsiCursorShape::HollowBlock => Self::HollowBlock,
            AnsiCursorShape::Hidden => Self::Hidden,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalGridSnapshot {
    pub cols: usize,
    pub rows: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub cursor_shape: TerminalCursorShape,
    pub cursor_blinking: bool,
    /// `false` when the application has hidden the cursor (DEC mode `?25l`),
    /// or alacritty's cursor style is `Hidden`. Renderers should suppress
    /// drawing entirely when this is false.
    pub cursor_visible: bool,
    pub marked_text_active: bool,
    pub display_offset: usize,
    pub history_size: usize,
    pub dirty_rows: Vec<bool>,
    pub bracketed_paste: bool,
    pub mouse_report_click: bool,
    pub mouse_report_motion: bool,
    pub mouse_report_drag: bool,
    pub sgr_mouse: bool,
    pub input_modes: TerminalInputModes,
    pub styles: Vec<TerminalCellStyleSnapshot>,
    pub lines: Vec<TerminalGridLineSnapshot>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerminalInputModes {
    pub alt_screen: bool,
    pub alternate_scroll: bool,
    pub app_cursor: bool,
    pub focus_in_out: bool,
    pub keyboard_disambiguate_escape: bool,
    pub keyboard_report_event_types: bool,
    pub keyboard_report_alternate_keys: bool,
    pub keyboard_report_all_as_escape: bool,
    pub keyboard_report_associated_text: bool,
}

impl TerminalInputModes {
    fn from_alacritty(mode: TermMode) -> Self {
        Self {
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
            alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            focus_in_out: mode.contains(TermMode::FOCUS_IN_OUT),
            keyboard_disambiguate_escape: mode.contains(TermMode::DISAMBIGUATE_ESC_CODES),
            keyboard_report_event_types: mode.contains(TermMode::REPORT_EVENT_TYPES),
            keyboard_report_alternate_keys: mode.contains(TermMode::REPORT_ALTERNATE_KEYS),
            keyboard_report_all_as_escape: mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC),
            keyboard_report_associated_text: mode.contains(TermMode::REPORT_ASSOCIATED_TEXT),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalClipboardStoreRequest {
    pub text: String,
}

#[derive(Clone)]
pub struct TerminalClipboardLoadRequest {
    formatter: Arc<dyn Fn(&str) -> String + Sync + Send + 'static>,
}

impl TerminalClipboardLoadRequest {
    pub fn response_bytes(&self, clipboard_text: &str) -> Vec<u8> {
        (self.formatter)(clipboard_text).into_bytes()
    }
}

impl TerminalGridSnapshot {
    pub fn empty() -> Self {
        Self {
            cols: 0,
            rows: 0,
            cursor_row: 0,
            cursor_col: 0,
            cursor_shape: TerminalCursorShape::Block,
            cursor_blinking: false,
            cursor_visible: false,
            marked_text_active: false,
            display_offset: 0,
            history_size: 0,
            dirty_rows: Vec::new(),
            bracketed_paste: false,
            mouse_report_click: false,
            mouse_report_motion: false,
            mouse_report_drag: false,
            sgr_mouse: false,
            input_modes: TerminalInputModes::default(),
            styles: vec![TerminalCellStyleSnapshot::default()],
            lines: Vec::new(),
        }
    }

    pub fn cell_style(&self, cell: &TerminalGridCellSnapshot) -> &TerminalCellStyleSnapshot {
        self.styles
            .get(cell.style_id as usize)
            .unwrap_or(&self.styles[0])
    }

    pub fn mouse_app_active(&self) -> bool {
        self.sgr_mouse
            && (self.mouse_report_click || self.mouse_report_motion || self.mouse_report_drag)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalGridLineSnapshot {
    pub text: String,
    pub cells: Vec<TerminalGridCellSnapshot>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TerminalCellStyleSnapshot {
    pub fg: TerminalColorSnapshot,
    pub bg: TerminalColorSnapshot,
    pub underline_color: Option<TerminalColorSnapshot>,
}

impl Default for TerminalCellStyleSnapshot {
    fn default() -> Self {
        Self {
            fg: TerminalColorSnapshot::Named("foreground"),
            bg: TerminalColorSnapshot::Named("background"),
            underline_color: None,
        }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerminalCellFlags(u32);

impl TerminalCellFlags {
    pub const INVERSE: Self = Self(0b0000_0000_0000_0001);
    pub const BOLD: Self = Self(0b0000_0000_0000_0010);
    pub const ITALIC: Self = Self(0b0000_0000_0000_0100);
    pub const BOLD_ITALIC: Self = Self(Self::BOLD.0 | Self::ITALIC.0);
    pub const UNDERLINE: Self = Self(0b0000_0000_0000_1000);
    pub const WRAPLINE: Self = Self(0b0000_0000_0001_0000);
    pub const WIDE_CHAR: Self = Self(0b0000_0000_0010_0000);
    pub const WIDE_CHAR_SPACER: Self = Self(0b0000_0000_0100_0000);
    pub const DIM: Self = Self(0b0000_0000_1000_0000);
    pub const DIM_BOLD: Self = Self(Self::DIM.0 | Self::BOLD.0);
    pub const HIDDEN: Self = Self(0b0000_0001_0000_0000);
    pub const STRIKEOUT: Self = Self(0b0000_0010_0000_0000);
    pub const LEADING_WIDE_CHAR_SPACER: Self = Self(0b0000_0100_0000_0000);
    pub const DOUBLE_UNDERLINE: Self = Self(0b0000_1000_0000_0000);
    pub const HAS_CURSOR: Self = Self(0b0001_0000_0000_0000);
    pub const SELECTED: Self = Self(0b0010_0000_0000_0000);
    pub const FIND_MATCH: Self = Self(0b0100_0000_0000_0000);
    pub const FIND_FOCUS: Self = Self(0b1000_0000_0000_0000);
    pub const UNDERCURL: Self = Self(0b0001_0000_0000_0000_0000);
    pub const DOTTED_UNDERLINE: Self = Self(0b0010_0000_0000_0000_0000);
    pub const DASHED_UNDERLINE: Self = Self(0b0100_0000_0000_0000_0000);
    pub const CELL_DECORATIONS: Self = Self(
        Self::UNDERLINE.0
            | Self::STRIKEOUT.0
            | Self::DOUBLE_UNDERLINE.0
            | Self::UNDERCURL.0
            | Self::DOTTED_UNDERLINE.0
            | Self::DASHED_UNDERLINE.0,
    );
    pub const ALL_UNDERLINES: Self = Self(
        Self::UNDERLINE.0
            | Self::DOUBLE_UNDERLINE.0
            | Self::UNDERCURL.0
            | Self::DOTTED_UNDERLINE.0
            | Self::DASHED_UNDERLINE.0,
    );

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    pub fn set(&mut self, other: Self, value: bool) {
        if value {
            self.insert(other);
        } else {
            self.remove(other);
        }
    }
}

impl BitOr for TerminalCellFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for TerminalCellFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.insert(rhs);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalGridCellSnapshot {
    pub ch: char,
    /// Cell text. Arc<str> so render path can clone refcount instead of allocating per cell.
    pub content: Arc<str>,
    pub style_id: u16,
    pub flags: TerminalCellFlags,
    pub hyperlink: Option<String>,
}

impl TerminalGridCellSnapshot {
    pub fn bold(&self) -> bool {
        self.flags.contains(TerminalCellFlags::BOLD)
    }

    pub fn dim(&self) -> bool {
        self.flags.contains(TerminalCellFlags::DIM)
    }

    pub fn italic(&self) -> bool {
        self.flags.contains(TerminalCellFlags::ITALIC)
    }

    pub fn underline(&self) -> bool {
        self.flags.intersects(TerminalCellFlags::ALL_UNDERLINES)
    }

    pub fn double_underline(&self) -> bool {
        self.flags.contains(TerminalCellFlags::DOUBLE_UNDERLINE)
    }

    pub fn undercurl(&self) -> bool {
        self.flags.contains(TerminalCellFlags::UNDERCURL)
    }

    pub fn dotted_underline(&self) -> bool {
        self.flags.contains(TerminalCellFlags::DOTTED_UNDERLINE)
    }

    pub fn dashed_underline(&self) -> bool {
        self.flags.contains(TerminalCellFlags::DASHED_UNDERLINE)
    }

    pub fn strikeout(&self) -> bool {
        self.flags.contains(TerminalCellFlags::STRIKEOUT)
    }

    pub fn hidden(&self) -> bool {
        self.flags.contains(TerminalCellFlags::HIDDEN)
    }

    pub fn inverse(&self) -> bool {
        self.flags.contains(TerminalCellFlags::INVERSE)
    }

    pub fn wide_spacer(&self) -> bool {
        self.flags.contains(TerminalCellFlags::WIDE_CHAR_SPACER)
    }

    pub fn selected(&self) -> bool {
        self.flags.contains(TerminalCellFlags::SELECTED)
    }

    pub fn find_match(&self) -> bool {
        self.flags.contains(TerminalCellFlags::FIND_MATCH)
    }

    pub fn find_focus(&self) -> bool {
        self.flags.contains(TerminalCellFlags::FIND_FOCUS)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TerminalColorSnapshot {
    Named(&'static str),
    Indexed(u8),
    Rgb { r: u8, g: u8, b: u8 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRenderRow {
    pub cells: Vec<TerminalRenderCell>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRenderCell {
    pub ch: char,
    pub content: Arc<str>,
    pub fg: u32,
    pub bg: u32,
    pub underline_color: Option<u32>,
    pub cursor: bool,
    pub selected: bool,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub double_underline: bool,
    pub undercurl: bool,
    pub dotted_underline: bool,
    pub dashed_underline: bool,
    pub strikeout: bool,
    pub hidden: bool,
    pub find_match: bool,
    pub find_focus: bool,
    /// `true` 时这个 cell 是上一个宽字符（CJK / 全角符号）的 spacer 占位。
    /// `coalesce_render_runs` 看到此标记会只 +1 列宽、不再 push 字符 ——
    /// 否则 "你" + spacer 在 run.text 里会变成 "你 "，CJK 字体把"你"渲染成
    /// 两列宽 + 空格再占一列，整段就被多撑出一列空隙（Warp 的 cell-by-cell
    /// 渲染天然跳过 spacer，对应 alacritty `Flags::WIDE_CHAR_SPACER`）。
    pub wide_spacer: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRenderRunRow {
    pub runs: Vec<TerminalRenderRun>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRenderRun {
    pub text: Arc<str>,
    pub cols: usize,
    pub fg: u32,
    pub bg: u32,
    pub underline_color: Option<u32>,
    pub cursor: bool,
    pub selected: bool,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub double_underline: bool,
    pub undercurl: bool,
    pub dotted_underline: bool,
    pub dashed_underline: bool,
    pub strikeout: bool,
    pub hidden: bool,
    pub find_match: bool,
    pub find_focus: bool,
}

pub fn terminal_render_rows(
    snapshot: &TerminalGridSnapshot,
    palette: &TerminalPalette,
) -> Vec<TerminalRenderRow> {
    snapshot
        .lines
        .iter()
        .enumerate()
        .map(|(row_idx, row)| TerminalRenderRow {
            cells: row
                .cells
                .iter()
                .enumerate()
                .map(|(col_idx, cell)| {
                    let style = snapshot.cell_style(cell);
                    let mut fg = resolve_terminal_color(&style.fg, palette);
                    let mut bg = resolve_terminal_color(&style.bg, palette);
                    let content: Arc<str> = if cell.hidden() {
                        Arc::from(" ")
                    } else {
                        Arc::clone(&cell.content)
                    };
                    let ch = content.chars().next().unwrap_or(' ');

                    if cell.dim() {
                        fg = dim_color(fg);
                    }

                    if cell.inverse() {
                        std::mem::swap(&mut fg, &mut bg);
                    }

                    let cursor = snapshot.cursor_visible
                        && row_idx == snapshot.cursor_row
                        && col_idx == snapshot.cursor_col;

                    if cursor
                        && snapshot.cursor_shape == TerminalCursorShape::Block
                        && !snapshot.marked_text_active
                        && !terminal_cell_occupied_by_text(cell)
                    {
                        fg = palette.background;
                        bg = palette.cursor;
                    } else if cell.find_focus() {
                        fg = palette.background;
                        bg = palette.find_focus;
                    } else if cell.find_match() {
                        fg = palette.background;
                        bg = palette.find_match;
                    } else if cell.selected() {
                        fg = palette.foreground;
                        bg = palette.selection;
                    }

                    TerminalRenderCell {
                        ch,
                        content,
                        fg,
                        bg,
                        underline_color: style
                            .underline_color
                            .as_ref()
                            .map(|c| resolve_terminal_color(c, palette)),
                        cursor,
                        selected: cell.selected(),
                        bold: cell.bold(),
                        dim: cell.dim(),
                        italic: cell.italic(),
                        underline: cell.underline() || cell.hyperlink.is_some(),
                        double_underline: cell.double_underline(),
                        undercurl: cell.undercurl(),
                        dotted_underline: cell.dotted_underline(),
                        dashed_underline: cell.dashed_underline(),
                        strikeout: cell.strikeout(),
                        hidden: cell.hidden(),
                        find_match: cell.find_match(),
                        find_focus: cell.find_focus(),
                        wide_spacer: cell.wide_spacer(),
                    }
                })
                .collect(),
        })
        .collect()
}

fn terminal_cell_occupied_by_text(cell: &TerminalGridCellSnapshot) -> bool {
    cell.wide_spacer() || (cell.content.as_ref() != " " && cell.content.as_ref() != "\0")
}

pub fn terminal_render_run_rows(
    snapshot: &TerminalGridSnapshot,
    palette: &TerminalPalette,
) -> Vec<TerminalRenderRunRow> {
    terminal_render_rows(snapshot, palette)
        .into_iter()
        .map(|row| TerminalRenderRunRow {
            runs: coalesce_render_runs(row.cells),
        })
        .collect()
}

fn coalesce_render_runs(cells: Vec<TerminalRenderCell>) -> Vec<TerminalRenderRun> {
    let mut runs: Vec<TerminalRenderRun> = Vec::with_capacity(cells.len());

    for cell in cells {
        if cell.wide_spacer {
            if let Some(run) = runs.last_mut() {
                run.cols += 1;
                continue;
            }
            // 行首就是 spacer 是异常情况（alacritty 不该产生），用空文本 run
            // 占位避免列位错位。
            runs.push(TerminalRenderRun {
                text: Arc::from(""),
                cols: 1,
                fg: cell.fg,
                bg: cell.bg,
                underline_color: cell.underline_color,
                cursor: cell.cursor,
                selected: cell.selected,
                bold: cell.bold,
                dim: cell.dim,
                italic: cell.italic,
                underline: cell.underline,
                double_underline: cell.double_underline,
                undercurl: cell.undercurl,
                dotted_underline: cell.dotted_underline,
                dashed_underline: cell.dashed_underline,
                strikeout: cell.strikeout,
                hidden: cell.hidden,
                find_match: cell.find_match,
                find_focus: cell.find_focus,
            });
            continue;
        }

        if let Some(run) = runs.last_mut() {
            if terminal_run_can_merge_cell(run, &cell) {
                let mut text = String::with_capacity(run.text.len() + cell.content.len());
                text.push_str(&run.text);
                text.push_str(&cell.content);
                run.text = Arc::from(text);
                run.cols += 1;
                continue;
            }
        }

        runs.push(terminal_render_run_from_cell(cell));
    }

    runs
}

fn terminal_render_run_from_cell(cell: TerminalRenderCell) -> TerminalRenderRun {
    TerminalRenderRun {
        text: cell.content,
        cols: 1,
        fg: cell.fg,
        bg: cell.bg,
        underline_color: cell.underline_color,
        cursor: cell.cursor,
        selected: cell.selected,
        bold: cell.bold,
        dim: cell.dim,
        italic: cell.italic,
        underline: cell.underline,
        double_underline: cell.double_underline,
        undercurl: cell.undercurl,
        dotted_underline: cell.dotted_underline,
        dashed_underline: cell.dashed_underline,
        strikeout: cell.strikeout,
        hidden: cell.hidden,
        find_match: cell.find_match,
        find_focus: cell.find_focus,
    }
}

fn terminal_run_can_merge_cell(run: &TerminalRenderRun, cell: &TerminalRenderCell) -> bool {
    run.cols == run.text.chars().count()
        && cell.content.chars().count() == 1
        && run.fg == cell.fg
        && run.bg == cell.bg
        && run.underline_color == cell.underline_color
        && run.cursor == cell.cursor
        && run.selected == cell.selected
        && run.bold == cell.bold
        && run.dim == cell.dim
        && run.italic == cell.italic
        && run.underline == cell.underline
        && run.double_underline == cell.double_underline
        && run.undercurl == cell.undercurl
        && run.dotted_underline == cell.dotted_underline
        && run.dashed_underline == cell.dashed_underline
        && run.strikeout == cell.strikeout
        && run.hidden == cell.hidden
        && run.find_match == cell.find_match
        && run.find_focus == cell.find_focus
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalTextBuffer {
    max_lines: usize,
    lines: VecDeque<String>,
    pending_carriage_return: bool,
}

impl TerminalTextBuffer {
    pub fn new(max_lines: usize) -> Self {
        let mut lines = VecDeque::new();
        lines.push_back(String::new());

        Self {
            max_lines: max_lines.max(1),
            lines,
            pending_carriage_return: false,
        }
    }

    pub fn push_output(&mut self, bytes: &[u8]) {
        let text = strip_ansi(&String::from_utf8_lossy(bytes));

        for ch in text.chars() {
            match ch {
                '\r' => {
                    self.pending_carriage_return = true;
                }
                '\n' => {
                    self.push_line();
                    self.pending_carriage_return = false;
                }
                '\x08' => {
                    self.apply_pending_carriage_return();
                    self.current_line_mut().pop();
                }
                '\t' => {
                    self.apply_pending_carriage_return();
                    self.current_line_mut().push_str("    ");
                }
                ch if !ch.is_control() => {
                    self.apply_pending_carriage_return();
                    self.current_line_mut().push(ch);
                }
                _ => {}
            }
        }
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }

    fn current_line_mut(&mut self) -> &mut String {
        self.lines.back_mut().expect("buffer always has one line")
    }

    fn push_line(&mut self) {
        self.lines.push_back(String::new());
        while self.lines.len() > self.max_lines {
            self.lines.pop_front();
        }
    }

    fn apply_pending_carriage_return(&mut self) {
        if self.pending_carriage_return {
            self.current_line_mut().clear();
            self.pending_carriage_return = false;
        }
    }
}

#[derive(Debug)]
struct TerminalEventProxy {
    sender: SyncSender<Event>,
}

impl EventListener for TerminalEventProxy {
    fn send_event(&self, event: Event) {
        let _ = self.sender.try_send(event);
    }
}

struct TerminalDimensions {
    cols: usize,
    rows: usize,
}

impl Dimensions for TerminalDimensions {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// Inclusive `[start, end]` cell range for a single regex match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FindMatchRange {
    pub start: Point,
    pub end: Point,
}

impl FindMatchRange {
    pub fn contains(&self, point: Point) -> bool {
        if self.start.line == self.end.line {
            point.line == self.start.line
                && point.column >= self.start.column
                && point.column <= self.end.column
        } else if point.line < self.start.line || point.line > self.end.line {
            false
        } else if point.line == self.start.line {
            point.column >= self.start.column
        } else if point.line == self.end.line {
            point.column <= self.end.column
        } else {
            true
        }
    }
}

impl From<RegexMatch> for FindMatchRange {
    fn from(m: RegexMatch) -> Self {
        Self {
            start: *m.start(),
            end: *m.end(),
        }
    }
}

pub struct TerminalGridCore {
    term: Term<TerminalEventProxy>,
    parser: ansi::Processor,
    event_rx: Receiver<Event>,
    dirty_rows: Vec<bool>,
    cached_lines: Vec<TerminalGridLineSnapshot>,
    styles: Vec<TerminalCellStyleSnapshot>,
    style_map: HashMap<TerminalCellStyleSnapshot, u16>,
    /// 备用屏 scrollback 容量（进 alt 屏时撑给 alt grid，见 ADR 0006）。
    scrollback: usize,
    /// 鼠标上报模式实时镜像（MOUSE_MODE_* 位），供 UI 线程免锁查询。
    mouse_modes: Arc<AtomicU8>,
}

struct TerminalPromptPrefixSnapshot {
    cursor_col: usize,
    cells: Vec<AlacrittyCell>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TerminalDirtyState {
    rows: usize,
    cols: usize,
    display_offset: i32,
    history_size: usize,
    cursor_line: i32,
}

impl TerminalGridCore {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        let (sender, event_rx) = mpsc::sync_channel(GRID_EVENT_CHANNEL_CAPACITY);
        let dimensions = TerminalDimensions {
            cols: cols.max(2),
            rows: rows.max(1),
        };
        let config = Config {
            kitty_keyboard: true,
            scrolling_history: scrollback,
            ..Config::default()
        };

        let mut core = Self {
            term: Term::new(config, &dimensions, TerminalEventProxy { sender }),
            parser: ansi::Processor::default(),
            event_rx,
            dirty_rows: vec![true; dimensions.rows],
            cached_lines: Vec::new(),
            styles: vec![TerminalCellStyleSnapshot::default()],
            style_map: HashMap::from([(TerminalCellStyleSnapshot::default(), 0u16)]),
            scrollback,
            mouse_modes: Arc::new(AtomicU8::new(0)),
        };
        // alacritty 的 TermMode 默认开 ALTERNATE_SCROLL(DECSET 1007)，与 iTerm2(默认关)
        // 相反：会让备用屏滚轮被转发成 ↑/↓ 给应用，而非滚本地 scrollback。关掉使默认=
        // 本地滚动；应用显式 ?1007h 时再开（iTerm2 同款）。见 ADR 0006。
        core.parser.advance(&mut core.term, b"\x1b[?1007l");
        core
    }

    pub fn process_output(&mut self, bytes: &[u8]) {
        let before = self.dirty_state();
        let snap_to_bottom = terminal_output_should_snap_to_bottom_after_clear(bytes);
        let alt_before = self.term.mode().contains(TermMode::ALT_SCREEN);
        let bytes_request_alt_exit = bytes_contain_alt_screen_exit(bytes);
        self.parser.advance(&mut self.term, bytes);
        // TUI 退出 alt-screen 时常漏发 mouse / focus / bracketed-paste 关闭
        // 序列，残留会让 shell readline 把后续 SGR 鼠标报告、focus event、
        // paste 包裹当成普通字符 echo。覆盖 ?1049l / ?1047l / ?47l 与单 chunk
        // 净零进出（alt_before == alt_after）。
        let alt_after = self.term.mode().contains(TermMode::ALT_SCREEN);
        if !alt_after && (alt_before || bytes_request_alt_exit) {
            self.reset_leaked_tui_modes();
        }
        // 进入备用屏：alacritty 默认给 alt grid 历史容量 0，滚出屏幕的行直接丢。
        // 这里给当前(alt) grid 撑开历史(先清上轮残留再设容量)，让 tmux/vim/less
        // 滚出的内容进 scrollback，原生滚动条/滚轮可回看(参照 iTerm2，见 ADR 0006)。
        // 防污染白嫖 alacritty 既有的 region.start==0 闸。
        if alt_after && !alt_before {
            self.term.grid_mut().update_history(0);
            self.term.grid_mut().update_history(self.scrollback);
        }
        if snap_to_bottom {
            self.term.scroll_display(Scroll::Bottom);
        }
        let after = self.dirty_state();
        self.mark_dirty_for_change(before, after);
        if snap_to_bottom {
            self.mark_all_dirty();
        }
        self.mark_dirty_for_content_drift();
        self.sync_mouse_modes();
    }

    /// pty 线程每次消化输出后刷新镜像，UI 线程发鼠标报告前实时读取，
    /// 不经渲染快照——避免 TUI 退出瞬间快照滞后把鼠标序列漏进 shell。
    fn sync_mouse_modes(&self) {
        let mode = self.term.mode();
        let mut bits = 0u8;
        if mode.contains(TermMode::SGR_MOUSE) {
            bits |= MOUSE_MODE_SGR;
        }
        if mode.contains(TermMode::MOUSE_REPORT_CLICK) {
            bits |= MOUSE_MODE_CLICK;
        }
        if mode.contains(TermMode::MOUSE_MOTION) {
            bits |= MOUSE_MODE_MOTION;
        }
        if mode.contains(TermMode::MOUSE_DRAG) {
            bits |= MOUSE_MODE_DRAG;
        }
        self.mouse_modes.store(bits, Ordering::Relaxed);
    }

    pub fn mouse_modes_handle(&self) -> Arc<AtomicU8> {
        Arc::clone(&self.mouse_modes)
    }

    /// 按 alacritty 当前 mode 位拼接关闭序列，只复位实际启用的 TUI modes，
    /// 避免幂等 no-op 喂入 parser 产生多余 MouseCursorDirty events。
    fn reset_leaked_tui_modes(&mut self) {
        let mode = *self.term.mode();
        let mut seq: Vec<u8> = Vec::with_capacity(48);
        if mode.intersects(TermMode::MOUSE_MODE) {
            seq.extend_from_slice(b"\x1b[?1000l\x1b[?1002l\x1b[?1003l");
        }
        if mode.contains(TermMode::SGR_MOUSE) {
            seq.extend_from_slice(b"\x1b[?1006l");
        }
        if mode.contains(TermMode::UTF8_MOUSE) {
            seq.extend_from_slice(b"\x1b[?1005l");
        }
        if mode.contains(TermMode::FOCUS_IN_OUT) {
            seq.extend_from_slice(b"\x1b[?1004l");
        }
        if mode.contains(TermMode::BRACKETED_PASTE) {
            seq.extend_from_slice(b"\x1b[?2004l");
        }
        if !seq.is_empty() {
            self.parser.advance(&mut self.term, &seq);
        }
    }

    /// 检测内容或样式变化（文字、flags、颜色）标记脏行
    fn mark_dirty_for_content_drift(&mut self) {
        let grid = self.term.grid();
        let cols = grid.columns();
        let display_offset = grid.display_offset() as i32;
        let rows = grid.screen_lines();
        let colors = self.term.colors();
        let mut drifted = Vec::new();
        for row_idx in 0..rows.min(self.cached_lines.len()) {
            if self.dirty_rows.get(row_idx).copied().unwrap_or(true) {
                continue;
            }
            let cached = &self.cached_lines[row_idx];
            if cached.cells.len() != cols {
                drifted.push(row_idx);
                continue;
            }
            let line = Line(row_idx as i32 - display_offset);
            let row = &grid[line];
            let mut changed = false;
            for col_idx in 0..cols {
                let cell = &row[Column(col_idx)];
                let wide_spacer = cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
                let ch = if wide_spacer { ' ' } else { cell.c };
                let cc = &cached.cells[col_idx];
                if ch != cc.ch
                    || wide_spacer != cc.wide_spacer()
                    || cell.flags.contains(Flags::INVERSE) != cc.inverse()
                    || cell.flags.contains(Flags::BOLD) != cc.bold()
                    || cell.flags.contains(Flags::DIM) != cc.dim()
                    || cell.flags.contains(Flags::HIDDEN) != cc.hidden()
                    || cell.flags.contains(Flags::ITALIC) != cc.italic()
                    || cell.flags.contains(Flags::UNDERLINE)
                        != cc.flags.contains(TerminalCellFlags::UNDERLINE)
                    || cell.flags.contains(Flags::DOUBLE_UNDERLINE) != cc.double_underline()
                    || cell.flags.contains(Flags::UNDERCURL) != cc.undercurl()
                    || cell.flags.contains(Flags::DOTTED_UNDERLINE) != cc.dotted_underline()
                    || cell.flags.contains(Flags::DASHED_UNDERLINE) != cc.dashed_underline()
                    || cell.flags.contains(Flags::STRIKEOUT) != cc.strikeout()
                {
                    changed = true;
                    break;
                }
                let cached_style = self
                    .styles
                    .get(cc.style_id as usize)
                    .cloned()
                    .unwrap_or_default();
                let current_fg = color_snapshot(cell.fg, colors);
                let current_bg = color_snapshot(cell.bg, colors);
                if current_fg != cached_style.fg || current_bg != cached_style.bg {
                    changed = true;
                    break;
                }
            }
            if changed {
                drifted.push(row_idx);
            }
        }
        for row_idx in drifted {
            self.mark_dirty_row(row_idx);
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.term.resize(TerminalDimensions {
            cols: cols.max(2),
            rows: rows.max(1),
        });
        self.mark_all_dirty();
    }

    pub fn scroll_lines(&mut self, lines: i32) {
        if lines == 0 {
            return;
        }
        let before = self.term.grid().display_offset() as i32;
        self.term.scroll_display(Scroll::Delta(lines));
        let after = self.term.grid().display_offset() as i32;
        self.mark_dirty_for_scroll_delta(after - before);
    }

    pub fn scroll_to_bottom(&mut self) {
        let before = self.term.grid().display_offset() as i32;
        self.term.scroll_display(Scroll::Bottom);
        let after = self.term.grid().display_offset() as i32;
        self.mark_dirty_for_scroll_delta(after - before);
    }

    /// Scroll the viewport so that `display_offset == offset` (clamped to
    /// `[0, history_size]`). Used by scrollbar drag and Find navigation.
    pub fn scroll_to_offset(&mut self, offset: usize) {
        let history = self.term.history_size();
        let target = offset.min(history) as i32;
        let current = self.term.grid().display_offset() as i32;
        let delta = target - current;
        if delta != 0 {
            self.term.scroll_display(Scroll::Delta(delta));
            let after = self.term.grid().display_offset() as i32;
            self.mark_dirty_for_scroll_delta(after - current);
        }
    }

    /// Clear visible output and scrollback for application-level Clear Buffer.
    pub fn clear_visible_screen(&mut self) {
        self.clear_visible_screen_impl(false);
    }

    pub fn clear_visible_screen_preserving_prompt_prefix(&mut self) {
        self.clear_visible_screen_impl(true);
    }

    fn clear_visible_screen_impl(&mut self, preserve_prompt_prefix: bool) {
        let prompt_prefix = preserve_prompt_prefix
            .then(|| self.prompt_prefix_before_cursor())
            .flatten();

        Handler::clear_screen(&mut self.term, ClearMode::All);
        Handler::clear_screen(&mut self.term, ClearMode::Saved);
        if let Some(prompt_prefix) = prompt_prefix {
            self.restore_prompt_prefix(prompt_prefix);
        }
        self.mark_all_dirty();
    }

    fn prompt_prefix_before_cursor(&self) -> Option<TerminalPromptPrefixSnapshot> {
        let grid = self.term.grid();
        if grid.display_offset() != 0 || self.term.mode().contains(TermMode::ALT_SCREEN) {
            return None;
        }

        let cursor = grid.cursor.point;
        let cursor_col = cursor.column.0.min(grid.columns());
        if cursor_col == 0 {
            return None;
        }

        let cells = (0..cursor_col)
            .map(|col| grid[cursor.line][Column(col)].clone())
            .collect::<Vec<_>>();
        if !cells.iter().any(|cell| !cell.is_empty()) {
            return None;
        }

        Some(TerminalPromptPrefixSnapshot { cursor_col, cells })
    }

    fn restore_prompt_prefix(&mut self, prompt_prefix: TerminalPromptPrefixSnapshot) {
        let grid = self.term.grid_mut();
        let cols = grid.columns();
        if cols == 0 {
            return;
        }

        let cursor_line = grid.cursor.point.line;
        for (col, cell) in prompt_prefix.cells.into_iter().take(cols).enumerate() {
            grid[cursor_line][Column(col)] = cell;
        }
        grid.cursor.point.column = Column(prompt_prefix.cursor_col.min(cols.saturating_sub(1)));
    }

    pub fn start_selection(&mut self, ty: SelectionType, point: Point, side: Side) {
        let before = self.selection_range();
        self.term.selection = Some(Selection::new(ty, point, side));
        let after = self.selection_range();
        self.mark_dirty_selection_change(before, after);
        self.mark_dirty_point(point);
    }

    pub fn update_selection(&mut self, point: Point, side: Side) {
        let before = self.selection_range();
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, side);
            let after = self.selection_range();
            self.mark_dirty_selection_change(before, after);
        }
    }

    pub fn clear_selection(&mut self) {
        let before = self.selection_range();
        self.term.selection = None;
        self.mark_dirty_selection_change(before, None);
    }

    pub fn clear_dirty_rows(&mut self) {
        self.ensure_dirty_rows_len();
        self.dirty_rows.fill(false);
    }

    fn dirty_state(&self) -> TerminalDirtyState {
        let grid = self.term.grid();
        TerminalDirtyState {
            rows: grid.screen_lines(),
            cols: grid.columns(),
            display_offset: grid.display_offset() as i32,
            history_size: grid.history_size(),
            cursor_line: grid.cursor.point.line.0,
        }
    }

    fn ensure_dirty_rows_len(&mut self) {
        let rows = self.term.grid().screen_lines();
        if self.dirty_rows.len() != rows {
            self.dirty_rows.resize(rows, true);
        }
    }

    fn mark_all_dirty(&mut self) {
        self.ensure_dirty_rows_len();
        self.dirty_rows.fill(true);
    }

    fn mark_dirty_row(&mut self, row: usize) {
        self.ensure_dirty_rows_len();
        if let Some(slot) = self.dirty_rows.get_mut(row) {
            *slot = true;
        }
    }

    fn mark_dirty_cursor_span(&mut self, before: TerminalDirtyState, after: TerminalDirtyState) {
        let start = before.cursor_line.min(after.cursor_line);
        let end = before.cursor_line.max(after.cursor_line);
        for line in start..=end {
            let row = line + after.display_offset;
            if row >= 0 {
                self.mark_dirty_row(row as usize);
            }
        }
    }

    fn mark_dirty_for_change(&mut self, before: TerminalDirtyState, after: TerminalDirtyState) {
        if before.rows != after.rows
            || before.cols != after.cols
            || before.display_offset != after.display_offset
            || before.history_size != after.history_size
        {
            self.mark_all_dirty();
            return;
        }
        self.mark_dirty_cursor_span(before, after);
    }

    fn mark_dirty_for_scroll_delta(&mut self, delta: i32) {
        self.ensure_dirty_rows_len();
        if delta == 0 {
            return;
        }

        let rows = self.dirty_rows.len();
        let shift = delta.unsigned_abs() as usize;
        if shift >= rows || self.cached_lines.len() != rows {
            self.mark_all_dirty();
            return;
        }

        let old_dirty = self.dirty_rows.clone();
        let old_lines = self.cached_lines.clone();
        for row in 0..rows {
            let source = if delta > 0 {
                row.checked_sub(shift)
            } else {
                row.checked_add(shift).filter(|source| *source < rows)
            };

            if let Some(source) = source {
                self.dirty_rows[row] = old_dirty.get(source).copied().unwrap_or(true);
                self.cached_lines[row] = old_lines[source].clone();
            } else {
                self.dirty_rows[row] = true;
            }
        }
    }

    fn mark_dirty_selection_change(
        &mut self,
        before: Option<SelectionRange>,
        after: Option<SelectionRange>,
    ) {
        if before == after {
            return;
        }
        if let Some(range) = before {
            self.mark_dirty_selection_range(range);
        }
        if let Some(range) = after {
            self.mark_dirty_selection_range(range);
        }
    }

    fn mark_dirty_selection_range(&mut self, range: SelectionRange) {
        let display_offset = self.term.grid().display_offset() as i32;
        for line in range.start.line.0..=range.end.line.0 {
            let row = line + display_offset;
            if row >= 0 {
                self.mark_dirty_row(row as usize);
            }
        }
    }

    fn mark_dirty_point(&mut self, point: Point) {
        let row = point.line.0 + self.term.grid().display_offset() as i32;
        if row >= 0 {
            self.mark_dirty_row(row as usize);
        }
    }

    pub fn mark_dirty_for_marked_text(
        &mut self,
        before: Option<&MarkedText>,
        after: Option<&MarkedText>,
    ) {
        let cursor = self.term.grid().cursor.point;
        if let Some(marked) = before {
            self.mark_dirty_marked_text_range(cursor, marked);
        }
        if let Some(marked) = after {
            self.mark_dirty_marked_text_range(cursor, marked);
        }
    }

    fn mark_dirty_marked_text_range(&mut self, cursor: Point, marked: &MarkedText) {
        if marked.is_empty() {
            return;
        }
        let cols = self.term.grid().columns().max(1);
        let display_offset = self.term.grid().display_offset() as i32;
        let mut line = cursor.line.0;
        let mut col = cursor.column.0;
        for ch in marked.text.chars() {
            let width = unicode_width::UnicodeWidthChar::width(ch)
                .unwrap_or(1)
                .max(1);
            while col + width > cols {
                line += 1;
                col = 0;
            }
            let row = line + display_offset;
            if row >= 0 {
                self.mark_dirty_row(row as usize);
            }
            col += width;
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        self.term.selection_to_string()
    }

    pub fn selection_range(&self) -> Option<SelectionRange> {
        self.term
            .selection
            .as_ref()
            .and_then(|sel| sel.to_range(&self.term))
    }

    pub fn columns(&self) -> usize {
        self.term.grid().columns()
    }

    pub fn screen_lines(&self) -> usize {
        self.term.grid().screen_lines()
    }

    pub fn bracketed_paste_enabled(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    pub fn event_response_bytes_with_window_size(
        &self,
        event: &Event,
        window_size: WindowSize,
        palette: &TerminalPalette,
    ) -> Option<Vec<u8>> {
        terminal_event_response_bytes_with_window_size_and_colors(
            event,
            window_size,
            Some(self.term.colors()),
            palette,
        )
    }

    /// Run a regex over the entire buffer (scrollback + visible) and return
    /// every match in document order. Returns `None` when the pattern fails
    /// to compile (caller should treat as "no matches"). Empty pattern is
    /// also treated as no-match to avoid the regex DFA matching every cell.
    pub fn find_all(&self, query: &str) -> Option<Vec<FindMatchRange>> {
        if query.is_empty() {
            return Some(Vec::new());
        }
        let mut regex = RegexSearch::new(query).ok()?;

        let topmost = self.term.topmost_line();
        let bottommost = self.term.bottommost_line();
        let last_col = self.term.last_column();
        let mut start = Point::new(topmost, Column(0));
        let end = Point::new(bottommost, last_col);
        let mut matches = Vec::new();

        // Hard cap so a pathological pattern like `.*` over a 50k scrollback
        // doesn't lock up the UI thread. 4096 is well past anything useful.
        const MAX_MATCHES: usize = 4096;

        while matches.len() < MAX_MATCHES && start <= end {
            let Some(m) = self.term.regex_search_right(&mut regex, start, end) else {
                break;
            };
            let match_end = *m.end();
            matches.push(FindMatchRange::from(m));

            // Advance one cell past the match end. Use Boundary::None so
            // wrapping past the bottom-right cell exits the loop cleanly.
            let next = match_end.add(&self.term, Boundary::None, 1);
            if next <= match_end {
                break;
            }
            start = next;
        }

        Some(matches)
    }

    pub fn cursor_renderable(&self) -> (TerminalCursorShape, bool, bool) {
        let style = self.term.cursor_style();
        let mode = self.term.mode();
        let visible =
            mode.contains(TermMode::SHOW_CURSOR) && !matches!(style.shape, AnsiCursorShape::Hidden);
        (
            TerminalCursorShape::from(style.shape),
            style.blinking,
            visible,
        )
    }

    pub fn snapshot(
        &mut self,
        find_matches: &[FindMatchRange],
        find_focus_index: Option<usize>,
    ) -> TerminalGridSnapshot {
        self.snapshot_with_marked_text(find_matches, find_focus_index, None)
    }

    /// 与 `snapshot` 等价；额外接受 IME marked text，并按 Warp 的写法
    /// (`app/src/terminal/grid_renderer.rs:639-695`) 把 marked 字符从光标
    /// 起逐 cell 覆盖到 grid 上：每个被覆盖的 cell 加 underline，CJK 宽字符
    /// 占两个 cell（紧邻的下一个 cell 标 wide_spacer 并跳过）。
    pub fn snapshot_with_marked_text(
        &mut self,
        find_matches: &[FindMatchRange],
        find_focus_index: Option<usize>,
        marked_text: Option<&MarkedText>,
    ) -> TerminalGridSnapshot {
        self.ensure_dirty_rows_len();
        let selection_range = self.selection_range();
        let (cursor_shape, cursor_blinking, cursor_visible) = self.cursor_renderable();
        let mode = *self.term.mode();
        let grid = self.term.grid();
        let colors = self.term.colors();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let display_offset = grid.display_offset() as i32;
        let cursor = grid.cursor.point;

        if self.cached_lines.len() != rows {
            self.cached_lines.clear();
            self.dirty_rows.resize(rows, true);
        }

        let mut lines = Vec::with_capacity(rows);
        for row_idx in 0..rows {
            if !self.dirty_rows.get(row_idx).copied().unwrap_or(true) {
                if let Some(cached) = self.cached_lines.get(row_idx) {
                    if cached.cells.len() == cols {
                        lines.push(cached.clone());
                        continue;
                    }
                }
            }

            let line = Line(row_idx as i32 - display_offset);
            let row = &grid[line];
            let text_len = row.line_length().0.min(cols);
            let mut text = String::new();
            let mut cells = Vec::with_capacity(cols);

            for col_idx in 0..cols {
                let cell = &row[Column(col_idx)];
                let wide_spacer = cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
                let ch = if wide_spacer { ' ' } else { cell.c };
                let content = terminal_cell_content(cell, wide_spacer);

                if col_idx < text_len && !wide_spacer {
                    text.push_str(&content);
                }

                let cell_point = Point::new(line, Column(col_idx));
                let selected = selection_range
                    .as_ref()
                    .map(|range| range.contains(cell_point))
                    .unwrap_or(false);
                let hyperlink = cell.hyperlink().map(|link| link.uri().to_string());

                let mut find_match = false;
                let mut find_focus = false;
                for (idx, range) in find_matches.iter().enumerate() {
                    if range.contains(cell_point) {
                        find_match = true;
                        if Some(idx) == find_focus_index {
                            find_focus = true;
                            break;
                        }
                    }
                }

                let mut flags = TerminalCellFlags::empty();
                flags.set(TerminalCellFlags::BOLD, cell.flags.contains(Flags::BOLD));
                flags.set(TerminalCellFlags::DIM, cell.flags.contains(Flags::DIM));
                flags.set(
                    TerminalCellFlags::ITALIC,
                    cell.flags.contains(Flags::ITALIC),
                );
                flags.set(
                    TerminalCellFlags::UNDERLINE,
                    cell.flags.contains(Flags::UNDERLINE),
                );
                flags.set(
                    TerminalCellFlags::DOUBLE_UNDERLINE,
                    cell.flags.contains(Flags::DOUBLE_UNDERLINE),
                );
                flags.set(
                    TerminalCellFlags::UNDERCURL,
                    cell.flags.contains(Flags::UNDERCURL),
                );
                flags.set(
                    TerminalCellFlags::DOTTED_UNDERLINE,
                    cell.flags.contains(Flags::DOTTED_UNDERLINE),
                );
                flags.set(
                    TerminalCellFlags::DASHED_UNDERLINE,
                    cell.flags.contains(Flags::DASHED_UNDERLINE),
                );
                flags.set(
                    TerminalCellFlags::STRIKEOUT,
                    cell.flags.contains(Flags::STRIKEOUT),
                );
                flags.set(
                    TerminalCellFlags::HIDDEN,
                    cell.flags.contains(Flags::HIDDEN),
                );
                flags.set(
                    TerminalCellFlags::INVERSE,
                    cell.flags.contains(Flags::INVERSE),
                );
                flags.set(
                    TerminalCellFlags::WIDE_CHAR,
                    cell.flags.contains(Flags::WIDE_CHAR),
                );
                flags.set(
                    TerminalCellFlags::LEADING_WIDE_CHAR_SPACER,
                    cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER),
                );
                flags.set(TerminalCellFlags::WIDE_CHAR_SPACER, wide_spacer);
                flags.set(TerminalCellFlags::SELECTED, selected);
                flags.set(TerminalCellFlags::FIND_MATCH, find_match);
                flags.set(TerminalCellFlags::FIND_FOCUS, find_focus);
                let style_id = intern_terminal_cell_style(
                    &mut self.styles,
                    &mut self.style_map,
                    TerminalCellStyleSnapshot {
                        fg: color_snapshot(cell.fg, colors),
                        bg: color_snapshot(cell.bg, colors),
                        underline_color: cell
                            .underline_color()
                            .map(|color| color_snapshot(color, colors)),
                    },
                );

                cells.push(TerminalGridCellSnapshot {
                    ch,
                    content,
                    style_id,
                    flags,
                    hyperlink,
                });
            }

            let snapshot_line = TerminalGridLineSnapshot { text, cells };
            if row_idx == self.cached_lines.len() {
                self.cached_lines.push(snapshot_line.clone());
            } else if let Some(slot) = self.cached_lines.get_mut(row_idx) {
                *slot = snapshot_line.clone();
            }
            lines.push(snapshot_line);
        }

        let raw_cursor_row =
            (cursor.line.0 + display_offset).clamp(0, rows.saturating_sub(1) as i32) as usize;
        let raw_cursor_col = cursor.column.0.min(cols.saturating_sub(1));
        let active_marked_text = marked_text.filter(|marked| !marked.is_empty());
        let (cursor_row, cursor_col) = active_marked_text
            .map(|marked| {
                let render_index = raw_cursor_col + marked.cell_len();
                (
                    (raw_cursor_row + render_index / cols).min(rows.saturating_sub(1)),
                    render_index % cols,
                )
            })
            .unwrap_or((raw_cursor_row, raw_cursor_col));

        // === IME marked text 注入（Warp grid_renderer 的等价实现） ===
        // 从光标当前 cell 起逐字符覆盖：UNDERLINE + 宽字符占两 cell（spacer
        // 紧随其后）。整段会跨行，但只往下游 / 当前 viewport 写，不动 scrollback。
        if let Some(marked) = marked_text.filter(|m| !m.is_empty()) {
            use unicode_width::UnicodeWidthChar;
            let mut row_idx = raw_cursor_row;
            let mut col_idx = raw_cursor_col;
            let marked_style_id = intern_terminal_cell_style(
                &mut self.styles,
                &mut self.style_map,
                TerminalCellStyleSnapshot {
                    fg: TerminalColorSnapshot::Named("foreground"),
                    bg: TerminalColorSnapshot::Named("background"),
                    underline_color: None,
                },
            );
            'inject: for ch in marked.text.chars() {
                let width = ch.width().unwrap_or(1).max(1);
                if row_idx >= rows {
                    break;
                }
                while col_idx + width > cols {
                    row_idx += 1;
                    col_idx = 0;
                    if row_idx >= rows {
                        break 'inject;
                    }
                }
                let line = &mut lines[row_idx];
                if let Some(cell) = line.cells.get_mut(col_idx) {
                    cell.ch = ch;
                    cell.content = {
                        let mut buf = [0u8; 4];
                        Arc::from(ch.encode_utf8(&mut buf) as &str)
                    };
                    cell.flags.insert(TerminalCellFlags::UNDERLINE);
                    cell.flags.remove(
                        TerminalCellFlags::DOUBLE_UNDERLINE
                            | TerminalCellFlags::STRIKEOUT
                            | TerminalCellFlags::HIDDEN
                            | TerminalCellFlags::WIDE_CHAR_SPACER,
                    );
                    cell.style_id = marked_style_id;
                    cell.flags.remove(TerminalCellFlags::INVERSE);
                }
                if width >= 2 {
                    if let Some(spacer) = line.cells.get_mut(col_idx + 1) {
                        spacer.ch = ' ';
                        spacer.content = Arc::from(" ");
                        spacer.flags.insert(
                            TerminalCellFlags::WIDE_CHAR_SPACER | TerminalCellFlags::UNDERLINE,
                        );
                        spacer.flags.remove(
                            TerminalCellFlags::DOUBLE_UNDERLINE
                                | TerminalCellFlags::STRIKEOUT
                                | TerminalCellFlags::HIDDEN,
                        );
                        spacer.style_id = marked_style_id;
                        spacer.flags.remove(TerminalCellFlags::INVERSE);
                    }
                }
                col_idx += width;
            }
        }

        TerminalGridSnapshot {
            cols,
            rows,
            cursor_row,
            cursor_col,
            cursor_shape,
            cursor_blinking,
            cursor_visible,
            marked_text_active: active_marked_text.is_some(),
            display_offset: display_offset.max(0) as usize,
            history_size: grid.history_size(),
            dirty_rows: self.dirty_rows.clone(),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            mouse_report_click: mode.contains(TermMode::MOUSE_REPORT_CLICK),
            mouse_report_motion: mode.contains(TermMode::MOUSE_MOTION),
            mouse_report_drag: mode.contains(TermMode::MOUSE_DRAG),
            sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
            input_modes: TerminalInputModes::from_alacritty(mode),
            styles: self.styles.clone(),
            lines,
        }
    }

    /// Drain pending alacritty events (Title / ResetTitle / Bell / etc.). The runtime
    /// wires this into the reader thread so emitter-side `try_send` doesn't fill the
    /// channel and silently drop later events.
    pub fn drain_events(&self) -> Vec<Event> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }
}

/// 字面扫描 bytes 中是否含 ?47l / ?1047l / ?1049l —— vte 把 ?47/?1047 当
/// Unknown 不会翻转 TermMode::ALT_SCREEN，且 alt_before==alt_after 的净零
/// 进出也漏边沿。这里基于字面命中触发 reset，由 reset_leaked_tui_modes
/// 进一步按实际 mode 决定是否发字节。
fn bytes_contain_alt_screen_exit(bytes: &[u8]) -> bool {
    bytes_contains_subseq(bytes, b"\x1b[?1049l")
        || bytes_contains_subseq(bytes, b"\x1b[?1047l")
        || bytes_contains_subseq(bytes, b"\x1b[?47l")
}

fn bytes_contains_subseq(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn terminal_output_should_snap_to_bottom_after_clear(bytes: &[u8]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b {
            if bytes.get(index + 1) == Some(&b'c') {
                return true;
            }
            if bytes.get(index + 1) == Some(&b'[') {
                if csi_sequence_clears_full_screen(bytes, index + 2) {
                    return true;
                }
            }
        } else if bytes[index] == 0x9b && csi_sequence_clears_full_screen(bytes, index + 1) {
            return true;
        }
        index += 1;
    }
    false
}

fn csi_sequence_clears_full_screen(bytes: &[u8], params_start: usize) -> bool {
    let mut final_index = params_start;
    while let Some(byte) = bytes.get(final_index) {
        if (0x40..=0x7e).contains(byte) {
            break;
        }
        final_index += 1;
    }

    if bytes.get(final_index) != Some(&b'J') {
        return false;
    }

    bytes[params_start..final_index]
        .split(|byte| *byte == b';' || *byte == b':')
        .any(|param| param == b"2" || param == b"3")
}

struct TerminalRuntimeState {
    session_id: String,
    connected: bool,
    status: String,
    title: Option<String>,
    grid: Option<TerminalGridCore>,
    revision: u64,
    window_size: WindowSize,
    bell_pulse: u64,
    find_query: Option<String>,
    find_matches: Vec<FindMatchRange>,
    find_current: Option<usize>,
    find_pulse: u64,
    /// IME 合成中的 marked text。Warp 在 `TerminalModel::set_marked_text` 中
    /// 维护，提交时清空（`replace_text_in_range` 会调用 `clear`），我们采用
    /// 完全相同的语义。
    marked_text: Option<MarkedText>,
    clipboard_store_requests: VecDeque<TerminalClipboardStoreRequest>,
    clipboard_load_requests: VecDeque<TerminalClipboardLoadRequest>,
    /// (revision, snapshot) — built once per revision, Arc::clone on subsequent reads.
    cached_snapshot: Option<(u64, Arc<TerminalRuntimeSnapshot>)>,
    palette: TerminalPalette,
    /// shell integration marker 触发后翻 true，UI 由占位切到 grid。
    bootstrapped: bool,
    /// 扫 bootstrap marker 的跨调用 buffer（marker 可能横跨多次 process_output）。
    bootstrap_scan_buf: Vec<u8>,
    /// 占位文本 "Starting {name}..." 使用的 shell 名。
    shell_display_name: Option<String>,
    /// 最近一次 OSC 7 上报的本地 cwd（远程 tab 不解析，恒为 None）。
    local_cwd: Option<PathBuf>,
    /// OSC 7 扫描跨 chunk 拼接缓冲；上限 OSC7_BUF_CAP，超出丢弃旧字节。
    osc7_scan_buf: Vec<u8>,
    /// 录制中为 Some；process_output 旁路 raw bytes 进去。
    recorder: Option<TerminalRecorder>,
}

impl TerminalRuntimeState {
    fn new(
        session_id: &str,
        connected: bool,
        status: impl Into<String>,
        cols: u16,
        rows: u16,
    ) -> Self {
        Self {
            session_id: session_id.to_string(),
            connected,
            status: status.into(),
            title: None,
            grid: Some(TerminalGridCore::new(
                usize::from(cols),
                usize::from(rows),
                2_000,
            )),
            revision: 0,
            window_size: terminal_window_size(
                cols,
                rows,
                DEFAULT_CELL_PIXEL_WIDTH,
                DEFAULT_CELL_PIXEL_HEIGHT,
            ),
            bell_pulse: 0,
            find_query: None,
            find_matches: Vec::new(),
            find_current: None,
            find_pulse: 0,
            marked_text: None,
            clipboard_store_requests: VecDeque::new(),
            clipboard_load_requests: VecDeque::new(),
            cached_snapshot: None,
            palette: TerminalPalette::default(),
            bootstrapped: false,
            bootstrap_scan_buf: Vec::new(),
            shell_display_name: None,
            local_cwd: None,
            osc7_scan_buf: Vec::new(),
            recorder: None,
        }
    }

    fn snapshot_arc(&mut self) -> Arc<TerminalRuntimeSnapshot> {
        if let Some((rev, snap)) = &self.cached_snapshot {
            if *rev == self.revision {
                return Arc::clone(snap);
            }
        }
        let snap = Arc::new(self.build_snapshot());
        self.cached_snapshot = Some((self.revision, Arc::clone(&snap)));
        snap
    }

    fn snapshot_arc_for_render(&mut self) -> Arc<TerminalRuntimeSnapshot> {
        let snap = self.snapshot_arc();
        if let Some(grid) = &mut self.grid {
            grid.clear_dirty_rows();
        }
        snap
    }

    fn build_snapshot(&mut self) -> TerminalRuntimeSnapshot {
        let grid = self
            .grid
            .as_mut()
            .map(|grid| {
                grid.snapshot_with_marked_text(
                    &self.find_matches,
                    self.find_current,
                    self.marked_text.as_ref(),
                )
            })
            .unwrap_or_else(TerminalGridSnapshot::empty);
        let lines: Vec<String> = if grid.lines.is_empty() {
            vec![String::new()]
        } else {
            grid.lines.iter().map(|line| line.text.clone()).collect()
        };

        TerminalRuntimeSnapshot {
            session_id: self.session_id.clone(),
            connected: self.connected,
            status: self.status.clone(),
            title: self.title.clone(),
            bootstrapped: self.bootstrapped,
            shell_display_name: self.shell_display_name.clone(),
            bell_pulse: self.bell_pulse,
            find_pulse: self.find_pulse,
            find_query: self.find_query.clone(),
            find_match_count: self.find_matches.len(),
            find_current_match: self.find_current,
            marked_text: self.marked_text.clone(),
            lines,
            grid,
            local_cwd: self.local_cwd.clone(),
        }
    }

    /// Recompute `find_matches` from the current grid state for the saved
    /// `find_query`. Adjusts `find_current` to stay in range. Returns true
    /// when matches actually changed.
    fn refresh_find_matches(&mut self) {
        let Some(query) = self.find_query.as_deref() else {
            return;
        };
        let new_matches = self
            .grid
            .as_ref()
            .and_then(|grid| grid.find_all(query))
            .unwrap_or_default();
        if new_matches != self.find_matches {
            self.find_matches = new_matches;
            self.find_current = if self.find_matches.is_empty() {
                None
            } else {
                Some(0)
            };
            self.find_pulse = self.find_pulse.wrapping_add(1);
        }
    }

    /// 扫描 OSC 7 序列上报的本地 cwd。每次 chunk 都调用，命中即更新 `local_cwd`。
    /// 与 bootstrap marker 不同，OSC 7 持续发，buf 需要做有上限的滚动管理。
    fn scan_osc7(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut combined = std::mem::take(&mut self.osc7_scan_buf);
        combined.extend_from_slice(bytes);
        // 处理所有完整 sequence，得到剩余 tail。
        let tail = consume_osc7_sequences(&combined, |path| {
            if let Some(cwd) = parse_osc7_payload(path) {
                self.local_cwd = Some(cwd);
            }
        });
        self.osc7_scan_buf = tail;
    }

    /// 扫 shell integration marker。一旦命中翻 `bootstrapped`，停止扫描。
    /// marker 可能跨多次 process_output 调用，用 scan_buf 拼前一次尾部。
    fn scan_bootstrap_marker(&mut self, bytes: &[u8]) {
        if self.bootstrapped || bytes.is_empty() {
            return;
        }
        let marker = crate::shell_integration::BOOTSTRAP_MARKER;
        let buf = &mut self.bootstrap_scan_buf;
        // 拼前一次尾部 + 本次输入，扫子串
        let mut combined = std::mem::take(buf);
        combined.extend_from_slice(bytes);
        if combined.windows(marker.len()).any(|w| w == marker) {
            self.bootstrapped = true;
            return;
        }
        // 未命中：保留末尾 marker.len()-1 字节，防止 marker 被切成两半
        let keep = marker.len().saturating_sub(1).min(combined.len());
        let drop_n = combined.len() - keep;
        combined.drain(..drop_n);
        *buf = combined;
    }
}

pub fn terminal_event_response_bytes(event: &Event) -> Option<Vec<u8>> {
    match event {
        Event::PtyWrite(text) => Some(text.as_bytes().to_vec()),
        _ => None,
    }
}

pub fn terminal_event_response_bytes_with_window_size(
    event: &Event,
    window_size: WindowSize,
    palette: &TerminalPalette,
) -> Option<Vec<u8>> {
    terminal_event_response_bytes_with_window_size_and_colors(event, window_size, None, palette)
}

fn terminal_event_response_bytes_with_window_size_and_colors(
    event: &Event,
    window_size: WindowSize,
    colors: Option<&Colors>,
    palette: &TerminalPalette,
) -> Option<Vec<u8>> {
    match event {
        Event::TextAreaSizeRequest(format) => Some(format(window_size).into_bytes()),
        Event::ColorRequest(index, format) => terminal_color_request_rgb(*index, colors, palette)
            .map(|color| format(color).into_bytes()),
        _ => terminal_event_response_bytes(event),
    }
}

pub fn terminal_clipboard_store_request_for_event(
    event: &Event,
) -> Option<TerminalClipboardStoreRequest> {
    match event {
        Event::ClipboardStore(_, text) => {
            Some(TerminalClipboardStoreRequest { text: text.clone() })
        }
        _ => None,
    }
}

pub fn terminal_clipboard_load_request_for_event(
    event: &Event,
) -> Option<TerminalClipboardLoadRequest> {
    match event {
        Event::ClipboardLoad(_, formatter) => Some(TerminalClipboardLoadRequest {
            formatter: Arc::clone(formatter),
        }),
        _ => None,
    }
}

fn apply_term_event(state: &mut TerminalRuntimeState, event: Event) {
    match event {
        Event::Title(title) => state.title = Some(title),
        Event::ResetTitle => state.title = None,
        Event::Bell => state.bell_pulse = state.bell_pulse.wrapping_add(1),
        Event::ClipboardStore(_, text) => state
            .clipboard_store_requests
            .push_back(TerminalClipboardStoreRequest { text }),
        Event::ClipboardLoad(_, formatter) => state
            .clipboard_load_requests
            .push_back(TerminalClipboardLoadRequest { formatter }),
        // MouseCursorDirty / Wakeup / etc. don't surface in this spike yet.
        _ => {}
    }
}

/// `PtySink` impl for `TerminalRuntimeState`. The PTY event loop holds the
/// `Arc<FairMutex<TerminalRuntimeState>>`, locks it inside `pty_read`, and
/// calls these methods. Mirrors how Warp's event loop calls
/// `state.parser.parse_bytes(terminal.deref_mut(), bytes, writer)` —
/// parser advances the terminal model and any reply bytes flow back into
/// the loop's `write_list`.
impl PtySink for TerminalRuntimeState {
    fn process_output(&mut self, bytes: &[u8]) -> Vec<Cow<'static, [u8]>> {
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.push_bytes(bytes);
        }
        self.scan_bootstrap_marker(bytes);
        self.scan_osc7(bytes);
        let window_size = self.window_size;
        let mut replies: Vec<Cow<'static, [u8]>> = Vec::new();
        let events: Vec<Event> = if let Some(grid) = &mut self.grid {
            grid.process_output(bytes);
            let events = grid.drain_events();
            for event in &events {
                if let Some(reply) =
                    grid.event_response_bytes_with_window_size(event, window_size, &self.palette)
                {
                    replies.push(Cow::Owned(reply));
                }
            }
            events
        } else {
            Vec::new()
        };
        for event in events {
            apply_term_event(self, event);
        }
        // Output may have shifted the buffer (scroll, new text), so any saved
        // find query needs its matches recomputed against the fresh grid.
        self.refresh_find_matches();
        self.revision = self.revision.wrapping_add(1);
        replies
    }

    fn handle_resize(&mut self, size: PtySize) {
        self.window_size = terminal_window_size(
            size.cols,
            size.rows,
            size.pixel_width.max(1),
            size.pixel_height.max(1),
        );
        if let Some(grid) = &mut self.grid {
            grid.resize(usize::from(size.cols), usize::from(size.rows));
        }
        self.revision = self.revision.wrapping_add(1);
    }

    fn clear_visible_screen(&mut self, preserve_prompt_prefix: bool) {
        if let Some(grid) = &mut self.grid {
            if preserve_prompt_prefix {
                grid.clear_visible_screen_preserving_prompt_prefix();
            } else {
                grid.clear_visible_screen();
            }
        }
        self.refresh_find_matches();
        self.revision = self.revision.wrapping_add(1);
    }

    fn mark_disconnected(&mut self, status: String) {
        self.connected = false;
        self.status = status;
        self.revision = self.revision.wrapping_add(1);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteSshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub key_passphrase: Option<String>,
    pub ca_cert: Option<String>,
    pub keep_alive_enabled: bool,
    pub keep_alive_interval: u16,
    pub keep_alive_max_failures: u8,
    pub tcp_connect_timeout: u16,
    pub auth_timeout: u16,
    pub term_encoding: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialPortRuntimeConfig {
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: String,
    pub flow_control: String,
    pub dtr: bool,
    pub rts: bool,
}

struct RemoteTerminalEncoding {
    encoding: &'static Encoding,
    decoder: Decoder,
    encoder: Encoder,
}

impl RemoteTerminalEncoding {
    fn new(label: &str) -> Self {
        let encoding = Encoding::for_label(label.trim().as_bytes()).unwrap_or(UTF_8);
        Self {
            encoding,
            decoder: encoding.new_decoder_without_bom_handling(),
            encoder: encoding.new_encoder(),
        }
    }

    fn is_utf8(&self) -> bool {
        self.encoding == UTF_8
    }

    fn decode_output<'a>(&mut self, data: &'a [u8]) -> Cow<'a, [u8]> {
        if self.is_utf8() {
            return Cow::Borrowed(data);
        }

        let mut output = String::new();
        output.reserve(data.len().saturating_mul(2).saturating_add(16));
        let mut input = data;
        while !input.is_empty() {
            let (result, read, _) = self.decoder.decode_to_string(input, &mut output, false);
            input = &input[read..];
            if matches!(result, CoderResult::OutputFull) {
                output.reserve(input.len().saturating_mul(2).saturating_add(16));
            }
            if read == 0 && !matches!(result, CoderResult::OutputFull) {
                break;
            }
        }

        Cow::Owned(output.into_bytes())
    }

    fn encode_input<'a>(&mut self, data: &'a [u8]) -> Cow<'a, [u8]> {
        if self.is_utf8() {
            return Cow::Borrowed(data);
        }

        let text = String::from_utf8_lossy(data);
        let mut output = Vec::new();
        output.reserve(text.len().saturating_mul(2).saturating_add(16));
        let mut input: &str = text.as_ref();
        while !input.is_empty() {
            let (result, read, _) = self
                .encoder
                .encode_from_utf8_to_vec(input, &mut output, false);
            input = &input[read..];
            if matches!(result, CoderResult::OutputFull) {
                output.reserve(input.len().saturating_mul(2).saturating_add(16));
            }
            if read == 0 && !matches!(result, CoderResult::OutputFull) {
                break;
            }
        }

        Cow::Owned(output)
    }
}

struct RemoteEventLoopHandle {
    request_tx: tokio::sync::mpsc::Sender<ChannelRequest>,
    shutdown: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl Drop for RemoteEventLoopHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = self.request_tx.try_send(ChannelRequest::Close);
    }
}

enum SerialRequest {
    Data(Vec<u8>),
    Close,
}

struct SerialEventLoopHandle {
    request_tx: mpsc::Sender<SerialRequest>,
    shutdown: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl Drop for SerialEventLoopHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = self.request_tx.send(SerialRequest::Close);
    }
}

pub struct LocalTerminalRuntime {
    state: Arc<FairMutex<TerminalRuntimeState>>,
    /// `None` for `failed()` runtimes — they have no PTY and no event loop.
    event_loop: Option<EventLoopHandle>,
    /// Remote SSH sessions are russh channels, not local OS PTYs. They still
    /// feed the same TerminalRuntimeState parser and wakeup/event streams.
    remote_event_loop: Option<RemoteEventLoopHandle>,
    /// Native serial sessions use the serialport crate directly, without
    /// spawning external `screen`/`cu` programs inside our PTY.
    serial_event_loop: Option<SerialEventLoopHandle>,
    /// 仅 remote SSH session 才有：take-once 拿到主 SSH handle，UI 用它开 SFTP。
    ssh_handle_rx: Option<async_channel::Receiver<SshHandle>>,
    /// Taken once by the UI on first `take_wakeup_rx` call. After that
    /// the PTY thread keeps pushing wakeups but they coalesce into the
    /// bounded(1) channel buffer.
    wakeup_rx: Option<async_channel::Receiver<()>>,
    /// Lifecycle channel (child exited, write error). Same one-shot
    /// take semantics as `wakeup_rx`.
    event_rx: Option<async_channel::Receiver<PtyEvent>>,
    /// PTY master fd，用于 tcgetpgrp 查询前台进程
    pty_fd: Option<LocalPtyDescriptor>,
    /// 前台进程是否为 shell（非 ssh/mosh），由 refresh_foreground_status 更新
    shell_is_foreground: Arc<std::sync::atomic::AtomicBool>,
}

impl LocalTerminalRuntime {
    pub fn spawn_local_or_failed(session_id: &str, cols: u16, rows: u16) -> Self {
        Self::spawn_local(session_id, cols, rows).unwrap_or_else(|error| {
            Self::failed(
                session_id,
                format!("failed to start local terminal: {error}"),
            )
        })
    }

    pub fn spawn_local_in_dir_or_failed(
        session_id: &str,
        cwd: &std::path::Path,
        cols: u16,
        rows: u16,
    ) -> Self {
        Self::spawn_local_in_dir(session_id, cwd, cols, rows).unwrap_or_else(|error| {
            Self::failed(
                session_id,
                format!(
                    "failed to start local terminal in {}: {error}",
                    cwd.display()
                ),
            )
        })
    }

    pub fn spawn_local(session_id: &str, cols: u16, rows: u16) -> Result<Self, String> {
        Self::spawn_local_with_cwd(session_id, None, cols, rows)
    }

    pub fn spawn_local_in_dir(
        session_id: &str,
        cwd: &std::path::Path,
        cols: u16,
        rows: u16,
    ) -> Result<Self, String> {
        Self::spawn_local_with_cwd(session_id, Some(cwd), cols, rows)
    }

    fn spawn_local_with_cwd(
        session_id: &str,
        cwd: Option<&std::path::Path>,
        cols: u16,
        rows: u16,
    ) -> Result<Self, String> {
        let shell = default_shell();
        let display = std::path::Path::new(&shell)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("shell")
            .to_string();
        let mut command = build_shell_command_with_integration(&shell, &display);
        command.env("SHELL", &shell);
        // zsh 通过 ZDOTDIR 注入 wrapper .zshrc（其它 shell 在 build_shell_command_with_integration 内部处理）
        if display == "zsh" {
            if let Some(zdotdir) = crate::shell_integration::setup_zsh_integration() {
                command.env("ZDOTDIR", &zdotdir);
            }
        }
        configure_command_environment(&mut command);
        if let Some(cwd) = cwd {
            command.cwd(cwd);
        }
        let runtime = Self::spawn_pty(
            session_id,
            command,
            format!("running local shell: {shell}"),
            "local shell",
            cols,
            rows,
            Some(display),
        )?;
        #[cfg(windows)]
        runtime.mark_bootstrapped_without_placeholder();
        Ok(runtime)
    }

    pub fn spawn_command_or_failed(
        session_id: &str,
        program: &str,
        args: &[&str],
        status: impl Into<String>,
        cols: u16,
        rows: u16,
    ) -> Self {
        Self::spawn_command(session_id, program, args, status, cols, rows).unwrap_or_else(|error| {
            Self::failed(
                session_id,
                format!("failed to start direct terminal command: {error}"),
            )
        })
    }

    pub fn spawn_command(
        session_id: &str,
        program: &str,
        args: &[&str],
        status: impl Into<String>,
        cols: u16,
        rows: u16,
    ) -> Result<Self, String> {
        let mut command = CommandBuilder::new(program);
        for arg in args {
            command.arg(*arg);
        }
        configure_command_environment(&mut command);
        let runtime = Self::spawn_pty(
            session_id,
            command,
            status.into(),
            "direct command",
            cols,
            rows,
            None,
        )?;
        runtime.mark_bootstrapped_without_placeholder();
        Ok(runtime)
    }

    fn mark_bootstrapped_without_placeholder(&self) {
        let mut state = self.state.lock();
        state.bootstrapped = true;
        state.shell_display_name = None;
        state.revision = state.revision.wrapping_add(1);
        state.cached_snapshot = None;
    }

    pub fn spawn_remote_ssh_or_failed(
        session_id: &str,
        config: RemoteSshConfig,
        status: impl Into<String>,
        cols: u16,
        rows: u16,
    ) -> Self {
        Self::spawn_remote_ssh(session_id, config, status, cols, rows).unwrap_or_else(|error| {
            Self::failed(
                session_id,
                format!("failed to start saved SSH session: {error}"),
            )
        })
    }

    pub fn spawn_remote_ssh(
        session_id: &str,
        config: RemoteSshConfig,
        status: impl Into<String>,
        cols: u16,
        rows: u16,
    ) -> Result<Self, String> {
        validate_remote_ssh_config(&config)?;

        let mut runtime_state =
            TerminalRuntimeState::new(session_id, true, status.into(), cols, rows);
        runtime_state.shell_display_name = Some("ssh".to_string());
        // 远程 shell 我们暂不注入 shell integration（要在远端写 rc 麻烦），
        // 没有 marker 时 bootstrapped 永远 false → 一直占位，不能这样。
        // 临时方案：远端 spawn 即视为 bootstrap 完成（直接连上就开始接受用户输入）。
        runtime_state.bootstrapped = true;
        let state = Arc::new(FairMutex::new(runtime_state));
        let (wakeup_tx, wakeup_rx) = async_channel::bounded::<()>(1);
        let (event_tx, event_rx) = async_channel::unbounded::<PtyEvent>();
        let (remote_event_loop, ssh_handle_rx) = spawn_remote_ssh_event_loop(
            session_id.to_string(),
            Arc::clone(&state),
            config,
            wakeup_tx,
            event_tx,
            cols,
            rows,
        )?;

        Ok(Self {
            state,
            event_loop: None,
            remote_event_loop: Some(remote_event_loop),
            serial_event_loop: None,
            ssh_handle_rx: Some(ssh_handle_rx),
            wakeup_rx: Some(wakeup_rx),
            event_rx: Some(event_rx),
            pty_fd: None,
            shell_is_foreground: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        })
    }

    pub fn spawn_serial_or_failed(
        session_id: &str,
        config: SerialPortRuntimeConfig,
        status: impl Into<String>,
        cols: u16,
        rows: u16,
    ) -> Self {
        Self::spawn_serial(session_id, config, status, cols, rows).unwrap_or_else(|error| {
            Self::failed(
                session_id,
                format!("failed to start serial session: {error}"),
            )
        })
    }

    pub fn spawn_serial(
        session_id: &str,
        config: SerialPortRuntimeConfig,
        status: impl Into<String>,
        cols: u16,
        rows: u16,
    ) -> Result<Self, String> {
        validate_serial_port_config(&config)?;

        let mut runtime_state =
            TerminalRuntimeState::new(session_id, true, status.into(), cols, rows);
        runtime_state.bootstrapped = true;
        let state = Arc::new(FairMutex::new(runtime_state));
        let (wakeup_tx, wakeup_rx) = async_channel::bounded::<()>(1);
        let (event_tx, event_rx) = async_channel::unbounded::<PtyEvent>();
        let serial_event_loop = spawn_serial_event_loop(
            session_id.to_string(),
            Arc::clone(&state),
            config,
            wakeup_tx,
            event_tx,
        )?;

        Ok(Self {
            state,
            event_loop: None,
            remote_event_loop: None,
            serial_event_loop: Some(serial_event_loop),
            ssh_handle_rx: None,
            wakeup_rx: Some(wakeup_rx),
            event_rx: Some(event_rx),
            pty_fd: None,
            shell_is_foreground: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        })
    }

    pub fn failed(session_id: &str, status: impl Into<String>) -> Self {
        Self {
            state: Arc::new(FairMutex::new(TerminalRuntimeState {
                session_id: session_id.to_string(),
                connected: false,
                status: status.into(),
                title: None,
                grid: None,
                revision: 0,
                window_size: terminal_window_size(
                    2,
                    1,
                    DEFAULT_CELL_PIXEL_WIDTH,
                    DEFAULT_CELL_PIXEL_HEIGHT,
                ),
                bell_pulse: 0,
                find_query: None,
                find_matches: Vec::new(),
                find_current: None,
                find_pulse: 0,
                marked_text: None,
                clipboard_store_requests: VecDeque::new(),
                clipboard_load_requests: VecDeque::new(),
                cached_snapshot: None,
                palette: TerminalPalette::default(),
                bootstrapped: true,
                bootstrap_scan_buf: Vec::new(),
                shell_display_name: None,
                local_cwd: None,
                osc7_scan_buf: Vec::new(),
                recorder: None,
            })),
            event_loop: None,
            remote_event_loop: None,
            serial_event_loop: None,
            ssh_handle_rx: None,
            wakeup_rx: None,
            event_rx: None,
            pty_fd: None,
            shell_is_foreground: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }

    #[cfg(any(unix, windows))]
    fn spawn_pty(
        session_id: &str,
        command: CommandBuilder,
        status: String,
        exit_label: &'static str,
        cols: u16,
        rows: u16,
        shell_display_name: Option<String>,
    ) -> Result<Self, String> {
        let mut runtime_state = TerminalRuntimeState::new(session_id, true, status, cols, rows);
        runtime_state.shell_display_name = shell_display_name;
        let state = Arc::new(FairMutex::new(runtime_state));

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(pty_size(
                cols,
                rows,
                DEFAULT_CELL_PIXEL_WIDTH,
                DEFAULT_CELL_PIXEL_HEIGHT,
            ))
            .map_err(|error| error.to_string())?;

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| error.to_string())?;
        drop(pair.slave);

        let killer = child.clone_killer();

        #[cfg(unix)]
        let pty_fd = pair.master.as_raw_fd(); // Option<RawFd>
        #[cfg(windows)]
        let pty_fd = None;

        // Unix keeps Warp's single mio Poll shape; Windows uses the
        // portable-pty ConPTY reader/writer handles because portable-pty does
        // not expose Warp's raw NamedPipe event source.
        let event_loop = pty_event_loop::spawn_event_loop(
            Arc::clone(&state) as Arc<FairMutex<TerminalRuntimeState>>,
            pair.master,
            child,
            killer,
        )
        .map_err(|error| format!("spawn {exit_label} event loop: {error}"))?;

        let wakeup_rx = Some(event_loop.wakeup_rx.clone());
        let event_rx = Some(event_loop.event_rx.clone());

        Ok(Self {
            state,
            event_loop: Some(event_loop),
            remote_event_loop: None,
            serial_event_loop: None,
            ssh_handle_rx: None,
            wakeup_rx,
            event_rx,
            pty_fd,
            shell_is_foreground: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        })
    }

    #[cfg(not(any(unix, windows)))]
    fn spawn_pty(
        _session_id: &str,
        _command: CommandBuilder,
        _status: String,
        _exit_label: &'static str,
        _cols: u16,
        _rows: u16,
        _shell_display_name: Option<String>,
    ) -> Result<Self, String> {
        Err("local terminal runtime is not wired for this platform yet".to_string())
    }

    /// Hand the wakeup channel to the UI thread. Wakeups land in a
    /// `bounded(1)` channel — successive PTY bursts coalesce into one
    /// pending wakeup. The UI is expected to wrap the receiver in a
    /// 60 Hz throttle (Warp's `WAKEUP_THROTTLE_PERIOD`,
    /// `view.rs:644`).
    pub fn take_wakeup_rx(&mut self) -> Option<async_channel::Receiver<()>> {
        self.wakeup_rx.take()
    }

    /// Hand the lifecycle event channel (child exited, write error) to
    /// the UI thread. Mirrors how Warp's view subscribes to terminal
    /// events separately from wakeups.
    pub fn take_event_rx(&mut self) -> Option<async_channel::Receiver<PtyEvent>> {
        self.event_rx.take()
    }

    /// 仅 remote SSH session 有：UI take 走后通过它接收主 SSH handle，
    /// 用来开 SFTP channel（C 方案：跟 PTY 共享同一 TCP 连接）。
    pub fn take_ssh_handle_rx(&mut self) -> Option<async_channel::Receiver<SshHandle>> {
        self.ssh_handle_rx.take()
    }

    /// 刷新前台进程状态快照，由 wakeup 回调（~60Hz）驱动
    pub fn refresh_foreground_status(&self) {
        let is_fg = Self::query_shell_foreground(self.pty_fd);
        self.shell_is_foreground
            .store(is_fg, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn shell_is_foreground(&self) -> bool {
        self.shell_is_foreground
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn shell_is_foreground_handle(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.shell_is_foreground)
    }

    /// 鼠标上报模式实时镜像（MOUSE_MODE_* 位）。failed 态无 grid，给常 0 桩。
    pub fn mouse_modes_handle(&self) -> Arc<AtomicU8> {
        self.state
            .lock()
            .grid
            .as_ref()
            .map(|grid| grid.mouse_modes_handle())
            .unwrap_or_else(|| Arc::new(AtomicU8::new(0)))
    }

    pub fn uses_remote_ssh(&self) -> bool {
        self.remote_event_loop.is_some()
    }

    #[cfg(unix)]
    fn query_shell_foreground(pty_fd: Option<LocalPtyDescriptor>) -> bool {
        let Some(fd) = pty_fd else { return true };
        let fg_pgid = unsafe { libc::tcgetpgrp(fd) };
        if fg_pgid <= 0 {
            return true;
        }
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let ret = unsafe {
            libc::proc_pidinfo(
                fg_pgid,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut info as *mut _ as *mut libc::c_void,
                std::mem::size_of::<libc::proc_bsdinfo>() as i32,
            )
        };
        if ret <= 0 {
            return true;
        }
        let name = unsafe { std::ffi::CStr::from_ptr(info.pbi_comm.as_ptr()) };
        let name = name.to_string_lossy();
        matches!(
            name.as_ref(),
            "bash"
                | "zsh"
                | "fish"
                | "sh"
                | "dash"
                | "ksh"
                | "tcsh"
                | "csh"
                | "nu"
                | "nushell"
                | "pwsh"
                | "powershell"
                | "elvish"
                | "oil"
                | "osh"
                | "xonsh"
        )
    }

    #[cfg(not(unix))]
    fn query_shell_foreground(_pty_fd: Option<LocalPtyDescriptor>) -> bool {
        true
    }

    pub fn send_input(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            terminal_runtime_debug_log(format_args!("runtime send_input ignored empty bytes"));
            return;
        }

        if let Some(event_loop) = &self.event_loop {
            terminal_runtime_debug_log(format_args!(
                "runtime send_input local {}",
                terminal_runtime_debug_bytes(&bytes)
            ));
            if let Err(error) = event_loop
                .message_tx
                .send(Message::Input(Cow::Owned(bytes)))
            {
                terminal_runtime_debug_log(format_args!(
                    "runtime send_input local failed: {error}"
                ));
            }
            return;
        }

        if let Some(serial_event_loop) = &self.serial_event_loop {
            terminal_runtime_debug_log(format_args!(
                "runtime send_input serial {}",
                terminal_runtime_debug_bytes(&bytes)
            ));
            if let Err(error) = serial_event_loop
                .request_tx
                .send(SerialRequest::Data(bytes))
            {
                terminal_runtime_debug_log(format_args!(
                    "runtime send_input serial failed: {error}"
                ));
            }
            return;
        }

        if let Some(remote_event_loop) = &self.remote_event_loop {
            terminal_runtime_debug_log(format_args!(
                "runtime send_input remote {}",
                terminal_runtime_debug_bytes(&bytes)
            ));
            if let Err(error) = remote_event_loop
                .request_tx
                .try_send(ChannelRequest::Data(bytes))
            {
                terminal_runtime_debug_log(format_args!(
                    "runtime send_input remote failed: {error}"
                ));
            }
        } else {
            terminal_runtime_debug_log(format_args!("runtime send_input dropped: no transport"));
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        self.resize_with_cell_size(
            cols,
            rows,
            DEFAULT_CELL_PIXEL_WIDTH,
            DEFAULT_CELL_PIXEL_HEIGHT,
        );
    }

    pub fn resize_with_cell_size(&self, cols: u16, rows: u16, cell_width: u16, cell_height: u16) {
        if let Some(event_loop) = &self.event_loop {
            let _ = event_loop.message_tx.send(Message::Resize(pty_size(
                cols,
                rows,
                cell_width,
                cell_height,
            )));
            return;
        }

        if self.serial_event_loop.is_some() {
            return;
        }

        if let Some(remote_event_loop) = &self.remote_event_loop {
            let _ = remote_event_loop
                .request_tx
                .try_send(ChannelRequest::Resize(u32::from(cols), u32::from(rows)));
        }
    }

    pub fn scroll(&self, lines: i32) {
        if lines == 0 {
            return;
        }
        update_state(&self.state, |state| {
            if let Some(grid) = &mut state.grid {
                grid.scroll_lines(lines);
            }
        });
    }

    pub fn scroll_to_bottom(&self) {
        update_state(&self.state, |state| {
            if let Some(grid) = &mut state.grid {
                grid.scroll_to_bottom();
            }
        });
    }

    pub fn scroll_to_offset(&self, offset: usize) {
        update_state(&self.state, |state| {
            if let Some(grid) = &mut state.grid {
                grid.scroll_to_offset(offset);
            }
        });
    }

    /// Synchronous version of `scroll_to_offset` that goes straight through
    /// the state mutex instead of queueing on the command channel. The
    /// command channel batches with PTY input/resize so a flurry of mouse
    /// moves during a scrollbar drag can stack up behind a single PTY write
    /// — that round-trip is what makes the thumb feel laggy. Selection
    /// updates already use the same direct path.
    pub fn set_display_offset_sync(&self, offset: usize) {
        update_state(&self.state, |state| {
            if let Some(grid) = state.grid.as_mut() {
                grid.scroll_to_offset(offset);
            }
        });
    }

    /// Wipe the visible screen and scrollback for Cmd+K/Clear Buffer.
    pub fn clear_visible_screen(&self, preserve_prompt_prefix: bool) {
        if let Some(event_loop) = &self.event_loop {
            let _ = event_loop.message_tx.send(Message::ClearVisibleScreen {
                preserve_prompt_prefix,
            });
            return;
        }

        update_state(&self.state, |state| {
            state.clear_visible_screen(preserve_prompt_prefix)
        });
    }

    /// 开始录制；已在录制中则不重置（保留已累积内容）。
    pub fn start_recording(&self) {
        let mut state = self.state.lock();
        if state.recorder.is_none() {
            state.recorder = Some(TerminalRecorder::start());
        }
    }

    /// 停止录制并取走含首尾 banner 的 transcript；未在录制返回 None。
    pub fn stop_recording(&self) -> Option<Vec<u8>> {
        self.state
            .lock()
            .recorder
            .take()
            .map(TerminalRecorder::finalize)
    }

    pub fn is_recording(&self) -> bool {
        self.state.lock().recorder.is_some()
    }

    pub fn is_connected(&self) -> bool {
        self.state.lock().connected
    }

    /// 主动断开远程/串口会话：丢弃 IO 句柄（Drop 触发 shutdown + Close），
    /// 保留 grid 内容；后台线程随即 remote_mark_disconnected 置 connected=false。
    pub fn disconnect(&mut self) {
        self.remote_event_loop = None;
        self.serial_event_loop = None;
    }

    /// 写入 IME marked text（合成中状态）。等价 Warp `TerminalModel::set_marked_text`：
    /// 不送 PTY，仅作为 overlay 渲染，提交时由 `clear_marked_text` + `send_input` 取代。
    /// 直接走 mutex 而非命令队列 —— GPUI 的 InputHandler 调用与帧渲染同线程，
    /// 命令队列会让 marked text 落后一帧。
    pub fn set_marked_text(&self, text: String, selected_range_utf16: Range<usize>) {
        update_state(&self.state, |state| {
            let before = state.marked_text.clone();
            state.marked_text = if text.is_empty() {
                None
            } else {
                Some(MarkedText {
                    text,
                    selected_range_utf16,
                })
            };
            if let Some(grid) = state.grid.as_mut() {
                grid.mark_dirty_for_marked_text(before.as_ref(), state.marked_text.as_ref());
            }
        });
    }

    /// 清除 IME marked text（取消合成 / 提交完成时调用）。
    pub fn clear_marked_text(&self) {
        update_state(&self.state, |state| {
            let before = state.marked_text.clone();
            state.marked_text = None;
            if let Some(grid) = state.grid.as_mut() {
                grid.mark_dirty_for_marked_text(before.as_ref(), None);
            }
        });
    }

    pub fn take_clipboard_store_requests(&self) -> Vec<TerminalClipboardStoreRequest> {
        self.state
            .lock()
            .clipboard_store_requests
            .drain(..)
            .collect()
    }

    pub fn take_clipboard_load_requests(&self) -> Vec<TerminalClipboardLoadRequest> {
        self.state
            .lock()
            .clipboard_load_requests
            .drain(..)
            .collect()
    }

    /// 当前 marked text 快照（输入法回调要回查 marked range / 文本）。
    pub fn marked_text(&self) -> Option<MarkedText> {
        self.state.lock().marked_text.clone()
    }

    /// Update the active find query and recompute matches. Pass `None` to
    /// clear find state entirely. Returns the resulting match count.
    pub fn set_find_query(&self, query: Option<String>) -> usize {
        let mut count = 0;
        update_state(&self.state, |state| {
            let normalized = query.as_ref().map(|q| q.as_str()).unwrap_or("");
            if normalized.is_empty() {
                if state.find_query.is_some() || !state.find_matches.is_empty() {
                    state.find_query = None;
                    state.find_matches.clear();
                    state.find_current = None;
                    state.find_pulse = state.find_pulse.wrapping_add(1);
                    if let Some(grid) = state.grid.as_mut() {
                        grid.mark_all_dirty();
                    }
                }
                return;
            }
            state.find_query = Some(normalized.to_string());
            let matches = state
                .grid
                .as_ref()
                .and_then(|grid| grid.find_all(normalized))
                .unwrap_or_default();
            state.find_matches = matches;
            state.find_current = if state.find_matches.is_empty() {
                None
            } else {
                Some(0)
            };
            state.find_pulse = state.find_pulse.wrapping_add(1);
            count = state.find_matches.len();
            if let Some(grid) = state.grid.as_mut() {
                grid.mark_all_dirty();
            }

            // Scroll the focused match into view so the user sees the first hit.
            if let (Some(grid), Some(idx)) = (state.grid.as_mut(), state.find_current) {
                if let Some(range) = state.find_matches.get(idx) {
                    scroll_grid_to_match(grid, range);
                }
            }
        });
        count
    }

    /// Move the find cursor by `step` (`+1` for next, `-1` for previous) and
    /// scroll the focused match into view.
    pub fn step_find(&self, step: i32) {
        update_state(&self.state, |state| {
            let total = state.find_matches.len();
            if total == 0 {
                return;
            }
            let current = state.find_current.unwrap_or(0) as i32;
            let next = (current + step).rem_euclid(total as i32) as usize;
            state.find_current = Some(next);
            state.find_pulse = state.find_pulse.wrapping_add(1);
            if let (Some(grid), Some(range)) = (state.grid.as_mut(), state.find_matches.get(next)) {
                grid.mark_all_dirty();
                scroll_grid_to_match(grid, range);
            }
        });
    }

    pub fn start_selection(&self, ty: SelectionType, point: Point, side: Side) {
        update_state(&self.state, |state| {
            if let Some(grid) = &mut state.grid {
                grid.start_selection(ty, point, side);
            }
        });
    }

    pub fn update_selection(&self, point: Point, side: Side) {
        update_state(&self.state, |state| {
            if let Some(grid) = &mut state.grid {
                grid.update_selection(point, side);
            }
        });
    }

    pub fn clear_selection(&self) {
        update_state(&self.state, |state| {
            if let Some(grid) = &mut state.grid {
                grid.clear_selection();
            }
        });
    }

    pub fn selected_text(&self) -> Option<String> {
        let state = self.state.lock();
        state.grid.as_ref().and_then(|grid| grid.selected_text())
    }

    /// Encode `text` for the PTY with bracketed-paste guards when alacritty's
    /// Term has BRACKETED_PASTE set. Mirrors Warp's `paste()` in `view.rs`:
    /// strip carriage returns + line-feeds down to `\r` so non-bracketed apps
    /// don't see `^J`, then wrap with `\x1b[200~ ... \x1b[201~` when supported.
    pub fn paste(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        let normalized: String = text
            .replace("\r\n", "\r")
            .chars()
            .map(|c| if c == '\n' { '\r' } else { c })
            .collect();

        let bracketed = self
            .state
            .lock()
            .grid
            .as_ref()
            .map(|grid| grid.bracketed_paste_enabled())
            .unwrap_or(false);

        let mut bytes = Vec::with_capacity(
            normalized.len()
                + if bracketed {
                    BRACKETED_PASTE_PREFIX.len() + BRACKETED_PASTE_SUFFIX.len()
                } else {
                    0
                },
        );
        if bracketed {
            bytes.extend_from_slice(BRACKETED_PASTE_PREFIX);
        }
        bytes.extend_from_slice(normalized.as_bytes());
        if bracketed {
            bytes.extend_from_slice(BRACKETED_PASTE_SUFFIX);
        }
        self.send_input(bytes);
    }

    pub fn snapshot(&self) -> Arc<TerminalRuntimeSnapshot> {
        self.state.lock().snapshot_arc()
    }

    pub fn snapshot_for_render(&self) -> Arc<TerminalRuntimeSnapshot> {
        self.state.lock().snapshot_arc_for_render()
    }

    /// shell integration marker 已收到？UI 用它判定占位还是 grid。
    pub fn is_bootstrapped(&self) -> bool {
        self.state.lock().bootstrapped
    }

    pub fn title(&self) -> Option<String> {
        self.state.lock().title.clone()
    }

    pub fn revision(&self) -> u64 {
        self.state.lock().revision
    }

    pub fn set_palette(&self, palette: TerminalPalette) {
        let mut state = self.state.lock();
        state.palette = palette;
        state.cached_snapshot = None;
    }

    pub fn palette(&self) -> TerminalPalette {
        self.state.lock().palette.clone()
    }
}

// EventLoopHandle::Drop sends Message::Shutdown, kills the child, and joins
// the PTY thread, so LocalTerminalRuntime needs no extra Drop impl —
// dropping `event_loop: Option<EventLoopHandle>` is enough.

fn update_state(
    state: &Arc<FairMutex<TerminalRuntimeState>>,
    update: impl FnOnce(&mut TerminalRuntimeState),
) {
    let mut guard = state.lock();
    update(&mut guard);
    guard.revision = guard.revision.wrapping_add(1);
}

fn validate_remote_ssh_config(config: &RemoteSshConfig) -> Result<(), String> {
    if config.host.trim().is_empty() {
        return Err("主机地址为空".to_string());
    }
    if config.username.trim().is_empty() {
        return Err("用户名为空".to_string());
    }
    if config.auth_method.eq_ignore_ascii_case("key") {
        if config
            .private_key
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            return Err("密钥认证未保存私钥".to_string());
        }
    } else if config
        .password
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        return Err("密码认证未保存密码".to_string());
    }
    Ok(())
}

fn validate_serial_port_config(config: &SerialPortRuntimeConfig) -> Result<(), String> {
    if config.port.trim().is_empty() {
        return Err("串口为空".to_string());
    }
    if config.baud_rate == 0 {
        return Err("串口波特率为空".to_string());
    }
    Ok(())
}

fn serial_data_bits(value: u8) -> serialport::DataBits {
    match value {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        _ => serialport::DataBits::Eight,
    }
}

fn serial_stop_bits(value: u8) -> serialport::StopBits {
    if value == 2 {
        serialport::StopBits::Two
    } else {
        serialport::StopBits::One
    }
}

fn serial_parity(value: &str) -> serialport::Parity {
    match value.trim().to_ascii_lowercase().as_str() {
        "odd" => serialport::Parity::Odd,
        "even" => serialport::Parity::Even,
        _ => serialport::Parity::None,
    }
}

fn serial_flow_control(value: &str) -> serialport::FlowControl {
    match value.trim().to_ascii_lowercase().as_str() {
        "hardware" => serialport::FlowControl::Hardware,
        "software" => serialport::FlowControl::Software,
        _ => serialport::FlowControl::None,
    }
}

fn spawn_serial_event_loop(
    session_id: String,
    state: Arc<FairMutex<TerminalRuntimeState>>,
    config: SerialPortRuntimeConfig,
    wakeup_tx: async_channel::Sender<()>,
    event_tx: async_channel::Sender<PtyEvent>,
) -> Result<SerialEventLoopHandle, String> {
    let (request_tx, request_rx) = mpsc::channel::<SerialRequest>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::Builder::new()
        .name(format!("nexshell-serial-{session_id}"))
        .spawn({
            let state = Arc::clone(&state);
            let wakeup_tx = wakeup_tx.clone();
            let event_tx = event_tx.clone();
            move || {
                run_serial_event_loop(
                    state,
                    config,
                    request_rx,
                    wakeup_tx,
                    event_tx,
                    thread_shutdown,
                );
            }
        })
        .map_err(|error| format!("spawn serial thread: {error}"))?;

    Ok(SerialEventLoopHandle {
        request_tx,
        shutdown,
        _thread: Some(thread),
    })
}

fn run_serial_event_loop(
    state: Arc<FairMutex<TerminalRuntimeState>>,
    config: SerialPortRuntimeConfig,
    request_rx: mpsc::Receiver<SerialRequest>,
    wakeup_tx: async_channel::Sender<()>,
    event_tx: async_channel::Sender<PtyEvent>,
    shutdown: Arc<AtomicBool>,
) {
    let port_name = config.port.trim().to_string();
    let mut port = match serialport::new(&port_name, config.baud_rate)
        .data_bits(serial_data_bits(config.data_bits))
        .stop_bits(serial_stop_bits(config.stop_bits))
        .parity(serial_parity(&config.parity))
        .flow_control(serial_flow_control(&config.flow_control))
        .timeout(Duration::from_millis(20))
        .open()
    {
        Ok(port) => port,
        Err(error) => {
            remote_mark_disconnected(
                &state,
                &wakeup_tx,
                &event_tx,
                format!("open serial {port_name}: {error}"),
            );
            return;
        }
    };

    let _ = port.write_data_terminal_ready(config.dtr);
    let _ = port.write_request_to_send(config.rts);
    remote_update_status(
        &state,
        &wakeup_tx,
        format!("connected serial: {port_name} @ {}", config.baud_rate),
    );

    let mut read_buf = [0_u8; 4096];
    let mut disconnect_status = "Serial session closed".to_string();
    'serial: loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        loop {
            match request_rx.try_recv() {
                Ok(SerialRequest::Data(bytes)) => {
                    if let Err(error) = port.write_all(&bytes) {
                        disconnect_status = format!("serial write error: {error}");
                        break 'serial;
                    }
                }
                Ok(SerialRequest::Close) => break 'serial,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break 'serial,
            }
        }

        match port.read(&mut read_buf) {
            Ok(0) => {}
            Ok(n) => {
                let replies = remote_process_output(&state, &wakeup_tx, &read_buf[..n]);
                for reply in replies {
                    if let Err(error) = port.write_all(reply.as_ref()) {
                        disconnect_status = format!("serial write error: {error}");
                        break 'serial;
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(error) => {
                disconnect_status = format!("serial read error: {error}");
                break;
            }
        }
    }

    remote_mark_disconnected(&state, &wakeup_tx, &event_tx, disconnect_status);
}

fn spawn_remote_ssh_event_loop(
    session_id: String,
    state: Arc<FairMutex<TerminalRuntimeState>>,
    config: RemoteSshConfig,
    wakeup_tx: async_channel::Sender<()>,
    event_tx: async_channel::Sender<PtyEvent>,
    cols: u16,
    rows: u16,
) -> Result<(RemoteEventLoopHandle, async_channel::Receiver<SshHandle>), String> {
    let (request_tx, request_rx) = tokio::sync::mpsc::channel::<ChannelRequest>(256);
    // bounded(1) 就够：handle 只发一次，UI 拿走后就消费完。
    let (ssh_handle_tx, ssh_handle_rx) = async_channel::bounded::<SshHandle>(1);
    let shutdown = Arc::new(AtomicBool::new(false));
    let thread_shutdown = Arc::clone(&shutdown);
    let thread = thread::Builder::new()
        .name(format!("nexshell-ssh-{session_id}"))
        .spawn({
            let state = Arc::clone(&state);
            let wakeup_tx = wakeup_tx.clone();
            let event_tx = event_tx.clone();
            move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        remote_mark_disconnected(
                            &state,
                            &wakeup_tx,
                            &event_tx,
                            format!("failed to start SSH runtime: {error}"),
                        );
                        return;
                    }
                };
                runtime.block_on(run_remote_ssh_event_loop(
                    session_id,
                    state,
                    config,
                    request_rx,
                    wakeup_tx,
                    event_tx,
                    ssh_handle_tx,
                    thread_shutdown,
                    cols,
                    rows,
                ));
            }
        })
        .map_err(|error| format!("spawn SSH thread: {error}"))?;

    Ok((
        RemoteEventLoopHandle {
            request_tx,
            shutdown,
            _thread: Some(thread),
        },
        ssh_handle_rx,
    ))
}

async fn run_remote_ssh_event_loop(
    _session_id: String,
    state: Arc<FairMutex<TerminalRuntimeState>>,
    config: RemoteSshConfig,
    mut request_rx: tokio::sync::mpsc::Receiver<ChannelRequest>,
    wakeup_tx: async_channel::Sender<()>,
    event_tx: async_channel::Sender<PtyEvent>,
    ssh_handle_tx: async_channel::Sender<SshHandle>,
    shutdown: Arc<AtomicBool>,
    cols: u16,
    rows: u16,
) {
    let host = config.host.trim().to_string();
    let username = config.username.trim().to_string();
    let target = format!("{username}@{host}:{}", config.port);

    // 连接阶段 banner 写入终端 grid
    let banner = |state: &Arc<FairMutex<TerminalRuntimeState>>,
                  wakeup: &async_channel::Sender<()>,
                  step: &str,
                  detail: &str| {
        // \x1b[2J 清屏, \x1b[3J 清 scrollback, \x1b[H 归位, \x1b[?25l 隐藏光标
        let mut buf = String::from("\x1b[2J\x1b[3J\x1b[H\x1b[?25l\r\n");
        // 标题: 青色粗体
        buf.push_str("  \x1b[1;36m⟐ NexShell SSH\x1b[0m\r\n\r\n");
        // 目标: 白色
        buf.push_str(&format!("  \x1b[37m  {detail}\x1b[0m\r\n\r\n"));
        // 当前步骤: 黄色 + 旋转符号
        buf.push_str(&format!("  \x1b[33m◌ {step}...\x1b[0m\r\n"));
        remote_process_output(state, wakeup, buf.as_bytes());
    };

    banner(&state, &wakeup_tx, "正在建立 TCP 连接", &target);
    remote_update_status(&state, &wakeup_tx, format!("connecting SSH: {target}"));

    if shutdown.load(Ordering::Relaxed) {
        remote_mark_disconnected(&state, &wakeup_tx, &event_tx, "connection cancelled");
        return;
    }

    let connect_timeout_secs = u64::from(config.tcp_connect_timeout.clamp(5, 60));
    let mut session = match tokio::time::timeout(
        Duration::from_secs(connect_timeout_secs),
        SshSession::connect(
            &host,
            config.port,
            SshConnectOptions {
                keep_alive_enabled: config.keep_alive_enabled,
                keep_alive_interval_secs: config.keep_alive_interval.clamp(10, 300),
                keep_alive_max_failures: config.keep_alive_max_failures.clamp(1, 10),
            },
        ),
    )
    .await
    {
        Ok(Ok(session)) => session,
        Ok(Err(error)) => {
            remote_mark_disconnected(&state, &wakeup_tx, &event_tx, error);
            return;
        }
        Err(_) => {
            remote_mark_disconnected(
                &state,
                &wakeup_tx,
                &event_tx,
                format!("TCP connection timeout after {connect_timeout_secs}s"),
            );
            return;
        }
    };

    if shutdown.load(Ordering::Relaxed) {
        session.close().await;
        remote_mark_disconnected(&state, &wakeup_tx, &event_tx, "connection cancelled");
        return;
    }

    banner(&state, &wakeup_tx, "正在进行身份验证", &target);
    remote_update_status(&state, &wakeup_tx, format!("authenticating SSH: {target}"));

    let auth_timeout_secs = u64::from(config.auth_timeout.clamp(10, 120));
    let auth_result = if config.auth_method.eq_ignore_ascii_case("key") {
        let key_data = match config.private_key.as_deref().map(resolve_private_key_data) {
            Some(Ok(key_data)) => key_data,
            Some(Err(error)) => {
                session.close().await;
                remote_mark_disconnected(&state, &wakeup_tx, &event_tx, error);
                return;
            }
            None => {
                session.close().await;
                remote_mark_disconnected(
                    &state,
                    &wakeup_tx,
                    &event_tx,
                    "No private key configured",
                );
                return;
            }
        };
        let key_passphrase = config
            .key_passphrase
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let ca_cert_data = match config.ca_cert.as_deref().map(resolve_ca_cert_data) {
            Some(Ok(ca_cert_data)) => Some(ca_cert_data),
            Some(Err(error)) => {
                session.close().await;
                remote_mark_disconnected(&state, &wakeup_tx, &event_tx, error);
                return;
            }
            None => None,
        };
        let ca_cert = ca_cert_data.as_deref();
        tokio::time::timeout(
            Duration::from_secs(auth_timeout_secs),
            session.auth_key(&username, &key_data, key_passphrase, ca_cert),
        )
        .await
        .map_err(|_| format!("Authentication timeout after {auth_timeout_secs}s"))
        .and_then(|result| result)
    } else {
        let password = config
            .password
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "No password configured for this host".to_string());
        match password {
            Ok(password) => tokio::time::timeout(
                Duration::from_secs(auth_timeout_secs),
                session.auth_password(&username, password),
            )
            .await
            .map_err(|_| format!("Authentication timeout after {auth_timeout_secs}s"))
            .and_then(|result| result),
            Err(error) => Err(error),
        }
    };

    if let Err(error) = auth_result {
        session.close().await;
        remote_mark_disconnected(&state, &wakeup_tx, &event_tx, error);
        return;
    }

    // 认证成功后把 handle clone 推给 UI，让文件面板可在同一 TCP 连接上开 SFTP channel。
    // try_send 失败也无所谓：UI 没在等就丢掉（bounded(1)，正常情况下不会满）。
    let _ = ssh_handle_tx.try_send(session.handle());

    banner(&state, &wakeup_tx, "正在打开远程终端", &target);
    remote_update_status(&state, &wakeup_tx, format!("opening SSH PTY: {target}"));

    if let Err(error) = session.request_pty(u32::from(cols), u32::from(rows)).await {
        session.close().await;
        remote_mark_disconnected(&state, &wakeup_tx, &event_tx, error);
        return;
    }

    let Some(mut channel) = session.take_channel().await else {
        session.close().await;
        remote_mark_disconnected(
            &state,
            &wakeup_tx,
            &event_tx,
            "Failed to acquire SSH channel",
        );
        return;
    };

    // 连接成功: 清屏 + 清 scrollback + 恢复光标
    remote_process_output(&state, &wakeup_tx, b"\x1b[2J\x1b[3J\x1b[H\x1b[?25h");
    remote_update_status(&state, &wakeup_tx, format!("connected SSH: {target}"));

    let mut terminal_encoding = RemoteTerminalEncoding::new(&config.term_encoding);
    let mut keepalive_cols = u32::from(cols);
    let mut keepalive_rows = u32::from(rows);
    let keepalive_interval =
        Duration::from_secs(u64::from(config.keep_alive_interval.clamp(10, 300)));
    let keepalive_sleep = tokio::time::sleep(keepalive_interval);
    tokio::pin!(keepalive_sleep);

    let disconnect_status = 'remote: loop {
        if shutdown.load(Ordering::Relaxed) {
            let _ = channel.eof().await;
            let _ = channel.close().await;
            break "SSH session closed".to_string();
        }

        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        let decoded = terminal_encoding.decode_output(&data);
                        let replies = remote_process_output(&state, &wakeup_tx, decoded.as_ref());
                        for reply in replies {
                            let encoded = terminal_encoding.encode_input(&reply);
                            if let Err(error) = channel.data(&encoded[..]).await {
                                break 'remote format!("SSH channel write error: {error}");
                            }
                        }
                    }
                    Some(ChannelMsg::Eof) => {
                        break 'remote "SSH session reached EOF".to_string();
                    }
                    Some(ChannelMsg::Close) | None => {
                        break 'remote "SSH session closed".to_string();
                    }
                    _ => {}
                }
            }
            req = request_rx.recv() => {
                match req {
                    Some(ChannelRequest::Data(data)) => {
                        let encoded = terminal_encoding.encode_input(&data);
                        terminal_runtime_debug_log(format_args!(
                            "remote channel write raw={} encoded={}",
                            terminal_runtime_debug_bytes(&data),
                            terminal_runtime_debug_bytes(&encoded)
                        ));
                        if let Err(error) = channel.data(&encoded[..]).await {
                            break 'remote format!("SSH channel write error: {error}");
                        }
                    }
                    Some(ChannelRequest::Resize(cols, rows)) => {
                        keepalive_cols = cols;
                        keepalive_rows = rows;
                        if let Err(error) = channel.window_change(cols, rows, 0, 0).await {
                            remote_update_status(
                                &state,
                                &wakeup_tx,
                                format!("SSH resize failed: {error}"),
                            );
                        }
                        remote_handle_resize(&state, &wakeup_tx, cols as u16, rows as u16);
                    }
                    Some(ChannelRequest::Close) => {
                        let _ = channel.eof().await;
                        let _ = channel.close().await;
                        break 'remote "SSH session closed".to_string();
                    }
                    None => {
                        break 'remote "SSH session closed".to_string();
                    }
                }
            }
            _ = &mut keepalive_sleep, if config.keep_alive_enabled => {
                if let Err(error) = channel
                    .window_change(keepalive_cols, keepalive_rows, 0, 0)
                    .await
                {
                    break 'remote format!("SSH keepalive failed: {error}");
                }
                keepalive_sleep
                    .as_mut()
                    .reset(tokio::time::Instant::now() + keepalive_interval);
            }
        }
    };

    session.close().await;
    remote_mark_disconnected(&state, &wakeup_tx, &event_tx, disconnect_status);
}

fn remote_update_status(
    state: &Arc<FairMutex<TerminalRuntimeState>>,
    wakeup_tx: &async_channel::Sender<()>,
    status: impl Into<String>,
) {
    {
        let mut guard = state.lock();
        guard.status = status.into();
        guard.revision = guard.revision.wrapping_add(1);
    }
    let _ = wakeup_tx.try_send(());
}

fn remote_process_output(
    state: &Arc<FairMutex<TerminalRuntimeState>>,
    wakeup_tx: &async_channel::Sender<()>,
    data: &[u8],
) -> Vec<Cow<'static, [u8]>> {
    let replies = {
        let mut guard = state.lock();
        guard.process_output(data)
    };
    let _ = wakeup_tx.try_send(());
    replies
}

fn remote_handle_resize(
    state: &Arc<FairMutex<TerminalRuntimeState>>,
    wakeup_tx: &async_channel::Sender<()>,
    cols: u16,
    rows: u16,
) {
    {
        let mut guard = state.lock();
        guard.handle_resize(pty_size(
            cols,
            rows,
            DEFAULT_CELL_PIXEL_WIDTH,
            DEFAULT_CELL_PIXEL_HEIGHT,
        ));
    }
    let _ = wakeup_tx.try_send(());
}

fn remote_mark_disconnected(
    state: &Arc<FairMutex<TerminalRuntimeState>>,
    wakeup_tx: &async_channel::Sender<()>,
    event_tx: &async_channel::Sender<PtyEvent>,
    status: impl Into<String>,
) {
    let status = status.into();
    {
        let mut guard = state.lock();
        guard.mark_disconnected(status.clone());
    }
    let _ = wakeup_tx.try_send(());
    let _ = event_tx.try_send(PtyEvent::Disconnected(status));
}

fn resolve_private_key_data(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.contains("BEGIN ") || trimmed.contains('\n') {
        return Ok(trimmed.to_string());
    }

    let path = expand_tilde(trimmed);
    if path.is_file() {
        return fs::read_to_string(&path)
            .map_err(|error| format!("Failed to read private key file: {error}"));
    }

    Err(
        "Invalid private key: saved value is neither key content nor a readable file path"
            .to_string(),
    )
}

fn resolve_ca_cert_data(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Empty OpenSSH certificate value".to_string());
    }

    let path = expand_tilde(trimmed);
    if path.is_file() {
        return fs::read_to_string(&path)
            .map_err(|error| format!("Failed to read OpenSSH certificate file: {error}"));
    }

    Ok(trimmed.to_string())
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Scroll the alacritty grid so `range.start` is visible in the viewport.
/// Mirrors how Warp's `TerminalFindModel` jumps to a focused match: it pins
/// the match line to the top half of the viewport so context above the hit
/// stays readable.
fn scroll_grid_to_match(grid: &mut TerminalGridCore, range: &FindMatchRange) {
    let history = grid.term.history_size();
    let screen_lines = grid.term.screen_lines() as i32;
    // alacritty `Line` is signed (negative = scrollback). `display_offset` of 0
    // corresponds to viewport top = Line(0), so to bring `Line(L)` into view
    // we want `display_offset = -L` (clamped to history). Pad by 2 rows so the
    // match isn't flush against the top edge.
    let target_line = range.start.line.0;
    let raw_offset = -target_line + (screen_lines / 4);
    let clamped = raw_offset.clamp(0, history as i32) as usize;
    grid.scroll_to_offset(clamped);
}

fn terminal_window_size(cols: u16, rows: u16, cell_width: u16, cell_height: u16) -> WindowSize {
    WindowSize {
        num_lines: rows.max(1),
        num_cols: cols.max(2),
        cell_width: cell_width.max(1),
        cell_height: cell_height.max(1),
    }
}

fn pty_size(cols: u16, rows: u16, cell_width: u16, cell_height: u16) -> PtySize {
    let window_size = terminal_window_size(cols, rows, cell_width, cell_height);
    PtySize {
        rows: window_size.num_lines,
        cols: window_size.num_cols,
        pixel_width: window_size.num_cols.saturating_mul(window_size.cell_width),
        pixel_height: window_size
            .num_lines
            .saturating_mul(window_size.cell_height),
    }
}

/// Warp: `app/src/terminal/local_tty/unix.rs:261` build_host_shell_command
fn configure_command_environment(command: &mut CommandBuilder) {
    if let Some(user) = env_var("USER").or_else(|| env_var("USERNAME")) {
        command.env("LOGNAME", &user);
        command.env("USER", &user);
        #[cfg(windows)]
        command.env("USERNAME", &user);
    }
    if let Some(home) = home_dir() {
        command.env("HOME", &home);
        command.cwd(&home);
    }
    command.env("TERM", "xterm-256color");
    command.env("TERM_PROGRAM", "WarpTerminal");
    command.env("COLORTERM", "truecolor");
    command.env("NEXSHELL_NATIVE_SPIKE", "1");

    // UTF-8 locale：解耦漏搬 warp 的 set_locale_environment（terminal/platform.rs）。PTY 子进程
    // 无 UTF-8 locale 时，vim 等 locale-aware 程序按 latin1 读 UTF-8 文件 → 中文乱码
    // （cat 不依赖 locale 故正常）。
    apply_utf8_locale(command);
}

/// 尊重用户已设的 UTF-8 locale，仅在缺失/为 C 时补 fallback（warp 同款策略：只设 LC_CTYPE）。
fn apply_utf8_locale(command: &mut CommandBuilder) {
    let has_utf8 = ["LC_ALL", "LC_CTYPE", "LANG"].iter().any(|key| {
        env_var(key).is_some_and(|v| {
            let upper = v.to_ascii_uppercase();
            upper.contains("UTF-8") || upper.contains("UTF8")
        })
    });
    if has_utf8 {
        return;
    }
    // macOS libc 接受裸 "UTF-8"（warp FALLBACK_LOCALE 同款）；其他平台用通用的 "C.UTF-8"。
    #[cfg(target_os = "macos")]
    command.env("LC_CTYPE", "UTF-8");
    #[cfg(not(target_os = "macos"))]
    command.env("LC_CTYPE", "C.UTF-8");
}

fn env_var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn home_dir() -> Option<String> {
    env_var("HOME")
        .or_else(|| {
            #[cfg(windows)]
            {
                windows_home_dir()
            }
            #[cfg(not(windows))]
            {
                None
            }
        })
        .or_else(|| {
            #[cfg(unix)]
            {
                let mut buf = [0i8; 1024];
                get_pw_home(&mut buf)
            }
            #[cfg(not(unix))]
            {
                None
            }
        })
}

#[cfg(windows)]
fn windows_home_dir() -> Option<String> {
    env_var("USERPROFILE").or_else(|| {
        let drive = env_var("HOMEDRIVE")?;
        let path = env_var("HOMEPATH")?;
        Some(format!("{drive}{path}"))
    })
}

fn default_shell() -> String {
    env::var("SHELL")
        .ok()
        .filter(|shell| !shell.trim().is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "powershell.exe".to_string()
            } else if cfg!(target_os = "macos") {
                "/bin/zsh".to_string()
            } else {
                "/bin/sh".to_string()
            }
        })
}

/// Warp: `app/src/terminal/local_tty/shell.rs:555` arguments_for_session_spawning_command
/// 通过 `exec -a -<shell>` 设置 argv[0] 为 "-<shell>"，使 shell 以 login shell 启动。
fn build_login_shell_command(shell: &str) -> CommandBuilder {
    let basename = std::path::Path::new(shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    match basename {
        "zsh" => {
            let mut cmd = CommandBuilder::new("/bin/bash");
            cmd.arg("-c");
            cmd.arg(format!("exec -a -zsh '{shell}'"));
            cmd
        }
        "bash" => {
            let mut cmd = CommandBuilder::new("/bin/bash");
            cmd.arg("-c");
            cmd.arg(format!("exec -a -bash '{shell}'"));
            cmd
        }
        "fish" => {
            let mut cmd = CommandBuilder::new(shell);
            cmd.arg("--login");
            cmd
        }
        _ => {
            let mut cmd = CommandBuilder::new(shell);
            cmd.arg("-l");
            cmd
        }
    }
}

/// 在 build_login_shell_command 基础上，根据 shell 类型注入 shell integration 启动参数：
/// - zsh: 走 ZDOTDIR（caller 在 spawn_local 设置 env）
/// - bash: `--rcfile <wrapper>`（interactive non-login，wrapper 内部 source login rc）
/// - fish: `--init-command '<inline>'`
/// 不支持或失败时 fall back 到普通 login shell。
fn build_shell_command_with_integration(shell: &str, display: &str) -> CommandBuilder {
    if cfg!(windows) {
        let mut cmd = CommandBuilder::new(shell);
        if matches!(
            display.to_ascii_lowercase().as_str(),
            "powershell.exe" | "pwsh.exe" | "powershell" | "pwsh"
        ) {
            cmd.arg("-NoLogo");
        }
        return cmd;
    }

    match display {
        "bash" => {
            if let Some(rc) = crate::shell_integration::setup_bash_integration() {
                let mut cmd = CommandBuilder::new(shell);
                cmd.arg("--rcfile");
                cmd.arg(rc.as_os_str());
                cmd.arg("-i");
                return cmd;
            }
        }
        "fish" => {
            let mut cmd = CommandBuilder::new(shell);
            cmd.arg("--login");
            cmd.arg("--init-command");
            cmd.arg(crate::shell_integration::FISH_INIT_COMMAND);
            return cmd;
        }
        _ => {}
    }
    build_login_shell_command(shell)
}

/// Warp: `app/src/terminal/local_tty/unix.rs:126` get_pw_entry
#[cfg(unix)]
fn get_pw_home(buf: &mut [i8; 1024]) -> Option<String> {
    use std::ffi::CStr;
    use std::mem::MaybeUninit;
    use std::ptr;
    let mut entry: MaybeUninit<libc::passwd> = MaybeUninit::uninit();
    let mut res: *mut libc::passwd = ptr::null_mut();
    let uid = unsafe { libc::getuid() };
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            entry.as_mut_ptr(),
            buf.as_mut_ptr() as *mut _,
            buf.len(),
            &mut res,
        )
    };
    if status != 0 || res.is_null() {
        return None;
    }
    let entry = unsafe { entry.assume_init() };
    unsafe { CStr::from_ptr(entry.pw_dir).to_str().ok().map(String::from) }
}

fn color_snapshot(color: Color, colors: &Colors) -> TerminalColorSnapshot {
    if let Some(rgb) = terminal_dynamic_rgb(color, colors) {
        return TerminalColorSnapshot::Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        };
    }

    match color {
        Color::Named(named) => TerminalColorSnapshot::Named(named_color_name(named)),
        Color::Indexed(index) => TerminalColorSnapshot::Indexed(index),
        Color::Spec(Rgb { r, g, b }) => TerminalColorSnapshot::Rgb { r, g, b },
    }
}

fn intern_terminal_cell_style(
    styles: &mut Vec<TerminalCellStyleSnapshot>,
    style_map: &mut HashMap<TerminalCellStyleSnapshot, u16>,
    style: TerminalCellStyleSnapshot,
) -> u16 {
    if let Some(idx) = style_map.get(&style) {
        return *idx;
    }
    if styles.len() >= u16::MAX as usize {
        return 0;
    }
    let idx = styles.len() as u16;
    style_map.insert(style.clone(), idx);
    styles.push(style);
    idx
}

fn terminal_cell_content(cell: &AlacrittyCell, wide_spacer: bool) -> Arc<str> {
    if wide_spacer {
        return Arc::from(" ");
    }
    let zerowidth = cell.zerowidth();
    if zerowidth.map_or(true, |zw| zw.is_empty()) {
        // Fast path — single-codepoint cells avoid the String round-trip.
        let mut buf = [0u8; 4];
        return Arc::from(cell.c.encode_utf8(&mut buf) as &str);
    }
    let mut content = String::with_capacity(4);
    content.push(cell.c);
    if let Some(zerowidth) = zerowidth {
        content.extend(zerowidth.iter().copied());
    }
    Arc::from(content.as_str())
}

#[derive(Clone)]
pub struct TerminalPalette {
    pub background: u32,
    pub background_alpha: u8,
    pub foreground: u32,
    pub cursor: u32,
    pub selection: u32,
    pub find_match: u32,
    pub find_focus: u32,
    pub ansi: [u32; 16],
}

fn ansi_color_to_u32(c: &warp_core::ui::theme::AnsiColor) -> u32 {
    (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
}

impl TerminalPalette {
    pub fn from_theme(theme: &warp_core::ui::theme::WarpTheme) -> Self {
        let tc = theme.terminal_colors();
        let bg = theme.background().into_solid();
        let fg = theme.foreground().into_solid();
        let cursor_color = theme.cursor().into_solid();
        let accent = theme.accent().into_solid();

        let bg_u32 = (u32::from(bg.r) << 16) | (u32::from(bg.g) << 8) | u32::from(bg.b);
        let fg_u32 = (u32::from(fg.r) << 16) | (u32::from(fg.g) << 8) | u32::from(fg.b);
        let cursor_u32 = (u32::from(cursor_color.r) << 16)
            | (u32::from(cursor_color.g) << 8)
            | u32::from(cursor_color.b);

        // selection = accent with reduced opacity, blended on background
        let sel_r = ((accent.r as u32 * 80 + bg.r as u32 * 176) / 256) as u8;
        let sel_g = ((accent.g as u32 * 80 + bg.g as u32 * 176) / 256) as u8;
        let sel_b = ((accent.b as u32 * 80 + bg.b as u32 * 176) / 256) as u8;
        let selection = (u32::from(sel_r) << 16) | (u32::from(sel_g) << 8) | u32::from(sel_b);

        // warp: workspace/view.rs:22856 — 有背景图时终端背景半透明
        let background_alpha = theme
            .background_image()
            .map(|img| ((100u16.saturating_sub(img.opacity as u16)) * 255 / 100) as u8)
            .unwrap_or(255);

        Self {
            background: bg_u32,
            background_alpha,
            foreground: fg_u32,
            cursor: cursor_u32,
            selection,
            find_match: 0xb38b00,
            find_focus: 0xffaa00,
            ansi: [
                ansi_color_to_u32(&tc.normal.black),
                ansi_color_to_u32(&tc.normal.red),
                ansi_color_to_u32(&tc.normal.green),
                ansi_color_to_u32(&tc.normal.yellow),
                ansi_color_to_u32(&tc.normal.blue),
                ansi_color_to_u32(&tc.normal.magenta),
                ansi_color_to_u32(&tc.normal.cyan),
                ansi_color_to_u32(&tc.normal.white),
                ansi_color_to_u32(&tc.bright.black),
                ansi_color_to_u32(&tc.bright.red),
                ansi_color_to_u32(&tc.bright.green),
                ansi_color_to_u32(&tc.bright.yellow),
                ansi_color_to_u32(&tc.bright.blue),
                ansi_color_to_u32(&tc.bright.magenta),
                ansi_color_to_u32(&tc.bright.cyan),
                ansi_color_to_u32(&tc.bright.white),
            ],
        }
    }
}

impl Default for TerminalPalette {
    fn default() -> Self {
        Self::from_theme(&crate::themes::default_themes::dark_theme())
    }
}

pub fn resolve_terminal_color(color: &TerminalColorSnapshot, palette: &TerminalPalette) -> u32 {
    match color {
        TerminalColorSnapshot::Named(name) => resolve_named_terminal_color(name, palette),
        TerminalColorSnapshot::Indexed(index) => resolve_indexed_terminal_color(*index, palette),
        TerminalColorSnapshot::Rgb { r, g, b } => {
            (u32::from(*r) << 16) | (u32::from(*g) << 8) | u32::from(*b)
        }
    }
}

fn resolve_named_terminal_color(name: &str, p: &TerminalPalette) -> u32 {
    match name {
        "black" | "background" => p.background,
        "red" => p.ansi[1],
        "green" => p.ansi[2],
        "yellow" => p.ansi[3],
        "blue" => p.ansi[4],
        "magenta" => p.ansi[5],
        "cyan" => p.ansi[6],
        "white" | "foreground" | "bright-foreground" => p.foreground,
        "bright-black" => p.ansi[8],
        "bright-red" => p.ansi[9],
        "bright-green" => p.ansi[10],
        "bright-yellow" => p.ansi[11],
        "bright-blue" => p.ansi[12],
        "bright-magenta" => p.ansi[13],
        "bright-cyan" => p.ansi[14],
        "bright-white" => p.ansi[15],
        "cursor" => p.cursor,
        "dim-black" => dim_color(p.background),
        "dim-red" => dim_color(p.ansi[1]),
        "dim-green" => dim_color(p.ansi[2]),
        "dim-yellow" => dim_color(p.ansi[3]),
        "dim-blue" => dim_color(p.ansi[4]),
        "dim-magenta" => dim_color(p.ansi[5]),
        "dim-cyan" => dim_color(p.ansi[6]),
        "dim-white" | "dim-foreground" => dim_color(p.foreground),
        _ => p.foreground,
    }
}

fn resolve_indexed_terminal_color(index: u8, p: &TerminalPalette) -> u32 {
    match index {
        0..=15 => p.ansi[index as usize],
        16..=231 => {
            let value = index - 16;
            let r = cube_channel(value / 36);
            let g = cube_channel((value / 6) % 6);
            let b = cube_channel(value % 6);
            (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
        }
        232..=255 => {
            let shade = 8 + (index - 232) * 10;
            (u32::from(shade) << 16) | (u32::from(shade) << 8) | u32::from(shade)
        }
    }
}

fn terminal_dynamic_rgb(color: Color, colors: &Colors) -> Option<Rgb> {
    let index = match color {
        Color::Named(named) => named as usize,
        Color::Indexed(index) => usize::from(index),
        Color::Spec(_) => return None,
    };

    terminal_palette_rgb(index, colors)
}

fn terminal_palette_rgb(index: usize, colors: &Colors) -> Option<Rgb> {
    if index < TERMINAL_COLOR_COUNT {
        colors[index]
    } else {
        None
    }
}

fn terminal_color_request_rgb(
    index: usize,
    colors: Option<&Colors>,
    palette: &TerminalPalette,
) -> Option<Rgb> {
    if let Some(rgb) = colors.and_then(|colors| terminal_palette_rgb(index, colors)) {
        return Some(rgb);
    }

    let color = match index {
        index if index <= u8::MAX as usize => resolve_indexed_terminal_color(index as u8, palette),
        index if index == NamedColor::Foreground as usize => palette.foreground,
        index if index == NamedColor::Background as usize => palette.background,
        index if index == NamedColor::Cursor as usize => palette.cursor,
        index if index == NamedColor::DimBlack as usize => dim_color(palette.background),
        index if index == NamedColor::DimRed as usize => dim_color(palette.ansi[1]),
        index if index == NamedColor::DimGreen as usize => dim_color(palette.ansi[2]),
        index if index == NamedColor::DimYellow as usize => dim_color(palette.ansi[3]),
        index if index == NamedColor::DimBlue as usize => dim_color(palette.ansi[4]),
        index if index == NamedColor::DimMagenta as usize => dim_color(palette.ansi[5]),
        index if index == NamedColor::DimCyan as usize => dim_color(palette.ansi[6]),
        index if index == NamedColor::DimWhite as usize => dim_color(palette.foreground),
        index if index == NamedColor::BrightForeground as usize => palette.foreground,
        index if index == NamedColor::DimForeground as usize => dim_color(palette.foreground),
        _ => return None,
    };

    Some(u32_to_rgb(color))
}

fn u32_to_rgb(color: u32) -> Rgb {
    Rgb {
        r: ((color >> 16) & 0xff) as u8,
        g: ((color >> 8) & 0xff) as u8,
        b: (color & 0xff) as u8,
    }
}

fn cube_channel(value: u8) -> u8 {
    if value == 0 {
        0
    } else {
        55 + value * 40
    }
}

pub fn dim_color(color: u32) -> u32 {
    let r = (((color >> 16) & 0xff) as f32 * 0.66).round() as u32;
    let g = (((color >> 8) & 0xff) as f32 * 0.66).round() as u32;
    let b = ((color & 0xff) as f32 * 0.66).round() as u32;
    (r << 16) | (g << 8) | b
}

fn named_color_name(color: NamedColor) -> &'static str {
    match color {
        NamedColor::Black => "black",
        NamedColor::Red => "red",
        NamedColor::Green => "green",
        NamedColor::Yellow => "yellow",
        NamedColor::Blue => "blue",
        NamedColor::Magenta => "magenta",
        NamedColor::Cyan => "cyan",
        NamedColor::White => "white",
        NamedColor::BrightBlack => "bright-black",
        NamedColor::BrightRed => "bright-red",
        NamedColor::BrightGreen => "bright-green",
        NamedColor::BrightYellow => "bright-yellow",
        NamedColor::BrightBlue => "bright-blue",
        NamedColor::BrightMagenta => "bright-magenta",
        NamedColor::BrightCyan => "bright-cyan",
        NamedColor::BrightWhite => "bright-white",
        NamedColor::Foreground => "foreground",
        NamedColor::Background => "background",
        NamedColor::Cursor => "cursor",
        NamedColor::DimBlack => "dim-black",
        NamedColor::DimRed => "dim-red",
        NamedColor::DimGreen => "dim-green",
        NamedColor::DimYellow => "dim-yellow",
        NamedColor::DimBlue => "dim-blue",
        NamedColor::DimMagenta => "dim-magenta",
        NamedColor::DimCyan => "dim-cyan",
        NamedColor::DimWhite => "dim-white",
        NamedColor::BrightForeground => "bright-foreground",
        NamedColor::DimForeground => "dim-foreground",
    }
}

pub fn encode_terminal_key(
    key: &str,
    key_char: Option<&str>,
    control: bool,
    alt: bool,
    platform: bool,
) -> Option<Vec<u8>> {
    encode_terminal_key_with_modes(
        key,
        key_char,
        control,
        alt,
        false,
        platform,
        TerminalInputModes::default(),
    )
}

pub fn encode_terminal_key_with_modes(
    key: &str,
    key_char: Option<&str>,
    control: bool,
    alt: bool,
    shift: bool,
    platform: bool,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    encode_terminal_key_event_with_modes(key, None, key_char, control, alt, shift, platform, modes)
}

pub fn encode_terminal_key_event_with_modes(
    key: &str,
    key_without_modifiers: Option<&str>,
    key_char: Option<&str>,
    control: bool,
    alt: bool,
    shift: bool,
    platform: bool,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    if platform {
        return None;
    }

    #[cfg(feature = "warpui-app")]
    if let Some(bytes) = warp_escape_sequence_for_key(
        key,
        key_without_modifiers,
        key_char,
        control,
        alt,
        shift,
        modes,
    ) {
        return Some(bytes);
    }

    if control {
        if let Some(byte) = key_without_modifiers.and_then(control_byte) {
            return Some(vec![byte]);
        }
        if let Some(byte) = control_byte(key) {
            return Some(vec![byte]);
        }
    }

    let bytes = match key {
        // kitty 未激活时 shift+enter 兜底发 `\` + CR，对齐 iTerm2 terminal-setup / Warp，
        // 供 Claude Code 等 TUI 识别为换行（kitty 激活时上方已编成 CSI-u，不会走到这）。
        "enter" | "numpadenter" if shift => b"\\\r".to_vec(),
        "enter" => b"\r".to_vec(),
        "numpadenter" => b"\r".to_vec(),
        "backspace" => vec![0x7f],
        "tab" if shift => b"\x1b[Z".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" => b"\x1b".to_vec(),
        "insert" => b"\x1b[2~".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        _ => key_char?.as_bytes().to_vec(),
    };

    if alt {
        let mut escaped = Vec::with_capacity(bytes.len() + 1);
        escaped.push(0x1b);
        escaped.extend(bytes);
        Some(escaped)
    } else {
        Some(bytes)
    }
}

pub fn terminal_focus_report_bytes(focused: bool, modes: TerminalInputModes) -> Option<Vec<u8>> {
    if !modes.focus_in_out {
        return None;
    }

    Some(if focused {
        focus_in_bytes()
    } else {
        focus_out_bytes()
    })
}

pub fn terminal_alt_scroll_bytes(
    lines_to_scroll: i32,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    if lines_to_scroll == 0 || !modes.alt_screen || !modes.alternate_scroll {
        return None;
    }

    let sequence = if lines_to_scroll > 0 {
        alt_scroll_up_bytes()
    } else {
        alt_scroll_down_bytes()
    };
    let repeats = lines_to_scroll.unsigned_abs() as usize;
    let mut bytes = Vec::with_capacity(sequence.len() * repeats);
    for _ in 0..repeats {
        bytes.extend_from_slice(&sequence);
    }
    Some(bytes)
}

#[cfg(feature = "warpui-app")]
pub fn encode_terminal_modifier_key_with_modes(
    key_code: &KeyCode,
    is_press: bool,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    maybe_kitty_keyboard_escape_sequence(&WarpKeyModeProvider { modes }, key_code, is_press)
}

#[cfg(feature = "warpui-app")]
fn focus_in_bytes() -> Vec<u8> {
    EscCodes::FOCUS_IN.to_vec()
}

#[cfg(not(feature = "warpui-app"))]
fn focus_in_bytes() -> Vec<u8> {
    b"\x1b[I".to_vec()
}

#[cfg(feature = "warpui-app")]
fn focus_out_bytes() -> Vec<u8> {
    EscCodes::FOCUS_OUT.to_vec()
}

#[cfg(not(feature = "warpui-app"))]
fn focus_out_bytes() -> Vec<u8> {
    b"\x1b[O".to_vec()
}

#[cfg(feature = "warpui-app")]
fn alt_scroll_up_bytes() -> Vec<u8> {
    EscCodes::build_escape_sequence_with_c1(C1::SS3, &[EscCodes::ARROW_UP])
}

#[cfg(not(feature = "warpui-app"))]
fn alt_scroll_up_bytes() -> Vec<u8> {
    b"\x1bOA".to_vec()
}

#[cfg(feature = "warpui-app")]
fn alt_scroll_down_bytes() -> Vec<u8> {
    EscCodes::build_escape_sequence_with_c1(C1::SS3, &[EscCodes::ARROW_DOWN])
}

#[cfg(not(feature = "warpui-app"))]
fn alt_scroll_down_bytes() -> Vec<u8> {
    b"\x1bOB".to_vec()
}

fn control_byte(key: &str) -> Option<u8> {
    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || !ch.is_ascii() {
        return None;
    }
    let byte = (ch as u8).to_ascii_lowercase();
    if byte.is_ascii_lowercase() {
        Some(byte - b'a' + 1)
    } else {
        None
    }
}

#[cfg(feature = "warpui-app")]
#[derive(Clone, Copy, Debug, Default)]
struct WarpKeyModeProvider {
    modes: TerminalInputModes,
}

#[cfg(feature = "warpui-app")]
impl ModeProvider for WarpKeyModeProvider {
    fn is_term_mode_set(&self, mode: WarpTermMode) -> bool {
        mode.intersects(WarpTermMode::APP_CURSOR) && self.modes.app_cursor
            || mode.intersects(WarpTermMode::KEYBOARD_DISAMBIGUATE_ESCAPE)
                && self.modes.keyboard_disambiguate_escape
            || mode.intersects(WarpTermMode::KEYBOARD_REPORT_EVENT_TYPES)
                && self.modes.keyboard_report_event_types
            || mode.intersects(WarpTermMode::KEYBOARD_REPORT_ALTERNATE_KEYS)
                && self.modes.keyboard_report_alternate_keys
            || mode.intersects(WarpTermMode::KEYBOARD_REPORT_ALL_AS_ESCAPE)
                && self.modes.keyboard_report_all_as_escape
            || mode.intersects(WarpTermMode::KEYBOARD_REPORT_ASSOCIATED_TEXT)
                && self.modes.keyboard_report_associated_text
    }
}

#[cfg(feature = "warpui-app")]
fn warp_escape_sequence_for_key(
    key: &str,
    key_without_modifiers: Option<&str>,
    chars: Option<&str>,
    control: bool,
    alt: bool,
    shift: bool,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    let keystroke = Keystroke {
        ctrl: control,
        alt,
        shift,
        cmd: false,
        meta: false,
        key: key.to_string(),
    };

    KeystrokeWithDetails {
        keystroke: &keystroke,
        key_without_modifiers,
        chars,
    }
    .to_escape_sequence(&WarpKeyModeProvider { modes })
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            output.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for seq in chars.by_ref() {
                    if ('@'..='~').contains(&seq) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                let mut prev_escape = false;
                for seq in chars.by_ref() {
                    if seq == '\x07' || (prev_escape && seq == '\\') {
                        break;
                    }
                    prev_escape = seq == '\x1b';
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }

    output
}

/// OSC 7 序列起始标记 `ESC ] 7 ;`。
const OSC7_PREFIX: &[u8] = b"\x1b]7;";
/// 单个 OSC 7 sequence 长度上限；超出视为脏数据丢弃。
const OSC7_MAX_LEN: usize = 4096;

/// 扫描 buf 中所有完整的 OSC 7 sequence，对 payload 调用 `on_payload`，
/// 返回剩余字节（含潜在不完整前缀），供下一轮拼接。
fn consume_osc7_sequences<F: FnMut(&[u8])>(buf: &[u8], mut on_payload: F) -> Vec<u8> {
    let mut cursor = 0usize;
    while cursor < buf.len() {
        let Some(rel_start) = find_subslice(&buf[cursor..], OSC7_PREFIX) else {
            // 没起点：只保留末尾 prefix.len()-1 字节（防 prefix 跨 chunk 切断）
            let keep = OSC7_PREFIX.len().saturating_sub(1).min(buf.len() - cursor);
            return buf[buf.len() - keep..].to_vec();
        };
        let payload_start = cursor + rel_start + OSC7_PREFIX.len();
        match find_osc_terminator(&buf[payload_start..]) {
            Some((payload_len, term_len)) => {
                on_payload(&buf[payload_start..payload_start + payload_len]);
                cursor = payload_start + payload_len + term_len;
            }
            None => {
                // 起点已出现但终止符未到：buf 留住从起点开始的全部，等下一轮拼接。
                // 超长 → 丢弃这一段，从 prefix 之后继续找下一个起点。
                if buf.len() - (cursor + rel_start) > OSC7_MAX_LEN {
                    cursor = payload_start;
                    continue;
                }
                return buf[cursor + rel_start..].to_vec();
            }
        }
    }
    Vec::new()
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 找 OSC 终止符：BEL (\x07) 或 ST (\x1b\\)。返回 (payload_len, terminator_len)。
fn find_osc_terminator(payload: &[u8]) -> Option<(usize, usize)> {
    for (i, &b) in payload.iter().enumerate() {
        if b == 0x07 {
            return Some((i, 1));
        }
        if b == 0x1b && payload.get(i + 1) == Some(&b'\\') {
            return Some((i, 2));
        }
    }
    None
}

/// 解析 OSC 7 payload，提取 PathBuf。支持两种形式：
/// - `file://host/path`：剥掉 `file://` + host，剩 `/path` 做 url-decode。
/// - 退化形式：直接 `/path` 或 `path`，整体 url-decode。
/// host 段当前忽略（v1 只关心本地）。
fn parse_osc7_payload(payload: &[u8]) -> Option<PathBuf> {
    let text = std::str::from_utf8(payload).ok()?;
    let path_part = if let Some(rest) = text.strip_prefix("file://") {
        // 跳过 host：第一个 '/' 之后是路径
        match rest.find('/') {
            Some(slash) => &rest[slash..],
            None => return None,
        }
    } else {
        text
    };
    let decoded = url_decode_percent(path_part);
    if decoded.is_empty() {
        None
    } else {
        Some(PathBuf::from(decoded))
    }
}

/// 极简 URL percent-decode：`%XX` → 单字节，其余原样。
/// 不依赖外部 crate；非 UTF-8 时尽量保留原始字节。
fn url_decode_percent(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_state_revision_advances_when_state_changes() {
        let state = Arc::new(FairMutex::new(TerminalRuntimeState::new(
            "sshtool", true, "running", 80, 24,
        )));

        assert_eq!(state.lock().revision, 0);

        update_state(&state, |state| {
            state.status = "updated".to_string();
        });

        assert_eq!(state.lock().revision, 1);
    }

    fn forty_lines() -> String {
        let mut s = String::new();
        for i in 1..=40 {
            s.push_str(&format!("line {i}\r\n"));
        }
        s
    }

    #[test]
    fn alt_screen_retains_scrollback() {
        // 备用屏滚出的行应进 scrollback（ADR 0006）。external crate 读不到
        // history_size，用"上滚后 display_offset 是否 >0"间接证明历史存在。
        let mut core = TerminalGridCore::new(80, 24, 1000);
        // 先单独切备用屏：触发 alt-enter，给 alt grid 撑历史；再灌 40 行（超屏高）。
        core.process_output(b"\x1b[?1049h");
        core.process_output(forty_lines().as_bytes());
        core.term.scroll_display(Scroll::Delta(5));
        assert!(
            core.term.grid().display_offset() > 0,
            "备用屏应保留 scrollback，可上滚回看"
        );
    }

    #[test]
    fn primary_screen_scrollback_baseline() {
        // 对照：主屏本就有历史，确认上述测法可信。
        let mut core = TerminalGridCore::new(80, 24, 1000);
        core.process_output(forty_lines().as_bytes());
        core.term.scroll_display(Scroll::Delta(5));
        assert!(core.term.grid().display_offset() > 0);
    }

    #[test]
    fn alternate_scroll_off_by_default_enabled_by_app() {
        // alacritty 默认开 ALTERNATE_SCROLL，会让备用屏滚轮被转发成 ↑/↓ 而非本地滚动。
        // new() 应已关掉（iTerm2 同款默认关）；应用显式 ?1007h 才重开。
        let mut core = TerminalGridCore::new(80, 24, 1000);
        assert!(
            !core.term.mode().contains(TermMode::ALTERNATE_SCROLL),
            "ALTERNATE_SCROLL 应默认关闭"
        );
        core.process_output(b"\x1b[?1007h");
        assert!(
            core.term.mode().contains(TermMode::ALTERNATE_SCROLL),
            "应用显式 ?1007h 后应重新开启"
        );
    }

    #[test]
    fn osc7_parse_handles_three_forms() {
        let p1 = parse_osc7_payload(b"file://host/home/matt").unwrap();
        assert_eq!(p1, PathBuf::from("/home/matt"));

        let p2 = parse_osc7_payload(b"file:///home/matt").unwrap();
        assert_eq!(p2, PathBuf::from("/home/matt"));

        let p3 = parse_osc7_payload(b"/tmp/foo").unwrap();
        assert_eq!(p3, PathBuf::from("/tmp/foo"));
    }

    #[test]
    fn osc7_parse_decodes_percent_escapes() {
        let payload = b"file://host/home/with%20space/%E4%B8%AD";
        let p = parse_osc7_payload(payload).unwrap();
        assert_eq!(p, PathBuf::from("/home/with space/中"));
    }

    #[test]
    fn consume_osc7_handles_full_sequence_bel() {
        let buf = b"junk\x1b]7;file:///a/b\x07more";
        let mut hits = Vec::new();
        let tail = consume_osc7_sequences(buf, |p| hits.push(p.to_vec()));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], b"file:///a/b");
        // 处理完后 cursor 落到 "more" 起点；剩余无 prefix 起点，只保留尾巴 prefix.len()-1=3 字节
        assert_eq!(tail, b"ore");
    }

    #[test]
    fn consume_osc7_handles_st_terminator() {
        let buf = b"\x1b]7;/path\x1b\\";
        let mut hits = Vec::new();
        let tail = consume_osc7_sequences(buf, |p| hits.push(p.to_vec()));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], b"/path");
        assert!(tail.is_empty());
    }

    #[test]
    fn consume_osc7_split_across_chunks() {
        let chunk1 = b"\x1b]7;/foo";
        let chunk2 = b"/bar\x07";
        let mut hits = Vec::new();
        let tail = consume_osc7_sequences(chunk1, |p| hits.push(p.to_vec()));
        assert!(hits.is_empty(), "未完成时不该触发");
        // 第二轮拼上 tail
        let mut combined = tail;
        combined.extend_from_slice(chunk2);
        let tail2 = consume_osc7_sequences(&combined, |p| hits.push(p.to_vec()));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], b"/foo/bar");
        assert!(tail2.is_empty());
    }

    #[test]
    fn consume_osc7_drops_oversized_payload() {
        // 起点存在但 payload 超长且无终止符 → 应丢弃，避免无限增长
        let mut buf = b"\x1b]7;".to_vec();
        buf.extend(std::iter::repeat(b'x').take(OSC7_MAX_LEN + 100));
        let mut hits = Vec::new();
        let tail = consume_osc7_sequences(&buf, |p| hits.push(p.to_vec()));
        assert!(hits.is_empty());
        // 丢弃后 tail 不应保留这一大坨脏数据（最多 prefix.len()-1 字节）
        assert!(tail.len() < OSC7_PREFIX.len());
    }

    #[test]
    fn scan_osc7_updates_local_cwd() {
        let mut state = TerminalRuntimeState::new("t", true, "ok", 80, 24);
        assert!(state.local_cwd.is_none());
        state.scan_osc7(b"prefix\x1b]7;file://h/tmp/x\x07tail");
        assert_eq!(state.local_cwd, Some(PathBuf::from("/tmp/x")));
        // 第二次 cwd 变更
        state.scan_osc7(b"\x1b]7;file://h/var\x07");
        assert_eq!(state.local_cwd, Some(PathBuf::from("/var")));
    }
}
