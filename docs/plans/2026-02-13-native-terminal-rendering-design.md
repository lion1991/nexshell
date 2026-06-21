# Native Terminal Rendering Design

> ⚠️ **历史文档（已废弃）**：本设计描述的 src-tauri / wgpu + WebView 渲染架构已于 2026-06 整体废弃删除，native-shell-spike（GPUI）成为唯一架构。仅作历史参考。

**Date:** 2026-02-13
**Status:** Superseded（架构已废弃）
**Scope:** Replace xterm.js with native GPU-accelerated terminal rendering

## Goals

- Pixel-perfect native font rendering on macOS (Core Text) and Windows (DirectWrite)
- GPU-accelerated rendering via wgpu (Metal/DX12/Vulkan)
- Full feature parity with current xterm.js implementation (50+ features)
- Maintain existing UI chrome (tabs, sidebar, settings, menus) in WebView

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    Tauri Window                      │
│                                                     │
│  ┌───────────────────────────────────────────────┐  │
│  │     wgpu Surface (Metal / DX12 / Vulkan)      │  │
│  │                                               │  │
│  │  alacritty_terminal → crossfont → Glyph Atlas │  │
│  │    (VTE + grid)      (native)    (GPU texture) │  │
│  │                  ↕ HarfBuzz                   │  │
│  │              (text shaping)                    │  │
│  └───────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────┐  │
│  │  Transparent WebView (UI overlay)              │  │
│  │  Tabs / Sidebar / Settings / Search / Menus   │  │
│  └───────────────────────────────────────────────┘  │
│                                                     │
│  Tauri IPC: WebView ↔ Rust (commands/events)        │
└─────────────────────────────────────────────────────┘
```

## Technology Stack

| Component | Crate | Purpose |
|-----------|-------|---------|
| Terminal emulation | `alacritty_terminal` | VTE parsing, terminal grid, scrollback, selection, damage tracking |
| Font rasterization | `crossfont` | Native glyph rasterization (Core Text / DirectWrite / FreeType) |
| Text shaping | `harfbuzz` (via `harfbuzz-rs` or `rustybuzz`) | Ligatures, complex script shaping |
| GPU rendering | `wgpu` | Cross-platform GPU abstraction (Metal, DX12, Vulkan) |
| Window handle | `raw-window-handle` | Access native window for wgpu surface creation |
| App framework | `tauri` (unstable feature) | Window management, IPC, transparent webview overlay |

## Module Design

### 1. `term_core` — Terminal Emulation

Wraps `alacritty_terminal::Term<EventProxy>`:
- VTE parsing and ANSI escape sequence handling
- Terminal grid management (normal + alternate buffer)
- Scrollback buffer
- Selection state management
- Damage tracking for incremental rendering
- OSC 133 command boundary detection (migrated from `useCommandNav.ts`)

### 2. `font_engine` — Font Management

Built on `crossfont` + HarfBuzz:
- System font discovery and loading
- Glyph rasterization via platform-native APIs
- Text shaping for ligature and complex script support
- Glyph atlas management (LRU cache of rasterized glyphs packed into GPU textures)
- Font metrics (cell width/height, baseline, underline position)
- Configurable: font family, size, weight, weight-bold, letter spacing, line height

### 3. `renderer` — GPU Rendering Pipeline

Built on `wgpu`:
- **Surface management:** Create wgpu surface from Tauri window handle
- **Glyph atlas texture:** Upload rasterized glyphs, manage atlas packing
- **Render passes:**
  - Pass 1: Cell background colors (solid color rectangles)
  - Pass 2: Glyph rendering (textured quads sampling from atlas)
  - Pass 3: Cursor rendering (block/underline/bar with blink animation)
  - Pass 4: Selection highlight overlay
  - Pass 5: Search match highlights + overview ruler
  - Pass 6: Custom highlight engine decorations
- **Vertex format:** position (x,y), atlas UV (u,v), fg color, bg color
- **Damage-based rendering:** Only re-render changed regions using `Term::damage()`

### 4. `features` — Custom Feature Modules (Rust)

Migrated from TypeScript:
- **Highlight engine:** Regex-based ANSI-aware highlighting with priority system, performance limits
- **Command navigation:** OSC 133 shell integration, command boundary tracking, exit code recording
- **Line numbers:** Row number rendering in left gutter
- **Search:** Regex/case-sensitive/whole-word search across terminal grid with match decorations
- **Ctrl+scroll zoom:** Font size adjustment with min/max bounds

### 5. `bridge` — Tauri IPC Bridge

Communication between native renderer and WebView UI:
- **Rust → WebView events:**
  - Terminal content updates (for suggestions panel input tracking)
  - Session status changes
  - Selection state changes
- **WebView → Rust commands:**
  - `copy()`, `paste()`, `select_all()`, `clear()`
  - `find_next()`, `find_previous()`, `clear_search()`
  - `create_session()`, `close_session()`, `write_to_session()`
  - `update_terminal_setting()`
  - `resize_terminal()` (from WebView layout changes)
- **Input routing:**
  - Keyboard events captured by native window → routed to terminal
  - Mouse events in terminal area → handled natively (selection, scroll, context menu trigger)
  - Mouse events in WebView area → handled by WebView

### 6. `session` — Session Management (Mostly Reused)

Existing code largely reused:
- `SshSession` (russh) — SSH connections
- `LocalPtySession` (portable-pty) — Local terminal
- `SessionManager` — Unified session abstraction
- **Change:** Output no longer sent via Tauri events to WebView; instead fed directly to `alacritty_terminal::Term`
- **Change:** PTY spawning may switch to `alacritty_terminal::tty` for consistency

## Data Flow

```
PTY/SSH output
    │
    ▼
