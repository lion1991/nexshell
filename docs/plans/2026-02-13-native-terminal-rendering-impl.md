# Native Terminal Rendering — Implementation Plan

> ⚠️ **历史文档（已废弃）**：本计划描述的 src-tauri / wgpu + WebView 渲染架构已于 2026-06 整体废弃删除，native-shell-spike（GPUI）成为唯一架构。仅作历史参考。

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace xterm.js with a native GPU-accelerated terminal renderer using `alacritty_terminal` + `crossfont` + HarfBuzz + `wgpu`, while keeping the existing WebView UI overlay (tabs, sidebar, settings, search bar) intact.

**Architecture:** The Tauri window hosts a `wgpu` surface for terminal rendering behind a transparent WebView overlay. `alacritty_terminal` handles VTE parsing and grid management. `crossfont` rasterizes glyphs via platform-native APIs (Core Text on macOS, DirectWrite on Windows). Glyphs are cached in a GPU texture atlas and rendered using `wgpu` shaders. The WebView communicates with the Rust renderer via Tauri IPC commands/events.

**Tech Stack:** Rust 1.92+, Tauri 2 (unstable feature), `alacritty_terminal` 0.24, `crossfont` 0.8, `wgpu` 24, `raw-window-handle`, React 19 (WebView overlay)

---

## Phase 1: Project Setup & Module Scaffolding

### Task 1: Add Dependencies to Cargo.toml

**Files:**
- Modify: `src-tauri/Cargo.toml`

**Step 1: Add new crate dependencies**

Add the following to `[dependencies]` in `Cargo.toml`:

```toml
# Native terminal rendering
alacritty_terminal = "0.24"
crossfont = "0.8"
wgpu = "24"
raw-window-handle = "0.6"
rustybuzz = "0.20"          # Pure-Rust HarfBuzz alternative
lru = "0.12"                # LRU cache for glyph atlas
parking_lot = "0.12"        # Fast mutexes for render thread

# Tauri unstable for raw window handle access
# Update existing tauri line:
# tauri = { version = "2", features = ["unstable"] }
```

Update existing tauri dependency to enable `unstable` feature:
```toml
tauri = { version = "2", features = ["unstable"] }
```

**Step 2: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Compiles successfully (may take a while to download crates)

**Step 3: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "deps: add native terminal rendering crates"
```

---

### Task 2: Create Rust Module Structure

**Files:**
- Create: `src-tauri/src/terminal/mod.rs`
- Create: `src-tauri/src/terminal/term_core.rs`
- Create: `src-tauri/src/terminal/font_engine.rs`
- Create: `src-tauri/src/terminal/renderer.rs`
- Create: `src-tauri/src/terminal/features.rs`
- Create: `src-tauri/src/terminal/bridge.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod terminal;`)

**Step 1: Create module files with placeholder content**

`src-tauri/src/terminal/mod.rs`:
```rust
pub mod term_core;
pub mod font_engine;
pub mod renderer;
pub mod features;
pub mod bridge;
```

Each sub-module gets a minimal placeholder:
```rust
// term_core.rs
//! Terminal emulation core wrapping alacritty_terminal.

// font_engine.rs
//! Font rasterization and glyph atlas management.

// renderer.rs
//! GPU rendering pipeline using wgpu.

// features.rs
//! Custom terminal features: highlight engine, command nav, search.

// bridge.rs
//! Tauri IPC bridge between native renderer and WebView UI.
```

**Step 2: Add module to lib.rs**

Add `mod terminal;` to `src-tauri/src/lib.rs` after existing module declarations.

**Step 3: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Compiles with no errors

**Step 4: Commit**

```bash
git add src-tauri/src/terminal/
git commit -m "scaffold: add native terminal module structure"
```

---

## Phase 2: Terminal Core (`alacritty_terminal` Integration)

### Task 3: Implement EventProxy and TermCore

**Files:**
- Modify: `src-tauri/src/terminal/term_core.rs`

**Context:** `alacritty_terminal::Term` requires an `EventListener` to receive terminal events (title changes, color changes, clipboard operations, bell, etc.). We implement a proxy that forwards these events through an mpsc channel.

**Step 1: Implement EventProxy**

```rust
use std::sync::mpsc;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::vte::ansi::{CursorShape, NamedColor};

/// Events emitted by the terminal that the renderer/bridge need to handle.
#[derive(Debug, Clone)]
pub enum TerminalEvent {
    Title(String),
    ClipboardStore(String),
    ClipboardLoad,
    Bell,
    ColorRequest(usize),
    Wakeup,
    Exit,
}

/// Forwards alacritty_terminal events to an mpsc channel.
#[derive(Clone)]
pub struct EventProxy {
    sender: mpsc::Sender<TerminalEvent>,
}

impl EventProxy {
    pub fn new(sender: mpsc::Sender<TerminalEvent>) -> Self {
        Self { sender }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: TermEvent) {
        let mapped = match event {
            TermEvent::Title(t) => Some(TerminalEvent::Title(t)),
            TermEvent::ClipboardStore(_, text) => Some(TerminalEvent::ClipboardStore(text)),
            TermEvent::ClipboardLoad(_, _) => Some(TerminalEvent::ClipboardLoad),
            TermEvent::Bell => Some(TerminalEvent::Bell),
            TermEvent::Wakeup => Some(TerminalEvent::Wakeup),
            TermEvent::Exit => Some(TerminalEvent::Exit),
            _ => None,
        };
        if let Some(e) = mapped {
            let _ = self.sender.send(e);
        }
    }
}
```

**Step 2: Implement TermCore wrapper**

```rust
use alacritty_terminal::term::SizeInfo;

/// Core terminal state wrapping alacritty_terminal.
pub struct TermCore {
    pub term: Term<EventProxy>,
    pub event_rx: mpsc::Receiver<TerminalEvent>,
    size: SizeInfo,
}

impl TermCore {
    /// Create a new terminal with the given cell dimensions.
    pub fn new(
        cols: u16,
        rows: u16,
        cell_width: f32,
        cell_height: f32,
        scrollback_lines: usize,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let event_proxy = EventProxy::new(tx);

        let size = SizeInfo::new(
            cols as f32 * cell_width,
            rows as f32 * cell_height,
            cell_width,
            cell_height,
            0.0, // padding_x
            0.0, // padding_y
        );

        let config = TermConfig::default();
        // Configure scrollback
        // Note: scrollback is set through TermConfig

        let term = Term::new(config, &size, event_proxy);

        Self {
            term,
            event_rx: rx,
            size,
        }
    }

    /// Write raw bytes from PTY/SSH into the terminal.
    pub fn process_input(&mut self, data: &[u8]) {
        use alacritty_terminal::vte::ansi::Processor;
        let mut processor = Processor::new();
        for byte in data {
            processor.advance(&mut self.term, *byte);
        }
    }

    /// Resize the terminal grid.
    pub fn resize(&mut self, cols: u16, rows: u16, cell_width: f32, cell_height: f32) {
        self.size = SizeInfo::new(
            cols as f32 * cell_width,
            rows as f32 * cell_height,
            cell_width,
            cell_height,
            0.0,
            0.0,
        );
        self.term.resize(self.size);
    }

    /// Get terminal dimensions.
    pub fn size(&self) -> &SizeInfo {
        &self.size
    }

    /// Get the number of columns.
    pub fn cols(&self) -> usize {
        self.size.columns()
    }

    /// Get the number of rows.
    pub fn rows(&self) -> usize {
        self.size.screen_lines()
    }

    /// Drain pending terminal events.
    pub fn drain_events(&self) -> Vec<TerminalEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }
}
```

**Step 3: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Compiles. Note: `alacritty_terminal` API may need adjustments based on exact version — check error messages and adapt `SizeInfo`, `Config`, and `Processor` constructors to match the actual API.

**Important:** The `alacritty_terminal` crate has undergone API changes. If `SizeInfo::new` or `Processor` don't match, consult `alacritty_terminal` 0.24 docs/source. Key potential differences:
- `SizeInfo` may use a builder pattern or `from_dimensions`
- `Processor` may need `Perform` trait instead of passing `&mut Term` directly
- `Term::new` may take different arguments

Fix any API mismatches before proceeding.

**Step 4: Commit**

```bash
git add src-tauri/src/terminal/term_core.rs
git commit -m "feat(terminal): implement TermCore wrapping alacritty_terminal"
```

---

## Phase 3: Font Engine

### Task 4: Implement Font Loading and Metrics

**Files:**
- Modify: `src-tauri/src/terminal/font_engine.rs`

**Context:** `crossfont` provides platform-native font rasterization. We need to:
1. Load a font by family name
2. Calculate cell metrics (width, height, baseline)
3. Rasterize individual glyphs

**Step 1: Implement FontEngine with font loading and metrics**

```rust
use crossfont::{
    FontDesc, FontKey, GlyphKey, Rasterize, RasterizedGlyph, Rasterizer, Size, Slant, Style, Weight,
};
use std::collections::HashMap;

