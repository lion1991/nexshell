// actions section — RootView 的文件面板 action handler + inline 输入编辑器 + 上传/下载/reveal。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。
// handle_* 由 root_view/mod.rs handle_action 分发；create_file_panel_input_editor 由 RootView::new() 调用；
// 跨文件依赖 mod.rs::refresh_or_restart_file_panel_worker（toggle/refresh 调用）。

use crate::file_panel_view_helpers::{
    file_panel_cd_command, file_panel_leaf_name, file_panel_relative_path, reveal_file_manager_path,
};
use crate::{
    FilePanelInputIntent, RootView, TerminalSessionKind, FILE_PANEL_WIDTH_MAX, FILE_PANEL_WIDTH_MIN,
};
use nexshell::file_panel::{
    apply_file_panel_selection, apply_file_panel_tree_selection, flatten_file_panel_tree,
    join_path, parent_path, toggle_file_panel_tree_dir, FilePanelSelectMode, FilePanelTreeToggle,
    SftpRequest,
};
use nexshell::sftp_ops::EntryKind;
use nexshell::text_editor::{
    EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions,
};
use warp_core::ui::appearance::Appearance;
use warpui::clipboard::ClipboardContent;
use warpui::{SingletonEntity as _, ViewContext};

impl RootView {
    pub(in crate::root_view) fn handle_toggle_file_panel(&mut self, ctx: &mut ViewContext<Self>) {
        // 关门滑出途中再按 = 取消关门滑回。
        if self.file_panel_closing.borrow().is_some() {
            *self.file_panel_closing.borrow_mut() = None;
            self.file_panel_slide.borrow_mut().set_target(0.0);
            ctx.notify();
            return;
        }
        let Some(tab) = self.file_panel_tab_mut() else {
            return;
        };
        if tab.file_panel_open {
            // 开始关门：滑出到面板宽，收敛后 finalize_panel_closes 落闸真正关闭。
            // git 面板无需联动：file 槽位释放的空间归中间的 terminal（flex），
            // git 作为最右元素位置不变；file 从 git 底下滑出（z 序 git 在上）。
            let width = tab
                .file_panel_width
                .clamp(FILE_PANEL_WIDTH_MIN, FILE_PANEL_WIDTH_MAX)
                + crate::file_panel_view_helpers::PANEL_BORDER_W;
            let tab_id = tab.id.clone();
            *self.file_panel_closing.borrow_mut() = Some(tab_id);
            self.file_panel_slide.borrow_mut().set_target(width);
            ctx.notify();
            return;
        }
        tab.file_panel_open = true;
        let tab_id = tab.id.clone();
        if matches!(tab.kind, TerminalSessionKind::Local) {
            tab.file_panel_state.follow_cwd = true;
        }
        self.file_panel_slide_pending.set(true); // 首帧按实际宽度起搏滑入
        ctx.notify();
        self.refresh_or_restart_file_panel_worker(&tab_id, ctx);
    }

    pub(in crate::root_view) fn handle_file_panel_refresh(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(tab) = self.file_panel_tab() {
            let tab_id = tab.id.clone();
            self.refresh_or_restart_file_panel_worker(&tab_id, ctx);
        }
    }

    pub(in crate::root_view) fn handle_file_panel_go_up(&mut self) {
        let parent = self
            .file_panel_tab()
            .map(|tab| parent_path(&tab.file_panel_state.cwd));
        if let Some(parent) = parent {
            if let Some(tab) = self.file_panel_tab_mut() {
                tab.file_panel_state.follow_cwd = false;
                if let Some(w) = tab.sftp_worker.as_ref() {
                    w.send(SftpRequest::List(parent));
                }
            }
        }
    }

    pub(in crate::root_view) fn handle_file_panel_enter_dir(&mut self, name: String) {
        if let Some(tab) = self.file_panel_tab_mut() {
            let next = join_path(&tab.file_panel_state.cwd, &name);
            tab.file_panel_state.follow_cwd = false;
            if let Some(w) = tab.sftp_worker.as_ref() {
                w.send(SftpRequest::List(next));
            }
        }
    }