alacritty_terminal::Term (VTE parse → update grid)
    │
    ▼ Wakeup event
    │
Lock Term → renderable_content()
    │
    ├─ For each cell:
    │   ├─ HarfBuzz shape → glyph IDs
    │   ├─ Check atlas cache (hit → UV coords)
    │   └─ Cache miss → crossfont rasterize → upload to atlas
    │
    ├─ Apply highlight rules (Rust highlight engine)
    ├─ Apply search matches
    ├─ Apply command nav markers
    │
    ▼
Build vertex buffer → wgpu render pipeline → present
```

## Feature Migration Checklist

### Core Terminal (Rust native)
- [ ] VTE/ANSI parsing (alacritty_terminal)
- [ ] Terminal grid + scrollback
- [ ] Cursor rendering (block/underline/bar + blink)
- [ ] Text selection (mouse drag, double-click word, triple-click line)
- [ ] Copy/paste (clipboard integration)
- [ ] Select all
- [ ] Clear screen
- [ ] Scroll (mouse wheel, scrollbar)
- [ ] Ctrl+scroll zoom (font size ± with bounds)
- [ ] Search (regex, case-sensitive, whole-word, incremental)
- [ ] Search decorations (match bg/border, active match, overview ruler)
- [ ] Highlight engine (regex rules, priority, ANSI-aware, perf limits)
- [ ] Command navigation (OSC 133, gutter icons, jump-to-command)
- [ ] Line numbers gutter
- [ ] Background image support
- [ ] Web link detection and click handling
- [ ] OSC 52 clipboard support
- [ ] Theme colors (16 ANSI + bright + fg/bg/cursor/selection)
- [ ] Font weight / font weight bold
- [ ] Letter spacing, line height
- [ ] Right-click → trigger context menu in WebView
- [ ] Copy-on-select option

### WebView UI (Retained)
- [ ] Tab bar (create, close, switch, rename, reorder)
- [ ] Sidebar (host list, file manager, monitor)
- [ ] Settings page (all terminal settings)
- [ ] Search bar UI (input, toggles, nav buttons)
- [ ] Context menu (copy, paste, select all, clear, search)
- [ ] Smart suggestions panel
- [ ] Command history integration
- [ ] Quick commands
- [ ] Connection status overlays
- [ ] Theme picker modal

### IPC Bridge
- [ ] Input buffer sync (for suggestions)
- [ ] Session lifecycle commands
- [ ] Terminal setting updates → re-configure renderer
- [ ] Layout resize → terminal resize
- [ ] Selection text → WebView (for context menu state)

## Tauri Integration Details

Requires `tauri` crate with `unstable` feature flag.

```rust
// Cargo.toml
[dependencies]
tauri = { version = "2", features = ["unstable"] }
alacritty_terminal = "0.24"
crossfont = "0.8"
wgpu = "24"
```

**Window setup:**
1. Create Tauri `Window` (bare, no default webview)
2. Get `raw_window_handle` → create `wgpu::Surface`
3. Add transparent child `Webview` overlay for UI chrome
4. Run render loop on dedicated thread, synchronized with terminal updates

**Known risks:**
- Tauri `unstable` API may change between releases
- Transparent webview overlay has known bugs on Windows
- Input event routing between native surface and webview needs careful handling

## Performance Targets

- **Render latency:** < 8ms per frame (120 FPS capable)
- **Large output:** Handle `cat large_file.txt` without visible lag
- **Memory:** Glyph atlas < 16MB for typical use
- **Incremental rendering:** Only re-draw damaged cells via `Term::damage()`

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| Tauri `unstable` API breaks | Pin Tauri version, abstract behind our own layer |
| Transparent webview bugs on Windows | Test early, have fallback to opaque webview with native terminal in separate region |
| crossfont limited to monospace | Terminal fonts are monospace by definition |
| Input routing complexity | Clearly define hit regions: terminal area → native, UI area → webview |
| HarfBuzz C dependency | Use `rustybuzz` (pure Rust port) as alternative if needed |