/// Font configuration matching the settings UI.
#[derive(Debug, Clone)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub weight: FontWeight,
    pub weight_bold: FontWeight,
    pub letter_spacing: f32,
    pub line_height: f32,
}

#[derive(Debug, Clone, Copy)]
pub enum FontWeight {
    Normal,
    Medium,    // 500
    SemiBold,  // 600
    Bold,      // 700
    Black,     // 900
}

impl FontWeight {
    pub fn to_crossfont(&self) -> Weight {
        match self {
            FontWeight::Normal => Weight::Normal,
            FontWeight::Medium => Weight::Normal, // crossfont may not have Medium
            FontWeight::SemiBold => Weight::Bold,
            FontWeight::Bold => Weight::Bold,
            FontWeight::Black => Weight::Bold,
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "monospace".to_string(),
            size: 14.0,
            weight: FontWeight::Normal,
            weight_bold: FontWeight::Bold,
            letter_spacing: 0.0,
            line_height: 1.2,
        }
    }
}

/// Cell dimensions derived from font metrics.
#[derive(Debug, Clone, Copy)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
}

/// Manages font loading, metrics, and glyph rasterization.
pub struct FontEngine {
    rasterizer: Rasterizer,
    regular_key: FontKey,
    bold_key: FontKey,
    italic_key: FontKey,
    bold_italic_key: FontKey,
    metrics: CellMetrics,
    config: FontConfig,
}

impl FontEngine {
    /// Create a new font engine with the given configuration.
    /// `dpr` is the display scale factor (e.g. 2.0 for Retina).
    pub fn new(config: FontConfig, dpr: f32) -> Result<Self, String> {
        let font_size = Size::new(config.size);
        let mut rasterizer = Rasterizer::new(dpr).map_err(|e| format!("Rasterizer init: {e}"))?;

        // Load regular font
        let regular_desc = FontDesc::new(
            &config.family,
            Style::Description {
                slant: Slant::Normal,
                weight: config.weight.to_crossfont(),
            },
        );
        let regular_key = rasterizer
            .load_font(&regular_desc, font_size)
            .map_err(|e| format!("Load regular font '{}': {e}", config.family))?;

        // Load bold font
        let bold_desc = FontDesc::new(
            &config.family,
            Style::Description {
                slant: Slant::Normal,
                weight: config.weight_bold.to_crossfont(),
            },
        );
        let bold_key = rasterizer
            .load_font(&bold_desc, font_size)
            .unwrap_or(regular_key);

        // Load italic font
        let italic_desc = FontDesc::new(
            &config.family,
            Style::Description {
                slant: Slant::Italic,
                weight: config.weight.to_crossfont(),
            },
        );
        let italic_key = rasterizer
            .load_font(&italic_desc, font_size)
            .unwrap_or(regular_key);

        // Load bold italic font
        let bold_italic_desc = FontDesc::new(
            &config.family,
            Style::Description {
                slant: Slant::Italic,
                weight: config.weight_bold.to_crossfont(),
            },
        );
        let bold_italic_key = rasterizer
            .load_font(&bold_italic_desc, font_size)
            .unwrap_or(bold_key);

        // Get font metrics
        let font_metrics = rasterizer
            .metrics(regular_key, font_size)
            .map_err(|e| format!("Get metrics: {e}"))?;

        let cell_width = (font_metrics.average_advance + config.letter_spacing).ceil();
        let cell_height = (font_metrics.line_height * config.line_height).ceil();

        let metrics = CellMetrics {
            width: cell_width,
            height: cell_height,
            baseline: font_metrics.descent.abs(),
            underline_position: font_metrics.underline_position,
            underline_thickness: font_metrics.underline_thickness.max(1.0),
        };

        Ok(Self {
            rasterizer,
            regular_key,
            bold_key,
            italic_key,
            bold_italic_key,
            metrics,
            config,
        })
    }

    pub fn metrics(&self) -> &CellMetrics {
        &self.metrics
    }

    pub fn config(&self) -> &FontConfig {
        &self.config
    }

    /// Rasterize a glyph for the given character and style.
    pub fn rasterize_glyph(
        &mut self,
        c: char,
        bold: bool,
        italic: bool,
    ) -> Result<RasterizedGlyph, String> {
        let font_key = match (bold, italic) {
            (false, false) => self.regular_key,
            (true, false) => self.bold_key,
            (false, true) => self.italic_key,
            (true, true) => self.bold_italic_key,
        };

        let glyph_key = GlyphKey {
            font_key,
            c,
            size: Size::new(self.config.size),
        };

        self.rasterizer
            .get_glyph(glyph_key)
            .map_err(|e| format!("Rasterize '{c}': {e}"))
    }
}
```

**Step 2: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Compiles. The `crossfont` API may differ slightly — `FontDesc`, `Style`, `Weight`, `Slant`, `Rasterizer::metrics()` return type, `GlyphKey` fields may need adjustments. Check the actual crossfont 0.8 API.

**Step 3: Commit**

```bash
git add src-tauri/src/terminal/font_engine.rs
git commit -m "feat(terminal): implement FontEngine with crossfont rasterization"
```

---

### Task 5: Implement Glyph Atlas

**Files:**
- Create: `src-tauri/src/terminal/atlas.rs`
- Modify: `src-tauri/src/terminal/mod.rs` (add `pub mod atlas;`)

**Context:** Rasterized glyphs are stored in a texture atlas — a single large GPU texture containing all rendered glyphs. We pack glyphs into rows using a simple shelf-based algorithm and look up UV coordinates when rendering.

**Step 1: Implement GlyphAtlas**

```rust
use lru::LruCache;
use std::num::NonZeroUsize;

/// UV coordinates for a glyph in the atlas texture.
#[derive(Debug, Clone, Copy)]
pub struct GlyphUV {
    /// Top-left UV (0.0-1.0)
    pub u0: f32,
    pub v0: f32,
    /// Bottom-right UV (0.0-1.0)
    pub u1: f32,
    pub v1: f32,
    /// Glyph bearing (offset from cell origin)
    pub bearing_x: f32,
    pub bearing_y: f32,
    /// Pixel dimensions of the glyph
    pub width: u32,
    pub height: u32,
}

/// Key for looking up glyphs in the cache.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct AtlasKey {
    pub c: char,
    pub bold: bool,
    pub italic: bool,
}

/// Shelf-based glyph atlas that packs glyphs into rows.
pub struct GlyphAtlas {
    /// Atlas texture dimensions.
    pub texture_width: u32,
    pub texture_height: u32,
    /// Raw pixel data (RGBA).
    pub pixels: Vec<u8>,
    /// Current packing position.
    cursor_x: u32,
    cursor_y: u32,
    /// Height of the current row (tallest glyph in this row).
    row_height: u32,
    /// LRU cache mapping glyph keys to atlas UV coords.
    cache: LruCache<AtlasKey, GlyphUV>,
    /// Whether the atlas texture has been modified and needs re-upload.
    pub dirty: bool,
}

impl GlyphAtlas {
    /// Create a new atlas with the given texture dimensions.
    /// `max_glyphs` controls the LRU cache capacity.
    pub fn new(texture_width: u32, texture_height: u32, max_glyphs: usize) -> Self {
        let pixel_count = (texture_width * texture_height * 4) as usize;
        Self {
            texture_width,
            texture_height,
            pixels: vec![0u8; pixel_count],
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
            cache: LruCache::new(NonZeroUsize::new(max_glyphs).unwrap()),
            dirty: false,
        }
    }

    /// Look up a glyph in the cache.
    pub fn get(&mut self, key: &AtlasKey) -> Option<&GlyphUV> {
        self.cache.get(key)
    }

