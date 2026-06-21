// host_library_section::editors — RootView 的主机库内联编辑器（改名 / 搜索 / 密钥名称口令）。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。
// create_*_editor（含密钥 name/passphrase）/ handle_host_*_editor_event 由 RootView::new() 调用（pub(crate)）。

use crate::RootView;
use nexshell::host_management::{
    default_database_path, upsert_host_card_in_db_path,
};
use warp::appearance::Appearance;
use warp::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions};
use warpui::{SingletonEntity as _, ViewContext};

impl RootView {
    pub(crate) fn create_host_search_editor(ctx: &mut ViewContext<Self>) -> warpui::ViewHandle<EditorView> {
        ctx.add_typed_action_view(|ctx| {
            let font_size = Appearance::as_ref(ctx).ui_font_size();
            let options = SingleLineEditorOptions {
                text: TextOptions {
                    font_size_override: Some(font_size),
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut editor = EditorView::single_line(options, ctx);
            editor.set_placeholder_text(rust_i18n::t!("host_search_placeholder"), ctx);
            editor
        })
    }

    pub(crate) fn create_host_rename_editor(ctx: &mut ViewContext<Self>) -> warpui::ViewHandle<EditorView> {
        let editor = ctx.add_typed_action_view(|ctx| {
            let font_size = Appearance::as_ref(ctx).ui_font_size();
            let options = SingleLineEditorOptions {
                text: TextOptions {
                    font_size_override: Some(font_size),
                    ..Default::default()
                },
                ..Default::default()
            };
            EditorView::single_line(options, ctx)
        });
        ctx.subscribe_to_view(&editor, |me, _, event: &EditorEvent, ctx| {
            me.handle_host_rename_editor_event(event, ctx);
        });
        editor
    }

    pub(crate) fn create_host_key_name_editor(
        ctx: &mut ViewContext<Self>,
    ) -> warpui::ViewHandle<EditorView> {
        Self::create_host_key_editor(ctx)
    }

    pub(crate) fn create_host_key_passphrase_editor(
        ctx: &mut ViewContext<Self>,
    ) -> warpui::ViewHandle<EditorView> {
        Self::create_host_key_editor(ctx)
    }

    // 密钥内联编辑 editor：Enter 保存 / Escape 取消（仅在编辑态生效）。
    fn create_host_key_editor(ctx: &mut ViewContext<Self>) -> warpui::ViewHandle<EditorView> {
        let editor = ctx.add_typed_action_view(|ctx| {
            let font_size = Appearance::as_ref(ctx).ui_font_size();
            let options = SingleLineEditorOptions {
                text: TextOptions {
                    font_size_override: Some(font_size),
                    ..Default::default()
                },
                ..Default::default()
            };
            EditorView::single_line(options, ctx)
        });
        ctx.subscribe_to_view(&editor, |me, _, event: &EditorEvent, ctx| {
            me.handle_host_key_editor_event(event, ctx);
        });
        editor
    }

    fn handle_host_key_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        if self.host_key_edit_target.is_none() {
            return;
        }
        match event {
            EditorEvent::Enter => self.handle_host_key_edit_save(ctx),
            EditorEvent::Escape => self.handle_host_key_edit_cancel(ctx),
            _ => {}
        }
    }

    fn handle_host_rename_editor_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        if self.host_rename_target.is_none() {
            return;
        }
        match event {
            EditorEvent::Enter => self.commit_host_rename(ctx),
            EditorEvent::Escape => self.cancel_host_rename(ctx),
            _ => {}
        }
    }

    pub(super) fn start_host_rename(&mut self, host_id: String, ctx: &mut ViewContext<Self>) {
        let Some(current) = self.host_state.host_by_id(&host_id).map(|h| h.name.clone()) else {
            return;
        };
        self.host_rename_target = Some(host_id);
        self.host_rename_editor.update(ctx, move |editor, ctx| {
            editor.clear_buffer_and_reset_undo_stack(ctx);
            if !current.is_empty() {
                editor.insert_selected_text(&current, ctx);
            }
        });
        ctx.focus(&self.host_rename_editor);
        ctx.notify();
    }

    fn commit_host_rename(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(host_id) = self.host_rename_target.take() else {
            return;
        };
        let text = self
            .host_rename_editor
            .as_ref(ctx)
            .buffer_text(ctx)
            .trim()
            .to_string();
        self.clear_host_rename_editor(ctx);
        let Some(mut snap) = self.host_state.host_by_id(&host_id).cloned() else {
            ctx.focus_self();
            ctx.notify();
            return;
        };
        if text.is_empty() || snap.name == text {
            ctx.focus_self();
            ctx.notify();
            return;
        }
        let Some(db_path) = default_database_path() else {
            self.host_state.notice =
                Some(rust_i18n::t!("toast_host_library_unavailable_save").to_string());
            ctx.focus_self();
            ctx.notify();
            return;
        };
        snap.name = text;
        match upsert_host_card_in_db_path(&db_path, &snap) {
            Ok(()) => match self.load_host_snapshot_from_db() {
                Ok(()) => {
                    self.host_state.notice = Some(rust_i18n::t!("toast_host_saved").to_string());
                }
                Err(error) => {
                    self.host_state.notice =
                        Some(rust_i18n::t!("toast_rename_failed", error = error).to_string());
                }
            },
            Err(error) => {
                self.host_state.notice =
                    Some(rust_i18n::t!("toast_rename_failed", error = error).to_string());
            }
        }
        ctx.focus_self();
        ctx.notify();
    }

    fn cancel_host_rename(&mut self, ctx: &mut ViewContext<Self>) {
        if self.host_rename_target.take().is_none() {
            return;
        }
        self.clear_host_rename_editor(ctx);
        ctx.focus_self();
        ctx.notify();
    }

    fn clear_host_rename_editor(&mut self, ctx: &mut ViewContext<Self>) {
        self.host_rename_editor.update(ctx, |editor, ctx| {
            editor.clear_buffer_and_reset_undo_stack(ctx);
        });
    }

    pub(in crate::root_view) fn handle_host_search_editor_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            EditorEvent::Edited(_) => {
                let query = self.host_search_editor.as_ref(ctx).buffer_text(ctx);
                self.host_state.set_query(query);
                self.host_view_states
                    .borrow_mut()
                    .search_bar
                    .protocol_dropdown_open = false;
                ctx.notify();
            }
            EditorEvent::Escape => {
                self.host_state.clear_search();
                self.host_view_states
                    .borrow_mut()
                    .search_bar
                    .protocol_dropdown_open = false;
                self.host_search_editor.update(ctx, |editor, ctx| {
                    if !editor.buffer_text(ctx).is_empty() {
                        editor.system_reset_buffer_text("", ctx);
                    }
                });
                ctx.notify();
            }
            _ => {}
        }
    }
}
