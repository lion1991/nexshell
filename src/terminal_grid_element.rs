//! WarpUI terminal grid element.

use std::{
    ops::Range,
    sync::{Arc, Mutex},
};

use alacritty_terminal::index::{Column, Line, Point as TermPoint, Side};
use alacritty_terminal::selection::SelectionType;
use pathfinder_color::ColorU;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::Vector2F;
use warp_core::ui::theme::AnsiColorIdentifier;

use nexshell::pane_state::NexPaneId;
use nexshell::pane_tree::DraggedBorder;

use warp_util::path::ShellFamily;

use crate::external_editor::EditorChoice;
use warpui_core::{
    clipboard_utils,
    elements::{
        compute_scrollbar_geometry, AfterLayoutContext, Axis, CornerRadius, Element, EventContext,
        Fill, LayoutContext, Point, Radius, ScrollData, ScrollbarAppearance, ScrollbarGeometry,
        ScrollbarWidth, SizeConstraint, DEFAULT_UI_LINE_HEIGHT_RATIO,
    },
    event::{DispatchedEvent, Event, InBoundsExt, KeyState, ModifiersState},
    fonts::{Cache as FontCache, FamilyId, Properties, Style, Weight},
    platform::LineStyle,
    scene::ClipBounds,
    text_layout::{
        ClipConfig, Line as TextLine, StyleAndFont, TextStyle, DEFAULT_TOP_BOTTOM_RATIO,
    },
    units::Pixels,
    AppContext, PaintContext,
};

use nexshell::file_panel::FilePanelSelectMode;
use nexshell::git_panel::GitPanelSelectMode;
use nexshell::terminal_runtime::{
    dim_color, encode_sgr_mouse_report, encode_terminal_key_event_with_modes,
    encode_terminal_modifier_key_with_modes, mouse_mode_bits_app_active,
    mouse_mode_bits_drag_active, mouse_mode_bits_motion_active, resolve_terminal_color,
    terminal_alt_scroll_bytes, terminal_input_editor_should_capture, LocalTerminalRuntime,
    MouseReportAction, MouseReportButton, MouseReportModifiers, TerminalCursorShape,
    TerminalGridCellSnapshot, TerminalGridSnapshot, TerminalInputEditor, TerminalPalette,
    TerminalRuntimeSnapshot,
};
use nexshell::warp_tab_context_menu::TabContextMenuAnchor;

pub const TERMINAL_CURSOR_POSITION_ID: &str = "terminal_view:cursor_native_shell_spike";

#[derive(Clone, Copy)]
pub(crate) struct TerminalImeLayout {
    element_origin: Vector2F,
    cell_metrics: CellMetrics,
    font_size: f32,
}

impl TerminalImeLayout {
    pub(crate) fn font_size(&self) -> f32 {
        self.font_size
    }

    pub(crate) fn cursor_rect_for_snapshot(
        &self,
        snapshot: &TerminalGridSnapshot,
        smooth_scroll_px: f32,
    ) -> Option<RectF> {
        let default_palette = TerminalPalette::default();
        let grid = RuntimeGridView {
            grid: snapshot,
            palette: &default_palette,
        };
        terminal_ime_cursor_rect_for_layout(&grid, self, smooth_scroll_px)
    }
}

// PTY 异步输出的 redraw 通路改由 RootView 端的后台轮询负责（见 main.rs
// `schedule_poll`），用 `ViewContext::spawn` + `Timer::after` 自重排，
// revision 变了才 `ctx.notify()`。dispatch_event 只在自己处理事件后立刻
// `ctx.notify()`，确保用户输入立即上屏。

#[allow(dead_code)]
#[derive(Clone)]
pub struct GridCell {
    pub ch: char,
    pub content: Arc<str>,
    pub fg: ColorU,
    pub bg: ColorU,
    pub underline_color: Option<ColorU>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub double_underline: bool,
    pub strikeout: bool,
    pub wide_spacer: bool,
    pub hyperlink: Option<Arc<str>>,
}

impl GridCell {
    fn empty_with_palette(palette: &TerminalPalette) -> Self {
        Self {
            ch: ' ',
            content: Arc::from(" "),
            fg: u32_to_color(palette.foreground),
            bg: u32_to_color(palette.background),
            underline_color: None,
            bold: false,
            italic: false,
            underline: false,
            double_underline: false,
            strikeout: false,
            wide_spacer: false,
            hyperlink: None,
        }
    }

    #[allow(dead_code)]
    pub fn empty() -> Self {
        Self::empty_with_palette(&TerminalPalette::default())
    }
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct GridSnapshot {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<GridCell>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub cursor_shape: TerminalCursorShape,
    pub cursor_visible: bool,
    pub cursor_blinking: bool,
    pub marked_text_active: bool,
    pub display_offset: usize,
    pub history_size: usize,
    pub mouse_report_click: bool,
    pub mouse_report_motion: bool,
    pub mouse_report_drag: bool,
    pub sgr_mouse: bool,
}

#[allow(dead_code)]
impl GridSnapshot {
    pub fn cell_ref(&self, row: usize, col: usize) -> Option<&GridCell> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        self.cells.get(row * self.cols + col)
    }

    pub fn cell(&self, row: usize, col: usize) -> Option<GridCell> {
        self.cell_ref(row, col).cloned()
    }

    pub fn from_runtime_snapshot(
        snapshot: &TerminalGridSnapshot,
        palette: &TerminalPalette,
    ) -> Self {
        let cols = snapshot.cols.max(1);
        let rows = snapshot.rows.max(1);
        let mut cells = Vec::with_capacity(cols * rows);
        for (row_idx, row) in snapshot.lines.iter().enumerate() {
            for col_idx in 0..cols {
                let Some(cell) = row.cells.get(col_idx) else {
                    cells.push(GridCell::empty_with_palette(palette));
                    continue;
                };
                let style = snapshot.cell_style(cell);
                let mut fg = resolve_terminal_color(&style.fg, palette);
                let mut bg = resolve_terminal_color(&style.bg, palette);
                let content = if cell.hidden() {
                    Arc::from(" ")
                } else {
                    Arc::clone(&cell.content)
                };
                if cell.dim() {
                    fg = dim_color(fg);
                }
                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let cursor = row_idx == snapshot.cursor_row && col_idx == snapshot.cursor_col;
                if cursor
                    && snapshot.cursor_visible
                    && snapshot.cursor_shape == TerminalCursorShape::Block
                    && !snapshot.marked_text_active
                    && !runtime_cell_occupied_by_text(cell)
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
                let hyperlink = cell.hyperlink.as_deref().map(Arc::from);
                cells.push(GridCell {
                    ch: cell.ch,
                    content,
                    fg: u32_to_color(fg),
                    bg: u32_to_color(bg),
                    underline_color: style
                        .underline_color
                        .as_ref()
                        .map(|c| u32_to_color(resolve_terminal_color(c, palette))),
                    bold: cell.bold(),
                    italic: cell.italic(),
                    underline: cell.underline() || cell.hyperlink.is_some(),
                    double_underline: cell.double_underline(),
                    strikeout: cell.strikeout(),
                    wide_spacer: cell.wide_spacer(),
                    hyperlink,
                });
            }
        }
        while cells.len() < cols * rows {
            cells.push(GridCell::empty_with_palette(palette));
        }
        cells.truncate(cols * rows);
        Self {
            cols,
            rows,
            cells,
            cursor_row: snapshot.cursor_row,
            cursor_col: snapshot.cursor_col,
            cursor_shape: snapshot.cursor_shape,
            cursor_visible: snapshot.cursor_visible,
            cursor_blinking: snapshot.cursor_blinking,
            marked_text_active: snapshot.marked_text_active,
            display_offset: snapshot.display_offset,
            history_size: snapshot.history_size,
            mouse_report_click: snapshot.mouse_report_click,
            mouse_report_motion: snapshot.mouse_report_motion,
            mouse_report_drag: snapshot.mouse_report_drag,
            sgr_mouse: snapshot.sgr_mouse,
        }
    }

    pub fn mouse_app_active(&self) -> bool {
        self.sgr_mouse
            && (self.mouse_report_click || self.mouse_report_motion || self.mouse_report_drag)
    }

    pub fn mouse_drag_reporting_active(&self) -> bool {
        self.sgr_mouse && (self.mouse_report_drag || self.mouse_report_motion)
    }

    pub fn mouse_motion_reporting_active(&self) -> bool {
        self.sgr_mouse && self.mouse_report_motion
    }
}

#[derive(Clone, Copy)]
struct RenderCell<'a> {
    content: &'a str,
    fg: ColorU,
    bg: ColorU,
    underline_color: Option<ColorU>,
    bold: bool,
    italic: bool,
    underline: bool,
    double_underline: bool,
    strikeout: bool,
    wide_spacer: bool,
    hyperlink: Option<&'a str>,
}

trait TerminalGridAccess {
    fn cols(&self) -> usize;
    fn rows(&self) -> usize;
    fn cursor_row(&self) -> usize;
    fn cursor_col(&self) -> usize;
    fn cursor_shape(&self) -> TerminalCursorShape;
    fn cursor_visible(&self) -> bool;
    fn display_offset(&self) -> usize;
    fn history_size(&self) -> usize;
    fn mouse_report_click(&self) -> bool;
    fn mouse_report_motion(&self) -> bool;
    fn mouse_report_drag(&self) -> bool;
    fn sgr_mouse(&self) -> bool;
    fn dirty_row(&self, row: usize) -> bool;
    fn line_text(&self, row: usize) -> &str;
    fn cell(&self, row: usize, col: usize) -> Option<RenderCell<'_>>;
}

impl TerminalGridAccess for GridSnapshot {
    fn cols(&self) -> usize {
        self.cols
    }

    fn rows(&self) -> usize {
        self.rows
    }

    fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    fn cursor_shape(&self) -> TerminalCursorShape {
        self.cursor_shape
    }

    fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    fn display_offset(&self) -> usize {
        self.display_offset
    }

    fn history_size(&self) -> usize {
        self.history_size
    }

    fn mouse_report_click(&self) -> bool {
        self.mouse_report_click
    }

    fn mouse_report_motion(&self) -> bool {
        self.mouse_report_motion
    }

    fn mouse_report_drag(&self) -> bool {
        self.mouse_report_drag
    }

    fn sgr_mouse(&self) -> bool {
        self.sgr_mouse
    }

    fn dirty_row(&self, _: usize) -> bool {
        true
    }

    fn line_text(&self, _row: usize) -> &str {
        ""
    }

    fn cell(&self, row: usize, col: usize) -> Option<RenderCell<'_>> {
        self.cell_ref(row, col).map(|cell| RenderCell {
            content: cell.content.as_ref(),
            fg: cell.fg,
            bg: cell.bg,
            underline_color: cell.underline_color,
            bold: cell.bold,
            italic: cell.italic,
            underline: cell.underline,
            double_underline: cell.double_underline,
            strikeout: cell.strikeout,
            wide_spacer: cell.wide_spacer,
            hyperlink: cell.hyperlink.as_deref(),
        })
    }
}

struct RuntimeGridView<'a> {
    grid: &'a TerminalGridSnapshot,
    palette: &'a TerminalPalette,
}

impl TerminalGridAccess for RuntimeGridView<'_> {
    fn cols(&self) -> usize {
        self.grid.cols.max(1)
    }

    fn rows(&self) -> usize {
        self.grid.rows.max(1)
    }

    fn cursor_row(&self) -> usize {
        self.grid.cursor_row
    }

    fn cursor_col(&self) -> usize {
        self.grid.cursor_col
    }

    fn cursor_shape(&self) -> TerminalCursorShape {
        self.grid.cursor_shape
    }

    fn cursor_visible(&self) -> bool {
        self.grid.cursor_visible
    }

    fn display_offset(&self) -> usize {
        self.grid.display_offset
    }

    fn history_size(&self) -> usize {
        self.grid.history_size
    }

    fn mouse_report_click(&self) -> bool {
        self.grid.mouse_report_click
    }

    fn mouse_report_motion(&self) -> bool {
        self.grid.mouse_report_motion
    }

    fn mouse_report_drag(&self) -> bool {
        self.grid.mouse_report_drag
    }

    fn sgr_mouse(&self) -> bool {
        self.grid.sgr_mouse
    }

    fn dirty_row(&self, row: usize) -> bool {
        self.grid.dirty_rows.get(row).copied().unwrap_or(true)
    }

    fn line_text(&self, row: usize) -> &str {
        self.grid
            .lines
            .get(row)
            .map(|line| line.text.as_str())
            .unwrap_or("")
    }

    fn cell(&self, row: usize, col: usize) -> Option<RenderCell<'_>> {
        let row_data = self.grid.lines.get(row)?;
        let cell = row_data.cells.get(col)?;
        Some(runtime_render_cell(self.grid, row, col, cell, self.palette))
    }
}

fn runtime_render_cell<'a>(
    snapshot: &TerminalGridSnapshot,
    row_idx: usize,
    col_idx: usize,
    cell: &'a TerminalGridCellSnapshot,
    palette: &TerminalPalette,
) -> RenderCell<'a> {
    let style = snapshot.cell_style(cell);
    let mut fg = resolve_terminal_color(&style.fg, palette);
    let mut bg = resolve_terminal_color(&style.bg, palette);
    if cell.dim() {
        fg = dim_color(fg);
    }
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }
    let cursor = row_idx == snapshot.cursor_row && col_idx == snapshot.cursor_col;
    if cursor
        && snapshot.cursor_visible
        && snapshot.cursor_shape == TerminalCursorShape::Block
        && !snapshot.marked_text_active
        && !runtime_cell_occupied_by_text(cell)
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

    RenderCell {
        content: if cell.hidden() {
            " "
        } else {
            cell.content.as_ref()
        },
        fg: u32_to_color(fg),
        bg: u32_to_color(bg),
        underline_color: style
            .underline_color
            .as_ref()
            .map(|color| u32_to_color(resolve_terminal_color(color, palette))),
        bold: cell.bold(),
        italic: cell.italic(),
        underline: cell.underline() || cell.hyperlink.is_some(),
        double_underline: cell.double_underline(),
        strikeout: cell.strikeout(),
        wide_spacer: cell.wide_spacer(),
        hyperlink: cell.hyperlink.as_deref(),
    }
}

fn runtime_cell_occupied_by_text(cell: &TerminalGridCellSnapshot) -> bool {
    cell.wide_spacer() || (cell.content.as_ref() != " " && cell.content.as_ref() != "\0")
}

fn u32_to_color(rgb: u32) -> ColorU {
    // terminal_runtime 用 0x__RRGGBB（alpha 隐含 0xff）；ColorU 需要 0xRRGGBBAA。
    ColorU::new(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
        0xff,
    )
}

/// warp: grid_size_util.rs — terminal cell 像素尺寸（advance × line_height）
#[derive(Clone, Copy)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
    /// warp: calculate_grid_baseline_position — 基线 Y 偏移
    pub baseline_y: f32,
}

impl CellMetrics {
    /// warp: grid_size_util.rs:15-57 grid_cell_dimensions + :61-81 calculate_grid_baseline_position
    pub fn from_font_cache(
        font_cache: &FontCache,
        font_family: FamilyId,
        font_size: f32,
        line_height_ratio: f32,
    ) -> Self {
        let font_id = font_cache.select_font(font_family, Default::default());
        let ascent = font_cache.ascent(font_id, font_size);
        let descent = font_cache.descent(font_id, font_size);
        let leading = font_cache.leading(font_id, font_size);

        // warp: grid_cell_dimensions — 'm' advance 取整为 cell 宽度
        let (m_glyph, _) = font_cache
            .glyph_for_char(font_id, 'm', false)
            .expect("font must contain 'm' glyph");
        let width = font_cache
            .glyph_advance(font_id, font_size, m_glyph)
            .map(|adv| adv.x().round().max(1.0))
            .unwrap_or_else(|_| font_cache.em_width(font_family, font_size).max(1.0));

        // warp: height = (ascent - descent + leading) * (ratio / DEFAULT_RATIO), ceil
        let height = ((ascent - descent + leading)
            * (line_height_ratio / DEFAULT_UI_LINE_HEIGHT_RATIO))
            .ceil()
            .max(1.0);

        // warp: calculate_grid_baseline_position
        let baseline_y = height - leading.floor()
            + (descent.floor() * (line_height_ratio / DEFAULT_UI_LINE_HEIGHT_RATIO).min(1.0));

        Self {
            width,
            height,
            baseline_y,
        }
    }
}

pub struct TerminalGridElement {
    snapshot: Arc<TerminalRuntimeSnapshot>,
    cell_metrics: CellMetrics,
    font_family: FamilyId,
    font_size: f32,
    terminal: Arc<Mutex<LocalTerminalRuntime>>,
    input_editor: Arc<Mutex<TerminalInputEditor>>,
    selection_drag: Arc<Mutex<bool>>,
    last_resize_cells: Arc<Mutex<(u16, u16)>>,
    scrollbar_drag: Arc<Mutex<Option<ScrollbarDrag>>>,
    cursor_over_terminal: Arc<Mutex<bool>>,
    scrollbar_thumb_hovered: Arc<Mutex<bool>>,
    find_state: Arc<Mutex<FindPanelState>>,
    smooth_scroll_px: Arc<Mutex<f64>>,
    shaped_line_cache: Arc<Mutex<TerminalShapedLineCache>>,
    terminal_ime_layout: Arc<Mutex<Option<TerminalImeLayout>>>,
    shell_is_foreground: Arc<std::sync::atomic::AtomicBool>,
    /// 鼠标上报模式实时镜像。发报告前查它而非渲染快照：TUI 退出瞬间快照
    /// 滞后一帧，按旧状态继续上报会把 \e[<35;x;yM 漏给 shell 回显。
    mouse_modes: Arc<std::sync::atomic::AtomicU8>,
    pane_id: Option<NexPaneId>,
    is_focused_pane: bool,
    palette: TerminalPalette,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BackgroundRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: ColorU,
}