    /// Insert a rasterized glyph into the atlas.
    /// Returns None if the atlas is full.
    pub fn insert(
        &mut self,
        key: AtlasKey,
        glyph_pixels: &[u8],   // RGBA or grayscale
        width: u32,
        height: u32,
        bearing_x: f32,
        bearing_y: f32,
        is_grayscale: bool,
    ) -> Option<GlyphUV> {
        if width == 0 || height == 0 {
            // Space or empty glyph — cache with zero-size UV
            let uv = GlyphUV {
                u0: 0.0, v0: 0.0, u1: 0.0, v1: 0.0,
                bearing_x, bearing_y,
                width: 0, height: 0,
            };
            self.cache.put(key, uv);
            return Some(uv);
        }

        // Check if we need to start a new row
        if self.cursor_x + width > self.texture_width {
            self.cursor_x = 0;
            self.cursor_y += self.row_height;
            self.row_height = 0;
        }

        // Check if we've run out of vertical space
        if self.cursor_y + height > self.texture_height {
            return None; // Atlas full
        }

        // Copy glyph pixels into atlas
        for row in 0..height {
            for col in 0..width {
                let atlas_x = self.cursor_x + col;
                let atlas_y = self.cursor_y + row;
                let atlas_idx = ((atlas_y * self.texture_width + atlas_x) * 4) as usize;

                if is_grayscale {
                    let src_idx = (row * width + col) as usize;
                    let alpha = glyph_pixels.get(src_idx).copied().unwrap_or(0);
                    self.pixels[atlas_idx] = 255;     // R
                    self.pixels[atlas_idx + 1] = 255; // G
                    self.pixels[atlas_idx + 2] = 255; // B
                    self.pixels[atlas_idx + 3] = alpha; // A
                } else {
                    let src_idx = ((row * width + col) * 4) as usize;
                    self.pixels[atlas_idx] = glyph_pixels.get(src_idx).copied().unwrap_or(0);
                    self.pixels[atlas_idx + 1] = glyph_pixels.get(src_idx + 1).copied().unwrap_or(0);
                    self.pixels[atlas_idx + 2] = glyph_pixels.get(src_idx + 2).copied().unwrap_or(0);
                    self.pixels[atlas_idx + 3] = glyph_pixels.get(src_idx + 3).copied().unwrap_or(0);
                }
            }
        }

        let tw = self.texture_width as f32;
        let th = self.texture_height as f32;

        let uv = GlyphUV {
            u0: self.cursor_x as f32 / tw,
            v0: self.cursor_y as f32 / th,
            u1: (self.cursor_x + width) as f32 / tw,
            v1: (self.cursor_y + height) as f32 / th,
            bearing_x,
            bearing_y,
            width,
            height,
        };

        self.cursor_x += width + 1; // +1 pixel gap to avoid bleed
        self.row_height = self.row_height.max(height + 1);
        self.dirty = true;
        self.cache.put(key, uv);
        Some(uv)
    }

    /// Clear the atlas (e.g. on font change).
    pub fn clear(&mut self) {
        self.pixels.fill(0);
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.row_height = 0;
        self.cache.clear();
        self.dirty = true;
    }
}
```

**Step 2: Add `pub mod atlas;` to `src-tauri/src/terminal/mod.rs`**

**Step 3: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: PASS

**Step 4: Commit**

```bash
git add src-tauri/src/terminal/atlas.rs src-tauri/src/terminal/mod.rs
git commit -m "feat(terminal): implement shelf-based glyph atlas with LRU cache"
```

---

## Phase 4: GPU Renderer

### Task 6: Implement wgpu Surface Setup

**Files:**
- Modify: `src-tauri/src/terminal/renderer.rs`

**Context:** Initialize wgpu, create a surface from the native window handle, and set up the render pipeline. This is the most complex module. We'll build it incrementally — first just surface setup, then shaders, then the full render loop.

**Step 1: Implement wgpu initialization**

```rust
use wgpu;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// Terminal renderer state.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    // Render pipelines (added in later tasks)
    bg_pipeline: Option<wgpu::RenderPipeline>,
    glyph_pipeline: Option<wgpu::RenderPipeline>,
    atlas_texture: Option<wgpu::Texture>,
    atlas_bind_group: Option<wgpu::BindGroup>,
}

impl Renderer {
    /// Create a new renderer from a window that implements HasWindowHandle.
    pub async fn new<W>(window: &W, width: u32, height: u32) -> Result<Self, String>
    where
        W: HasWindowHandle + HasDisplayHandle,
    {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // Safety: window must outlive the surface (Tauri window lives for app lifetime)
        let surface = instance
            .create_surface(window)
            .map_err(|e| format!("Create surface: {e}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("No suitable GPU adapter found")?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Terminal Renderer"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            }, None)
            .await
            .map_err(|e| format!("Request device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo, // VSync
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            bg_pipeline: None,
            glyph_pipeline: None,
            atlas_texture: None,
            atlas_bind_group: None,
        })
    }

    /// Resize the render surface.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }
}
```

**Step 2: Verify compilation**

Run: `cd src-tauri && cargo check`
Expected: Compiles. Note: `wgpu` 24 API may differ — check `Instance::new`, `create_surface` signature, `SurfaceConfiguration` fields.

**Step 3: Commit**

```bash
git add src-tauri/src/terminal/renderer.rs
git commit -m "feat(terminal): implement wgpu surface initialization"
```

---

### Task 7: Implement Shaders and Render Pipelines

**Files:**
- Create: `src-tauri/src/terminal/shaders/bg.wgsl`
- Create: `src-tauri/src/terminal/shaders/glyph.wgsl`
- Modify: `src-tauri/src/terminal/renderer.rs`

**Step 1: Write background cell shader (WGSL)**

`src-tauri/src/terminal/shaders/bg.wgsl`:
```wgsl
// Background cell shader — renders colored rectangles for cell backgrounds.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

struct Uniforms {
    projection: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.projection * vec4<f32>(in.position, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
```

**Step 2: Write glyph shader (WGSL)**

`src-tauri/src/terminal/shaders/glyph.wgsl`:
```wgsl
// Glyph shader — renders textured quads sampling from the glyph atlas.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) fg_color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fg_color: vec4<f32>,
}

struct Uniforms {
    projection: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(1) @binding(0)
var atlas_texture: texture_2d<f32>;

@group(1) @binding(1)
var atlas_sampler: sampler;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.projection * vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    out.fg_color = in.fg_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(atlas_texture, atlas_sampler, in.uv).a;
    return vec4<f32>(in.fg_color.rgb, in.fg_color.a * alpha);
}
```

**Step 3: Create render pipeline setup in renderer.rs**

Add to `Renderer` impl:

```rust
impl Renderer {
    /// Initialize render pipelines. Call after surface setup.
    pub fn init_pipelines(&mut self) {
        // Background pipeline
        let bg_shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bg_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/bg.wgsl").into()),
        });

        // Uniform buffer layout (shared projection matrix)
        let uniform_bind_group_layout = self.device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("uniform_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            }
        );

        let bg_pipeline_layout = self.device.create_pipeline_layout(
            &wgpu::PipelineLayoutDescriptor {
                label: Some("bg_pipeline_layout"),
                bind_group_layouts: &[&uniform_bind_group_layout],
                push_constant_ranges: &[],
            }
        );

        self.bg_pipeline = Some(self.device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
                label: Some("bg_pipeline"),
                layout: Some(&bg_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &bg_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 24, // 2 floats pos + 4 floats color = 6 * 4
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                            wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x4 },
                        ],
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &bg_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: self.config.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            }
        ));

        // Glyph pipeline (similar, with texture binding)
        // ... (glyph pipeline setup follows same pattern with atlas texture bind group)
    }
}
```

**Step 4: Verify compilation**

Run: `cd src-tauri && cargo check`

**Step 5: Commit**

```bash
git add src-tauri/src/terminal/shaders/ src-tauri/src/terminal/renderer.rs
git commit -m "feat(terminal): add WGSL shaders and render pipeline setup"
```

---

### Task 8: Implement Frame Rendering

**Files:**
- Modify: `src-tauri/src/terminal/renderer.rs`

**Context:** Build vertex buffers from terminal grid state, upload to GPU, and execute render passes.

**Step 1: Define vertex types and frame builder**

```rust
/// Vertex for background rectangles.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BgVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

