use crate::layout::Rect;

pub const SCROLLBAR_GUTTER_PX: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontSpec {
    pub family: &'static str,
    pub size: f32,
    pub letter_spacing: f32,
    pub line_height: f32,
    pub dpr: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

impl CellMetrics {
    pub fn from_em_advance(em_advance: f32, line_height: f32) -> Self {
        Self {
            width: em_advance.max(1.0),
            height: line_height.max(1.0),
        }
    }

    pub fn from_font_metrics(
        em_advance: f32,
        ascent: f32,
        descent: f32,
        line_gap: f32,
        line_height_ratio: f32,
    ) -> Self {
        let natural = ascent + descent.abs() + line_gap;
        let scale = if line_height_ratio > 0.0 {
            line_height_ratio
        } else {
            1.0
        };
        Self::from_em_advance(em_advance, (natural * scale).ceil())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridSize {
    pub cols: usize,
    pub rows: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalLifecycleConfig {
    pub session_id: &'static str,
    pub rect: Rect,
    pub surface: SurfaceSize,
    pub font: FontSpec,
    pub scrollback_lines: usize,
    pub is_local: bool,
    pub cursor_style: &'static str,
    pub sync_highlight_rules: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TerminalLifecycleCommand {
    UpdateTheme {
        session_id: &'static str,
    },
    Create {
        session_id: &'static str,
        cols: usize,
        rows: usize,
        cell_width: f32,
        cell_height: f32,
        scrollback_lines: usize,
        is_local: bool,
    },
    SetCursorStyle {
        session_id: &'static str,
        style: &'static str,
    },
    UpdateHighlightRules {
        session_id: &'static str,
    },
    UpdateFont {
        session_id: &'static str,
        font: FontSpec,
    },
    Resize {
        session_id: &'static str,
        cols: usize,
        rows: usize,
        cell_width: f32,
        cell_height: f32,
    },
    ResizeSurface {
        session_id: &'static str,
        surface: SurfaceSize,
    },
}

impl TerminalLifecycleCommand {
    pub fn tauri_command_name(&self) -> &'static str {
        match self {
            Self::UpdateTheme { .. } => "terminal_update_theme",
            Self::Create { .. } => "terminal_create",
            Self::SetCursorStyle { .. } => "terminal_set_cursor_style",
            Self::UpdateHighlightRules { .. } => "terminal_update_highlight_rules",
            Self::UpdateFont { .. } => "terminal_update_font",
            Self::Resize { .. } => "terminal_resize",
            Self::ResizeSurface { .. } => "terminal_resize_surface",
        }
    }
}

pub fn estimated_cell_metrics(font: FontSpec) -> CellMetrics {
    CellMetrics {
        width: font.size * 0.6,
        height: font.size * font.line_height,
    }
}

pub fn grid_size(rect: Rect, cell: CellMetrics) -> GridSize {
    let cell_width = cell.width.max(1.0);
    let cell_height = cell.height.max(1.0);
    let available_width = (rect.width as f32 - SCROLLBAR_GUTTER_PX).max(0.0);

    GridSize {
        cols: ((available_width / cell_width).floor() as usize).max(2),
        rows: ((rect.height as f32 / cell_height).floor() as usize).max(1),
    }
}

pub fn initial_plan(config: &TerminalLifecycleConfig) -> Vec<TerminalLifecycleCommand> {
    let cell = estimated_cell_metrics(config.font);
    let grid = grid_size(config.rect, cell);
    let mut commands = vec![
        TerminalLifecycleCommand::UpdateTheme {
            session_id: config.session_id,
        },
        TerminalLifecycleCommand::Create {
            session_id: config.session_id,
            cols: grid.cols,
            rows: grid.rows,
            cell_width: cell.width,
            cell_height: cell.height,
            scrollback_lines: config.scrollback_lines,
            is_local: config.is_local,
        },
        TerminalLifecycleCommand::SetCursorStyle {
            session_id: config.session_id,
            style: config.cursor_style,
        },
    ];

    if config.sync_highlight_rules {
        commands.push(TerminalLifecycleCommand::UpdateHighlightRules {
            session_id: config.session_id,
        });
    }

    commands.push(TerminalLifecycleCommand::UpdateFont {
        session_id: config.session_id,
        font: config.font,
    });
    commands.push(TerminalLifecycleCommand::ResizeSurface {
        session_id: config.session_id,
        surface: config.surface,
    });

    commands
}

pub fn resize_plan(
    session_id: &'static str,
    rect: Rect,
    surface: SurfaceSize,
    cell: CellMetrics,
) -> Vec<TerminalLifecycleCommand> {
    let grid = grid_size(rect, cell);

    vec![
        TerminalLifecycleCommand::Resize {
            session_id,
            cols: grid.cols,
            rows: grid.rows,
            cell_width: cell.width,
            cell_height: cell.height,
        },
        TerminalLifecycleCommand::ResizeSurface {
            session_id,
            surface,
        },
    ]
}
