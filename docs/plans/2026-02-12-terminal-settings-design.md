# Terminal Settings Page Design

> ⚠️ **历史文档（已废弃）**：本设计描述的 Tauri / xterm.js / React 终端设置架构已于 2026-06 整体废弃删除，native-shell-spike（GPUI）成为唯一架构。仅作历史参考。

## Overview
Implement a comprehensive terminal settings page for the SSH tool, covering terminal configuration, appearance, advanced options, mouse behavior, line numbers, command navigation, search highlight colors, highlight performance, and smart suggestions.

## Data Model

Extend `AppSettings` with a `terminal` field of type `TerminalSettings`:

```typescript
export interface TerminalSettings {
  // Terminal config
  defaultShell: string;
  customShellPath: string;
  terminalTheme: string;
  backgroundImage: string;

  // Appearance
  terminalFont: string;
  fontSize: number;
  fontSizeMin: number;
  fontSizeMax: number;
  letterSpacing: number;
  lineHeight: number;
  ctrlScrollZoom: boolean;
  cursorBlink: boolean;
  cursorStyle: "block" | "underline" | "bar";

  // Advanced
  scrollbackLines: number;
  completionFontSize: number;
  convertEol: boolean;
  allowProposedApi: boolean;
  osc52Clipboard: boolean;
  webglRenderer: boolean;

  // Mouse
  copyOnSelect: boolean;
  rightClickAction: "menu" | "paste";

  // Line numbers
  showLineNumbers: boolean;

  // Command navigation
  commandNavigation: boolean;

  // Search highlight colors
  searchMatchBackground: string;
  searchMatchBorder: string;
  searchMatchOverviewRuler: string;
  searchActiveBackground: string;
  searchActiveBorder: string;
  searchActiveOverviewRuler: string;

  // Highlight performance
  highlightExtraScanLines: number;
  highlightMaxDecorations: number;
  highlightMaxLineLength: number;
  highlightThrottleMs: number;
  highlightOnScroll: boolean;
  highlightSkipAltBuffer: boolean;

  // Smart suggestions
  smartSuggestions: boolean;
  suggestMinChars: number;
  suggestMaxCount: number;
  suggestPanelLineHeight: number;
  showHistoryCommands: boolean;
  showQuickCommands: boolean;
  serialCommandSuggestions: boolean;
}
```

## Component Architecture

Settings UI split into sub-components under `src/components/settings/`:
- TerminalConfig.tsx (shell, theme, background)
- TerminalAppearance.tsx (font, size, cursor)
- TerminalAdvanced.tsx (scrollback, renderers, flags)
- TerminalMouse.tsx (copy on select, right click)
- TerminalLineNumbers.tsx
- TerminalCommandNav.tsx
- TerminalSearchHighlight.tsx (color pickers)
- TerminalHighlightPerf.tsx (numeric inputs, toggles)
- TerminalSmartSuggest.tsx (toggles, numeric inputs)

## Theme Presets

6 built-in themes in `src/data/terminalThemes.ts`:
Tokyo Night, Dracula, Solarized Dark, One Dark, Nord, Monokai

## Settings Application

Terminal.tsx subscribes to settings changes via useSettings hook. Most xterm options can be hot-updated; WebGL renderer toggle requires addon reload.

## Decisions
- All settings are global (not per-device)
- No VIP/membership system - all features open
- Smart suggestions: history commands + quick commands only (no AI suggestions, no command library)
- Background image: open to all users via Tauri file dialog