/// Vertex for glyph quads.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GlyphVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub fg_color: [f32; 4],
}
```

Note: Add `bytemuck = { version = "1", features = ["derive"] }` to Cargo.toml.

**Step 2: Implement `render_frame` method**

```rust
impl Renderer {
    /// Render a single frame: backgrounds, then glyphs, then cursor.
    pub fn render_frame(
        &self,
        bg_vertices: &[BgVertex],
        glyph_vertices: &[GlyphVertex],
        clear_color: wgpu::Color,
    ) -> Result<(), String> {
        let output = self.surface
            .get_current_texture()
            .map_err(|e| format!("Get surface texture: {e}"))?;

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("frame_encoder") }
        );

        // Create temporary buffers for this frame
        if !bg_vertices.is_empty() {
            let bg_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("bg_vertices"),
                contents: bytemuck::cast_slice(bg_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

            // Background pass
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bg_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            if let Some(pipeline) = &self.bg_pipeline {
                pass.set_pipeline(pipeline);
                pass.set_vertex_buffer(0, bg_buffer.slice(..));
                pass.draw(0..bg_vertices.len() as u32, 0..1);
            }
        }

        // Glyph pass (load existing, don't clear)
        if !glyph_vertices.is_empty() {
            // Similar pattern for glyph vertices...
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}
```

Note: Add `wgpu = { version = "24", features = ["wgsl"] }` and `bytemuck` dependency.

**Step 3: Verify compilation**

Run: `cd src-tauri && cargo check`

**Step 4: Commit**

```bash
git add src-tauri/src/terminal/renderer.rs src-tauri/Cargo.toml
git commit -m "feat(terminal): implement frame rendering with vertex buffers"
```

---

## Phase 5: Tauri Window Integration

### Task 9: Set Up Bare Window + Transparent WebView Overlay

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tauri.conf.json`

**Context:** This is the critical integration point. We need to:
1. Create a Tauri window without a default webview
2. Attach a wgpu surface to the native window
3. Add a transparent webview overlay for UI chrome
4. Route input events correctly

**Step 1: Update tauri.conf.json**

Remove the default `windows` array — we'll create windows programmatically:

```json
{
  "app": {
    "windows": []
  }
}
```

**Step 2: Modify lib.rs to create window programmatically**

```rust
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

// In the setup closure:
.setup(|app| {
    // ... existing DB and session manager setup ...

    // Create the main window with a webview
    // For now, keep the standard webview approach.
    // We'll migrate to bare window + overlay in a later task
    // once the renderer is proven to work.
    let _window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("iTerm")
        .inner_size(1200.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .build()
        .expect("failed to create window");

    Ok(())
})
```

**Important design decision:** Rather than immediately switching to the bare window + overlay architecture (which is high-risk), we'll first build and test the renderer independently, then integrate it into the window. This task establishes the programmatic window creation pattern. The actual bare-window switch happens in Task 12.

**Step 3: Verify the app still launches**

Run: `cd src-tauri && cargo tauri dev`
Expected: App launches normally with the existing WebView UI.

**Step 4: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/tauri.conf.json
git commit -m "refactor: create window programmatically for native renderer prep"
```

---

### Task 10: Implement Terminal Session → TermCore Data Flow

**Files:**
- Modify: `src-tauri/src/commands/session.rs`
- Modify: `src-tauri/src/terminal/term_core.rs`

**Context:** Currently, PTY/SSH output is sent to the frontend via Tauri events, where xterm.js processes it. With native rendering, output should instead be fed into `alacritty_terminal::Term`. This task redirects the data flow.

We introduce a `NativeTerminalManager` that holds `TermCore` instances keyed by session ID. When a session produces output, it goes into the TermCore instead of being emitted to the WebView.

**Step 1: Create NativeTerminalManager**

Add to `src-tauri/src/terminal/term_core.rs`:

```rust
use std::collections::HashMap;
use parking_lot::Mutex;
use std::sync::Arc;

/// Manages all native terminal instances.
pub struct NativeTerminalManager {
    terminals: Mutex<HashMap<String, Arc<Mutex<TermCore>>>>,
}

impl NativeTerminalManager {
    pub fn new() -> Self {
        Self {
            terminals: Mutex::new(HashMap::new()),
        }
    }

    /// Create a new terminal for the given session.
    pub fn create(
        &self,
        session_id: String,
        cols: u16,
        rows: u16,
        cell_width: f32,
        cell_height: f32,
        scrollback: usize,
    ) -> Arc<Mutex<TermCore>> {
        let term = TermCore::new(cols, rows, cell_width, cell_height, scrollback);
        let arc = Arc::new(Mutex::new(term));
        self.terminals.lock().insert(session_id, arc.clone());
        arc
    }

    /// Get a terminal by session ID.
    pub fn get(&self, session_id: &str) -> Option<Arc<Mutex<TermCore>>> {
        self.terminals.lock().get(session_id).cloned()
    }

    /// Remove a terminal when session closes.
    pub fn remove(&self, session_id: &str) {
        self.terminals.lock().remove(session_id);
    }
}
```

**Step 2: Register NativeTerminalManager as Tauri state**

In `lib.rs` setup:
```rust
use crate::terminal::term_core::NativeTerminalManager;

// In setup:
app.manage(NativeTerminalManager::new());
```

**Step 3: Modify session output flow**

In `commands/session.rs`, the reader task currently does:
```rust
let text = encode_pty_output(&data);
let _ = reader_app.emit(&output_event, &text);
```

Add a parallel path that feeds data into the native terminal:
```rust
// After existing emit, also feed into native terminal
if let Some(term) = native_mgr.get(&reader_session_id) {
    term.lock().process_input(&data);
}
```

The `native_mgr` needs to be cloned into the reader task. For now, use `app.state::<NativeTerminalManager>()`.

**Step 4: Verify compilation and existing behavior unbroken**

Run: `cd src-tauri && cargo tauri dev`
Expected: App works as before (xterm.js still active). Data is now also flowing into TermCore.

**Step 5: Commit**

```bash
git add src-tauri/src/terminal/term_core.rs src-tauri/src/commands/session.rs src-tauri/src/lib.rs
git commit -m "feat(terminal): add NativeTerminalManager and dual output path"
```

---

## Phase 6: IPC Bridge

### Task 11: Implement Tauri Commands for Native Terminal

**Files:**
- Modify: `src-tauri/src/terminal/bridge.rs`
- Modify: `src-tauri/src/lib.rs` (register commands)

**Context:** The WebView needs to communicate with the native terminal for operations like copy, paste, search, resize, etc. We expose these as Tauri commands.

**Step 1: Implement bridge commands**

```rust
use tauri::{AppHandle, State};
use crate::terminal::term_core::NativeTerminalManager;

/// Get the current terminal content as text (for copy/select operations).
#[tauri::command]
pub fn terminal_get_selection(
    manager: State<'_, NativeTerminalManager>,
    session_id: String,
) -> Result<String, String> {
    let term = manager.get(&session_id).ok_or("Terminal not found")?;
    let term = term.lock();
    // Access selection from alacritty_terminal
    // Implementation depends on selection state tracking
    Ok(String::new()) // Placeholder
}

/// Clear the terminal screen.
#[tauri::command]
pub fn terminal_clear(
    manager: State<'_, NativeTerminalManager>,
    session_id: String,
) -> Result<(), String> {
    let term = manager.get(&session_id).ok_or("Terminal not found")?;
    let mut term = term.lock();
    // Clear visible area
    Ok(())
}

/// Update terminal settings (font, theme, etc.).
#[tauri::command]
pub fn terminal_update_settings(
    session_id: String,
    settings_json: String,
) -> Result<(), String> {
    // Parse settings and update renderer configuration
    Ok(())
}

/// Notify native renderer of layout changes from WebView.
#[tauri::command]
pub fn terminal_resize_viewport(
    manager: State<'_, NativeTerminalManager>,
    session_id: String,
    width: u32,
    height: u32,
) -> Result<(), String> {
    // Resize the renderer surface and terminal grid
    Ok(())
}

/// Search the terminal content.
#[tauri::command]
pub fn terminal_find(
    manager: State<'_, NativeTerminalManager>,
    session_id: String,
    query: String,
    case_sensitive: bool,
    regex: bool,
    whole_word: bool,
    direction: String, // "next" or "previous"
) -> Result<bool, String> {
    // Search through the terminal grid
    Ok(false) // Placeholder
}

/// Clear search highlights.
#[tauri::command]
pub fn terminal_clear_search(
    session_id: String,
) -> Result<(), String> {
    Ok(())
}
```

**Step 2: Register commands in lib.rs**

Add to `invoke_handler`:
```rust
// Native terminal
commands::terminal_bridge::terminal_get_selection,
commands::terminal_bridge::terminal_clear,
commands::terminal_bridge::terminal_update_settings,
commands::terminal_bridge::terminal_resize_viewport,
commands::terminal_bridge::terminal_find,
commands::terminal_bridge::terminal_clear_search,
```

Wait — the bridge module is in `terminal/bridge.rs`, not `commands/`. We need to decide on the module structure. Since these are Tauri commands, it's cleaner to have them in the `commands/` directory. Create a new file:

- Create: `src-tauri/src/commands/terminal_bridge.rs`
- Modify: `src-tauri/src/commands/mod.rs` (add `pub mod terminal_bridge;`)

**Step 3: Verify compilation**

Run: `cd src-tauri && cargo check`

**Step 4: Commit**

```bash
git add src-tauri/src/commands/terminal_bridge.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(terminal): add Tauri IPC bridge commands for native terminal"
```

---

## Phase 7: Render Loop Integration

### Task 12: Wire Up TermCore → Renderer → Screen

**Files:**
- Modify: `src-tauri/src/terminal/renderer.rs`
- Create: `src-tauri/src/terminal/render_loop.rs`
- Modify: `src-tauri/src/terminal/mod.rs`

**Context:** This task connects everything: read the terminal grid from `TermCore`, build vertex buffers, and render via `wgpu`. This is where the terminal content actually appears on screen.

**Step 1: Implement grid-to-vertices conversion**

```rust
use crate::terminal::term_core::TermCore;
use crate::terminal::font_engine::{FontEngine, CellMetrics};
use crate::terminal::atlas::{GlyphAtlas, AtlasKey};
use crate::terminal::renderer::{BgVertex, GlyphVertex};

/// Theme colors for rendering.
pub struct RenderTheme {
    pub background: [f32; 4],
    pub foreground: [f32; 4],
    pub cursor: [f32; 4],
    pub selection: [f32; 4],
    /// ANSI colors: [black, red, green, yellow, blue, magenta, cyan, white,
    ///               bright_black, bright_red, ... bright_white]
    pub ansi: [[f32; 4]; 16],
}

/// Build vertex buffers from the terminal grid.
pub fn build_frame(
    term: &TermCore,
    font_engine: &mut FontEngine,
    atlas: &mut GlyphAtlas,
    theme: &RenderTheme,
    metrics: &CellMetrics,
) -> (Vec<BgVertex>, Vec<GlyphVertex>) {
    let mut bg_verts = Vec::new();
    let mut glyph_verts = Vec::new();

    let content = term.term.renderable_content();

    for cell in content.display_iter {
        let col = cell.point.column.0 as f32;
        let row = cell.point.line.0 as f32;

        let x = col * metrics.width;
        let y = row * metrics.height;

        // Resolve background color
        let bg_color = resolve_color(&cell.bg, theme);

        // Background quad (two triangles)
        let x0 = x;
        let y0 = y;
        let x1 = x + metrics.width;
        let y1 = y + metrics.height;

        bg_verts.extend_from_slice(&[
            BgVertex { position: [x0, y0], color: bg_color },
            BgVertex { position: [x1, y0], color: bg_color },
            BgVertex { position: [x0, y1], color: bg_color },
            BgVertex { position: [x1, y0], color: bg_color },
            BgVertex { position: [x1, y1], color: bg_color },
            BgVertex { position: [x0, y1], color: bg_color },
        ]);

        // Glyph rendering
        let c = cell.c;
        if c == ' ' || c == '\t' {
            continue; // No glyph needed for whitespace
        }

        let bold = cell.flags.contains(alacritty_terminal::term::cell::Flags::BOLD);
        let italic = cell.flags.contains(alacritty_terminal::term::cell::Flags::ITALIC);
        let fg_color = resolve_color(&cell.fg, theme);

        let atlas_key = AtlasKey { c, bold, italic };

        let uv = if let Some(uv) = atlas.get(&atlas_key) {
            *uv
        } else {
            // Cache miss — rasterize and insert
            match font_engine.rasterize_glyph(c, bold, italic) {
                Ok(glyph) => {
                    let is_grayscale = glyph.buf.len() == (glyph.width * glyph.height) as usize;
                    atlas.insert(
                        atlas_key,
                        &glyph.buf,
                        glyph.width as u32,
                        glyph.height as u32,
                        glyph.left as f32,
                        glyph.top as f32,
                        is_grayscale,
                    ).unwrap_or_default()
                }
                Err(_) => continue,
            }
        };

        if uv.width == 0 || uv.height == 0 {
            continue;
        }

        // Glyph quad with bearing offset
        let gx0 = x + uv.bearing_x;
        let gy0 = y + metrics.height - metrics.baseline - uv.bearing_y;
        let gx1 = gx0 + uv.width as f32;
        let gy1 = gy0 + uv.height as f32;

        glyph_verts.extend_from_slice(&[
            GlyphVertex { position: [gx0, gy0], uv: [uv.u0, uv.v0], fg_color },
            GlyphVertex { position: [gx1, gy0], uv: [uv.u1, uv.v0], fg_color },
            GlyphVertex { position: [gx0, gy1], uv: [uv.u0, uv.v1], fg_color },
            GlyphVertex { position: [gx1, gy0], uv: [uv.u1, uv.v0], fg_color },
            GlyphVertex { position: [gx1, gy1], uv: [uv.u1, uv.v1], fg_color },
            GlyphVertex { position: [gx0, gy1], uv: [uv.u0, uv.v1], fg_color },
        ]);
    }

    (bg_verts, glyph_verts)
}

fn resolve_color(color: &alacritty_terminal::vte::ansi::Color, theme: &RenderTheme) -> [f32; 4] {
    use alacritty_terminal::vte::ansi::Color;
    match color {
        Color::Named(named) => {
            let idx = *named as usize;
            if idx < 16 { theme.ansi[idx] } else { theme.foreground }
        }
        Color::Spec(rgb) => {
            [rgb.r as f32 / 255.0, rgb.g as f32 / 255.0, rgb.b as f32 / 255.0, 1.0]
        }
        Color::Indexed(idx) => {
            if (*idx as usize) < 16 {
                theme.ansi[*idx as usize]
            } else {
                // 256-color lookup (6x6x6 cube + grayscale ramp)
                ansi_256_to_rgb(*idx)
            }
        }
    }
}

fn ansi_256_to_rgb(idx: u8) -> [f32; 4] {
    if idx < 16 {
        return [0.5, 0.5, 0.5, 1.0]; // Shouldn't reach here
    }
    if idx < 232 {
        // 6x6x6 color cube
        let idx = idx - 16;
        let r = (idx / 36) % 6;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        let to_f = |v: u8| if v == 0 { 0.0 } else { (55.0 + 40.0 * v as f32) / 255.0 };
        [to_f(r), to_f(g), to_f(b), 1.0]
    } else {
        // Grayscale ramp (232-255)
        let v = (8 + 10 * (idx - 232)) as f32 / 255.0;
        [v, v, v, 1.0]
    }
}
```

**Step 2: Implement the main render loop**

`src-tauri/src/terminal/render_loop.rs`:

```rust
use std::sync::Arc;
use parking_lot::Mutex;

use crate::terminal::term_core::TermCore;
use crate::terminal::font_engine::FontEngine;
use crate::terminal::atlas::GlyphAtlas;
use crate::terminal::renderer::Renderer;

/// Owns all rendering state for one terminal session and drives the render loop.
pub struct RenderLoop {
    renderer: Renderer,
    font_engine: FontEngine,
    atlas: GlyphAtlas,
    term: Arc<Mutex<TermCore>>,
    theme: super::render_loop::RenderTheme, // Will be in this module
    running: Arc<std::sync::atomic::AtomicBool>,
}

// The render loop runs on a dedicated thread. On each wakeup:
// 1. Lock the TermCore
// 2. Build vertex buffers from grid
// 3. Upload atlas if dirty
// 4. Render frame
// 5. Present

// This is started by the bridge when a session is created and uses
// native rendering.
```

**Step 3: Verify compilation**

Run: `cd src-tauri && cargo check`

**Step 4: Commit**

```bash
git add src-tauri/src/terminal/render_loop.rs src-tauri/src/terminal/mod.rs
git commit -m "feat(terminal): implement grid-to-vertices conversion and render loop"
```

---

## Phase 8: Feature Migration (Rust)

### Task 13: Implement Highlight Engine in Rust

**Files:**
- Modify: `src-tauri/src/terminal/features.rs`

**Context:** Port the TypeScript highlight engine (`src/lib/highlightEngine.ts`) to Rust. The Rust version operates on the terminal grid cells directly rather than on raw ANSI text, which is more efficient.

**Step 1: Implement the Rust highlight engine**

```rust
use regex::Regex;

/// A compiled highlight rule.
pub struct HighlightRule {
    pub id: String,
    pub name: String,
    pub regex: Regex,
    pub color: [f32; 4],  // RGBA
    pub priority: u32,     // Lower = higher priority
    pub enabled: bool,
}

/// A match found by the highlight engine.
pub struct HighlightMatch {
    pub row: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub color: [f32; 4],
}

/// Performance configuration for the highlight engine.
pub struct HighlightPerfConfig {
    pub max_line_length: usize,
    pub max_decorations: usize,
    pub skip_alt_buffer: bool,
}

/// Find all highlight matches in the visible terminal grid.
pub fn find_highlights(
    // Takes the visible grid text, row by row
    visible_lines: &[String],
    rules: &[HighlightRule],
    config: &HighlightPerfConfig,
) -> Vec<HighlightMatch> {
    let mut matches = Vec::new();

    for (row_idx, line) in visible_lines.iter().enumerate() {
        if line.len() > config.max_line_length {
            continue;
        }

        for rule in rules {
            if !rule.enabled { continue; }
            if matches.len() >= config.max_decorations { return matches; }

            for m in rule.regex.find_iter(line) {
                matches.push(HighlightMatch {
                    row: row_idx,
                    col_start: m.start(),
                    col_end: m.end(),
                    color: rule.color,
                });

                if matches.len() >= config.max_decorations { return matches; }
            }
        }
    }

    // Resolve overlaps: sort by position, then priority
    matches.sort_by(|a, b| {
        a.row.cmp(&b.row)
            .then(a.col_start.cmp(&b.col_start))
    });

    matches
}
```

Note: Add `regex = "1"` to Cargo.toml dependencies.

**Step 2: Verify compilation**

Run: `cd src-tauri && cargo check`

**Step 3: Commit**

```bash
git add src-tauri/src/terminal/features.rs src-tauri/Cargo.toml
git commit -m "feat(terminal): port highlight engine to Rust"
```

---

### Task 14: Implement Search in Rust

**Files:**
- Create: `src-tauri/src/terminal/search.rs`
- Modify: `src-tauri/src/terminal/mod.rs`

**Context:** Port the xterm.js SearchAddon functionality. Search needs to scan the terminal grid (visible + scrollback) for matches and return their positions.

**Step 1: Implement terminal search**

```rust
use regex::Regex;

/// A search match in the terminal.
#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub row: i32,       // Absolute row (negative = scrollback)
    pub col_start: u32,
    pub col_end: u32,
}

