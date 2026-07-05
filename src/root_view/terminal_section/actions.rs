// terminal_section::actions — 终端 action handler（#1-3 复制/粘贴/清屏、#7-9 字号、#40 TerminalMouseDown）。
// 只含 impl RootView；handler 由 mod.rs handle_action 分发，用 pub(in crate::root_view)。

use warpui::clipboard::{should_insert_text_on_paste, ClipboardContent};
use warpui::ViewContext;

use crate::ui_settings::{TERMINAL_FONT_SIZE_DEFAULT, TERMINAL_FONT_SIZE_STEP};
use crate::RootView;
use nexshell::terminal_runtime::terminal_input_editor_should_capture;
use nexshell::warp_tab_context_menu::should_finish_tab_rename_on_external_mouse_down;

impl RootView {
    pub(in crate::root_view) fn handle_copy_selection(&mut self, ctx: &mut ViewContext<Self>) {
        let selected_text = self.terminal.lock().ok().and_then(|rt| rt.selected_text());
        if let Some(text) = selected_text.filter(|text| !text.is_empty()) {
            ctx.clipboard().write(ClipboardContent::plain_text(text));
        }
    }

    pub(in crate::root_view) fn handle_paste_clipboard(&mut self, ctx: &mut ViewContext<Self>) {
        let content = ctx.clipboard().read();
        if should_insert_text_on_paste(&content) && !content.plain_text.is_empty() {
            if let Ok(mut editor) = self.input_editor.lock() {
                editor.clear_marked_text();
            }
            if let Ok(rt) = self.terminal.lock() {
                rt.clear_marked_text();
                rt.paste(&content.plain_text);
            }
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn handle_clear_visible_screen(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        let preserve_prompt_prefix = self
            .terminal
            .lock()
            .map(|rt| {
                !rt.uses_remote_ssh()
                    && rt.shell_is_foreground()
                    && terminal_input_editor_should_capture(&rt.snapshot().grid)
            })
            .unwrap_or(false);
        if let Ok(mut editor) = self.input_editor.lock() {
            editor.clear();
        }
        if let Ok(rt) = self.terminal.lock() {
            rt.clear_visible_screen(preserve_prompt_prefix);
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_increase_font_size(&mut self, ctx: &mut ViewContext<Self>) {
        self.set_terminal_font_size(self.terminal_font_size + TERMINAL_FONT_SIZE_STEP);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_decrease_font_size(&mut self, ctx: &mut ViewContext<Self>) {
        self.set_terminal_font_size(self.terminal_font_size - TERMINAL_FONT_SIZE_STEP);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_reset_font_size(&mut self, ctx: &mut ViewContext<Self>) {
        self.set_terminal_font_size(TERMINAL_FONT_SIZE_DEFAULT);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_terminal_mouse_down(&mut self, ctx: &mut ViewContext<Self>) {
        if should_finish_tab_rename_on_external_mouse_down(self.tab_being_renamed) {
            self.finish_tab_rename(ctx);
        } else {
            ctx.focus_self();
        }
        self.show_terminal_context_menu = None;
        self.show_git_panel_context_menu = None;
    }
}