impl BackgroundRect {
    fn new(x: f32, y: f32, width: f32, height: f32, color: ColorU) -> Self {
        Self {
            x,
            y,
            width,
            height,
            color,
        }
    }

    fn to_rect(self, origin: Vector2F) -> RectF {
        RectF::new(
            origin + Vector2F::new(self.x, self.y),
            Vector2F::new(self.width, self.height),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CursorRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl CursorRect {
    fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn to_rect(self, origin: Vector2F) -> RectF {
        RectF::new(
            origin + Vector2F::new(self.x, self.y),
            Vector2F::new(self.width, self.height),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DecorationRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: ColorU,
}

impl DecorationRect {
    fn new(x: f32, y: f32, width: f32, height: f32, color: ColorU) -> Self {
        Self {
            x,
            y,
            width,
            height,
            color,
        }
    }

    fn to_rect(self, origin: Vector2F) -> RectF {
        RectF::new(
            origin + Vector2F::new(self.x, self.y),
            Vector2F::new(self.width, self.height),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarDrag {
    previous_y: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScrollbarHit {
    Thumb,
    Track,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FindPanelState {
    pub active: bool,
    pub query: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TerminalGridAction {
    CopySelection,
    PasteClipboard,
    ClearVisibleScreen,
    OpenFindBar,
    CloseFindBar,
    FindStep(i32),
    IncreaseFontSize,
    DecreaseFontSize,
    ResetFontSize,
    // Chrome actions: 不影响 grid，沿用同一 action 类型避免 RootView 多重 TypedActionView。
    ToggleSidebar,
    WindowMinimize,
    WindowToggleMaximize,
    WindowClose,
    ToggleHostNetworkDropdown,
    SelectHostNetwork(String),
    SortHostProcesses(nexshell::host_overview::ProcessSortKey),
    SortHostNetwork(nexshell::host_overview::NetworkSortKey),
    CopyHostAddress(String),
    NewTab,
    ToggleNewSessionMenu,
    SelectTab(usize),
    MoveTabLeft(usize),
    MoveTabRight(usize),
    RenameTab(usize),
    ResetTabName(usize),
    CloseTab(usize),
    CloseOtherTabs(usize),
    CloseTabsRight(usize),
    ReconnectTab(usize),
    DisconnectTab(usize),
    ToggleTabRecording(usize),
    DuplicateTab(usize),
    ToggleTabColor {
        color: AnsiColorIdentifier,
        tab_index: usize,
    },
    ActivatePrevTab,
    ActivateNextTab,
    ToggleTabRightClickMenu {
        tab_index: usize,
        anchor: TabContextMenuAnchor,
    },
    TabHoverWidthStart {
        width: f32,
    },
    TabHoverWidthEnd,
    StartTabDrag,
    DragTab {
        tab_index: usize,
        tab_position: RectF,
    },
    DropTab,
    TerminalMouseDown,
    ShowTerminalContextMenu {
        position: Vector2F,
        has_selection: bool,
    },
    // --- 主机管理面板 actions ---
    HostShowContextMenu {
        host_id: String,
        position: Vector2F,
    },
    HostClipboardCopy(String),
    HostClipboardCut(String),
    HostClipboardPaste,
    HostRestoreDeleted,
    HostRenameInline(String),
    HostEditOne(String),
    HostDeleteOne(String),
    HostQuickConnect(String),
    HostToggleSelect(String),
    HostSelectSingle(String),
    HostToggleSelectAll,
    HostSelectGroup(String),
    HostToggleTag(String),
    HostToggleProtocolDropdown,
    HostSetProtocolFilter(nexshell::host_management::ProtocolFilter),
    HostSetViewMode(nexshell::host_management::HostViewMode),
    HostTogglePrivacy,
    HostRefresh,
    HostNewHost,
    HostDeleteSelected,
    HostEditSelected,
    HostConnectSelected,
    HostClearSelection,
    HostEnterReorderMode,
    HostExitReorderMode,
    HostStartCardDrag,
    HostDragCard {
        host_id: String,
        card_position: RectF,
    },
    HostDropCard,
    HostImport,
    HostExport,
    HostPasswordConfirm,
    HostPasswordCancel,
    HostCloudSync,
    HostManageGroupsTags,
    HostImportKeyFile(String),
    HostDeleteKey(String),
    HostSelectKey(String),
    HostCopyKeyToServer,
    HostEditKey,
    HostKeyEditSave,
    HostKeyEditCancel,
    HostDeleteKeyPrompt,
    HostDeleteKeyCancel,
    ShowHostManagement,
    OpenProcessList,
    OpenNetworkList,
    OpenSystemInfo,
    ProcessListShowContextMenu {
        pid: u32,
        command: String,
        args: String,
        exe_path: String,
        position: Vector2F,
    },
    KillRemoteProcess {
        pid: u32,
        label: String,
    },
    // --- 分屏 actions ---
    SplitRight,
    SplitDown,
    SplitLeft,
    SplitUp,
    ClosePane,
    FocusPane(NexPaneId),
    NavigatePaneLeft,
    NavigatePaneRight,
    NavigatePaneUp,
    NavigatePaneDown,
    StartPaneResizing(DraggedBorder),
    PaneResizeMove(Vector2F),
    EndPaneResizing,
    ToggleMaximizePane,
    // --- 文件面板 actions ---
    ToggleFilePanel,
    FilePanelRefresh,
    FilePanelGoUp,
    FilePanelEnterDir(String),
    /// 点击 entry。mode 由 UI 层根据 cmd / shift 修饰键决定。
    FilePanelSelect {
        name: String,
        mode: FilePanelSelectMode,
    },
    /// 本地 Project explorer 树行点击。远程 SSH 文件面板不使用该 action。
    FilePanelTreeItemClicked {
        path: String,
        is_dir: bool,
        mode: FilePanelSelectMode,
    },
    FilePanelDropFiles(Vec<String>),
    FilePanelShowContextMenu {
        /// None 表示在空白区域右键，菜单只显示与"上下文目录"相关的项；
        /// Some(name) 时同时把该 entry 选中。
        name: Option<String>,
        is_dir: bool,
        position: Vector2F,
    },
    FilePanelDownload {
        name: String,
        is_dir: bool,
    },
    FilePanelOpenUploadDialog,
    FilePanelCancelTransfer(u64),
    FilePanelDelete {
        name: String,
        is_dir: bool,
    },
    FilePanelStartRename {
        name: String,
    },
    FilePanelStartNewDir,
    FilePanelStartNewFile,
    FilePanelStartNewFileIn {
        parent: String,
    },
    /// 文件面板跳回终端所在目录并恢复跟随（与 FilePanelCdToDirectory 反向，仅本地终端）。
    FilePanelSyncToTerminalCwd,
    FilePanelCdToDirectory {
        path: String,
    },
    FilePanelOpenDirectoryInNewTab {
        path: String,
    },
    FilePanelRevealInFileManager {
        path: String,
    },
    FilePanelOpenWithDefault {
        path: String,
    },
    FilePanelOpenInEditor {
        path: String,
    },
    /// 在内置查看器/编辑器中打开本地文本文件（ADR 0002/0003）。
    FilePanelOpenInCodeViewer {
        path: String,
    },
    /// 保存内置代码编辑器当前文件（Cmd+S；仅 active 为 CodeViewer 时生效，ADR 0003）。
    CodeViewerSave,
    FilePanelCopyPath {
        name: String,
    },
    FilePanelCopyRelativePath {
        path: String,
    },
    FilePanelResizeStart(f32),
    FilePanelResizeMove(f32),
    FilePanelResizeEnd,
    // --- git 面板 actions ---
    ToggleGitPanel,
    GitPanelRefresh,
    GitPanelSelectEntry {
        path: String,
        kind: nexshell::git_ops::GitDiffKind,
        mode: GitPanelSelectMode,
    },
    GitPanelStage(String),
    GitPanelStageAll(Vec<String>),
    GitPanelUnstage(String),
    GitPanelStagePaths {
        tab_id: String,
        paths: Vec<String>,
    },
    GitPanelUnstagePaths {
        tab_id: String,
        paths: Vec<String>,
    },
    GitPanelAddToGitignore {
        tab_id: String,
        paths: Vec<String>,
    },
    GitPanelShowContextMenu {
        tab_id: String,
        path: String,
        kind: nexshell::git_ops::GitDiffKind,
        discard_enabled: bool,
        position: Vector2F,
    },
    GitPanelDiscardWorktreeChanges {
        tab_id: String,
        path: String,
    },
    GitPanelDeleteUntracked {
        tab_id: String,
        path: String,
    },
    GitPanelResizeStart(f32),
    GitPanelResizeMove(f32),
    GitPanelResizeEnd,
    GitHistoryResizeStart(f32),
    GitHistoryResizeMove(f32),
    GitHistoryResizeEnd,
    GitHistoryScrolled {
        tab_id: String,
        scroll_start: f32,
        delta_y: f32,
    },
    GitCommitRowHover {
        tab_id: String,
        sha: String,
        hovered: bool,
    },
    GitCommitDetailHover {
        tab_id: String,
        sha: String,
        hovered: bool,
    },
    GitCommitSelect {
        tab_id: String,
        sha: String,
    },
    GitCommitHoverSweep,
    GitCommitCopySha(String),
    GitCommitEditorFocus,
    // commit footer：inline 输入框 + 提交按钮（VS Code Source Control 同款）
    GitCommitConfirm,
    GitPushConfirm,
    // --- 设置菜单 actions ---
    ToggleSettingsMenu,
    SettingsMenuWhatsNew,
    SettingsMenuDocumentation,
    SettingsMenuFeedback,
    SettingsMenuViewLogs,
    // --- 设置页面 actions ---
    ShowSettings,
    ShowSettingsKeybindings,
    CloseSettingsTab,
    SettingsSelectPage(NexSettingsSection),
    SetTheme(ThemeChoice),
    SetTerminalFontSize(f32),
    SetOpacity(u8),
    SetCursorStyle(CursorStyleChoice),
    SetFontFamily(String),
    SetFontWeight(warpui::fonts::Weight),
    SetOpenFileEditor(EditorChoice),
    /// 「diff / 查看器复用单标签」开关（ADR 0002，默认开启）。
    SetReuseViewTab(bool),
    SetLineHeight(f32),
    ResetLineHeight,
    ToggleViewAllFonts,
    ShowThemeChooser,
    CloseThemeChooser,
    SetLanguage(LanguageChoice),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    Dark,
    Light,
    Dracula,
    SolarizedDark,
    SolarizedLight,
    GruvboxDark,
    GruvboxLight,
    CyberWave,
    WillowDream,
    Adeberry,
    Phenomenon,
}

impl ThemeChoice {
    pub const ALL: [Self; 11] = [
        Self::Dark,
        Self::Light,
        Self::Dracula,
        Self::SolarizedDark,
        Self::SolarizedLight,
        Self::GruvboxDark,
        Self::GruvboxLight,
        Self::CyberWave,
        Self::WillowDream,
        Self::Adeberry,
        Self::Phenomenon,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::Dracula => "Dracula",
            Self::SolarizedDark => "Solarized Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::GruvboxLight => "Gruvbox Light",
            Self::CyberWave => "Cyber Wave",
            Self::WillowDream => "Willow Dream",
            Self::Adeberry => "Adeberry",
            Self::Phenomenon => "Phenomenon",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Dracula => "dracula",
            Self::SolarizedDark => "solarized_dark",
            Self::SolarizedLight => "solarized_light",
            Self::GruvboxDark => "gruvbox_dark",
            Self::GruvboxLight => "gruvbox_light",
            Self::CyberWave => "cyber_wave",
            Self::WillowDream => "willow_dream",
            Self::Adeberry => "adeberry",
            Self::Phenomenon => "phenomenon",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "dark" | "Dark" => Some(Self::Dark),
            "light" | "Light" => Some(Self::Light),
            "dracula" => Some(Self::Dracula),
            "solarized_dark" => Some(Self::SolarizedDark),
            "solarized_light" => Some(Self::SolarizedLight),
            "gruvbox_dark" => Some(Self::GruvboxDark),
            "gruvbox_light" => Some(Self::GruvboxLight),
            "cyber_wave" => Some(Self::CyberWave),
            "willow_dream" => Some(Self::WillowDream),
            "adeberry" => Some(Self::Adeberry),
            "phenomenon" => Some(Self::Phenomenon),
            _ => None,
        }
    }

    pub fn to_theme_kind(self) -> nexshell::themes::theme::ThemeKind {
        use nexshell::themes::theme::ThemeKind;
        match self {
            Self::Dark => ThemeKind::Dark,
            Self::Light => ThemeKind::Light,
            Self::Dracula => ThemeKind::Dracula,
            Self::SolarizedDark => ThemeKind::SolarizedDark,
            Self::SolarizedLight => ThemeKind::SolarizedLight,
            Self::GruvboxDark => ThemeKind::GruvboxDark,
            Self::GruvboxLight => ThemeKind::GruvboxLight,
            Self::CyberWave => ThemeKind::CyberWave,
            Self::WillowDream => ThemeKind::WillowDream,
            Self::Adeberry => ThemeKind::Adeberry,
            Self::Phenomenon => ThemeKind::Phenomenon,
        }
    }

    pub fn to_warp_theme(self) -> warp_core::ui::theme::WarpTheme {
        nexshell::themes::theme::WarpThemeConfig::new().theme(&self.to_theme_kind())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyleChoice {
    Block,
    Beam,
    Underline,
}

impl CursorStyleChoice {
    pub const ALL: [Self; 3] = [Self::Block, Self::Beam, Self::Underline];

    pub fn label(self) -> String {
        match self {
            Self::Block => rust_i18n::t!("cursor_block").to_string(),
            Self::Beam => rust_i18n::t!("cursor_beam").to_string(),
            Self::Underline => rust_i18n::t!("cursor_underline").to_string(),
        }
    }

    pub fn to_index(self) -> usize {
        match self {
            Self::Block => 0,
            Self::Beam => 1,
            Self::Underline => 2,
        }
    }

    pub fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(Self::Block),
            1 => Some(Self::Beam),
            2 => Some(Self::Underline),
            _ => None,
        }
    }

    pub fn to_terminal_shape(self) -> TerminalCursorShape {
        match self {
            Self::Block => TerminalCursorShape::Block,
            Self::Beam => TerminalCursorShape::Beam,
            Self::Underline => TerminalCursorShape::Underline,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanguageChoice {
    Auto,
    English,
    Chinese,
}

impl LanguageChoice {
    pub const ALL: [Self; 3] = [Self::Auto, Self::English, Self::Chinese];

    pub fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::English => "en",
            Self::Chinese => "zh-CN",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "en" => Some(Self::English),
            "zh-CN" | "zh_CN" | "zh" => Some(Self::Chinese),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NexSettingsSection {
    Appearance,
    Keybindings,
}

impl TerminalGridElement {
    pub fn new(
        snapshot: Arc<TerminalRuntimeSnapshot>,
        cell_metrics: CellMetrics,
        font_family: FamilyId,
        font_size: f32,
        terminal: Arc<Mutex<LocalTerminalRuntime>>,
        input_editor: Arc<Mutex<TerminalInputEditor>>,
        selection_drag: Arc<Mutex<bool>>,
        last_resize_cells: Arc<Mutex<(u16, u16)>>,
        scrollbar_drag: Arc<Mutex<Option<ScrollbarDrag>>>,
        cursor_over_terminal: Arc<Mutex<bool>>,
        scrollbar_thumb_hovered: Arc<Mutex<bool>>,
        find_state: Arc<Mutex<FindPanelState>>,
        smooth_scroll_px: Arc<Mutex<f64>>,
        shaped_line_cache: Arc<Mutex<TerminalShapedLineCache>>,
        terminal_ime_layout: Arc<Mutex<Option<TerminalImeLayout>>>,
        shell_is_foreground: Arc<std::sync::atomic::AtomicBool>,
        pane_id: Option<NexPaneId>,
        is_focused_pane: bool,
        palette: TerminalPalette,
    ) -> Self {
        let mouse_modes = terminal
            .lock()
            .map(|rt| rt.mouse_modes_handle())
            .unwrap_or_else(|_| Arc::new(std::sync::atomic::AtomicU8::new(0)));
        Self {
            snapshot,
            cell_metrics,
            font_family,
            font_size,
            terminal,
            input_editor,
            selection_drag,
            last_resize_cells,
            scrollbar_drag,
            cursor_over_terminal,
            scrollbar_thumb_hovered,
            find_state,
            smooth_scroll_px,
            shaped_line_cache,
            terminal_ime_layout,
            shell_is_foreground,
            mouse_modes,
            pane_id,
            is_focused_pane,
            palette,
            size: None,
            origin: None,
        }
    }

    fn grid(&self) -> RuntimeGridView<'_> {
        RuntimeGridView {
            grid: &self.snapshot.grid,
            palette: &self.palette,
        }
    }

    /// 把窗口像素坐标映射到 alacritty TermPoint + side。基本与现行
    /// terminal-emulator 通用做法一致：减去 grid 原点 → /cell → floor → 加上
    /// `Line(-display_offset)` 把 viewport row 还原成绝对 line。
    fn position_to_term_point(&self, position: Vector2F) -> Option<(TermPoint, Side)> {
        let origin = self.origin?.xy() + grid_content_offset();
        let grid = self.grid();
        let cell_w = self.cell_metrics.width.max(1.0);
        let cell_h = self.cell_metrics.height.max(1.0);
        let local_x = (position.x() - origin.x()).max(0.0);
        let local_y = (position.y() - origin.y()).max(0.0);

        let max_col = grid.cols().saturating_sub(1) as f32;
        let max_row = grid.rows().saturating_sub(1) as f32;
        let col_frac = local_x / cell_w;
        let row_frac = local_y / cell_h;

        let col = col_frac.floor().clamp(0.0, max_col) as usize;
        let row = row_frac.floor().clamp(0.0, max_row) as usize;

        let line = Line(row as i32 - grid.display_offset() as i32);
        let side = if (col_frac - col_frac.floor()) < 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        Some((TermPoint::new(line, Column(col)), side))
    }

    fn mouse_position_is_in_bounds(&self, position: Vector2F) -> bool {
        self.origin.zip(self.size).is_some_and(|(origin, size)| {
            terminal_mouse_position_is_in_bounds(origin.xy(), size, position)
        })
    }

    // 实时鼠标模式（pty 线程同步的原子镜像），gating 一律用它，别用渲染快照。
    fn live_mouse_bits(&self) -> u8 {
        self.mouse_modes.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn live_mouse_app_active(&self) -> bool {
        mouse_mode_bits_app_active(self.live_mouse_bits())
    }

    fn live_mouse_drag_reporting_active(&self) -> bool {
        mouse_mode_bits_drag_active(self.live_mouse_bits())
    }

    fn live_mouse_motion_reporting_active(&self) -> bool {
        mouse_mode_bits_motion_active(self.live_mouse_bits())
    }

    fn mouse_report_for_position(
        &self,
        position: Vector2F,
        button: MouseReportButton,
        action: MouseReportAction,
        modifiers: ModifiersState,
    ) -> Option<Vec<u8>> {
        if !self.live_mouse_app_active() || !self.mouse_position_is_in_bounds(position) {
            return None;
        }
        let origin = self.origin?.xy() + grid_content_offset();
        let grid = self.grid();
        mouse_report_bytes(
            &grid,
            self.cell_metrics,
            origin,
            position,
            button,
            action,
            modifiers,
        )
    }

    fn hyperlink_at_position(&self, position: Vector2F) -> Option<Arc<str>> {
        let (point, _) = self.position_to_term_point(position)?;
        let grid = self.grid();
        let row = (point.line.0 + grid.display_offset() as i32)
            .clamp(0, grid.rows().saturating_sub(1) as i32) as usize;
        grid.cell(row, point.column.0)
            .and_then(|cell| cell.hyperlink.map(Arc::from))
    }

    fn send_input_bytes(&self, bytes: Vec<u8>) {
        if terminal_input_bytes_should_reset_smooth_scroll(&bytes) {
            if let Ok(mut acc) = self.smooth_scroll_px.lock() {
                *acc = 0.0;
            }
        }
        if let Ok(rt) = self.terminal.lock() {
            rt.send_input(bytes);
        }
    }

    fn should_use_local_input_editor(&self) -> bool {
        if !self
            .shell_is_foreground
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return false;
        }
        if let Ok(rt) = self.terminal.lock() {
            if rt.uses_remote_ssh() {
                return false;
            }
        }
        terminal_input_editor_should_capture(&self.snapshot.grid)
    }

    fn scrollbar_geometry(&self) -> Option<ScrollbarGeometry> {
        terminal_scrollbar_geometry(
            self.origin?.xy(),
            self.size?,
            &self.grid(),
            self.cell_metrics,
        )
    }

    fn jump_to_scrollbar_position(&self, geometry: &ScrollbarGeometry, position_y: f32) {
        let offset = scrollbar_display_offset_for_center(
            geometry,
            self.snapshot.grid.history_size,
            position_y,
        );
        if let Some(off) = offset {
            if let Ok(rt) = self.terminal.lock() {
                rt.set_display_offset_sync(off);
            }
            if let Ok(mut acc) = self.smooth_scroll_px.lock() {
                *acc = 0.0;
            }
        }
    }

    fn drag_scrollbar_to_position(&self, previous_y: f32, position_y: f32) {
        let scroll_data = terminal_scroll_data(&self.grid(), self.cell_metrics);
        let Some(delta) = scrollbar_drag_delta_px(
            scroll_data,
            self.snapshot.grid.history_size,
            previous_y,
            position_y,
        ) else {
            return;
        };

        let line_h = f64::from(self.cell_metrics.height.max(1.0));
        let hs = self.snapshot.grid.history_size;
        let current_sub = self.smooth_scroll_px.lock().map(|v| *v).unwrap_or(0.0);
        let current_virt = self.snapshot.grid.display_offset as f64 * line_h + current_sub;
        let new_virt = (current_virt + delta).clamp(0.0, hs as f64 * line_h);
        let new_offset = (new_virt / line_h) as usize;
        let new_sub = new_virt - new_offset as f64 * line_h;

        if let Ok(rt) = self.terminal.lock() {
            rt.set_display_offset_sync(new_offset);
        }
        if let Ok(mut acc) = self.smooth_scroll_px.lock() {
            *acc = new_sub;
        }
    }

    fn update_scrollbar_hover_state(&self, position: Vector2F) -> bool {
        let Some(origin) = self.origin.map(|origin| origin.xy()) else {
            return false;
        };
        let Some(size) = self.size else {
            return false;
        };
        if self
            .scrollbar_drag
            .lock()
            .map(|drag| drag.is_some())
            .unwrap_or(false)
        {
            return false;
        }

        let child_hovered = RectF::new(origin, size).contains_point(position);
        let thumb_hovered = child_hovered
            && self.scrollbar_geometry().is_some_and(|geometry| {
                terminal_scrollbar_hit(&geometry, position) == Some(ScrollbarHit::Thumb)
            });

        let child_changed = self
            .cursor_over_terminal
            .lock()
            .map(|mut state| {
                if *state == child_hovered {
                    false
                } else {
                    *state = child_hovered;
                    true
                }
            })
            .unwrap_or(false);
        let thumb_changed = self
            .scrollbar_thumb_hovered
            .lock()
            .map(|mut state| {
                if *state == thumb_hovered {
                    false
                } else {
                    *state = thumb_hovered;
                    true
                }
            })
            .unwrap_or(false);

        child_changed || thumb_changed
    }
}

fn selection_type_for_click_count(click_count: u32) -> SelectionType {
    match click_count {
        0 | 1 => SelectionType::Simple,
        2 => SelectionType::Semantic,
        _ => SelectionType::Lines,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalShortcutPlatform {
    Mac,
    Windows,
    Other,
}

fn current_terminal_shortcut_platform() -> TerminalShortcutPlatform {
    if cfg!(target_os = "macos") {
        TerminalShortcutPlatform::Mac
    } else if cfg!(target_os = "windows") {
        TerminalShortcutPlatform::Windows
    } else {
        TerminalShortcutPlatform::Other
    }
}

fn terminal_shortcut_for_key(
    key: &str,
    cmd: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
) -> Option<TerminalGridAction> {
    terminal_shortcut_for_key_with_selection(key, cmd, ctrl, alt, shift, false)
}

fn terminal_shortcut_for_key_with_selection(
    key: &str,
    cmd: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
    has_selection: bool,
) -> Option<TerminalGridAction> {
    terminal_shortcut_for_key_on_platform(
        key,
        cmd,
        ctrl,
        alt,
        shift,
        has_selection,
        current_terminal_shortcut_platform(),
    )
}

fn terminal_shortcut_for_key_on_platform(
    key: &str,
    cmd: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
    has_selection: bool,
    platform: TerminalShortcutPlatform,
) -> Option<TerminalGridAction> {
    if alt {
        return None;
    }

    if key.eq_ignore_ascii_case("k") && ((cmd && !ctrl && !shift) || (!cmd && ctrl && shift)) {
        return Some(TerminalGridAction::ClearVisibleScreen);
    }

    match platform {
        TerminalShortcutPlatform::Windows => {
            if !cmd && ctrl && shift {
                if key.eq_ignore_ascii_case("c") {
                    return Some(TerminalGridAction::CopySelection);
                } else if key.eq_ignore_ascii_case("v") {
                    return Some(TerminalGridAction::PasteClipboard);
                }
            } else if !cmd && ctrl && !shift {
                if key.eq_ignore_ascii_case("v") {
                    return Some(TerminalGridAction::PasteClipboard);
                } else if key.eq_ignore_ascii_case("c") && has_selection {
                    return Some(TerminalGridAction::CopySelection);
                }
            }
        }
        TerminalShortcutPlatform::Other => {
            if !cmd && ctrl && shift {
                if key.eq_ignore_ascii_case("c") {
                    return Some(TerminalGridAction::CopySelection);
                } else if key.eq_ignore_ascii_case("v") {
                    return Some(TerminalGridAction::PasteClipboard);
                }
            }
        }
        TerminalShortcutPlatform::Mac => {}
    }

    if !cmd || ctrl {
        return None;
    }
    if key.eq_ignore_ascii_case("c") && !shift {
        Some(TerminalGridAction::CopySelection)
    } else if key.eq_ignore_ascii_case("v") && !shift {
        Some(TerminalGridAction::PasteClipboard)
    } else if key.eq_ignore_ascii_case("f") && !shift {
        Some(TerminalGridAction::OpenFindBar)
    } else if (key == "=" && !shift) || key == "+" {
        Some(TerminalGridAction::IncreaseFontSize)
    } else if key == "-" && !shift {
        Some(TerminalGridAction::DecreaseFontSize)
    } else if key == "0" && !shift {
        Some(TerminalGridAction::ResetFontSize)
    } else {
        None
    }
}

fn terminal_shortcut_needs_selection_state(
    key: &str,
    cmd: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
) -> bool {
    current_terminal_shortcut_platform() == TerminalShortcutPlatform::Windows
        && key.eq_ignore_ascii_case("c")
        && !cmd
        && ctrl
        && !alt
        && !shift
}

fn terminal_input_editor_should_defer_keydown_to_typed_characters(chars: &str, cmd: bool) -> bool {
    !cmd && !chars.is_empty()
        && chars
            .chars()
            .any(|ch| !ch.is_control() && !is_macos_function_char(ch))
}

fn terminal_page_scroll_lines_for_key(
    key: &str,
    cmd: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
    snapshot: &TerminalGridSnapshot,
) -> Option<i32> {
    if cmd || ctrl || alt || shift || snapshot.input_modes.alt_screen || snapshot.mouse_app_active()
    {
        return None;
    }

    let page_lines = snapshot.rows.saturating_sub(1).max(1) as i32;
    match key {
        "pageup" => Some(page_lines),
        "pagedown" => Some(-page_lines),
        _ => None,
    }
}

fn terminal_debug_key_log(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("NEXSHELL_DEBUG_KEYS").is_some() {
        eprintln!("[nexshell key-debug] {args}");
    }
}

fn terminal_debug_bytes(bytes: &[u8]) -> String {
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    let text = String::from_utf8_lossy(bytes);
    format!("hex=[{hex}] text=\"{}\"", text.escape_debug())
}

/// macOS 方向键等功能键使用 U+F700..U+F8FF 私有区域
fn is_macos_function_char(ch: char) -> bool {
    ('\u{F700}'..='\u{F8FF}').contains(&ch)
}

fn terminal_typed_characters_for_input(chars: &str) -> Option<std::borrow::Cow<'_, str>> {
    if chars.is_empty() {
        return None;
    }
    if !chars.chars().any(is_macos_function_char) {
        return Some(std::borrow::Cow::Borrowed(chars));
    }

    let filtered: String = chars
        .chars()
        .filter(|ch| !is_macos_function_char(*ch))
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(std::borrow::Cow::Owned(filtered))
    }
}

fn terminal_action_needs_notify(action: &TerminalGridAction) -> bool {
    !matches!(action, TerminalGridAction::CopySelection)
}

// 焦点在终端时 find bar 的快捷键（文字输入由 EditorView 处理）
fn find_action_for_key(
    key: &str,
    _chars: &str,
    _cmd: bool,
    _ctrl: bool,
    _alt: bool,
    shift: bool,
    active: bool,
    _current_query: &str,
) -> Option<TerminalGridAction> {
    if !active {
        return None;
    }
    match key {
        "escape" => Some(TerminalGridAction::CloseFindBar),
        "enter" => Some(TerminalGridAction::FindStep(if shift { -1 } else { 1 })),
        "up" => Some(TerminalGridAction::FindStep(-1)),
        "down" => Some(TerminalGridAction::FindStep(1)),
        _ => None,
    }
}

fn terminal_drag_drop_input(paths: &[String]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }

    Some(clipboard_utils::escaped_paths_str(
        paths,
        Some(ShellFamily::Posix),
    ))
}

fn mouse_report_bytes(
    snapshot: &impl TerminalGridAccess,
    cell_metrics: CellMetrics,
    origin: Vector2F,
    position: Vector2F,
    button: MouseReportButton,
    action: MouseReportAction,
    modifiers: ModifiersState,
) -> Option<Vec<u8>> {
    if !grid_mouse_app_active(snapshot) || modifiers.shift {
        return None;
    }

    let (col, row) = pty_cell_position(
        position,
        origin,
        cell_metrics,
        snapshot.cols(),
        snapshot.rows(),
    );
    Some(encode_sgr_mouse_report(
        button,
        action,
        col,
        row,
        modifiers_for_report(modifiers),
    ))
}

fn pty_cell_position(
    position: Vector2F,
    origin: Vector2F,
    cell_metrics: CellMetrics,
    cols: usize,
    rows: usize,
) -> (u16, u16) {
    let local_x = (position.x() - origin.x()).max(0.0);
    let local_y = (position.y() - origin.y()).max(0.0);
    let cell_w = cell_metrics.width.max(1.0);
    let cell_h = cell_metrics.height.max(1.0);
    let max_col = cols.max(1).saturating_sub(1) as f32;
    let max_row = rows.max(1).saturating_sub(1) as f32;
    let col = (local_x / cell_w).floor().clamp(0.0, max_col) as usize;
    let row = (local_y / cell_h).floor().clamp(0.0, max_row) as usize;

    (
        col.saturating_add(1).min(u16::MAX as usize) as u16,
        row.saturating_add(1).min(u16::MAX as usize) as u16,
    )
}

fn terminal_mouse_position_is_in_bounds(
    origin: Vector2F,
    size: Vector2F,
    position: Vector2F,
) -> bool {
    RectF::new(origin, size).contains_point(position)
}

fn modifiers_for_report(modifiers: ModifiersState) -> MouseReportModifiers {
    MouseReportModifiers {
        shift: modifiers.shift,
        alt: modifiers.alt,
        ctrl: modifiers.ctrl,
    }
}

fn grid_mouse_app_active(snapshot: &impl TerminalGridAccess) -> bool {
    snapshot.sgr_mouse()
        && (snapshot.mouse_report_click()
            || snapshot.mouse_report_motion()
            || snapshot.mouse_report_drag())
}

fn repeat_mouse_report_bytes(bytes: &[u8], repeats: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * repeats);
    for _ in 0..repeats {
        out.extend_from_slice(bytes);
    }
    out
}

fn terminal_scrollbar_geometry(
    origin: Vector2F,
    size: Vector2F,
    snapshot: &impl TerminalGridAccess,
    cell_metrics: CellMetrics,
) -> Option<ScrollbarGeometry> {
    let scroll_data = terminal_scroll_data(snapshot, cell_metrics)?;
    Some(compute_scrollbar_geometry(
        Axis::Vertical,
        origin,
        size,
        scroll_data,
        ScrollbarAppearance::new(ScrollbarWidth::Auto, true),
    ))
}

/// `sub_px` 是 `smooth_scroll_px` 的当前值（> 0 表示向上翻了亚行距离）。
/// 把它折进 `scroll_start` 让 thumb 位置与内容像素偏移同步。
fn terminal_scrollbar_geometry_smooth(
    origin: Vector2F,
    size: Vector2F,
    snapshot: &impl TerminalGridAccess,
    cell_metrics: CellMetrics,
    sub_px: f32,
) -> Option<ScrollbarGeometry> {
    let mut data = terminal_scroll_data(snapshot, cell_metrics)?;
    data.scroll_start = Pixels::new((data.scroll_start.as_f32() - sub_px).max(0.0));
    Some(compute_scrollbar_geometry(
        Axis::Vertical,
        origin,
        size,
        data,
        ScrollbarAppearance::new(ScrollbarWidth::Auto, true),
    ))
}

fn terminal_scrollbar_hit(
    geometry: &ScrollbarGeometry,
    position: Vector2F,
) -> Option<ScrollbarHit> {
    if !geometry.track_bounds.contains_point(position) || !geometry.has_thumb() {
        return None;
    }
    if geometry.thumb_bounds.contains_point(position) {
        Some(ScrollbarHit::Thumb)
    } else {
        Some(ScrollbarHit::Track)
    }
}

fn terminal_scroll_data(
    snapshot: &impl TerminalGridAccess,
    cell_metrics: CellMetrics,
) -> Option<ScrollData> {
    if snapshot.history_size() == 0 || snapshot.rows() == 0 {
        return None;
    }

    let cell_h = cell_metrics.height.max(1.0);
    let visible_lines = snapshot.rows().max(1) as f32;
    let history_lines = snapshot.history_size() as f32;
    let scroll_start_lines = snapshot
        .history_size()
        .saturating_sub(snapshot.display_offset().min(snapshot.history_size()))
        as f32;

    Some(ScrollData {
        scroll_start: Pixels::new(scroll_start_lines * cell_h),
        visible_px: Pixels::new(visible_lines * cell_h),
        total_size: Pixels::new((history_lines + visible_lines) * cell_h),
    })
}

fn scrollbar_display_offset_for_center(
    geometry: &ScrollbarGeometry,
    history_size: usize,
    center_y: f32,
) -> Option<usize> {
    if history_size == 0 || !geometry.has_thumb() {
        return None;
    }

    let travel = geometry.track_bounds.height() - geometry.thumb_bounds.height();
    if travel <= 0.0 {
        return Some(0);
    }

    let thumb_top = center_y - geometry.thumb_bounds.height() / 2.0;
    let ratio = ((thumb_top - geometry.track_bounds.min_y()) / travel).clamp(0.0, 1.0);
    let scroll_start_lines = (ratio * history_size as f32).round() as usize;
    Some(history_size.saturating_sub(scroll_start_lines.min(history_size)))
}

/// 滚动条拖动产生的 content-space 像素 delta。
/// 返回值 > 0 → 向上翻历史（display_offset 增加）。
fn scrollbar_drag_delta_px(
    scroll_data: Option<ScrollData>,
    history_size: usize,
    previous_y: f32,
    next_y: f32,
) -> Option<f64> {
    let data = scroll_data?;
    if history_size == 0 || data.total_size <= Pixels::zero() || data.visible_px <= Pixels::zero() {
        return None;
    }
    let pct = data.visible_px / data.total_size;
    if pct <= Pixels::zero() {
        return None;
    }
    Some(f64::from(previous_y - next_y) / f64::from(pct.as_f32()))
}

#[allow(dead_code)]
fn scrollbar_display_offset_for_pointer_movement(
    scroll_data: Option<ScrollData>,
    history_size: usize,
    display_offset: usize,
    line_height: f32,
    previous_y: f32,
    next_y: f32,
) -> Option<usize> {
    let delta = scrollbar_drag_delta_px(scroll_data, history_size, previous_y, next_y)?;
    let line_h = f64::from(line_height.max(1.0));
    let virt = (display_offset as f64 * line_h + delta).clamp(0.0, history_size as f64 * line_h);
    Some((virt / line_h).round() as usize)
}

fn terminal_foreground_with_opacity(palette: &TerminalPalette, alpha: u8) -> ColorU {
    let color = u32_to_color(palette.foreground);
    ColorU::new(color.r, color.g, color.b, alpha)
}

fn terminal_scrollbar_thumb_fill(
    palette: &TerminalPalette,
    child_hovered: bool,
    thumb_hovered: bool,
    dragging: bool,
) -> Fill {
    if thumb_hovered || dragging {
        Fill::Solid(terminal_foreground_with_opacity(palette, 0xe5))
    } else if child_hovered {
        Fill::Solid(terminal_foreground_with_opacity(palette, 0x66))
    } else {
        Fill::None
    }
}

// warp view.rs:750-756 PADDING_LEFT = 16 (LessHorizontalTerminalPadding)。
const GRID_PADDING_LEFT: f32 = 16.0;
const GRID_PADDING_TOP: f32 = 4.0;
// ScrollbarWidth::Auto = 8px + 4px 间距。
const SCROLLBAR_GUTTER_PX: f32 = 12.0;

fn grid_content_offset() -> Vector2F {
    Vector2F::new(GRID_PADDING_LEFT, GRID_PADDING_TOP)
}

fn viewport_cells_for_available_size(
    available: Vector2F,
    cell_metrics: CellMetrics,
    fallback: (usize, usize),
) -> (u16, u16) {
    let fallback = clamp_viewport_cells(fallback);
    if !available.x().is_finite() || !available.y().is_finite() {
        return fallback;
    }

    let usable_w = available.x() - GRID_PADDING_LEFT - SCROLLBAR_GUTTER_PX;
    let usable_h = available.y() - GRID_PADDING_TOP;
    (
        cell_count_for_extent(usable_w, cell_metrics.width),
        cell_count_for_extent(usable_h, cell_metrics.height),
    )
}

#[cfg(test)]
fn split_pane_terminal_body_size(available: Vector2F, header_height: f32) -> Vector2F {
    if !available.x().is_finite() || !available.y().is_finite() || !header_height.is_finite() {
        return available;
    }

    Vector2F::new(
        available.x(),
        (available.y() - header_height.max(0.0)).max(0.0),
    )
}

fn resize_cells_for_available_size(
    last: &mut (u16, u16),
    available: Vector2F,
    cell_metrics: CellMetrics,
    fallback: (usize, usize),
) -> Option<(u16, u16)> {
    let next = viewport_cells_for_available_size(available, cell_metrics, fallback);
    if next == *last {
        return None;
    }

    *last = next;
    Some(next)
}

/// 像素级平滑滚动：trackpad 直接用 pixel delta，discrete wheel 按
/// Warp scrollable.rs:44 的 `NUM_PIXELS_PER_LINE = 40` 放大。
const DISCRETE_SCROLL_LINES_PER_NOTCH: f64 = 3.0;
const TRACKPAD_SCROLL_MULTIPLIER: f64 = 2.0;

/// 把 wheel delta 转为像素并累积到 `acc_px`，返回跨过的整行数。
/// 余数留在 `acc_px`（|余数| < cell_height），供 paint 做亚行偏移。
fn accumulate_scroll_px(delta_y: f32, precise: bool, cell_height: f32, acc_px: &mut f64) -> i32 {
    let cell_h = f64::from(cell_height.max(1.0));
    let delta_px = if precise {
        f64::from(delta_y) * TRACKPAD_SCROLL_MULTIPLIER
    } else {
        f64::from(delta_y) * DISCRETE_SCROLL_LINES_PER_NOTCH * cell_h
    };
    *acc_px += delta_px;
    let whole = (*acc_px / cell_h).trunc() as i32;
    *acc_px -= whole as f64 * cell_h;
    whole
}

fn terminal_input_bytes_should_reset_smooth_scroll(bytes: &[u8]) -> bool {
    bytes == b"\x0c"
}

fn cell_metric_pixels(cell_metrics: CellMetrics) -> (u16, u16) {
    (
        cell_metrics.width.round().clamp(1.0, u16::MAX as f32) as u16,
        cell_metrics.height.round().clamp(1.0, u16::MAX as f32) as u16,
    )
}

fn cell_count_for_extent(extent: f32, cell_extent: f32) -> u16 {
    let cell_extent = cell_extent.max(1.0);
    let cells = (extent.max(0.0) / cell_extent).floor();
    cells.max(1.0).min(u16::MAX as f32) as u16
}

fn clamp_viewport_cells(cells: (usize, usize)) -> (u16, u16) {
    (
        cells.0.max(1).min(u16::MAX as usize) as u16,
        cells.1.max(1).min(u16::MAX as usize) as u16,
    )
}

fn cursor_rects(snapshot: &impl TerminalGridAccess, cell_metrics: CellMetrics) -> Vec<CursorRect> {
    if !snapshot.cursor_visible() || snapshot.cursor_shape() == TerminalCursorShape::Hidden {
        return Vec::new();
    }

    let cell_w = cell_metrics.width.max(1.0);
    let cell_h = cell_metrics.height.max(1.0);
    let col = snapshot.cursor_col().min(snapshot.cols().saturating_sub(1));
    let row = snapshot.cursor_row().min(snapshot.rows().saturating_sub(1));
    let x = col as f32 * cell_w;
    let y = row as f32 * cell_h;
    let thickness = 0.15 * cell_w.round().max(1.0);

    let wide = col + 1 < snapshot.cols()
        && snapshot
            .cell(row, col + 1)
            .map(|c| c.wide_spacer)
            .unwrap_or(false);
    let w = if wide { cell_w * 2.0 } else { cell_w };

    match snapshot.cursor_shape() {
        TerminalCursorShape::Block => vec![CursorRect::new(x, y, w, cell_h)],
        TerminalCursorShape::Underline => {
            vec![CursorRect::new(x, y + cell_h - thickness, w, thickness)]
        }
        TerminalCursorShape::Beam => vec![CursorRect::new(x, y, thickness, cell_h)],
        TerminalCursorShape::HollowBlock => vec![
            CursorRect::new(x, y, w, thickness),
            CursorRect::new(x, y + cell_h - thickness, w, thickness),
            CursorRect::new(x, y, thickness, cell_h),
            CursorRect::new(x + w - thickness, y, thickness, cell_h),
        ],
        TerminalCursorShape::Hidden => Vec::new(),
    }
}

fn terminal_ime_cursor_rect(
    snapshot: &impl TerminalGridAccess,
    origin: Vector2F,
    cell_metrics: CellMetrics,
    font_size: f32,
) -> Option<RectF> {
    let cursor_is_hidden =
        !snapshot.cursor_visible() || snapshot.cursor_shape() == TerminalCursorShape::Hidden;
    // Some TUI apps briefly hide the terminal cursor while resetting it to the
    // top-left cell. Treat that as transient noise, but still honor hidden
    // cursors elsewhere so IME follows TUI input fields.
    if cursor_is_hidden && snapshot.cursor_row() == 0 && snapshot.cursor_col() == 0 {
        return None;
    }

    let cell_w = cell_metrics.width.max(1.0);
    let col = snapshot.cursor_col().min(snapshot.cols().saturating_sub(1));
    let row = snapshot.cursor_row().min(snapshot.rows().saturating_sub(1));
    let line_top = origin
        + Vector2F::new(cell_metrics.width, cell_metrics.height)
            * Vector2F::new(col as f32, row as f32);
    let y_offset = (cell_metrics.baseline_y
        - font_size * DEFAULT_UI_LINE_HEIGHT_RATIO * DEFAULT_TOP_BOTTOM_RATIO)
        .max(0.0);

    Some(RectF::new(
        line_top + Vector2F::new(0.0, y_offset),
        Vector2F::new(cell_w, font_size * DEFAULT_UI_LINE_HEIGHT_RATIO),
    ))
}

fn terminal_ime_cursor_rect_for_layout(
    snapshot: &impl TerminalGridAccess,
    layout: &TerminalImeLayout,
    smooth_scroll_px: f32,
) -> Option<RectF> {
    let content_origin =
        layout.element_origin + grid_content_offset() + Vector2F::new(0.0, smooth_scroll_px);
    terminal_ime_cursor_rect(
        snapshot,
        content_origin,
        layout.cell_metrics,
        layout.font_size,
    )
}

fn terminal_background_rects(
    snapshot: &impl TerminalGridAccess,
    cell_metrics: CellMetrics,
) -> Vec<BackgroundRect> {
    let cell_w = cell_metrics.width.max(1.0);
    let cell_h = cell_metrics.height.max(1.0);
    let mut rects = Vec::with_capacity(snapshot.rows());

    for row in 0..snapshot.rows() {
        let Some(first) = snapshot.cell(row, 0) else {
            continue;
        };
        let mut start_col = 0usize;
        let mut color = first.bg;

        for col in 1..snapshot.cols() {
            let Some(cell) = snapshot.cell(row, col) else {
                continue;
            };
            if cell.bg == color {
                continue;
            }
            rects.push(BackgroundRect::new(
                start_col as f32 * cell_w,
                row as f32 * cell_h,
                (col - start_col) as f32 * cell_w,
                cell_h,
                color,
            ));
            start_col = col;
            color = cell.bg;
        }

        rects.push(BackgroundRect::new(
            start_col as f32 * cell_w,
            row as f32 * cell_h,
            (snapshot.cols() - start_col) as f32 * cell_w,
            cell_h,
            color,
        ));
    }

    rects
}

fn terminal_cell_decoration_rects(
    snapshot: &impl TerminalGridAccess,
    cell_metrics: CellMetrics,
) -> Vec<DecorationRect> {
    let cell_w = cell_metrics.width.max(1.0);
    let cell_h = cell_metrics.height.max(1.0);
    let thickness = 0.15 * cell_w.round().max(1.0);
    let mut rects = Vec::new();

    for row in 0..snapshot.rows() {
        for col in 0..snapshot.cols() {
            let Some(cell) = snapshot.cell(row, col) else {
                continue;
            };

            let (height, y) = if cell.double_underline {
                (thickness * 2.0, cell_h - thickness * 2.0)
            } else if cell.underline || cell.hyperlink.is_some() {
                (thickness, cell_h - thickness)
            } else if cell.strikeout {
                (thickness, cell_h / 2.0 - thickness)
            } else {
                continue;
            };

            rects.push(DecorationRect::new(
                col as f32 * cell_w,
                row as f32 * cell_h + y,
                cell_w,
                height,
                cell.underline_color.unwrap_or(cell.fg),
            ));
        }
    }

    rects
}

#[allow(dead_code)]
fn terminal_font_properties_for_cell(cell: &GridCell) -> Properties {
    terminal_font_properties(cell.bold, cell.italic)
}

fn terminal_font_properties(bold: bool, italic: bool) -> Properties {
    match (bold, italic) {
        (true, true) => Properties::default()
            .weight(Weight::Bold)
            .style(Style::Italic),
        (true, false) => Properties::default().weight(Weight::Bold),
        (false, true) => Properties::default().style(Style::Italic),
        (false, false) => Properties::default(),
    }
}

#[derive(Debug, Clone)]
struct TerminalShapedLineData {
    text: String,
    style_runs: Vec<(Range<usize>, StyleAndFont)>,
    character_index_to_cell_map: Vec<usize>,
}

#[derive(Default)]
pub struct TerminalShapedLineCache {
    cols: usize,
    rows: usize,
    display_offset: usize,
    font_family: Option<FamilyId>,
    input_editor_revision: u64,
    lines: Vec<Option<Arc<TerminalShapedLineData>>>,
}

impl TerminalShapedLineCache {
    fn line_data(
        &mut self,
        snapshot: &impl TerminalGridAccess,
        row: usize,
        font_family: FamilyId,
        input_editor_revision: u64,
    ) -> Option<Arc<TerminalShapedLineData>> {
        if self.cols != snapshot.cols()
            || self.rows != snapshot.rows()
            || self.font_family != Some(font_family)
        {
            self.cols = snapshot.cols();
            self.rows = snapshot.rows();
            self.display_offset = snapshot.display_offset();
            self.font_family = Some(font_family);
            self.input_editor_revision = input_editor_revision;
            self.lines = vec![None; self.rows];
        }
        if self.display_offset != snapshot.display_offset() {
            self.shift_for_display_offset(snapshot.display_offset());
        }
        if self.input_editor_revision != input_editor_revision {
            self.input_editor_revision = input_editor_revision;
            self.lines.fill(None);
        }

        if row >= self.rows {
            return None;
        }
        if !snapshot.dirty_row(row) {
            if let Some(line) = self.lines.get(row).and_then(|line| line.as_ref()) {
                if line.text == snapshot.line_text(row) {
                    return Some(Arc::clone(line));
                }
            }
        }

        let line = Arc::new(terminal_shaped_line_data(snapshot, row, font_family)?);
        self.lines[row] = Some(Arc::clone(&line));
        Some(line)
    }

    fn shift_for_display_offset(&mut self, next_display_offset: usize) {
        let delta = next_display_offset as i32 - self.display_offset as i32;
        self.display_offset = next_display_offset;
        if delta == 0 || self.rows == 0 {
            return;
        }

        let shift = delta.unsigned_abs() as usize;
        if shift >= self.rows {
            self.lines.fill(None);
            return;
        }

        let old = self.lines.clone();
        for row in 0..self.rows {
            let source = if delta > 0 {
                row.checked_sub(shift)
            } else {
                row.checked_add(shift).filter(|source| *source < self.rows)
            };
            self.lines[row] = source.and_then(|source| old[source].clone());
        }
    }
}

struct TerminalShapedLineBuilder {
    font_family: FamilyId,
    current_style: Option<StyleAndFont>,
    current_style_start: usize,
    style_runs: Vec<(Range<usize>, StyleAndFont)>,
    character_index_to_cell_map: Vec<usize>,
    text: String,
}

impl TerminalShapedLineBuilder {
    fn new(font_family: FamilyId, cols: usize) -> Self {
        Self {
            font_family,
            current_style: None,
            current_style_start: 0,
            style_runs: Vec::new(),
            character_index_to_cell_map: Vec::with_capacity(cols),
            text: String::with_capacity(cols),
        }
    }

    fn flush_style_run(&mut self) {
        let next = self.character_index_to_cell_map.len();
        if let Some(style) = self.current_style {
            if next > self.current_style_start {
                self.style_runs
                    .push((self.current_style_start..next, style));
            }
        }
        self.current_style_start = next;
    }

    fn update_style(&mut self, cell: RenderCell<'_>) {
        let style = StyleAndFont::new(
            self.font_family,
            terminal_font_properties(cell.bold, cell.italic),
            TextStyle {
                foreground_color: Some(cell.fg),
                ..TextStyle::new()
            },
        );
        if self.current_style != Some(style) {
            self.flush_style_run();
            self.current_style = Some(style);
        }
    }

    fn append_char(&mut self, ch: char, col: usize) {
        self.text.push(ch);
        self.character_index_to_cell_map.push(col);
    }

    fn append_content(&mut self, content: &str, col: usize) {
        if content == "\t" || content == "\0" {
            self.append_char(' ', col);
            return;
        }
        for ch in content.chars() {
            self.append_char(ch, col);
        }
    }

    fn build(mut self) -> Option<TerminalShapedLineData> {
        if self.text.is_empty() {
            return None;
        }
        self.flush_style_run();
        Some(TerminalShapedLineData {
            text: self.text,
            style_runs: self.style_runs,
            character_index_to_cell_map: self.character_index_to_cell_map,
        })
    }
}

fn terminal_shaped_line_data(
    snapshot: &impl TerminalGridAccess,
    row: usize,
    font_family: FamilyId,
) -> Option<TerminalShapedLineData> {
    let last_col = (0..snapshot.cols()).rev().find(|col| {
        snapshot
            .cell(row, *col)
            .is_some_and(|cell| !cell.wide_spacer && cell.content != " " && cell.content != "\0")
    })?;

    let mut builder = TerminalShapedLineBuilder::new(font_family, snapshot.cols());
    for col in 0..=last_col {
        let Some(cell) = snapshot.cell(row, col) else {
            continue;
        };
        if cell.wide_spacer {
            continue;
        }
        builder.update_style(cell);
        builder.append_content(cell.content, col);
    }
    builder.build()
}

fn paint_terminal_shaped_line(
    line: &TextLine,
    baseline: Vector2F,
    cell_width: f32,
    character_index_to_cell_map: &[usize],
    palette: &TerminalPalette,
    ctx: &mut PaintContext,
) {
    for run in &line.runs {
        let color = run
            .styles
            .foreground_color
            .unwrap_or_else(|| u32_to_color(palette.foreground));
        for glyph in &run.glyphs {
            let Some(col) = character_index_to_cell_map.get(glyph.index) else {
                continue;
            };
            let glyph_origin = baseline + Vector2F::new(*col as f32 * cell_width, 0.0);
            ctx.scene
                .draw_glyph(glyph_origin, glyph.id, run.font_id, line.font_size, color);
        }
    }
}

impl Element for TerminalGridElement {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _: &mut LayoutContext,
        _: &AppContext,
    ) -> Vector2F {
        let grid = self.grid();
        let fallback = (grid.cols(), grid.rows());
        let _cells = viewport_cells_for_available_size(constraint.max, self.cell_metrics, fallback);
        let resize_target = self.last_resize_cells.lock().ok().and_then(|mut last| {
            resize_cells_for_available_size(&mut last, constraint.max, self.cell_metrics, fallback)
        });

        if let Some((cols, rows)) = resize_target {
            if let Ok(rt) = self.terminal.lock() {
                let (cell_width, cell_height) = cell_metric_pixels(self.cell_metrics);
                rt.resize_with_cell_size(cols, rows, cell_width, cell_height);
            }
        }

        // 返回完整可用空间（占满父容器），grid 内容通过 paint 内的
        // offset 实现边距，scrollbar 贴右边缘。
        let size = constraint.max;
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, _: &mut AfterLayoutContext, _: &AppContext) {}

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));

        let grid = self.grid();
        let cell_w = self.cell_metrics.width;
        let baseline_y = self.cell_metrics.baseline_y;
        let font_size = self.font_size;
        let input_editor_revision = self
            .input_editor
            .lock()
            .map(|editor| editor.revision())
            .unwrap_or(0);

        let size = self.size.unwrap_or_default();

        let bg_base = u32_to_color(self.palette.background);
        let bg_fill = ColorU::new(
            bg_base.r,
            bg_base.g,
            bg_base.b,
            self.palette.background_alpha,
        );
        ctx.scene
            .draw_rect_without_hit_recording(RectF::new(origin, size))
            .with_background(bg_fill);

        // 亚行像素偏移：sub_offset > 0 时内容向下平移（向上翻历史）。
        let sub_offset = self
            .smooth_scroll_px
            .lock()
            .map(|v| *v as f32)
            .unwrap_or(0.0);

        // Grid 内容偏移：左边距 + 上边距 + 平滑滚动亚行偏移。
        let content_origin = origin + grid_content_offset() + Vector2F::new(0.0, sub_offset);

        if self.is_focused_pane {
            if let Ok(mut layout) = self.terminal_ime_layout.lock() {
                *layout = Some(TerminalImeLayout {
                    element_origin: origin,
                    cell_metrics: self.cell_metrics,
                    font_size,
                });
            }

            if let Some(rect) =
                terminal_ime_cursor_rect(&grid, content_origin, self.cell_metrics, font_size)
            {
                ctx.position_cache
                    .cache_position_indefinitely(TERMINAL_CURSOR_POSITION_ID.to_string(), rect);
            }
        }

        // Clip grid 内容区域，亚行偏移导致的溢出不可见。
        let content_clip = RectF::new(
            origin + Vector2F::new(GRID_PADDING_LEFT, GRID_PADDING_TOP),
            Vector2F::new(
                size.x() - GRID_PADDING_LEFT - SCROLLBAR_GUTTER_PX,
                size.y() - GRID_PADDING_TOP,
            ),
        );
        ctx.scene
            .start_layer(ClipBounds::BoundedByActiveLayerAnd(content_clip));

        let default_bg = u32_to_color(self.palette.background);
        for rect in terminal_background_rects(&grid, self.cell_metrics) {
            // warp: workspace/util.rs:364-386 — 有背景图时默认 bg 单元格也要半透明
            let color = if self.palette.background_alpha < 255
                && rect.color.r == default_bg.r
                && rect.color.g == default_bg.g
                && rect.color.b == default_bg.b
            {
                ColorU::new(
                    rect.color.r,
                    rect.color.g,
                    rect.color.b,
                    self.palette.background_alpha,
                )
            } else {
                rect.color
            };
            ctx.scene
                .draw_rect_with_hit_recording(rect.to_rect(content_origin))
                .with_background(color);
        }

        for row in 0..grid.rows() {
            let line_data = self
                .shaped_line_cache
                .lock()
                .ok()
                .and_then(|mut cache| {
                    cache.line_data(&grid, row, self.font_family, input_editor_revision)
                })
                .or_else(|| terminal_shaped_line_data(&grid, row, self.font_family).map(Arc::new));
            let Some(line_data) = line_data else {
                continue;
            };
            let line_style = LineStyle {
                font_size,
                line_height_ratio: (self.cell_metrics.height / font_size).max(1.0),
                baseline_ratio: (baseline_y / self.cell_metrics.height.max(1.0)).clamp(0.0, 1.0),
                fixed_width_tab_size: None,
            };
            let laid_out = ctx.text_layout_cache.layout_line(
                &line_data.text,
                line_style,
                &line_data.style_runs,
                cell_w * grid.cols() as f32,
                ClipConfig::default(),
                &ctx.font_cache.text_layout_system(),
            );
            let baseline = content_origin
                + Vector2F::new(0.0, row as f32 * self.cell_metrics.height + baseline_y);
            paint_terminal_shaped_line(
                laid_out.as_ref(),
                baseline,
                cell_w,
                &line_data.character_index_to_cell_map,
                &self.palette,
                ctx,
            );
        }

        for rect in terminal_cell_decoration_rects(&grid, self.cell_metrics) {
            ctx.scene
                .draw_rect_without_hit_recording(rect.to_rect(content_origin))
                .with_background(rect.color);
        }

        let cursor_color = u32_to_color(self.palette.cursor);
        for rect in cursor_rects(&grid, self.cell_metrics) {
            ctx.scene
                .draw_rect_with_hit_recording(rect.to_rect(content_origin))
                .with_background(cursor_color);
        }

        ctx.scene.stop_layer(); // 结束 grid content clip

        let child_hovered = self
            .cursor_over_terminal
            .lock()
            .map(|state| *state)
            .unwrap_or(false);
        let thumb_hovered = self
            .scrollbar_thumb_hovered
            .lock()
            .map(|state| *state)
            .unwrap_or(false);
        let dragging = self
            .scrollbar_drag
            .lock()
            .map(|drag| drag.is_some())
            .unwrap_or(false);
        // 用亚像素偏移修正 scroll_start，让 thumb 位置与内容平移同步。
        if let Some(geometry) =
            terminal_scrollbar_geometry_smooth(origin, size, &grid, self.cell_metrics, sub_offset)
        {
            if geometry.has_thumb() {
                ctx.scene
                    .start_layer(ClipBounds::BoundedByActiveLayerAnd(geometry.track_bounds));
                ctx.scene
                    .draw_rect_with_hit_recording(geometry.track_bounds)
                    .with_background(Fill::Solid(ColorU::transparent_black()));
                ctx.scene
                    .draw_rect_with_hit_recording(geometry.thumb_bounds)
                    .with_background(terminal_scrollbar_thumb_fill(
                        &self.palette,
                        child_hovered,
                        thumb_hovered,
                        dragging,
                    ))
                    .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.0)));
                ctx.scene.stop_layer();
            }
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        match event.raw_event() {
            Event::KeyDown {
                keystroke,
                chars,
                details,
                is_composing,
            } if !self.is_focused_pane => {
                terminal_debug_key_log(format_args!(
                    "ignored unfocused pane={:?} key={:?} chars=\"{}\" key_without_modifiers={:?} mods(cmd={}, ctrl={}, alt={}, shift={}) composing={}",
                    self.pane_id,
                    keystroke.key,
                    chars.escape_debug(),
                    details.key_without_modifiers,
                    keystroke.cmd,
                    keystroke.ctrl,
                    keystroke.alt,
                    keystroke.shift,
                    is_composing
                ));
                return false;
            }
            Event::TypedCharacters { .. }
            | Event::SetMarkedText { .. }
            | Event::ClearMarkedText
            | Event::DragAndDropFiles { .. }
                if !self.is_focused_pane =>
            {
                return false;
            }
            Event::KeyDown {
                keystroke,
                chars,
                details,
                is_composing,
            } => {
                terminal_debug_key_log(format_args!(
                    "keydown pane={:?} key={:?} chars=\"{}\" key_without_modifiers={:?} mods(cmd={}, ctrl={}, alt={}, shift={}) composing={} modes={:?}",
                    self.pane_id,
                    keystroke.key,
                    chars.escape_debug(),
                    details.key_without_modifiers,
                    keystroke.cmd,
                    keystroke.ctrl,
                    keystroke.alt,
                    keystroke.shift,
                    is_composing,
                    self.snapshot.grid.input_modes
                ));
                if *is_composing {
                    terminal_debug_key_log(format_args!("ignored composing keydown"));
                    return false;
                }

                let find_state = self.find_state.lock().map(|state| state.clone()).ok();
                if let Some(action) = find_action_for_key(
                    keystroke.key.as_str(),
                    chars.as_str(),
                    keystroke.cmd,
                    keystroke.ctrl,
                    keystroke.alt,
                    keystroke.shift,
                    find_state.as_ref().is_some_and(|state| state.active),
                    find_state
                        .as_ref()
                        .map(|state| state.query.as_str())
                        .unwrap_or(""),
                ) {
                    terminal_debug_key_log(format_args!("find action={action:?}"));
                    ctx.dispatch_typed_action(action);
                    ctx.notify();
                    return true;
                }

                let action = if terminal_shortcut_needs_selection_state(
                    keystroke.key.as_str(),
                    keystroke.cmd,
                    keystroke.ctrl,
                    keystroke.alt,
                    keystroke.shift,
                ) {
                    let has_selection = self
                        .terminal
                        .lock()
                        .ok()
                        .and_then(|rt| rt.selected_text())
                        .is_some_and(|text| !text.is_empty());
                    terminal_shortcut_for_key_with_selection(
                        keystroke.key.as_str(),
                        keystroke.cmd,
                        keystroke.ctrl,
                        keystroke.alt,
                        keystroke.shift,
                        has_selection,
                    )
                } else {
                    terminal_shortcut_for_key(
                        keystroke.key.as_str(),
                        keystroke.cmd,
                        keystroke.ctrl,
                        keystroke.alt,
                        keystroke.shift,
                    )
                };

                if let Some(action) = action {
                    terminal_debug_key_log(format_args!("terminal shortcut action={action:?}"));
                    if action == TerminalGridAction::ClearVisibleScreen {
                        let preserve_prompt_prefix = self.should_use_local_input_editor();
                        if let Ok(mut editor) = self.input_editor.lock() {
                            editor.clear();
                        }
                        if let Ok(rt) = self.terminal.lock() {
                            rt.clear_visible_screen(preserve_prompt_prefix);
                        }
                        ctx.notify();
                        return true;
                    }

                    let should_notify = terminal_action_needs_notify(&action);
                    ctx.dispatch_typed_action(action);
                    if should_notify {
                        ctx.notify();
                    }
                    return true;
                }

                if let Ok(mut editor) = self.input_editor.lock() {
                    editor.clear();
                }

                if let Some(lines) = terminal_page_scroll_lines_for_key(
                    keystroke.key.as_str(),
                    keystroke.cmd,
                    keystroke.ctrl,
                    keystroke.alt,
                    keystroke.shift,
                    &self.snapshot.grid,
                ) {
                    terminal_debug_key_log(format_args!("page scroll lines={lines}"));
                    if let Ok(rt) = self.terminal.lock() {
                        rt.scroll(lines);
                    }
                    if let Ok(mut acc) = self.smooth_scroll_px.lock() {
                        *acc = 0.0;
                    }
                    ctx.notify();
                    return true;
                }

                if terminal_input_editor_should_defer_keydown_to_typed_characters(
                    chars.as_str(),
                    keystroke.cmd,
                ) {
                    terminal_debug_key_log(format_args!(
                        "defer keydown to typed characters chars=\"{}\"",
                        chars.escape_debug()
                    ));
                    return false;
                }

                let key_char = if chars.is_empty() {
                    None
                } else {
                    Some(chars.as_str())
                };
                let bytes = encode_terminal_key_event_with_modes(
                    keystroke.key.as_str(),
                    details.key_without_modifiers.as_deref(),
                    key_char,
                    keystroke.ctrl,
                    keystroke.alt,
                    keystroke.shift,
                    keystroke.cmd,
                    self.snapshot.grid.input_modes,
                );
                if let Some(bytes) = bytes {
                    terminal_debug_key_log(format_args!(
                        "send encoded key {}",
                        terminal_debug_bytes(&bytes)
                    ));
                    self.send_input_bytes(bytes);
                    // 立刻标 dirty，下一帧 render 就能看到自己的输入。
                    ctx.notify();
                    return true;
                }
                terminal_debug_key_log(format_args!("no key bytes generated"));
                // 仿 Warp alt_screen_element.rs key_down：encode 返回 None 时，
                // 若 NSEvent 自带的 chars 全是控制字符（例如 macOS Ctrl+B → "\x02"），
                // 原样发到 PTY。
                if !chars.is_empty() && chars.chars().all(|c| c.is_control()) {
                    terminal_debug_key_log(format_args!(
                        "send raw control chars {}",
                        terminal_debug_bytes(chars.as_bytes())
                    ));
                    self.send_input_bytes(chars.as_bytes().to_vec());
                    ctx.notify();
                    return true;
                }
                false
            }
            Event::ModifierKeyChanged { key_code, state } => {
                let is_press = matches!(state, KeyState::Pressed);
                if let Some(bytes) = encode_terminal_modifier_key_with_modes(
                    key_code,
                    is_press,
                    self.snapshot.grid.input_modes,
                ) {
                    if let Ok(rt) = self.terminal.lock() {
                        rt.send_input(bytes);
                    }
                    ctx.notify();
                    return true;
                }
                false
            }
            Event::TypedCharacters { chars } => {
                let Some(input_chars) = terminal_typed_characters_for_input(chars.as_str()) else {
                    return false;
                };
                let input_chars = input_chars.as_ref();
                let find_state = self.find_state.lock().map(|state| state.clone()).ok();
                if let Some(action) = find_action_for_key(
                    "",
                    input_chars,
                    false,
                    false,
                    false,
                    false,
                    find_state.as_ref().is_some_and(|state| state.active),
                    find_state
                        .as_ref()
                        .map(|state| state.query.as_str())
                        .unwrap_or(""),
                ) {
                    ctx.dispatch_typed_action(action);
                    ctx.notify();
                    return true;
                }

                if let Ok(mut editor) = self.input_editor.lock() {
                    editor.clear_marked_text();
                }
                if let Ok(rt) = self.terminal.lock() {
                    rt.clear_marked_text();
                    rt.send_input(input_chars.as_bytes().to_vec());
                }
                ctx.notify();
                true
            }
            Event::DragAndDropFiles { paths, .. } => {
                // 只吞落在 terminal grid 自身 bounds 里的 drop；
                // 落在文件面板上的由 FileDropTarget 处理。
                let in_self = match (self.origin, self.size) {
                    (Some(origin), Some(size)) => {
                        event.raw_event().in_bounds(RectF::new(origin.xy(), size))
                    }
                    _ => false,
                };
                if !in_self {
                    return false;
                }
                let Some(input) = terminal_drag_drop_input(paths) else {
                    return false;
                };

                if let Ok(mut editor) = self.input_editor.lock() {
                    editor.clear_marked_text();
                }
                if let Ok(rt) = self.terminal.lock() {
                    rt.clear_marked_text();
                    rt.send_input(input.into_bytes());
                }
                ctx.notify();
                true
            }

            // ── 鼠标选区 ────────────────────────────────────────────────
            Event::LeftMouseDown {
                position,
                modifiers,
                click_count,
                ..
            } => {
                if !self.mouse_position_is_in_bounds(*position) {
                    return false;
                }

                if let Some(pid) = self.pane_id {
                    if !self.is_focused_pane {
                        ctx.dispatch_typed_action(TerminalGridAction::FocusPane(pid));
                        ctx.notify();
                        return true;
                    }
                }

                ctx.dispatch_typed_action(TerminalGridAction::TerminalMouseDown);

                if let Some(geometry) = self.scrollbar_geometry() {
                    match terminal_scrollbar_hit(&geometry, *position) {
                        Some(ScrollbarHit::Thumb) => {
                            if let Ok(mut drag) = self.scrollbar_drag.lock() {
                                *drag = Some(ScrollbarDrag {
                                    previous_y: position.y(),
                                });
                            }
                            ctx.notify();
                            return true;
                        }
                        Some(ScrollbarHit::Track) => {
                            self.jump_to_scrollbar_position(&geometry, position.y());
                            ctx.notify();
                            return true;
                        }
                        None => {}
                    }
                }

                if modifiers.cmd {
                    if let Some(uri) = self.hyperlink_at_position(*position) {
                        app.open_url(uri.as_ref());
                        return true;
                    }
                }

                if let Some(bytes) = self.mouse_report_for_position(
                    *position,
                    MouseReportButton::Left,
                    MouseReportAction::Press,
                    *modifiers,
                ) {
                    self.send_input_bytes(bytes);
                    if let Ok(mut flag) = self.selection_drag.lock() {
                        *flag = false;
                    }
                    ctx.notify();
                    return true;
                }

                let Some((point, side)) = self.position_to_term_point(*position) else {
                    return false;
                };
                let ty = selection_type_for_click_count(*click_count);
                if let Ok(rt) = self.terminal.lock() {
                    rt.start_selection(ty, point, side);
                }
                if let Ok(mut flag) = self.selection_drag.lock() {
                    *flag = true;
                }
                ctx.notify();
                true
            }
            Event::LeftMouseDragged {
                position,
                modifiers,
            } => {
                let scrollbar_drag = self.scrollbar_drag.lock().ok().and_then(|drag| *drag);
                if let Some(drag) = scrollbar_drag {
                    if self.scrollbar_geometry().is_some() {
                        self.drag_scrollbar_to_position(drag.previous_y, position.y());
                    }
                    if let Ok(mut drag_state) = self.scrollbar_drag.lock() {
                        *drag_state = Some(ScrollbarDrag {
                            previous_y: position.y(),
                        });
                    }
                    ctx.notify();
                    return true;
                }

                if self.live_mouse_app_active() && !modifiers.shift {
                    if self.live_mouse_drag_reporting_active() {
                        if let Some(bytes) = self.mouse_report_for_position(
                            *position,
                            MouseReportButton::Left,
                            MouseReportAction::Drag,
                            *modifiers,
                        ) {
                            self.send_input_bytes(bytes);
                            ctx.notify();
                        }
                    }
                    return true;
                }

                let dragging = self.selection_drag.lock().map(|f| *f).unwrap_or(false);
                if !dragging {
                    return false;
                }
                let Some((point, side)) = self.position_to_term_point(*position) else {
                    return false;
                };
                if let Ok(rt) = self.terminal.lock() {
                    rt.update_selection(point, side);
                }
                ctx.notify();
                true
            }
            Event::LeftMouseUp {
                position,
                modifiers,
            } => {
                if self
                    .scrollbar_drag
                    .lock()
                    .map(|drag| drag.is_some())
                    .unwrap_or(false)
                {
                    if let Ok(mut drag) = self.scrollbar_drag.lock() {
                        *drag = None;
                    }
                    self.update_scrollbar_hover_state(*position);
                    ctx.notify();
                    return true;
                }

                if let Ok(mut flag) = self.selection_drag.lock() {
                    *flag = false;
                }
                if let Some(bytes) = self.mouse_report_for_position(
                    *position,
                    MouseReportButton::Left,
                    MouseReportAction::Release,
                    *modifiers,
                ) {
                    self.send_input_bytes(bytes);
                    ctx.notify();
                    return true;
                }
                false
            }

            // ── 滚轮（像素级平滑滚动）─────────────────────────────
            Event::ScrollWheel {
                position,
                delta,
                precise,
                modifiers,
                ..
            } => {
                if !self.mouse_position_is_in_bounds(*position) {
                    return false;
                }

                let mut fallback_acc = 0.0;
                let whole_lines = self
                    .smooth_scroll_px
                    .lock()
                    .map(|mut acc| {
                        accumulate_scroll_px(
                            delta.y(),
                            *precise,
                            self.cell_metrics.height,
                            &mut acc,
                        )
                    })
                    .unwrap_or_else(|_| {
                        accumulate_scroll_px(
                            delta.y(),
                            *precise,
                            self.cell_metrics.height,
                            &mut fallback_acc,
                        )
                    });

                let grid = self.grid();

                // mouse-app 模式：逐行发 wheel report，清除亚行余量。
                if self.live_mouse_app_active() && !modifiers.shift {
                    if whole_lines == 0 {
                        if let Ok(mut acc) = self.smooth_scroll_px.lock() {
                            *acc = 0.0;
                        }
                        return false;
                    }
                    if let Ok(mut acc) = self.smooth_scroll_px.lock() {
                        *acc = 0.0;
                    }
                    let button = if whole_lines > 0 {
                        MouseReportButton::WheelUp
                    } else {
                        MouseReportButton::WheelDown
                    };
                    if let Some(bytes) = self.mouse_report_for_position(
                        *position,
                        button,
                        MouseReportAction::Press,
                        *modifiers,
                    ) {
                        self.send_input_bytes(repeat_mouse_report_bytes(
                            &bytes,
                            whole_lines.unsigned_abs() as usize,
                        ));
                    }
                    ctx.notify();
                    return true;
                }

                // alt-screen 模式：发 escape 序列，清除亚行余量。
                if !modifiers.shift && whole_lines != 0 {
                    if let Some(bytes) =
                        terminal_alt_scroll_bytes(whole_lines, self.snapshot.grid.input_modes)
                    {
                        if let Ok(mut acc) = self.smooth_scroll_px.lock() {
                            *acc = 0.0;
                        }
                        self.send_input_bytes(bytes);
                        ctx.notify();
                        return true;
                    }
                }

                // 普通滚动：整行交给 alacritty display_offset，亚行余量留给
                // paint() 做像素级平移。
                if whole_lines != 0 {
                    if let Ok(rt) = self.terminal.lock() {
                        rt.scroll(whole_lines);
                    }
                }

                // 边界钳位：到顶/到底后清除亚行余量，防止内容抖动。
                if let Ok(mut acc) = self.smooth_scroll_px.lock() {
                    let off = grid.display_offset();
                    let hs = grid.history_size();
                    let scrolled_up = whole_lines > 0;
                    let scrolled_down = whole_lines < 0;
                    // 预估 scroll 后 display_offset 是否撞顶/触底
                    let at_top = if scrolled_up {
                        off.saturating_add(whole_lines as usize) >= hs
                    } else {
                        off >= hs
                    };
                    let at_bottom = if scrolled_down {
                        (off as i64 + whole_lines as i64) <= 0
                    } else {
                        off == 0
                    };
                    if (at_top && *acc > 0.0) || (at_bottom && *acc < 0.0) || hs == 0 {
                        *acc = 0.0;
                    }
                }

                ctx.notify();
                true
            }

            Event::MouseMoved {
                position,
                shift,
                is_synthetic,
                ..
            } => {
                if *is_synthetic || !self.live_mouse_motion_reporting_active() || *shift {
                    if self.update_scrollbar_hover_state(*position) {
                        ctx.notify();
                    }
                    return false;
                }
                if self.update_scrollbar_hover_state(*position) {
                    ctx.notify();
                }
                let Some(bytes) = self.mouse_report_for_position(
                    *position,
                    MouseReportButton::Move,
                    MouseReportAction::Press,
                    ModifiersState {
                        shift: *shift,
                        ..Default::default()
                    },
                ) else {
                    return false;
                };
                self.send_input_bytes(bytes);
                ctx.notify();
                true
            }

            Event::MiddleMouseDown {
                position, shift, ..
            } => {
                let Some(bytes) = self.mouse_report_for_position(
                    *position,
                    MouseReportButton::Middle,
                    MouseReportAction::Press,
                    ModifiersState {
                        shift: *shift,
                        ..Default::default()
                    },
                ) else {
                    return false;
                };
                self.send_input_bytes(bytes);
                ctx.notify();
                true
            }

            Event::RightMouseDown {
                position, shift, ..
            } => {
                if !self.mouse_position_is_in_bounds(*position) {
                    return false;
                }
                if let Some(bytes) = self.mouse_report_for_position(
                    *position,
                    MouseReportButton::Right,
                    MouseReportAction::Press,
                    ModifiersState {
                        shift: *shift,
                        ..Default::default()
                    },
                ) {
                    self.send_input_bytes(bytes);
                    ctx.notify();
                    return true;
                }
                // warp: alt_screen_element.rs:292-313 — 非鼠标报告模式弹出右键菜单
                let has_selection = self
                    .terminal
                    .lock()
                    .ok()
                    .and_then(|rt| rt.selected_text())
                    .is_some_and(|t| !t.is_empty());
                ctx.dispatch_typed_action(TerminalGridAction::ShowTerminalContextMenu {
                    position: *position,
                    has_selection,
                });
                ctx.notify();
                true
            }

            // ── IME marked text（合成中预览）──────────────────────────
            // 正式输入由 shell/readline/zsh 处理；marked text 只作为 runtime
            // overlay 渲染，不再让本地 input editor 拥有命令行内容。
            Event::SetMarkedText {
                marked_text,
                selected_range,
            } => {
                if let Ok(mut editor) = self.input_editor.lock() {
                    editor.clear_marked_text();
                }
                if let Ok(rt) = self.terminal.lock() {
                    rt.set_marked_text(marked_text.clone(), selected_range.clone());
                }
                ctx.notify();
                true
            }
            Event::ClearMarkedText => {
                if let Ok(mut editor) = self.input_editor.lock() {
                    editor.clear_marked_text();
                }
                if let Ok(rt) = self.terminal.lock() {
                    rt.clear_marked_text();
                }
                ctx.notify();
                true
            }

            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        accumulate_scroll_px, cursor_rects, encode_terminal_key_event_with_modes,
        find_action_for_key, mouse_report_bytes, repeat_mouse_report_bytes,
        resize_cells_for_available_size, scrollbar_display_offset_for_center,
        scrollbar_display_offset_for_pointer_movement, split_pane_terminal_body_size,
        terminal_action_needs_notify, terminal_background_rects, terminal_cell_decoration_rects,
        terminal_drag_drop_input, terminal_font_properties_for_cell, terminal_ime_cursor_rect,
        terminal_ime_cursor_rect_for_layout, terminal_input_bytes_should_reset_smooth_scroll,
        terminal_input_editor_should_defer_keydown_to_typed_characters,
        terminal_mouse_position_is_in_bounds, terminal_page_scroll_lines_for_key,
        terminal_scroll_data, terminal_scrollbar_geometry, terminal_scrollbar_hit,
        terminal_scrollbar_thumb_fill, terminal_shaped_line_data, terminal_shortcut_for_key,
        terminal_shortcut_for_key_on_platform, terminal_typed_characters_for_input,
        viewport_cells_for_available_size, BackgroundRect, CellMetrics, CursorRect, DecorationRect,
        FamilyId, Fill, GridCell, GridSnapshot, RuntimeGridView, ScrollbarHit, TerminalGridAction,
        TerminalImeLayout, TerminalShapedLineCache, TerminalShortcutPlatform, GRID_PADDING_LEFT,
        GRID_PADDING_TOP,
    };
    use nexshell::terminal_runtime::{
        MarkedText, MouseReportAction, MouseReportButton, TerminalCursorShape, TerminalGridCore,
        TerminalGridSnapshot, TerminalInputModes, TerminalPalette,
    };
    use pathfinder_color::ColorU;
    use pathfinder_geometry::vector::Vector2F;
    use std::sync::Arc;
    use warpui_core::event::ModifiersState;
    use warpui_core::fonts::{Properties, Style, Weight};

    #[test]
    fn cell_metrics_fields_are_accessible() {
        let metrics = CellMetrics {
            width: 9.0,
            height: 20.0,
            baseline_y: 15.0,
        };
        assert_eq!(metrics.width, 9.0);
        assert_eq!(metrics.height, 20.0);
        assert_eq!(metrics.baseline_y, 15.0);
    }

    #[test]
    fn viewport_cells_floor_available_pixels_to_whole_terminal_cells() {
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        // usable = (828-16-12=800) x (404-4=400) → 80 cols × 20 rows
        assert_eq!(
            viewport_cells_for_available_size(Vector2F::new(828.0, 404.0), cell, (100, 30)),
            (80, 20)
        );
    }

    #[test]
    fn split_pane_terminal_body_size_reserves_header_before_grid_resize() {
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };
        let pane_size = Vector2F::new(828.0, 430.0);
        let header_height = 26.0;

        // Body height is 430 - 26, then grid padding leaves 400 px => 20 rows.
        assert_eq!(
            viewport_cells_for_available_size(
                split_pane_terminal_body_size(pane_size, header_height),
                cell,
                (100, 30)
            ),
            (80, 20)
        );
    }

    #[test]
    fn viewport_cells_clamp_tiny_sizes_and_fallback_for_unbounded_constraints() {
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        assert_eq!(
            viewport_cells_for_available_size(Vector2F::new(0.0, 7.0), cell, (100, 30)),
            (1, 1)
        );
        assert_eq!(
            viewport_cells_for_available_size(Vector2F::new(f32::INFINITY, 403.0), cell, (120, 40)),
            (120, 40)
        );
    }

    #[test]
    fn resize_cells_only_emit_when_viewport_cell_count_changes() {
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };
        let mut last = (100, 30);

        // usable = (828-16-12=800) x (404-4=400) → 80 × 20
        assert_eq!(
            resize_cells_for_available_size(
                &mut last,
                Vector2F::new(828.0, 404.0),
                cell,
                (100, 30)
            ),
            Some((80, 20))
        );
        assert_eq!(last, (80, 20));
        // usable = (837-28=809) x (423-4=419) → 80 × 20 → no change
        assert_eq!(
            resize_cells_for_available_size(
                &mut last,
                Vector2F::new(837.0, 423.0),
                cell,
                (100, 30)
            ),
            None
        );
        assert_eq!(last, (80, 20));
    }

    #[test]
    fn terminal_scrollbar_geometry_uses_warpui_auto_width_and_bottom_relative_offset() {
        let mut grid = active_mouse_grid();
        grid.rows = 20;
        grid.cells = vec![GridCell::empty(); grid.cols * grid.rows];
        grid.history_size = 100;
        grid.display_offset = 0;
        let cell = CellMetrics {
            width: 10.0,
            height: 24.0,
            baseline_y: 18.0,
        };
        let size = Vector2F::new(800.0, 480.0);

        let bottom = terminal_scrollbar_geometry(Vector2F::zero(), size, &grid, cell)
            .expect("history should produce scrollbar geometry");
        assert!(bottom.has_thumb());
        assert!((bottom.track_bounds.width() - 12.0).abs() < 0.001);
        assert!((bottom.thumb_bounds.width() - 8.0).abs() < 0.001);
        assert!((bottom.thumb_bounds.max_y() - bottom.track_bounds.max_y()).abs() < 0.5);

        grid.display_offset = 100;
        let top = terminal_scrollbar_geometry(Vector2F::zero(), size, &grid, cell)
            .expect("history should produce scrollbar geometry");
        assert!((top.thumb_bounds.min_y() - top.track_bounds.min_y()).abs() < 0.5);
    }

    #[test]
    fn scrollbar_display_offset_for_center_maps_pointer_to_history_offset() {
        let mut grid = active_mouse_grid();
        grid.rows = 20;
        grid.cells = vec![GridCell::empty(); grid.cols * grid.rows];
        grid.history_size = 100;
        grid.display_offset = 0;
        let cell = CellMetrics {
            width: 10.0,
            height: 24.0,
            baseline_y: 18.0,
        };
        let geometry =
            terminal_scrollbar_geometry(Vector2F::zero(), Vector2F::new(800.0, 480.0), &grid, cell)
                .unwrap();

        let top_center = geometry.track_bounds.min_y() + geometry.thumb_bounds.height() / 2.0;
        let bottom_center = geometry.track_bounds.max_y() - geometry.thumb_bounds.height() / 2.0;
        let middle_center = (top_center + bottom_center) / 2.0;

        assert_eq!(
            scrollbar_display_offset_for_center(&geometry, grid.history_size, top_center),
            Some(100)
        );
        assert_eq!(
            scrollbar_display_offset_for_center(&geometry, grid.history_size, middle_center),
            Some(50)
        );
        assert_eq!(
            scrollbar_display_offset_for_center(&geometry, grid.history_size, bottom_center),
            Some(0)
        );
    }

    #[test]
    fn terminal_scrollbar_hit_matches_warp_thumb_and_track_priority() {
        let mut grid = active_mouse_grid();
        grid.rows = 20;
        grid.cells = vec![GridCell::empty(); grid.cols * grid.rows];
        grid.history_size = 100;
        let cell = CellMetrics {
            width: 10.0,
            height: 24.0,
            baseline_y: 18.0,
        };
        let geometry =
            terminal_scrollbar_geometry(Vector2F::zero(), Vector2F::new(800.0, 480.0), &grid, cell)
                .unwrap();

        assert_eq!(
            terminal_scrollbar_hit(&geometry, geometry.thumb_bounds.center()),
            Some(ScrollbarHit::Thumb)
        );
        assert_eq!(
            terminal_scrollbar_hit(
                &geometry,
                Vector2F::new(
                    geometry.track_bounds.center().x(),
                    geometry.track_bounds.min_y() + 1.0
                )
            ),
            Some(ScrollbarHit::Track)
        );
        assert_eq!(
            terminal_scrollbar_hit(&geometry, Vector2F::new(1.0, 1.0)),
            None
        );
    }

    #[test]
    fn terminal_scrollbar_thumb_fill_follows_warp_hover_states() {
        let p = TerminalPalette::default();
        assert_eq!(
            terminal_scrollbar_thumb_fill(&p, false, false, false),
            Fill::None
        );
        assert_eq!(
            terminal_scrollbar_thumb_fill(&p, true, false, false),
            Fill::Solid(ColorU::new(0xff, 0xff, 0xff, 0x66))
        );
        assert_eq!(
            terminal_scrollbar_thumb_fill(&p, true, true, false),
            Fill::Solid(pathfinder_color::ColorU::new(0xff, 0xff, 0xff, 0xe5))
        );
        assert_eq!(
            terminal_scrollbar_thumb_fill(&p, false, false, true),
            Fill::Solid(pathfinder_color::ColorU::new(0xff, 0xff, 0xff, 0xe5))
        );
    }

    #[test]
    fn scrollbar_pointer_movement_uses_warp_scrollbar_percentage_delta() {
        let mut grid = active_mouse_grid();
        grid.rows = 20;
        grid.cells = vec![GridCell::empty(); grid.cols * grid.rows];
        grid.history_size = 100;
        grid.display_offset = 50;
        let cell = CellMetrics {
            width: 10.0,
            height: 24.0,
            baseline_y: 18.0,
        };
        let data = terminal_scroll_data(&grid, cell);

        assert_eq!(
            scrollbar_display_offset_for_pointer_movement(data, 100, 50, 24.0, 100.0, 124.0),
            Some(44)
        );
        assert_eq!(
            scrollbar_display_offset_for_pointer_movement(data, 100, 50, 24.0, 124.0, 100.0),
            Some(56)
        );
    }

    #[test]
    fn grid_snapshot_preserves_runtime_mouse_app_modes() {
        let mut snapshot = TerminalGridSnapshot::empty();
        snapshot.cols = 4;
        snapshot.rows = 3;
        snapshot.cursor_blinking = true;
        snapshot.mouse_report_drag = true;
        snapshot.sgr_mouse = true;

        let grid = GridSnapshot::from_runtime_snapshot(&snapshot, &TerminalPalette::default());

        assert!(grid.cursor_blinking);
        assert!(grid.mouse_app_active());
        assert!(grid.mouse_drag_reporting_active());
        assert!(!grid.mouse_motion_reporting_active());
    }

    #[test]
    fn grid_snapshot_preserves_main_screen_mouse_modes() {
        // 守门：主屏内合法启用 mouse tracking 的 TUI（less -X / mc / htop 不进
        // alt-screen 的模式等）必须保留 mouse 字段，runtime 已在 alt 退出瞬间
        // 主动复位，无需 UI 层再 gate。
        let mut snapshot = TerminalGridSnapshot::empty();
        snapshot.cols = 4;
        snapshot.rows = 3;
        snapshot.mouse_report_click = true;
        snapshot.mouse_report_drag = true;
        snapshot.sgr_mouse = true;
        snapshot.input_modes.alt_screen = false;

        let grid = GridSnapshot::from_runtime_snapshot(&snapshot, &TerminalPalette::default());

        assert!(grid.mouse_app_active());
        assert!(grid.mouse_drag_reporting_active());
        assert!(grid.sgr_mouse);
    }

    #[test]
    fn grid_snapshot_does_not_reapply_block_cursor_background_during_marked_text() {
        let mut core = TerminalGridCore::new(4, 1, 100);
        let snapshot = core.snapshot_with_marked_text(
            &[],
            None,
            Some(&MarkedText {
                text: "a".to_string(),
                selected_range_utf16: 0..0,
            }),
        );

        let grid = GridSnapshot::from_runtime_snapshot(&snapshot, &TerminalPalette::default());

        assert_ne!(
            grid.cell(snapshot.cursor_row, snapshot.cursor_col)
                .unwrap()
                .bg,
            ColorU::new(0x2e, 0x5d, 0x9e, 255),
            "marked text should not leave a block cursor background in the GPUI grid snapshot"
        );
    }

    #[test]
    fn grid_snapshot_omits_block_cursor_background_when_cursor_hidden() {
        // ?25l 隐藏光标后，光标所在空 cell 不能被涂成光标色，否则 TUI 隐藏光标时
        // grid 会残留一个块（Claude Code 思考态把光标停在左下角即此现象）。
        let palette = TerminalPalette::default();
        let cursor_color = super::u32_to_color(palette.cursor);
        let mut core = TerminalGridCore::new(4, 2, 100);
        core.process_output(b"\r\n"); // 光标落到 (1,0) 空 cell，避开 (0,0) IME 特例
        core.process_output(b"\x1b[?25l");
        let hidden = core.snapshot(&[], None);
        assert!(!hidden.cursor_visible);
        let grid = GridSnapshot::from_runtime_snapshot(&hidden, &palette);
        assert_ne!(
            grid.cell(hidden.cursor_row, hidden.cursor_col).unwrap().bg,
            cursor_color,
            "hidden cursor (?25l) must not paint a block-cursor background"
        );

        // 对照：?25h 显示时同一空 cell 仍应涂光标色。
        core.process_output(b"\x1b[?25h");
        let shown = core.snapshot(&[], None);
        let grid = GridSnapshot::from_runtime_snapshot(&shown, &palette);
        assert_eq!(
            grid.cell(shown.cursor_row, shown.cursor_col).unwrap().bg,
            cursor_color,
            "visible block cursor should still paint its background"
        );
    }

    #[test]
    fn grid_snapshot_does_not_apply_block_cursor_background_over_existing_text() {
        let mut core = TerminalGridCore::new(4, 1, 100);
        core.process_output(b"ab\x1b[D");
        let snapshot = core.snapshot(&[], None);

        let grid = GridSnapshot::from_runtime_snapshot(&snapshot, &TerminalPalette::default());

        assert_eq!(grid.cell(0, 1).unwrap().content.as_ref(), "b");
        assert_ne!(
            grid.cell(0, 1).unwrap().bg,
            ColorU::new(0x2e, 0x5d, 0x9e, 255),
            "block cursor should not cover an occupied text cell"
        );
    }

    #[test]
    fn grid_snapshot_preserves_osc8_hyperlink_uri() {
        let mut core = TerminalGridCore::new(8, 1, 100);
        core.process_output(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\");
        let snapshot = core.snapshot(&[], None);

        let grid = GridSnapshot::from_runtime_snapshot(&snapshot, &TerminalPalette::default());

        assert_eq!(
            grid.cell(0, 0).and_then(|cell| cell.hyperlink).as_deref(),
            Some("https://example.com")
        );
        assert!(grid.cell(0, 4).and_then(|cell| cell.hyperlink).is_none());
    }

    #[test]
    fn grid_snapshot_preserves_terminal_text_decorations() {
        let mut core = TerminalGridCore::new(8, 1, 100);
        core.process_output(b"\x1b[4mU\x1b[0m\x1b[9mS\x1b[0m\x1b[4:2mD\x1b[0m");
        let snapshot = core.snapshot(&[], None);

        let grid = GridSnapshot::from_runtime_snapshot(&snapshot, &TerminalPalette::default());

        assert!(grid.cell(0, 0).unwrap().underline);
        assert!(grid.cell(0, 1).unwrap().strikeout);
        assert!(grid.cell(0, 2).unwrap().double_underline);
    }

    #[test]
    fn terminal_background_rects_merge_adjacent_cells_with_same_color() {
        let mut grid = GridSnapshot {
            cols: 4,
            rows: 1,
            cells: vec![GridCell::empty(); 4],
            cursor_row: 0,
            cursor_col: 0,
            cursor_shape: TerminalCursorShape::Block,
            cursor_visible: false,
            cursor_blinking: false,
            marked_text_active: false,
            display_offset: 0,
            history_size: 0,
            mouse_report_click: false,
            mouse_report_motion: false,
            mouse_report_drag: false,
            sgr_mouse: false,
        };
        grid.cells[2].bg = ColorU::new(1, 2, 3, 255);
        grid.cells[3].bg = ColorU::new(1, 2, 3, 255);

        let rects = terminal_background_rects(
            &grid,
            CellMetrics {
                width: 10.0,
                height: 20.0,
                baseline_y: 14.0,
            },
        );

        assert_eq!(
            rects,
            vec![
                BackgroundRect::new(0.0, 0.0, 20.0, 20.0, GridCell::empty().bg),
                BackgroundRect::new(20.0, 0.0, 20.0, 20.0, ColorU::new(1, 2, 3, 255)),
            ]
        );
    }

    #[test]
    fn terminal_shaped_line_data_tracks_style_runs_and_cell_columns() {
        let mut grid = active_mouse_grid();
        grid.cols = 5;
        grid.rows = 1;
        grid.cells = vec![GridCell::empty(); 5];
        grid.cells[0].ch = 'A';
        grid.cells[0].content = Arc::from("A");
        grid.cells[0].fg = ColorU::new(200, 10, 10, 255);
        grid.cells[1].fg = grid.cells[0].fg;
        grid.cells[2].ch = 'B';
        grid.cells[2].content = Arc::from("B");
        grid.cells[2].fg = grid.cells[0].fg;
        grid.cells[2].bold = true;
        grid.cells[3].ch = '你';
        grid.cells[3].content = Arc::from("你");
        grid.cells[3].fg = ColorU::new(10, 20, 200, 255);
        grid.cells[4].wide_spacer = true;

        let line = terminal_shaped_line_data(&grid, 0, FamilyId(0)).unwrap();

        assert_eq!(line.text, "A B你");
        assert_eq!(line.character_index_to_cell_map, vec![0, 1, 2, 3]);
        assert_eq!(line.style_runs.len(), 3);
        assert_eq!(line.style_runs[0].0, 0..2);
        assert_eq!(
            line.style_runs[0].1.style.foreground_color,
            Some(grid.cells[0].fg)
        );
        assert_eq!(line.style_runs[1].0, 2..3);
        assert_eq!(
            line.style_runs[1].1.properties,
            Properties::default().weight(Weight::Bold)
        );
        assert_eq!(line.style_runs[2].0, 3..4);
        assert_eq!(
            line.style_runs[2].1.style.foreground_color,
            Some(grid.cells[3].fg)
        );
    }

    #[test]
    fn terminal_shaped_line_cache_shifts_clean_rows_on_scroll() {
        let mut core = TerminalGridCore::new(8, 3, 100);
        for line in 0..6 {
            core.process_output(format!("line-{line}\r\n").as_bytes());
        }
        let first = core.snapshot(&[], None);
        let default_palette = TerminalPalette::default();
        let first_view = RuntimeGridView {
            grid: &first,
            palette: &default_palette,
        };
        let mut cache = TerminalShapedLineCache::default();
        let old_top = cache.line_data(&first_view, 0, FamilyId(0), 0).unwrap();
        let _old_second = cache.line_data(&first_view, 1, FamilyId(0), 0).unwrap();

        core.clear_dirty_rows();
        core.scroll_lines(1);
        let second = core.snapshot(&[], None);
        let second_view = RuntimeGridView {
            grid: &second,
            palette: &default_palette,
        };
        let shifted = cache.line_data(&second_view, 1, FamilyId(0), 0).unwrap();

        assert_eq!(second.dirty_rows, vec![true, false, false]);
        assert!(Arc::ptr_eq(&old_top, &shifted));
    }

    #[test]
    fn grid_snapshot_preserves_terminal_font_styles() {
        let mut core = TerminalGridCore::new(8, 1, 100);
        core.process_output(b"\x1b[1mB\x1b[0m\x1b[3mI\x1b[0m\x1b[1;3mZ\x1b[0m");
        let snapshot = core.snapshot(&[], None);

        let grid = GridSnapshot::from_runtime_snapshot(&snapshot, &TerminalPalette::default());

        let bold = grid.cell(0, 0).unwrap();
        assert!(bold.bold);
        assert!(!bold.italic);
        assert_eq!(
            terminal_font_properties_for_cell(&bold),
            Properties::default().weight(Weight::Bold)
        );

        let italic = grid.cell(0, 1).unwrap();
        assert!(!italic.bold);
        assert!(italic.italic);
        assert_eq!(
            terminal_font_properties_for_cell(&italic),
            Properties::default().style(Style::Italic)
        );

        let bold_italic = grid.cell(0, 2).unwrap();
        assert!(bold_italic.bold);
        assert!(bold_italic.italic);
        assert_eq!(
            terminal_font_properties_for_cell(&bold_italic),
            Properties::default()
                .weight(Weight::Bold)
                .style(Style::Italic)
        );
    }

    #[test]
    fn grid_snapshot_preserves_zero_width_cell_content() {
        let mut core = TerminalGridCore::new(8, 1, 100);
        core.process_output(b"e\xcc\x81x");
        let snapshot = core.snapshot(&[], None);

        let grid = GridSnapshot::from_runtime_snapshot(&snapshot, &TerminalPalette::default());

        assert_eq!(grid.cell(0, 0).unwrap().ch, 'e');
        assert_eq!(
            grid.cell(0, 0).unwrap().content.as_ref(),
            format!("e{}", '\u{0301}')
        );
        assert_eq!(grid.cell(0, 1).unwrap().content.as_ref(), "x");
    }

    #[test]
    fn terminal_cell_decoration_rects_match_warp_geometry() {
        let mut grid = active_mouse_grid();
        grid.cols = 3;
        grid.rows = 1;
        grid.cells = vec![GridCell::empty(); 3];
        grid.cells[0].underline = true;
        grid.cells[1].strikeout = true;
        grid.cells[2].double_underline = true;
        grid.cells[0].fg = pathfinder_color::ColorU::new(1, 2, 3, 255);
        grid.cells[1].fg = pathfinder_color::ColorU::new(4, 5, 6, 255);
        grid.cells[2].fg = pathfinder_color::ColorU::new(7, 8, 9, 255);
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        assert_eq!(
            terminal_cell_decoration_rects(&grid, cell),
            vec![
                DecorationRect::new(0.0, 18.5, 10.0, 1.5, grid.cells[0].fg),
                DecorationRect::new(10.0, 8.5, 10.0, 1.5, grid.cells[1].fg),
                DecorationRect::new(20.0, 17.0, 10.0, 3.0, grid.cells[2].fg),
            ]
        );
    }

    #[test]
    fn terminal_cell_decoration_rects_use_explicit_underline_color() {
        let mut core = TerminalGridCore::new(4, 1, 100);
        core.process_output(b"\x1b[4;58;2;1;2;3mU\x1b[0m");
        let snapshot = core.snapshot(&[], None);
        let grid = GridSnapshot::from_runtime_snapshot(&snapshot, &TerminalPalette::default());
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        assert_eq!(
            grid.cell(0, 0).unwrap().underline_color,
            Some(pathfinder_color::ColorU::new(1, 2, 3, 255))
        );
        assert_eq!(
            terminal_cell_decoration_rects(&grid, cell)[0],
            DecorationRect::new(
                0.0,
                18.5,
                10.0,
                1.5,
                pathfinder_color::ColorU::new(1, 2, 3, 255)
            )
        );
    }

    #[test]
    fn cursor_rects_match_warp_beam_and_underline_geometry() {
        let mut grid = active_mouse_grid();
        grid.cursor_row = 1;
        grid.cursor_col = 2;
        grid.cursor_visible = true;
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        grid.cursor_shape = TerminalCursorShape::Beam;
        assert_eq!(
            cursor_rects(&grid, cell),
            vec![CursorRect::new(20.0, 20.0, 1.5, 20.0)]
        );

        grid.cursor_shape = TerminalCursorShape::Underline;
        assert_eq!(
            cursor_rects(&grid, cell),
            vec![CursorRect::new(20.0, 38.5, 10.0, 1.5)]
        );
    }

    #[test]
    fn cursor_rects_draw_full_block_cursor_during_marked_text() {
        let mut grid = active_mouse_grid();
        grid.cursor_row = 1;
        grid.cursor_col = 2;
        grid.cursor_visible = true;
        grid.cursor_shape = TerminalCursorShape::Block;
        grid.marked_text_active = true;
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        assert_eq!(
            cursor_rects(&grid, cell),
            vec![CursorRect::new(20.0, 20.0, 10.0, 20.0)]
        );
    }

    #[test]
    fn cursor_rects_draw_full_block_cursor_on_empty_cell() {
        let mut grid = active_mouse_grid();
        grid.cursor_row = 1;
        grid.cursor_col = 2;
        grid.cursor_visible = true;
        grid.cursor_shape = TerminalCursorShape::Block;
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        assert_eq!(
            cursor_rects(&grid, cell),
            vec![CursorRect::new(20.0, 20.0, 10.0, 20.0)]
        );
    }

    #[test]
    fn cursor_rects_draw_full_block_cursor_on_existing_text() {
        let mut grid = active_mouse_grid();
        grid.cursor_row = 1;
        grid.cursor_col = 2;
        grid.cursor_visible = true;
        grid.cursor_shape = TerminalCursorShape::Block;
        grid.cells[grid.cursor_row * grid.cols + grid.cursor_col].content = Arc::from("x");
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        assert_eq!(
            cursor_rects(&grid, cell),
            vec![CursorRect::new(20.0, 20.0, 10.0, 20.0)]
        );
    }

    #[test]
    fn cursor_rects_span_two_cells_for_wide_character() {
        let mut grid = active_mouse_grid();
        grid.cursor_row = 0;
        grid.cursor_col = 1;
        grid.cursor_visible = true;
        grid.cursor_shape = TerminalCursorShape::Block;
        grid.cells[1].ch = '中';
        grid.cells[1].content = Arc::from("中");
        grid.cells[2].wide_spacer = true;
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        assert_eq!(
            cursor_rects(&grid, cell),
            vec![CursorRect::new(10.0, 0.0, 20.0, 20.0)]
        );
    }

    #[test]
    fn terminal_ime_cursor_rect_uses_visible_cursor_cell_anchor() {
        let mut grid = active_mouse_grid();
        grid.cursor_row = 2;
        grid.cursor_col = 3;
        let cell = CellMetrics {
            width: 10.0,
            height: 24.0,
            baseline_y: 18.0,
        };

        let rect = terminal_ime_cursor_rect(&grid, Vector2F::new(5.0, 7.0), cell, 14.0).unwrap();

        assert_eq!(rect.origin_x(), 35.0);
        assert!((rect.origin_y() - 59.56).abs() < 0.001);
        assert_eq!(rect.width(), 10.0);
        assert!((rect.height() - 16.8).abs() < 0.001);
    }

    #[test]
    fn terminal_ime_cursor_rect_uses_hidden_cursor_when_not_top_left() {
        let mut grid = active_mouse_grid();
        grid.cursor_row = 2;
        grid.cursor_col = 3;
        grid.cursor_visible = false;
        let cell = CellMetrics {
            width: 10.0,
            height: 24.0,
            baseline_y: 18.0,
        };
        assert!(terminal_ime_cursor_rect(&grid, Vector2F::new(5.0, 7.0), cell, 14.0).is_some());

        grid.cursor_visible = true;
        grid.cursor_shape = TerminalCursorShape::Hidden;
        assert!(terminal_ime_cursor_rect(&grid, Vector2F::new(5.0, 7.0), cell, 14.0).is_some());
    }

    #[test]
    fn terminal_ime_cursor_rect_rejects_hidden_top_left_cursor() {
        let mut grid = active_mouse_grid();
        grid.cursor_row = 0;
        grid.cursor_col = 0;
        grid.cursor_visible = false;
        let cell = CellMetrics {
            width: 10.0,
            height: 24.0,
            baseline_y: 18.0,
        };
        assert!(terminal_ime_cursor_rect(&grid, Vector2F::new(5.0, 7.0), cell, 14.0).is_none());

        grid.cursor_visible = true;
        grid.cursor_shape = TerminalCursorShape::Hidden;
        assert!(terminal_ime_cursor_rect(&grid, Vector2F::new(5.0, 7.0), cell, 14.0).is_none());
    }

    #[test]
    fn terminal_ime_layout_rect_uses_current_snapshot_cursor() {
        let mut grid = active_mouse_grid();
        grid.cursor_row = 10;
        grid.cursor_col = 3;
        grid.rows = 20;
        let cell = CellMetrics {
            width: 10.0,
            height: 24.0,
            baseline_y: 18.0,
        };
        let layout = TerminalImeLayout {
            element_origin: Vector2F::new(2.0, 3.0),
            cell_metrics: cell,
            font_size: 14.0,
        };

        let rect = terminal_ime_cursor_rect_for_layout(&grid, &layout, 4.0).unwrap();

        assert_eq!(rect.origin_x(), 2.0 + GRID_PADDING_LEFT + 30.0);
        assert!((rect.origin_y() - (3.0 + GRID_PADDING_TOP + 4.0 + 240.0 + 4.56)).abs() < 0.001);
    }

    #[test]
    fn cursor_rects_skip_hidden_shape_and_invisible_cursor() {
        let mut grid = active_mouse_grid();
        grid.cursor_visible = false;
        grid.cursor_shape = TerminalCursorShape::Block;
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        // ?25l 隐藏光标时不渲染
        assert!(cursor_rects(&grid, cell).is_empty());

        grid.cursor_visible = true;
        grid.cursor_shape = TerminalCursorShape::Hidden;
        assert!(cursor_rects(&grid, cell).is_empty());

        grid.cursor_visible = true;
        grid.cursor_shape = TerminalCursorShape::Block;
        assert!(!cursor_rects(&grid, cell).is_empty());
    }

    #[test]
    fn cursor_rects_show_static_cursor_regardless_of_blink_flag() {
        let mut grid = active_mouse_grid();
        grid.cursor_shape = TerminalCursorShape::Beam;
        grid.cursor_visible = true;
        // 协议层 blinking 标记不再影响显隐——光标只由 mode + shape 决定
        grid.cursor_blinking = true;
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        assert_eq!(
            cursor_rects(&grid, cell),
            vec![CursorRect::new(0.0, 0.0, 1.5, 20.0)]
        );

        // 仅当 mode 隐藏光标时才不绘制
        grid.cursor_visible = false;
        assert!(cursor_rects(&grid, cell).is_empty());
    }

    #[test]
    fn mouse_report_bytes_use_viewport_cell_coordinates_and_xterm_sgr_format() {
        let grid = active_mouse_grid();
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        let bytes = mouse_report_bytes(
            &grid,
            cell,
            Vector2F::new(10.0, 20.0),
            Vector2F::new(32.0, 51.0),
            MouseReportButton::Left,
            MouseReportAction::Press,
            ModifiersState {
                alt: true,
                ctrl: true,
                ..Default::default()
            },
        )
        .expect("active mouse mode should emit SGR bytes");

        assert_eq!(String::from_utf8(bytes).unwrap(), "\x1b[<24;3;2M");
    }

    #[test]
    fn mouse_report_bytes_ignore_inactive_modes_and_shift_override() {
        let mut grid = active_mouse_grid();
        let cell = CellMetrics {
            width: 10.0,
            height: 20.0,
            baseline_y: 14.0,
        };

        let shifted = mouse_report_bytes(
            &grid,
            cell,
            Vector2F::new(0.0, 0.0),
            Vector2F::new(5.0, 5.0),
            MouseReportButton::Left,
            MouseReportAction::Press,
            ModifiersState {
                shift: true,
                ..Default::default()
            },
        );
        assert!(shifted.is_none());

        grid.sgr_mouse = false;
        let inactive = mouse_report_bytes(
            &grid,
            cell,
            Vector2F::new(0.0, 0.0),
            Vector2F::new(5.0, 5.0),
            MouseReportButton::Left,
            MouseReportAction::Press,
            ModifiersState::default(),
        );
        assert!(inactive.is_none());
    }

    #[test]
    fn terminal_shortcuts_map_common_clipboard_and_clear_commands() {
        assert_eq!(
            terminal_shortcut_for_key("c", true, false, false, false),
            Some(TerminalGridAction::CopySelection)
        );
        assert_eq!(
            terminal_shortcut_for_key("v", true, false, false, false),
            Some(TerminalGridAction::PasteClipboard)
        );
        assert_eq!(
            terminal_shortcut_for_key("k", true, false, false, false),
            Some(TerminalGridAction::ClearVisibleScreen)
        );
        assert_eq!(
            terminal_shortcut_for_key("K", false, true, false, true),
            Some(TerminalGridAction::ClearVisibleScreen)
        );
        assert_eq!(
            terminal_shortcut_for_key("f", true, false, false, false),
            Some(TerminalGridAction::OpenFindBar)
        );
        assert_eq!(
            terminal_shortcut_for_key("=", true, false, false, false),
            Some(TerminalGridAction::IncreaseFontSize)
        );
        assert_eq!(
            terminal_shortcut_for_key("+", true, false, false, true),
            Some(TerminalGridAction::IncreaseFontSize)
        );
        assert_eq!(
            terminal_shortcut_for_key("-", true, false, false, false),
            Some(TerminalGridAction::DecreaseFontSize)
        );
        assert_eq!(
            terminal_shortcut_for_key("0", true, false, false, false),
            Some(TerminalGridAction::ResetFontSize)
        );
    }

    #[test]
    fn terminal_shortcuts_match_warp_windows_copy_paste_policy() {
        assert_eq!(
            terminal_shortcut_for_key_on_platform(
                "C",
                false,
                true,
                false,
                true,
                false,
                TerminalShortcutPlatform::Windows
            ),
            Some(TerminalGridAction::CopySelection)
        );
        assert_eq!(
            terminal_shortcut_for_key_on_platform(
                "V",
                false,
                true,
                false,
                true,
                false,
                TerminalShortcutPlatform::Windows
            ),
            Some(TerminalGridAction::PasteClipboard)
        );
        assert_eq!(
            terminal_shortcut_for_key_on_platform(
                "v",
                false,
                true,
                false,
                false,
                false,
                TerminalShortcutPlatform::Windows
            ),
            Some(TerminalGridAction::PasteClipboard)
        );
        assert_eq!(
            terminal_shortcut_for_key_on_platform(
                "c",
                false,
                true,
                false,
                false,
                true,
                TerminalShortcutPlatform::Windows
            ),
            Some(TerminalGridAction::CopySelection)
        );
        assert_eq!(
            terminal_shortcut_for_key_on_platform(
                "c",
                false,
                true,
                false,
                false,
                false,
                TerminalShortcutPlatform::Windows
            ),
            None
        );
    }

    #[test]
    fn windows_ctrl_v_policy_does_not_apply_to_macos_or_linux() {
        assert_eq!(
            terminal_shortcut_for_key_on_platform(
                "v",
                false,
                true,
                false,
                false,
                false,
                TerminalShortcutPlatform::Mac
            ),
            None
        );
        assert_eq!(
            terminal_shortcut_for_key_on_platform(
                "v",
                false,
                true,
                false,
                false,
                false,
                TerminalShortcutPlatform::Other
            ),
            None
        );
    }

    #[test]
    fn copy_shortcut_does_not_request_repaint() {
        assert!(!terminal_action_needs_notify(
            &TerminalGridAction::CopySelection
        ));
        assert!(terminal_action_needs_notify(
            &TerminalGridAction::PasteClipboard
        ));
        assert!(terminal_action_needs_notify(
            &TerminalGridAction::ClearVisibleScreen
        ));
    }

    #[test]
    fn terminal_shortcuts_do_not_steal_control_sequences_or_chords() {
        assert_eq!(
            terminal_shortcut_for_key("c", false, true, false, false),
            None
        );
        assert_eq!(
            terminal_shortcut_for_key("v", true, false, true, false),
            None
        );
        assert_eq!(
            terminal_shortcut_for_key("k", true, false, false, true),
            None
        );
        assert_eq!(
            terminal_shortcut_for_key("l", false, true, false, false),
            None
        );
    }

    #[test]
    fn terminal_input_editor_leaves_shell_editing_keys_to_warp_encoding() {
        let modes = TerminalInputModes::default();
        assert_eq!(
            encode_terminal_key_event_with_modes(
                "enter", None, None, false, false, false, false, modes
            ),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            encode_terminal_key_event_with_modes(
                "backspace",
                None,
                None,
                false,
                false,
                false,
                false,
                modes
            ),
            Some(vec![0x7f])
        );
        assert_eq!(
            encode_terminal_key_event_with_modes(
                "tab", None, None, false, false, false, false, modes
            ),
            Some(b"\t".to_vec())
        );
        assert_eq!(
            encode_terminal_key_event_with_modes(
                "left", None, None, false, false, false, false, modes
            ),
            Some(b"\x1b[D".to_vec())
        );
        assert_eq!(
            encode_terminal_key_event_with_modes(
                "right", None, None, false, false, false, false, modes
            ),
            Some(b"\x1b[C".to_vec())
        );
        assert_eq!(
            encode_terminal_key_event_with_modes(
                "up", None, None, false, false, false, false, modes
            ),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            encode_terminal_key_event_with_modes(
                "down", None, None, false, false, false, false, modes
            ),
            Some(b"\x1b[B".to_vec())
        );
        assert_eq!(
            encode_terminal_key_event_with_modes("c", None, None, true, false, false, false, modes),
            Some(vec![0x03])
        );
        assert_eq!(
            encode_terminal_key_event_with_modes("l", None, None, true, false, false, false, modes),
            Some(vec![0x0c])
        );
        assert_eq!(
            encode_terminal_key_event_with_modes(
                "enter", None, None, false, false, false, true, modes
            ),
            None
        );
    }

    #[test]
    fn terminal_input_editor_keydown_defers_printable_chars_to_typed_characters() {
        assert!(terminal_input_editor_should_defer_keydown_to_typed_characters("a", false));
        assert!(terminal_input_editor_should_defer_keydown_to_typed_characters("你", false));
        assert!(!terminal_input_editor_should_defer_keydown_to_typed_characters("\u{3}", false));
        assert!(!terminal_input_editor_should_defer_keydown_to_typed_characters("\u{F700}", false));
        assert!(!terminal_input_editor_should_defer_keydown_to_typed_characters("a", true));
    }

    #[test]
    fn terminal_page_keys_scroll_normal_screen_and_defer_alt_screen() {
        let mut snapshot = TerminalGridSnapshot::empty();
        snapshot.rows = 24;
        snapshot.input_modes.alt_screen = false;

        assert_eq!(
            terminal_page_scroll_lines_for_key("pageup", false, false, false, false, &snapshot),
            Some(23)
        );
        assert_eq!(
            terminal_page_scroll_lines_for_key("pagedown", false, false, false, false, &snapshot),
            Some(-23)
        );
        assert_eq!(
            terminal_page_scroll_lines_for_key("pageup", false, false, false, true, &snapshot),
            None
        );

        snapshot.input_modes.alt_screen = true;
        assert_eq!(
            terminal_page_scroll_lines_for_key("pageup", false, false, false, false, &snapshot),
            None
        );
    }

    #[test]
    fn terminal_typed_characters_skip_macos_function_keys() {
        assert_eq!(terminal_typed_characters_for_input("\u{F700}"), None);
        assert_eq!(
            terminal_typed_characters_for_input("a")
                .as_ref()
                .map(|chars| chars.as_ref() as &str),
            Some("a")
        );
        assert_eq!(
            terminal_typed_characters_for_input("a\u{F700}b")
                .as_ref()
                .map(|chars| chars.as_ref() as &str),
            Some("ab")
        );
    }

    #[test]
    fn active_find_keys_update_query_and_step_matches() {
        assert_eq!(
            find_action_for_key("escape", "", false, false, false, false, true, "abc"),
            Some(TerminalGridAction::CloseFindBar)
        );
        assert_eq!(
            find_action_for_key("enter", "", false, false, false, false, true, "abc"),
            Some(TerminalGridAction::FindStep(1))
        );
        assert_eq!(
            find_action_for_key("enter", "", false, false, false, true, true, "abc"),
            Some(TerminalGridAction::FindStep(-1))
        );
        // backspace / 字符输入由 EditorView 处理，不再经过 find_action_for_key
        assert_eq!(
            find_action_for_key("backspace", "", false, false, false, false, true, "abc"),
            None
        );
        assert_eq!(
            find_action_for_key("x", "x", false, false, false, false, true, "ab"),
            None
        );
    }

    #[test]
    fn find_keys_ignore_inactive_state_and_command_chords() {
        assert_eq!(
            find_action_for_key("x", "x", false, false, false, false, false, "ab"),
            None
        );
        assert_eq!(
            find_action_for_key("c", "c", true, false, false, false, true, "ab"),
            None
        );
        assert_eq!(
            find_action_for_key("x", "x", false, true, false, false, true, "ab"),
            None
        );
    }

    #[test]
    fn terminal_drag_drop_input_uses_warp_path_escaping_and_trailing_space() {
        let paths = vec![
            "/tmp/plain.txt".to_string(),
            "/tmp/with space/$name[1].txt".to_string(),
        ];

        assert_eq!(
            terminal_drag_drop_input(&paths),
            Some("/tmp/plain.txt /tmp/with\\ space/\\$name\\[1\\].txt ".to_string())
        );
        assert_eq!(terminal_drag_drop_input(&[]), None);
    }

    #[test]
    fn accumulate_scroll_px_smooth() {
        let cell_h = 20.0;
        let mut acc = 0.0;

        // precise=true: 42px * 2.0 = 84px → 84/20 = 4 lines, 余 4px
        assert_eq!(accumulate_scroll_px(42.0, true, cell_h, &mut acc), 4);
        assert!((acc - 4.0).abs() < 0.001);

        // 再来 8px * 2.0 = 16px → 16+4 = 20px → 1 line, 余 0px
        assert_eq!(accumulate_scroll_px(8.0, true, cell_h, &mut acc), 1);
        assert!(acc.abs() < 0.001);

        // precise=false: 1 notch * 3.0 * 20px = 60px → 3 lines
        acc = 0.0;
        assert_eq!(accumulate_scroll_px(1.0, false, cell_h, &mut acc), 3);
        assert!(acc.abs() < 0.001);

        // 反方向: -1 notch → -60px → -3 lines
        assert_eq!(accumulate_scroll_px(-1.0, false, cell_h, &mut acc), -3);
        assert!(acc.abs() < 0.001);

        // 亚行累积：5px * 2.0 = 10px → 0 lines, 余 10px
        acc = 0.0;
        assert_eq!(accumulate_scroll_px(5.0, true, cell_h, &mut acc), 0);
        assert!((acc - 10.0).abs() < 0.001);
    }

    #[test]
    fn ctrl_l_input_resets_smooth_scroll_remainder() {
        assert!(terminal_input_bytes_should_reset_smooth_scroll(b"\x0c"));
        assert!(!terminal_input_bytes_should_reset_smooth_scroll(b"a"));
        assert!(!terminal_input_bytes_should_reset_smooth_scroll(
            b"\x1b[<64;6;6M"
        ));
    }

    #[test]
    fn mouse_wheel_report_repeats_are_batched_like_warp() {
        assert_eq!(
            repeat_mouse_report_bytes(b"\x1b[<64;6;6M", 3),
            b"\x1b[<64;6;6M\x1b[<64;6;6M\x1b[<64;6;6M"
        );
    }

    #[test]
    fn terminal_mouse_position_bounds_reject_titlebar_coordinates() {
        let origin = Vector2F::new(0.0, 35.0);
        let size = Vector2F::new(400.0, 300.0);

        assert!(!terminal_mouse_position_is_in_bounds(
            origin,
            size,
            Vector2F::new(200.0, 20.0),
        ));
        assert!(terminal_mouse_position_is_in_bounds(
            origin,
            size,
            Vector2F::new(200.0, 36.0),
        ));
    }

    fn active_mouse_grid() -> GridSnapshot {
        GridSnapshot {
            cols: 4,
            rows: 3,
            cells: vec![GridCell::empty(); 12],
            display_offset: 7,
            history_size: 0,
            mouse_report_click: true,
            mouse_report_motion: false,
            mouse_report_drag: false,
            sgr_mouse: true,
            cursor_row: 0,
            cursor_col: 0,
            cursor_shape: TerminalCursorShape::Block,
            cursor_visible: true,
            cursor_blinking: false,
            marked_text_active: false,
        }
    }
}