pub struct TerminalSearch {
    matches: Vec<SearchMatch>,
    active_index: Option<usize>,
    last_query: String,
}

impl TerminalSearch {
    pub fn new() -> Self {
        Self {
            matches: Vec::new(),
            active_index: None,
            last_query: String::new(),
        }
    }

    /// Search the terminal grid for the given query.
    /// Returns the number of matches found.
    pub fn find(
        &mut self,
        grid_lines: &[(i32, String)], // (row_index, text)
        query: &str,
        case_sensitive: bool,
        is_regex: bool,
        whole_word: bool,
    ) -> usize {
        self.matches.clear();
        self.active_index = None;
        self.last_query = query.to_string();

        if query.is_empty() { return 0; }

        let pattern = if is_regex {
            if case_sensitive { query.to_string() }
            else { format!("(?i){query}") }
        } else {
            let escaped = regex::escape(query);
            let pat = if whole_word { format!("\\b{escaped}\\b") } else { escaped };
            if case_sensitive { pat } else { format!("(?i){pat}") }
        };

        let re = match Regex::new(&pattern) {
            Ok(r) => r,
            Err(_) => return 0,
        };

        for (row_idx, line) in grid_lines {
            for m in re.find_iter(line) {
                self.matches.push(SearchMatch {
                    row: *row_idx,
                    col_start: m.start() as u32,
                    col_end: m.end() as u32,
                });
            }
        }

        if !self.matches.is_empty() {
            self.active_index = Some(0);
        }

        self.matches.len()
    }