    pub(in crate::root_view) fn handle_file_panel_select(
        &mut self,
        name: String,
        mode: FilePanelSelectMode,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(tab) = self.file_panel_tab_mut() {
            apply_file_panel_selection(&mut tab.file_panel_state, &name, mode);
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn handle_file_panel_tree_item_clicked(
        &mut self,
        path: String,
        is_dir: bool,
        mode: FilePanelSelectMode,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(tab) = self.file_panel_tab_mut() {
            if !matches!(tab.kind, TerminalSessionKind::Local) {
                return;
            }
            apply_file_panel_tree_selection(&mut tab.file_panel_state, &path, mode);
            if is_dir && mode == FilePanelSelectMode::Replace {
                let should_load = matches!(
                    toggle_file_panel_tree_dir(&mut tab.file_panel_state, &path),
                    FilePanelTreeToggle::ExpandedNeedsLoad
                );
                if should_load {
                    if let Some(worker) = tab.sftp_worker.as_ref() {
                        worker.send(SftpRequest::ListTreeChild(path.clone()));
                    }
                }
            }
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn handle_file_panel_drop_files(&mut self, paths: Vec<String>) {
        if let Some(tab) = self.file_panel_tab() {
            let Some(worker) = tab.sftp_worker.as_ref() else {
                return;
            };
            let locals: Vec<std::path::PathBuf> =
                paths.iter().map(std::path::PathBuf::from).collect();
            if locals.is_empty() {
                return;
            }
            worker.send(SftpRequest::Upload {
                locals,
                remote_dir: tab.file_panel_state.cwd.clone(),
            });
        }
    }

    pub(in crate::root_view) fn handle_file_panel_cancel_transfer(&self, id: u64) {
        if let Some(tab) = self.file_panel_tab() {
            if let Some(worker) = tab.sftp_worker.as_ref() {
                worker.cancel(id);
            }
        }
    }

    pub(in crate::root_view) fn handle_file_panel_delete(&mut self, name: String, is_dir: bool) {
        if let Some(tab) = self.file_panel_tab() {
            let Some(worker) = tab.sftp_worker.as_ref() else {
                return;
            };
            let state = &tab.file_panel_state;
            // 多选场景：右键目标已在 selected_names 集合 → 批量删整个集合；
            // 否则按单个删（与右键行为一致：未在集合内时菜单只针对那一项）
            let batch: Vec<(String, bool)> = if matches!(tab.kind, TerminalSessionKind::Local)
                && state.selected_names.len() > 1
                && state.selected_names.contains(&name)
            {
                flatten_file_panel_tree(state)
                    .into_iter()
                    .filter(|row| state.selected_names.contains(&row.path))
                    .map(|row| {
                        let is_dir = row.is_dir();
                        (row.path, is_dir)
                    })
                    .collect()
            } else if state.selected_names.len() > 1 && state.selected_names.contains(&name) {
                state
                    .entries
                    .iter()
                    .filter(|e| state.selected_names.contains(&e.name))
                    .map(|e| {
                        (
                            join_path(&state.cwd, &e.name),
                            matches!(e.kind, EntryKind::Dir),
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
            if batch.is_empty() {
                let path = join_path(&state.cwd, &name);
                worker.send(SftpRequest::Delete { path, is_dir });
            } else {
                worker.send(SftpRequest::DeleteMany { items: batch });
            }
        }
    }

    /// 文件面板跳回终端所在目录并恢复跟随（与 cd_to_directory 反向，仅本地终端）。
    pub(in crate::root_view) fn handle_file_panel_sync_to_terminal_cwd(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        let cwd = self.file_panel_tab().and_then(|tab| {
            if !matches!(tab.kind, TerminalSessionKind::Local) {
                return None;
            }
            tab.terminal.lock().ok()?.snapshot().local_cwd.clone()
        });
        let Some(cwd) = cwd else {
            return;
        };
        if let Some(tab) = self.file_panel_tab_mut() {
            tab.file_panel_state.follow_cwd = true;
            if let Some(w) = tab.sftp_worker.as_ref() {
                w.send(SftpRequest::List(cwd.to_string_lossy().into_owned()));
            }
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_file_panel_cd_to_directory(&self, path: String) {
        if let Some(tab) = self.file_panel_tab() {
            if matches!(tab.kind, TerminalSessionKind::Local) {
                if let Ok(rt) = tab.terminal.lock() {
                    rt.send_input(file_panel_cd_command(&path));
                }
            }
        }
    }

    pub(in crate::root_view) fn handle_file_panel_copy_path(
        &self,
        name: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(tab) = self.file_panel_tab() {
            let path = if name.is_empty() {
                tab.file_panel_state.cwd.clone()
            } else {
                join_path(&tab.file_panel_state.cwd, &name)
            };
            ctx.clipboard().write(ClipboardContent::plain_text(path));
        }
    }

    pub(in crate::root_view) fn handle_file_panel_copy_relative_path(
        &self,
        path: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(tab) = self.file_panel_tab() {
            let relative = file_panel_relative_path(&tab.file_panel_state.cwd, &path);
            ctx.clipboard()
                .write(ClipboardContent::plain_text(relative));
        }
    }

    pub(in crate::root_view) fn handle_file_panel_resize_start(&mut self, start_x: f32) {
        if let Some(tab) = self.file_panel_tab() {
            self.file_panel_resize_anchor = Some((start_x, tab.file_panel_width));
        }
    }

    pub(in crate::root_view) fn handle_file_panel_resize_move(
        &mut self,
        current_x: f32,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some((anchor_x, anchor_w)) = self.file_panel_resize_anchor {
            // 面板贴右边：mouse 往左移 → 宽度变大
            let new_w = (anchor_w + (anchor_x - current_x))
                .clamp(FILE_PANEL_WIDTH_MIN, FILE_PANEL_WIDTH_MAX);
            if let Some(tab) = self.file_panel_tab_mut() {
                if (tab.file_panel_width - new_w).abs() > f32::EPSILON {
                    tab.file_panel_width = new_w;
                    ctx.notify();
                }
            }
        }
    }

    pub(in crate::root_view) fn handle_file_panel_resize_end(&mut self) {
        self.file_panel_resize_anchor = None;
    }

    pub(crate) fn create_file_panel_input_editor(
        ctx: &mut ViewContext<Self>,
    ) -> warpui::ViewHandle<EditorView> {
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
            me.handle_file_panel_input_editor_event(event, ctx);
        });
        editor
    }

    pub(crate) fn file_panel_input_active(&self) -> bool {
        self.file_panel_input_intent.is_some()
    }

    pub(crate) fn start_file_panel_input(
        &mut self,
        intent: FilePanelInputIntent,
        initial: String,
        ctx: &mut ViewContext<Self>,
    ) {
        self.file_panel_input_intent = Some(intent);
        self.file_panel_input_editor
            .update(ctx, move |editor, ctx| {
                editor.clear_buffer_and_reset_undo_stack(ctx);
                if !initial.is_empty() {
                    editor.insert_selected_text(&initial, ctx);
                }
            });
        ctx.focus(&self.file_panel_input_editor);
        ctx.notify();
    }

    pub(crate) fn commit_file_panel_input(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(intent) = self.file_panel_input_intent.take() else {
            return;
        };
        let text = self
            .file_panel_input_editor
            .as_ref(ctx)
            .buffer_text(ctx)
            .trim()
            .to_string();
        self.clear_file_panel_input_editor(ctx);
        if text.is_empty() {
            ctx.notify();
            return;
        }
        if let Some(tab) = self.file_panel_tab() {
            if let Some(worker) = tab.sftp_worker.as_ref() {
                let cwd = tab.file_panel_state.cwd.clone();
                match intent {
                    FilePanelInputIntent::Rename { old_name } => {
                        let from = join_path(&cwd, &old_name);
                        let to = join_path(&parent_path(&from), &text);
                        if from != to {
                            worker.send(SftpRequest::Rename { from, to });
                        }
                    }
                    FilePanelInputIntent::NewDir => {
                        worker.send(SftpRequest::Mkdir {
                            parent: cwd,
                            name: text,
                        });
                    }
                    FilePanelInputIntent::NewFile => {
                        worker.send(SftpRequest::Touch {
                            parent: cwd,
                            name: text,
                        });
                    }
                    FilePanelInputIntent::NewFileIn { parent } => {
                        worker.send(SftpRequest::Touch { parent, name: text });
                    }
                }
            }
        }
        ctx.focus_self();
        ctx.notify();
    }

    pub(crate) fn cancel_file_panel_input(&mut self, ctx: &mut ViewContext<Self>) {
        if self.file_panel_input_intent.take().is_none() {
            return;
        }
        self.clear_file_panel_input_editor(ctx);
        ctx.focus_self();
        ctx.notify();
    }

    fn clear_file_panel_input_editor(&mut self, ctx: &mut ViewContext<Self>) {
        self.file_panel_input_editor.update(ctx, |editor, ctx| {
            editor.clear_buffer_and_reset_undo_stack(ctx);
        });
    }

    fn handle_file_panel_input_editor_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        if self.file_panel_input_intent.is_none() {
            return;
        }
        match event {
            EditorEvent::Enter => self.commit_file_panel_input(ctx),
            EditorEvent::Escape => self.cancel_file_panel_input(ctx),
            _ => {}
        }
    }

    pub(crate) fn start_file_panel_upload_dialog(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(tab) = self.file_panel_tab() else {
            return;
        };
        if tab.sftp_worker.is_none() {
            return;
        }
        let tab_id = tab.id.clone();
        let remote_dir = tab.file_panel_state.cwd.clone();
        let weak = ctx.handle();
        let config = warpui::platform::FilePickerConfiguration::new()
            .allow_folder()
            .allow_multi_select();
        ctx.open_file_picker(
            move |result, view_ctx| {
                let Ok(paths) = result else { return };
                if paths.is_empty() {
                    return;
                }
                use warpui::UpdateView;
                let Some(handle) = weak.upgrade(view_ctx) else {
                    return;
                };
                view_ctx.update_view(&handle, |view, sub_ctx| {
                    let Some(tab) = view.terminal_tabs.iter().find(|t| t.id == tab_id) else {
                        return;
                    };
                    let Some(worker) = tab.sftp_worker.as_ref() else {
                        return;
                    };
                    let locals: Vec<std::path::PathBuf> =
                        paths.iter().map(std::path::PathBuf::from).collect();
                    worker.send(SftpRequest::Upload {
                        locals,
                        remote_dir: remote_dir.clone(),
                    });
                    sub_ctx.notify();
                });
            },
            config,
        );
    }

    pub(crate) fn start_file_panel_download(
        &mut self,
        name: String,
        is_dir: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(tab) = self.file_panel_tab() else {
            return;
        };
        if tab.sftp_worker.is_none() {
            return;
        }
        let remote = join_path(&tab.file_panel_state.cwd, &name);
        let display_name = file_panel_leaf_name(&name);
        let tab_id = tab.id.clone();
        if is_dir {
            // 目录：让用户选父文件夹，子目录用 name 创建
            let weak = ctx.handle();
            let file_name = display_name;
            let config = warpui::platform::FilePickerConfiguration::new().folders_only();
            ctx.open_file_picker(
                move |result, view_ctx| {
                    let Ok(paths) = result else { return };
                    let Some(parent_str) = paths.into_iter().next() else {
                        return;
                    };
                    use warpui::UpdateView;
                    let Some(handle) = weak.upgrade(view_ctx) else {
                        return;
                    };
                    view_ctx.update_view(&handle, |view, sub_ctx| {
                        let Some(tab) = view.terminal_tabs.iter().find(|t| t.id == tab_id) else {
                            return;
                        };
                        let Some(worker) = tab.sftp_worker.as_ref() else {
                            return;
                        };
                        let local = std::path::PathBuf::from(parent_str).join(&file_name);
                        worker.send(SftpRequest::Download {
                            remote: remote.clone(),
                            local,
                            file_name: file_name.clone(),
                            is_dir: true,
                        });
                        sub_ctx.notify();
                    });
                },
                config,
            );
        } else {
            let config = warpui::platform::SaveFilePickerConfiguration::new()
                .with_default_filename(display_name.clone());
            let file_name = display_name;
            ctx.open_save_file_picker(
                move |chosen, view, view_ctx| {
                    let Some(local_str) = chosen else { return };
                    let Some(tab) = view.terminal_tabs.iter().find(|t| t.id == tab_id) else {
                        return;
                    };
                    let Some(worker) = tab.sftp_worker.as_ref() else {
                        return;
                    };
                    worker.send(SftpRequest::Download {
                        remote: remote.clone(),
                        local: std::path::PathBuf::from(local_str),
                        file_name: file_name.clone(),
                        is_dir: false,
                    });
                    view_ctx.notify();
                },
                config,
            );
        }
    }

    pub(crate) fn reveal_local_file_panel_path(&mut self, path: &str, ctx: &mut ViewContext<Self>) {
        if let Err(error) = reveal_file_manager_path(path) {
            self.host_state.notice = Some(error);
            ctx.notify();
        }
    }

    /// 文件面板宿主 tab 是否本地（决定能否「用查看器 / 编辑器打开」）。
    fn file_panel_tab_is_local(&self) -> bool {
        self.file_panel_tab()
            .is_some_and(|tab| matches!(tab.kind, TerminalSessionKind::Local))
    }

    /// 「打开」：系统默认关联程序。仅本地。
    pub(in crate::root_view) fn handle_file_panel_open_with_default(
        &mut self,
        path: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        if !self.file_panel_tab_is_local() {
            return;
        }
        if let Err(error) = crate::external_editor::open_path_with_default(path) {
            self.host_state.notice = Some(error);
            ctx.notify();
        }
    }

    /// 「编辑」：配置的编辑器；二进制文件回退「打开」。仅本地。
    pub(in crate::root_view) fn handle_file_panel_open_in_editor(
        &mut self,
        path: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        if !self.file_panel_tab_is_local() {
            return;
        }
        let result = if warp_util::file_type::is_binary_file(path) {
            crate::external_editor::open_path_with_default(path)
        } else {
            crate::external_editor::open_path_with_editor(self.open_file_editor, path)
        };
        if let Err(error) = result {
            self.host_state.notice = Some(error);
            ctx.notify();
        }
    }

    /// 「用编辑器打开」：本地 / 远程文本文件进内置编辑器（ADR 0003/0005）。
    /// 本地超大 / 二进制回退「用外部程序打开」；远程的超大 / 二进制在 SFTP 读任务里判定（提示下载）。
    pub(in crate::root_view) fn handle_file_panel_open_in_code_viewer(
        &mut self,
        path: String,
        ctx: &mut ViewContext<Self>,
    ) {
        // 源终端标签 id（复用匹配按 host_id == source）+ 远程 handle（仅远程 tab 有）。
        let Some((source_tab_id, remote_handle)) = self
            .file_panel_tab()
            .map(|tab| (tab.id.clone(), tab.ssh_handle.clone()))
        else {
            return;
        };

        if self.file_panel_tab_is_local() {
            // 内容嗅探（读前 1KB）而非扩展名判二进制：无扩展名 / 点开头的文本文件（如 .viminfo）
            // 不被纯扩展名/文件名白名单误判（Matt 反馈）。先判超大避免对大文件读取。
            if Self::code_viewer_file_too_large(&path)
                || warp_util::file_type::is_file_content_binary(&path)
            {
                if let Err(error) = crate::external_editor::open_path_with_default(&path) {
                    self.host_state.notice = Some(error);
                    ctx.notify();
                }
                return;
            }
            self.open_code_viewer_tab(path, source_tab_id, ctx);
        } else {
            let Some(handle) = remote_handle else {
                self.host_state.notice =
                    Some(rust_i18n::t!("code_viewer_remote_not_ready").to_string());
                ctx.notify();
                return;
            };
            self.open_remote_code_viewer_tab(path, source_tab_id, handle, ctx);
        }
    }
}