    /// Move to the next match. Returns the match if found.
    pub fn find_next(&mut self) -> Option<&SearchMatch> {
        if self.matches.is_empty() { return None; }
        let idx = self.active_index.map(|i| (i + 1) % self.matches.len()).unwrap_or(0);
        self.active_index = Some(idx);
        Some(&self.matches[idx])
    }

    /// Move to the previous match.
    pub fn find_previous(&mut self) -> Option<&SearchMatch> {
        if self.matches.is_empty() { return None; }
        let idx = self.active_index.map(|i| {
            if i == 0 { self.matches.len() - 1 } else { i - 1 }
        }).unwrap_or(0);
        self.active_index = Some(idx);
        Some(&self.matches[idx])
    }

    /// Get all matches (for rendering decorations).
    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    /// Get the active match index.
    pub fn active_index(&self) -> Option<usize> {
        self.active_index
    }

    /// Clear search state.
    pub fn clear(&mut self) {
        self.matches.clear();
        self.active_index = None;
        self.last_query.clear();
    }
}
```

**Step 2: Add `pub mod search;` to mod.rs**

**Step 3: Verify compilation**

Run: `cd src-tauri && cargo check`

**Step 4: Commit**

```bash
git add src-tauri/src/terminal/search.rs src-tauri/src/terminal/mod.rs
git commit -m "feat(terminal): implement terminal search with regex support"
```

---

### Task 15: Implement Command Navigation (OSC 133) in Rust

**Files:**
- Create: `src-tauri/src/terminal/command_nav.rs`
- Modify: `src-tauri/src/terminal/mod.rs`

**Context:** Port `useCommandNav.ts`. Detect OSC 133 shell integration sequences in the terminal output stream and track command boundaries.

**Step 1: Implement CommandNav**

```rust
/// A detected command in the terminal.
#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub prompt_line: usize,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub timestamp: u64,
}

/// Tracks command boundaries via OSC 133 sequences.
pub struct CommandNav {
    commands: Vec<CommandEntry>,
    current_line: usize,
    pending: Option<PartialCommand>,
    enabled: bool,
}

struct PartialCommand {
    prompt_line: usize,
    command: Option<String>,
    timestamp: u64,
}

impl CommandNav {
    pub fn new(enabled: bool) -> Self {
        Self {
            commands: Vec::new(),
            current_line: 0,
            pending: None,
            enabled,
        }
    }

    /// Process a chunk of terminal output, scanning for OSC 133 markers.
    pub fn process_output(&mut self, data: &[u8]) {
        if !self.enabled { return; }

        let text = String::from_utf8_lossy(data);

        // Count newlines
        let newlines = text.chars().filter(|&c| c == '\n').count();

        // OSC 133;A — Prompt start
        if text.contains("\x1b]133;A") {
            self.pending = Some(PartialCommand {
                prompt_line: self.current_line,
                command: None,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            });
        }

        // OSC 133;D;N — Command done with exit code
        if let Some(captures) = find_osc133d(&text) {
            if let Some(pending) = self.pending.take() {
                self.commands.push(CommandEntry {
                    prompt_line: pending.prompt_line,
                    command: pending.command,
                    exit_code: Some(captures),
                    timestamp: pending.timestamp,
                });
            }
        }

        self.current_line += newlines;
    }

    pub fn commands(&self) -> &[CommandEntry] {
        &self.commands
    }

    pub fn clear(&mut self) {
        self.commands.clear();
        self.current_line = 0;
        self.pending = None;
    }
}

fn find_osc133d(text: &str) -> Option<i32> {
    // Look for \x1b]133;D;N where N is the exit code
    let marker = "\x1b]133;D";
    if let Some(pos) = text.find(marker) {
        let rest = &text[pos + marker.len()..];
        if rest.starts_with(';') {
            let num_str: String = rest[1..].chars().take_while(|c| c.is_ascii_digit()).collect();
            num_str.parse().ok()
        } else {
            Some(0) // No exit code means success
        }
    } else {
        None
    }
}
```

**Step 2: Add to mod.rs**

**Step 3: Verify and commit**

```bash
git add src-tauri/src/terminal/command_nav.rs src-tauri/src/terminal/mod.rs
git commit -m "feat(terminal): implement OSC 133 command navigation"
```

---

## Phase 9: Window Integration (Bare Window + Overlay)

### Task 16: Implement Bare Window with wgpu Surface and WebView Overlay

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tauri.conf.json`

**Context:** This is the critical integration step. We create a Tauri window without a default webview, attach a wgpu surface, then add a transparent webview overlay for UI chrome.

**Important:** This requires `tauri = { version = "2", features = ["unstable"] }` for multi-webview / raw window handle access.

**Step 1: Implement bare window creation**

```rust
use tauri::window::WindowBuilder;
use tauri::webview::WebviewBuilder;

.setup(|app| {
    // ... existing setup ...

    // Create a bare window (no default webview)
    let window = WindowBuilder::new(app, "main")
        .title("iTerm")
        .inner_size(1200.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .build()
        .expect("failed to create window");

    // Get raw window handle for wgpu
    // The wgpu surface will be created in the render thread

    // Add transparent webview overlay
    let webview = window.add_child(
        WebviewBuilder::new("main-ui", WebviewUrl::App("index.html".into()))
            .transparent(true),
        tauri::LogicalPosition::new(0, 0),
        window.inner_size().unwrap(),
    ).expect("failed to create webview overlay");

    // Store window reference for the render thread
    app.manage(window.clone());

    Ok(())
})
```

**Note:** The exact Tauri 2 `unstable` API for `WindowBuilder` (bare, without webview) and `add_child` webview may differ. Check `tauri` 2.x docs for:
- `WindowBuilder::new()` vs `Window::builder()`
- `window.add_child()` or `WebviewBuilder` attachment methods
- Transparent webview support per platform

**Step 2: Start the wgpu render thread**

After window creation, spawn a thread that:
1. Creates the wgpu surface from the window handle
2. Initializes the renderer
3. Runs the render loop (waiting for wakeup events from TermCore)

```rust
let window_handle = window.clone();
std::thread::spawn(move || {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let renderer = Renderer::new(&window_handle, width, height).await?;
        // ... render loop ...
    });
});
```

**Step 3: Handle input routing**

Keyboard events from the native window need to be:
- Captured before the WebView sees them (when terminal has focus)
- Converted to terminal input bytes
- Sent to the PTY/SSH session

Mouse events in the terminal area:
- Handled natively for selection
- Right-click triggers context menu via WebView

This requires registering window event listeners in Tauri.

**Step 4: Verify basic rendering**

Run: `cd src-tauri && cargo tauri dev`
Expected: Window opens, terminal area shows rendered content from wgpu, UI chrome (tabs, sidebar) overlays on top.

**Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/tauri.conf.json
git commit -m "feat(terminal): integrate bare window with wgpu surface and webview overlay"
```

---

## Phase 10: Frontend Adaptation

### Task 17: Create NativeTerminal React Component

**Files:**
- Create: `src/components/terminal/NativeTerminal.tsx`
- Modify: `src/components/terminal/Terminal.tsx` (keep as fallback)

**Context:** The new `NativeTerminal` component doesn't render a canvas — the terminal is rendered natively by wgpu. This component only handles:
- Communicating with the native renderer via Tauri IPC
- Passing settings changes
- Forwarding search commands

**Step 1: Create NativeTerminal component**

```tsx
import { useEffect, useImperativeHandle, forwardRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSettings } from "../../hooks/useSettings";
import type { TerminalHandle } from "./Terminal";

interface NativeTerminalProps {
  sessionId: string;
  active: boolean;
  onStatusChange: (sessionId: string, status: string) => void;
  onContextMenu?: (e: React.MouseEvent) => void;
}

export const NativeTerminal = forwardRef<TerminalHandle, NativeTerminalProps>(
  function NativeTerminal({ sessionId, active, onStatusChange, onContextMenu }, ref) {
    const { settings } = useSettings();

    useImperativeHandle(ref, () => ({
      async copy() {
        await invoke("terminal_get_selection", { sessionId });
      },
      async paste() {
        // Handled natively
      },
      selectAll() {
        invoke("terminal_select_all", { sessionId });
      },
      clear() {
        invoke("terminal_clear", { sessionId });
      },
      focus() {
        invoke("terminal_focus", { sessionId });
      },
      hasSelection() {
        return false; // Checked via IPC if needed
      },
      findNext(term, options) {
        return invoke("terminal_find", {
          sessionId,
          query: term,
          caseSensitive: options?.caseSensitive ?? false,
          regex: options?.regex ?? false,
          wholeWord: options?.wholeWord ?? false,
          direction: "next",
        }).then(() => true).catch(() => false);
      },
      findPrevious(term, options) {
        return invoke("terminal_find", {
          sessionId,
          query: term,
          caseSensitive: options?.caseSensitive ?? false,
          regex: options?.regex ?? false,
          wholeWord: options?.wholeWord ?? false,
          direction: "previous",
        }).then(() => true).catch(() => false);
      },
      clearSearch() {
        invoke("terminal_clear_search", { sessionId });
      },
    }));

    // Sync settings to native renderer
    useEffect(() => {
      invoke("terminal_update_settings", {
        sessionId,
        settingsJson: JSON.stringify(settings.terminal),
      });
    }, [settings.terminal, sessionId]);

    // The terminal area is rendered by wgpu behind this webview.
    // This div is transparent and just captures context menu events.
    return (
      <div
        className="terminal-wrapper"
        style={{ width: "100%", height: "100%", background: "transparent" }}
        onContextMenu={onContextMenu}
      />
    );
  }
);
```

**Step 2: Add a feature flag to switch between renderers**

In `useSettings.ts`, add:
```typescript
// In TerminalSettings interface:
nativeRenderer: boolean;  // default: false for safe rollout
```

**Step 3: Update the parent component to conditionally use NativeTerminal**

Where `<Terminal>` is used, conditionally render `<NativeTerminal>` when `settings.terminal.nativeRenderer` is true.

**Step 4: Verify both paths work**

Run: `cd src-tauri && cargo tauri dev`
Expected: With `nativeRenderer: false`, the old xterm.js renderer works. With `nativeRenderer: true`, the native renderer is used.

**Step 5: Commit**

```bash
git add src/components/terminal/NativeTerminal.tsx src/hooks/useSettings.ts
git commit -m "feat: add NativeTerminal component with feature flag"
```

---

### Task 18: Remove xterm.js (After Native Renderer Is Stable)

**Files:**
- Modify: `package.json` (remove xterm dependencies)
- Delete: `src/components/terminal/Terminal.tsx` (old xterm.js component)
- Modify: parent component to always use `NativeTerminal`

**This task should only be done after the native renderer is fully stable and all features are verified.**

**Step 1: Remove xterm.js packages**

```bash
pnpm remove @xterm/xterm @xterm/addon-fit @xterm/addon-search @xterm/addon-web-links @xterm/addon-webgl
```

**Step 2: Delete the old Terminal component**

Remove `src/components/terminal/Terminal.tsx`.

**Step 3: Remove feature flag and make NativeTerminal the default**

Remove `nativeRenderer` setting. Update all references.

**Step 4: Remove IPC encoding workaround**

The `encode_pty_output` function in `commands/session.rs` that replaces ESC with U+E000 for WKWebView compatibility is no longer needed — data goes directly to `alacritty_terminal` now. Remove it and the frontend `decodeIpc` function.

**Step 5: Clean up unused TypeScript code**

- Remove `src/lib/highlightEngine.ts` (now in Rust)
- Remove `src/hooks/useCommandNav.ts` (now in Rust)
- Remove highlight types if only used by the old engine
- Keep `src/hooks/useHighlight.ts` (still manages rule state for settings UI, but sends rules to Rust)

**Step 6: Verify everything works**

Run: `cd src-tauri && cargo tauri dev`
Expected: Full app works with native renderer only.

**Step 7: Commit**

```bash
git add -A
git commit -m "refactor: remove xterm.js, use native terminal renderer exclusively"
```

---

## Phase 11: Selection, Clipboard, and Input Handling

### Task 19: Implement Selection and Clipboard

**Files:**
- Create: `src-tauri/src/terminal/selection.rs`
- Modify: `src-tauri/src/terminal/mod.rs`

**Context:** Mouse-based text selection in the terminal area is handled natively. `alacritty_terminal` already has selection state management — we need to wire it up to mouse events and the system clipboard.

Selection types:
- Click-drag: character selection
- Double-click: word selection
- Triple-click: line selection
- Shift+click: extend selection

**Step 1: Implement selection handling**

Use `alacritty_terminal::selection::Selection` which is built into the `Term` struct. The renderer needs to:
1. Track mouse state (pressed, position)
2. Convert pixel coordinates to grid coordinates
3. Update `Term`'s selection state
4. Read selected text for clipboard operations

```rust
use alacritty_terminal::index::Point;
use alacritty_terminal::selection::{Selection, SelectionType};

/// Convert pixel position to terminal grid point.
pub fn pixel_to_grid(
    x: f32, y: f32,
    cell_width: f32, cell_height: f32,
    cols: usize, rows: usize,
) -> Point {
    let col = (x / cell_width).min(cols as f32 - 1.0).max(0.0) as usize;
    let row = (y / cell_height).min(rows as f32 - 1.0).max(0.0) as i32;
    Point::new(alacritty_terminal::index::Line(row), alacritty_terminal::index::Column(col))
}
```

Selection text extraction uses `Term::selection_to_string()`.

**Step 2: Clipboard integration**

Use `tauri-plugin-clipboard-manager` from Rust side:
```rust
// Read
let text = app.clipboard().read_text().unwrap_or_default();
// Write
app.clipboard().write_text(&selected_text);
```

**Step 3: Verify and commit**

---

### Task 20: Implement Keyboard Input Routing

**Files:**
- Modify: `src-tauri/src/terminal/bridge.rs`

**Context:** Keyboard events from the Tauri window need to be:
1. Intercepted before the WebView processes them (when terminal has focus)
2. Translated to terminal input sequences (e.g., Arrow keys → `\x1b[A`)
3. Written to the PTY/SSH session

`alacritty_terminal` doesn't handle keyboard → byte translation directly. We need a key binding module that maps key events to byte sequences.

Common mappings:
- Printable characters → UTF-8 bytes
- Enter → `\r`
- Backspace → `\x7f`
- Tab → `\t`
- Arrow keys → `\x1b[A/B/C/D`
- Ctrl+C → `\x03`
- Ctrl+D → `\x04`
- etc.

**Step 1: Implement key-to-bytes mapping**

```rust
/// Convert a key event to terminal input bytes.
pub fn key_to_bytes(
    key: &str,          // Key name (e.g., "a", "Enter", "ArrowUp")
    modifiers: u32,     // Bitmask: 1=Shift, 2=Ctrl, 4=Alt, 8=Meta
) -> Option<Vec<u8>> {
    let ctrl = modifiers & 2 != 0;
    let alt = modifiers & 4 != 0;

    // Ctrl+letter shortcuts
    if ctrl && key.len() == 1 {
        let c = key.chars().next().unwrap().to_ascii_lowercase();
        if c >= 'a' && c <= 'z' {
            let code = c as u8 - b'a' + 1;
            return Some(if alt { vec![0x1b, code] } else { vec![code] });
        }
    }

    match key {
        "Enter" => Some(vec![b'\r']),
        "Backspace" => Some(vec![0x7f]),
        "Tab" => Some(vec![b'\t']),
        "Escape" => Some(vec![0x1b]),
        "ArrowUp" => Some(b"\x1b[A".to_vec()),
        "ArrowDown" => Some(b"\x1b[B".to_vec()),
        "ArrowRight" => Some(b"\x1b[C".to_vec()),
        "ArrowLeft" => Some(b"\x1b[D".to_vec()),
        "Home" => Some(b"\x1b[H".to_vec()),
        "End" => Some(b"\x1b[F".to_vec()),
        "PageUp" => Some(b"\x1b[5~".to_vec()),
        "PageDown" => Some(b"\x1b[6~".to_vec()),
        "Insert" => Some(b"\x1b[2~".to_vec()),
        "Delete" => Some(b"\x1b[3~".to_vec()),
        // F1-F12
        "F1" => Some(b"\x1bOP".to_vec()),
        "F2" => Some(b"\x1bOQ".to_vec()),
        "F3" => Some(b"\x1bOR".to_vec()),
        "F4" => Some(b"\x1bOS".to_vec()),
        "F5" => Some(b"\x1b[15~".to_vec()),
        // ... F6-F12
        _ => {
            // Printable character
            if key.len() == 1 {
                let c = key.chars().next().unwrap();
                let mut bytes = vec![0u8; c.len_utf8()];
                c.encode_utf8(&mut bytes);
                if alt {
                    let mut result = vec![0x1b];
                    result.extend_from_slice(&bytes);
                    Some(result)
                } else {
                    Some(bytes)
                }
            } else {
                None
            }
        }
    }
}
```

**Step 2: Register a window event listener**

In the Tauri setup or render loop, listen for keyboard events and route them:

```rust
window.on_window_event(move |event| {
    if let tauri::WindowEvent::KeyboardInput { event: key_event, .. } = event {
        // Convert and send to PTY
    }
});
```

**Step 3: Verify and commit**

---

## Phase 12: Theme Integration

### Task 21: Port Terminal Themes to Rust

**Files:**
- Create: `src-tauri/src/terminal/theme.rs`
- Modify: `src-tauri/src/terminal/mod.rs`

**Context:** Terminal themes define 16 ANSI colors + fg/bg/cursor/selection. Currently defined in `src/data/terminalThemes.ts`. We need a Rust equivalent that the renderer uses.

Two approaches:
1. **Duplicate in Rust** — Maintain themes in both TS and Rust
2. **Send from frontend** — Frontend sends theme colors to Rust via IPC

**Recommended: Option 2** — The frontend already owns theme management. When a theme changes, it sends the color values to the Rust renderer via `terminal_update_settings`. This avoids duplication and leverages the existing theme picker UI.

**Step 1: Define Rust theme struct**

```rust
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct TerminalTheme {
    pub foreground: [f32; 4],
    pub background: [f32; 4],
    pub cursor: [f32; 4],
    pub cursor_accent: [f32; 4],
    pub selection_background: [f32; 4],
    pub selection_foreground: [f32; 4],
    /// 16 ANSI colors: [black, red, green, yellow, blue, magenta, cyan, white,
    ///                   bright_black, bright_red, ... bright_white]
    pub ansi_colors: [[f32; 4]; 16],
}

impl TerminalTheme {
    /// Parse from the JSON format sent by the frontend.
    pub fn from_xterm_theme(json: &serde_json::Value) -> Self {
        // Convert hex "#RRGGBB" to [f32; 4]
        fn hex_to_rgba(hex: &str) -> [f32; 4] {
            let hex = hex.trim_start_matches('#');
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f32 / 255.0;
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f32 / 255.0;
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f32 / 255.0;
            [r, g, b, 1.0]
        }

        // Parse the xterm.js ITheme format
        // ...
        todo!()
    }
}
```

**Step 2: Update bridge to accept theme changes**

The `terminal_update_settings` command should parse the theme and update the renderer's theme.

**Step 3: Commit**

```bash
git add src-tauri/src/terminal/theme.rs
git commit -m "feat(terminal): add Rust theme struct with frontend JSON parsing"
```

---

## Dependency Graph

```
Task 1 (deps) ──→ Task 2 (scaffolding) ──→ Task 3 (term_core)
                                             ├──→ Task 4 (font_engine) ──→ Task 5 (atlas)
                                             │                              │
                                             │    Task 6 (wgpu init) ───────┤
                                             │    Task 7 (shaders) ─────────┤
                                             │                              ▼
                                             │                      Task 8 (frame render)
                                             │                              │
                                             ├──→ Task 10 (data flow) ──────┤
                                             │                              ▼
                                             │                      Task 12 (render loop)
                                             │                              │
                                             │    Task 9 (window setup) ────┤
                                             │                              ▼
                                             │                      Task 16 (bare window)
                                             │                              │
                                             ├──→ Task 11 (IPC bridge)      │
                                             ├──→ Task 13 (highlights)      │
                                             ├──→ Task 14 (search)          │
                                             ├──→ Task 15 (command nav)     │
                                             │                              │
                                             │              Task 17 (React component) ◄──┤
                                             │              Task 19 (selection)
                                             │              Task 20 (keyboard input)
                                             │              Task 21 (themes)
                                             │                              │
                                             └──────────────────────────────┤
                                                                            ▼
                                                                    Task 18 (remove xterm.js)
```

## Parallelization Strategy

The following task groups can run in parallel:

**Group A (Independent Rust modules):**
- Task 4 (font_engine) + Task 5 (atlas)
- Task 6 (wgpu init) + Task 7 (shaders)
- Task 13 (highlights) + Task 14 (search) + Task 15 (command nav)

**Group B (Sequential critical path):**
- Task 1 → 2 → 3 → 10 → 12 → 16 → 17 → 18

**Group C (Can start after Task 3):**
- Task 11 (bridge) — independent of renderer
- Task 19 (selection) — needs term_core only
- Task 20 (keyboard) — needs bridge only
- Task 21 (themes) — independent

## Notes for Implementer

1. **API Compatibility:** `alacritty_terminal` 0.24, `crossfont` 0.8, and `wgpu` 24 APIs may differ from what's shown in code snippets. These are based on the most recent documented APIs. **Always check `cargo doc --open` for the actual API** and adapt accordingly.

2. **Incremental Testing:** After each task, verify the app still compiles and the existing xterm.js renderer still works. The feature flag (`nativeRenderer`) ensures we don't break anything.

3. **Platform Testing:** Test on macOS first (Core Text + Metal). Windows (DirectWrite + DX12) should be tested in Task 16+.

4. **Performance:** The initial implementation may not hit the <8ms target. That's fine — optimize after correctness is established.

5. **The 80/20 Rule:** Getting basic text rendering (Tasks 1-8, 12) covers 80% of the visual result. Features like selection, search highlights, and command navigation are refinements.
