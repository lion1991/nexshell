// code viewer section — RootView 的内置编辑器：复用 Warp CodeEditorView，本地 + 远程文本可编辑可存。
//
// 详见 docs/adr/0002/0003/0005。本文件只含 impl RootView + enum PostSave（异步保存续作，pub(in crate::root_view)），无自由函数。
// 调用方：file_panel_section::actions 双击/右键 → handle_file_panel_open_in_code_viewer → open_code_viewer_tab / open_remote_code_viewer_tab。

use std::path::PathBuf;

use crate::file_panel_view_helpers::{code_viewer_tab_id, code_viewer_tab_label, file_panel_message};
use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{RootView, TerminalSessionKind, TerminalSessionTab};
use nexshell::remote_edit_io::{
    self, RemoteMeta, RemoteReadOutcome, RemoteSaveOutcome,
};
use nexshell::ssh_session::SshHandle;
use nexshell::terminal_runtime::LocalTerminalRuntime;
use nexshell::text_editor::InteractionState;
use nexshell::code_editor::{CodeEditorEvent, CodeEditorRenderOptions, CodeEditorView};
use warp_editor::content::buffer::InitialBufferState;
use warp_editor::render::element::VerticalExpansionBehavior;
use warpui::elements::{
    Border, ChildView, Clipped, Container, CrossAxisAlignment, DispatchEventResult, EventHandler,
    Expanded, Flex, MainAxisSize, ParentElement, Text,
};
use warpui::modals::{AlertDialogWithCallbacks, ModalButton};
use warpui::{AppContext, Element, ViewContext, ViewHandle};

/// 本地文本文件查看上限：超过则回退「用外部程序打开」，避免大文件卡死渲染。
const CODE_VIEWER_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// 保存成功后的续作。本地同步保存当场执行；远程异步保存在写成功的回调里执行。
/// pub(in crate::root_view) 以便 RootView 的 code_viewer_pending_post 字段持有（review C）。
#[derive(Clone)]
pub(in crate::root_view) enum PostSave {
    /// 仅保存（Cmd+S）。
    None,
    /// 保存后关闭该 tab（关闭确认 / 批量关闭）。
    CloseTab,
    /// 保存后把新内容换进该 tab（reuse 换文件确认）。
    Reuse {
        content: String,
        path: String,
        handle: Option<SshHandle>,
        meta: Option<RemoteMeta>,
    },
}

impl RootView {
    /// 在内置编辑器中打开本地文本文件（同步读 fs）。二进制 / 超大已在 handler 排除。
    pub(in crate::root_view) fn open_code_viewer_tab(
        &mut self,
        path: String,
        source_tab_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => {
                // 内容嗅探判为文本但读不出 UTF-8（非 UTF-8 编码 / 权限等）→ 回退外部程序。
                if let Err(error) = crate::external_editor::open_path_with_default(&path) {
                    self.host_state.notice = Some(error);
                    ctx.notify();
                }
                return;
            }
        };
        self.place_code_viewer_content(source_tab_id, path, content, None, None, ctx);
    }

    /// 在内置编辑器中打开远程文本文件（ADR 0005）：克隆 handle 异步 SFTP 读，结果回来再落 tab。
    pub(in crate::root_view) fn open_remote_code_viewer_tab(
        &mut self,
        path: String,
        source_tab_id: String,
        handle: SshHandle,
        ctx: &mut ViewContext<Self>,
    ) {
        let rx = remote_edit_io::spawn_remote_read(
            handle.clone(),
            path.clone(),
            CODE_VIEWER_MAX_BYTES as usize,
        );
        ctx.spawn_stream_local(
            rx,
            move |me, outcome, ctx| {
                me.on_remote_read_outcome(
                    outcome,
                    source_tab_id.clone(),
                    path.clone(),
                    handle.clone(),
                    ctx,
                );
            },
            |_, _| {},
        );
    }

    /// 远程读结果回灌：文本→落 tab；超大 / 二进制 / 非 UTF-8 → 提示先下载；出错 → notice。
    fn on_remote_read_outcome(
        &mut self,
        outcome: RemoteReadOutcome,
        source_tab_id: String,
        path: String,
        handle: SshHandle,
        ctx: &mut ViewContext<Self>,
    ) {
        match outcome {
            RemoteReadOutcome::Text { content, meta } => {
                self.place_code_viewer_content(
                    source_tab_id,
                    path,
                    content,
                    Some(handle),
                    Some(meta),
                    ctx,
                );
            }
            RemoteReadOutcome::TooLarge
            | RemoteReadOutcome::Binary
            | RemoteReadOutcome::NotUtf8 => {
                self.host_state.notice =
                    Some(rust_i18n::t!("code_viewer_remote_download_first").to_string());
                ctx.notify();
            }
            RemoteReadOutcome::Error(error) => {
                self.host_state.notice = Some(error);
                ctx.notify();
            }
        }
    }

    /// reuse-或-create 落内容到编辑器标签：本地 handle=None，远程 handle=Some + meta（ADR 0005）。
    /// 复用开启（默认）：同源终端标签共用一个编辑器标签，命中即换内容；关闭：每文件一个标签。
    fn place_code_viewer_content(
        &mut self,
        source_tab_id: String,
        path: String,
        content: String,
        handle: Option<SshHandle>,
        meta: Option<RemoteMeta>,
        ctx: &mut ViewContext<Self>,
    ) {
        let reuse = self.reuse_view_tab;
        if let Some(idx) = self.terminal_tabs.iter().position(|tab| {
            matches!(tab.kind, TerminalSessionKind::CodeViewer)
                && tab.host_id.as_deref() == Some(source_tab_id.as_str())
                && (reuse || tab.code_viewer_path.as_deref() == Some(path.as_str()))
        }) {
            // 当前 tab 有未保存改动则先弹确认；否则直接换内容（ADR 0003）。
            if self.terminal_tabs[idx].code_viewer_dirty {
                self.confirm_discard_code_viewer_reuse(idx, content, path, handle, meta, ctx);
            } else {
                self.apply_code_viewer_reuse(idx, content, path, handle, meta, ctx);
            }
            return;
        }

        let session_id = code_viewer_tab_id(&source_tab_id, &path);
        let terminal = LocalTerminalRuntime::failed(&session_id, "code viewer");
        let label = code_viewer_tab_label(&path);
        self.push_terminal_tab(
            terminal,
            &session_id,
            label,
            TerminalSessionKind::CodeViewer,
            Some(source_tab_id),
            None,
            ctx,
        );
        let view = self.build_code_viewer_view(&content, &path, &session_id, ctx);
        // 脏判定基线用编辑器归一化后的文本（view.text() 按 primary 行尾重建），而非原始字节，
        // 否则混合行尾文件 round-trip 后 text()≠原始 content，会「打开即脏」（审查 #3）。
        let baseline = view.as_ref(ctx).text(ctx).into_string();
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            tab.code_viewer = Some(view);
            tab.code_viewer_path = Some(path);
            tab.code_viewer_saved_content = Some(baseline);
            tab.code_viewer_dirty = false;
            tab.code_viewer_ssh_handle = handle;
            tab.code_viewer_remote_meta = meta;
        }
    }

    /// 文件超过查看上限时为真（调用方据此回退「用外部程序打开」）。
    pub(in crate::root_view) fn code_viewer_file_too_large(path: &str) -> bool {
        std::fs::metadata(path)
            .map(|meta| meta.len() > CODE_VIEWER_MAX_BYTES)
            .unwrap_or(false)
    }

    /// 构造可编辑 CodeEditorView：纯文本 buffer + 按路径推断语言高亮 + 可编辑 + 订阅变更维护脏标记（ADR 0003）。
    fn build_code_viewer_view(
        &self,
        content: &str,
        path: &str,
        tab_id: &str,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<CodeEditorView> {
        let view = ctx.add_typed_action_view(|ctx| {
            CodeEditorView::new(
                None,
                None,
                CodeEditorRenderOptions::new(VerticalExpansionBehavior::FillMaxHeight),
                ctx,
            )
        });
        let model = view.as_ref(ctx).model.clone();
        let path_buf = PathBuf::from(path);
        model.update(ctx, |model, ctx| {
            model.reset_content(InitialBufferState::plain_text(content), ctx);
            // 按扩展名/文件名推断 tree-sitter 语言，触发语法高亮。
            // 上游 set_language_with_path 改收 &StandardizedPath；本地路径走 _local 版（收 &Path）。
            model.set_language_with_local_path(&path_buf, ctx);
            // 可编辑（ADR 0003）：编辑 + Cmd+S 保存；二进制 / 超大已在 handler 排除。
            model.set_interaction_state(InteractionState::Editable, ctx);
        });
        // reset 之后再订阅，避免重载自身触发误标脏；脏判定用「当前 text vs 已保存基线」文本对比
        // （EditOrigin::UserInitiated 同时用于重载与粘贴/退格，无法区分，故不能靠 origin）。
        let tab_id = tab_id.to_string();
        ctx.subscribe_to_view(&view, move |me, handle, event: &CodeEditorEvent, ctx| {
            if matches!(event, CodeEditorEvent::ContentChanged { .. }) {
                me.refresh_code_viewer_dirty(&tab_id, &handle, ctx);
            }
        });
        view
    }

    /// 对比编辑器当前内容与已保存基线，刷新指定 tab 的脏标记。
    fn refresh_code_viewer_dirty(
        &mut self,
        tab_id: &str,
        view: &ViewHandle<CodeEditorView>,
        ctx: &mut ViewContext<Self>,
    ) {
        let text = view.as_ref(ctx).text(ctx);
        let Some(tab) = self.terminal_tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let dirty = tab.code_viewer_saved_content.as_deref() != Some(text.as_str());
        if tab.code_viewer_dirty != dirty {
            tab.code_viewer_dirty = dirty;
            ctx.notify();
        }
    }

    /// 保存内置编辑器当前文件（Cmd+S）：仅 active 为编辑器标签时触发（ADR 0003/0005）。
    pub(in crate::root_view) fn handle_code_viewer_save(&mut self, ctx: &mut ViewContext<Self>) {
        let idx = self.active_tab_index;
        if self
            .terminal_tabs
            .get(idx)
            .map_or(false, |t| matches!(t.kind, TerminalSessionKind::CodeViewer))
        {
            self.start_code_viewer_save(idx, PostSave::None, ctx);
        }
    }

    /// 保存指定编辑器 tab：本地同步 fs::write（成功即执行 post）；远程乐观异步 SFTP 写（ADR 0005）。
    fn start_code_viewer_save(&mut self, idx: usize, post: PostSave, ctx: &mut ViewContext<Self>) {
        let Some(tab) = self.terminal_tabs.get(idx) else {
            return;
        };
        let (Some(view), Some(path)) = (tab.code_viewer.clone(), tab.code_viewer_path.clone())
        else {
            return;
        };
        let saving = tab.code_viewer_saving;
        let handle = tab.code_viewer_ssh_handle.clone();
        let tab_id = tab.id.clone();
        let expected = tab.code_viewer_remote_meta;

        // 远程保存在途：不并发再写；带续作（关闭/换文件）则暂存，待写成功后补执行（review C）。
        if saving {
            if !matches!(post, PostSave::None) {
                self.code_viewer_pending_post.insert(tab_id, post);
            }
            return;
        }
        let text = view.as_ref(ctx).text(ctx).into_string();

        match handle {
            // 本地：同步覆盖写（ADR 0003 行为不变）。
            None => match std::fs::write(&path, &text) {
                Ok(()) => {
                    if let Some(tab) = self.terminal_tabs.get_mut(idx) {
                        tab.code_viewer_saved_content = Some(text);
                        tab.code_viewer_dirty = false;
                    }
                    self.host_state.notice = Some(rust_i18n::t!("code_viewer_saved").to_string());
                    ctx.notify();
                    self.run_post_save(post, &tab_id, ctx);
                }
                Err(error) => {
                    self.host_state.notice = Some(
                        rust_i18n::t!("code_viewer_save_error", error = error.to_string().as_str())
                            .to_string(),
                    );
                    ctx.notify();
                }
            },
            // 远程：乐观异步，先冲突检测（force=false）。
            Some(handle) => {
                self.fire_remote_save(tab_id, path, handle, text, expected, false, post, ctx);
            }
        }
    }

    /// 远程异步保存引擎：置「保存中」态、起任务，结果回 on_remote_save_outcome（ADR 0005）。
    /// force=true 跳过冲突检测（用户在冲突弹窗里选了「覆盖」）。
    fn fire_remote_save(
        &mut self,
        tab_id: String,
        path: String,
        handle: SshHandle,
        snapshot: String,
        expected: Option<RemoteMeta>,
        force: bool,
        post: PostSave,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(tab) = self.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.code_viewer_saving = true;
        }
        ctx.notify();
        let rx = remote_edit_io::spawn_remote_save(
            handle.clone(),
            path.clone(),
            snapshot.clone().into_bytes(),
            expected,
            force,
        );
        ctx.spawn_stream_local(
            rx,
            move |me, outcome, ctx| {
                me.on_remote_save_outcome(
                    outcome,
                    tab_id.clone(),
                    path.clone(),
                    handle.clone(),
                    snapshot.clone(),
                    post.clone(),
                    ctx,
                );
            },
            |_, _| {},
        );
    }

    /// 远程保存结果回灌：成功→更新基线/meta + 清保存中 + 重算脏 + 执行 post；
    /// 冲突→弹覆盖/取消；出错→notice + 保留脏（编辑器仍持内容可重试，ADR 0005）。
    fn on_remote_save_outcome(
        &mut self,
        outcome: RemoteSaveOutcome,
        tab_id: String,
        path: String,
        handle: SshHandle,
        snapshot: String,
        post: PostSave,
        ctx: &mut ViewContext<Self>,
    ) {
        // 身份校验：tab 已关 / 已被换成别的文件 → 本次异步结果作废（review F）。
        let still_same = self
            .terminal_tabs
            .iter()
            .find(|t| t.id == tab_id)
            .map_or(false, |t| t.code_viewer_path.as_deref() == Some(path.as_str()));
        if !still_same {
            // tab 已换文件/已关：本次结果作废。不动 pending——它属于当前占用者，由其生命周期管理。
            return;
        }
        if let Some(tab) = self.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.code_viewer_saving = false;
        }
        match outcome {
            RemoteSaveOutcome::Saved { meta } => {
                let view = self
                    .terminal_tabs
                    .iter()
                    .find(|t| t.id == tab_id)
                    .and_then(|t| t.code_viewer.clone());
                if let Some(tab) = self.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.code_viewer_saved_content = Some(snapshot);
                    tab.code_viewer_remote_meta = Some(meta);
                }
                self.host_state.notice = Some(rust_i18n::t!("code_viewer_saved").to_string());
                // 保存中可能又改了：按当前编辑器文本 vs 新基线重算脏。
                if let Some(view) = view {
                    self.refresh_code_viewer_dirty(&tab_id, &view, ctx);
                }
                ctx.notify();
                self.run_post_save(post, &tab_id, ctx);
                // 补执行保存在途期间暂存的续作（review C）。
                if let Some(pending) = self.code_viewer_pending_post.remove(&tab_id) {
                    self.run_post_save(pending, &tab_id, ctx);
                }
            }
            RemoteSaveOutcome::Conflict { .. } => {
                self.code_viewer_pending_post.remove(&tab_id);
                ctx.notify();
                self.confirm_overwrite_remote(tab_id, path, handle, post, ctx);
            }
            RemoteSaveOutcome::Error(error) => {
                self.code_viewer_pending_post.remove(&tab_id);
                self.host_state.notice = Some(
                    rust_i18n::t!("code_viewer_save_error", error = error.as_str()).to_string(),
                );
                ctx.notify();
            }
        }
    }

    /// 远程文件保存前被外部改动：弹「覆盖 / 取消」。
    /// 覆盖→重读编辑器**当前**文本 force 重存（不用旧快照，避免丢失弹窗期间的编辑，review D）。
    fn confirm_overwrite_remote(
        &mut self,
        tab_id: String,
        path: String,
        handle: SshHandle,
        post: PostSave,
        ctx: &mut ViewContext<Self>,
    ) {
        let name = self.code_viewer_unsaved_name(&tab_id);
        ctx.show_native_platform_modal(AlertDialogWithCallbacks::for_view(
            rust_i18n::t!("code_viewer_remote_conflict_title"),
            rust_i18n::t!("code_viewer_remote_conflict_message", name = name.as_str()).to_string(),
            vec![
                ModalButton::for_view(rust_i18n::t!("code_viewer_remote_conflict_overwrite"), {
                    let (tab_id, path, handle, post) =
                        (tab_id.clone(), path.clone(), handle.clone(), post.clone());
                    move |view: &mut Self, ctx: &mut ViewContext<Self>| {
                        let Some(tab) = view.terminal_tabs.iter().find(|t| t.id == tab_id) else {
                            return;
                        };
                        // tab 已被换文件则放弃覆盖（review F）。
                        if tab.code_viewer_path.as_deref() != Some(path.as_str()) {
                            return;
                        }
                        let Some(editor) = tab.code_viewer.clone() else {
                            return;
                        };
                        let text = editor.as_ref(ctx).text(ctx).into_string();
                        view.fire_remote_save(
                            tab_id.clone(),
                            path.clone(),
                            handle.clone(),
                            text,
                            None,
                            true,
                            post.clone(),
                            ctx,
                        );
                    }
                }),
                ModalButton::for_view(
                    rust_i18n::t!("dialog_cancel"),
                    |_: &mut Self, _: &mut ViewContext<Self>| {},
                ),
            ],
            |_: &mut Self, _: &mut ViewContext<Self>| {},
        ));
    }

    /// 执行保存成功后的续作（ADR 0005）。
    fn run_post_save(&mut self, post: PostSave, tab_id: &str, ctx: &mut ViewContext<Self>) {
        match post {
            PostSave::None => {}
            PostSave::CloseTab => {
                if let Some(idx) = self.terminal_tabs.iter().position(|t| t.id == tab_id) {
                    self.close_terminal_tab_inner(idx, ctx);
                }
            }
            PostSave::Reuse {
                content,
                path,
                handle,
                meta,
            } => {
                if let Some(idx) = self.terminal_tabs.iter().position(|t| t.id == tab_id) {
                    self.apply_code_viewer_reuse(idx, content, path, handle, meta, ctx);
                }
            }
        }
    }

    /// 关闭 dirty 的 CodeViewer 前弹三按钮确认（保存→存后关 / 不保存→直接关 / 取消，ADR 0003）。
    pub(in crate::root_view) fn confirm_discard_code_viewer_close(
        &mut self,
        tab_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        let name = self.code_viewer_unsaved_name(&tab_id);
        ctx.show_native_platform_modal(AlertDialogWithCallbacks::for_view(
            rust_i18n::t!("code_viewer_unsaved_title"),
            rust_i18n::t!("code_viewer_unsaved_message", name = name.as_str()).to_string(),
            vec![
                ModalButton::for_view(rust_i18n::t!("code_viewer_unsaved_save"), {
                    let tab_id = tab_id.clone();
                    move |view: &mut Self, ctx: &mut ViewContext<Self>| {
                        if let Some(idx) = view.terminal_tabs.iter().position(|t| t.id == tab_id) {
                            // 本地同步存完即关；远程异步存成功才自关（ADR 0005）。
                            view.start_code_viewer_save(idx, PostSave::CloseTab, ctx);
                        }
                    }
                }),
                ModalButton::for_view(rust_i18n::t!("code_viewer_unsaved_discard"), {
                    let tab_id = tab_id.clone();
                    move |view: &mut Self, ctx: &mut ViewContext<Self>| {
                        if let Some(idx) = view.terminal_tabs.iter().position(|t| t.id == tab_id) {
                            view.close_terminal_tab_inner(idx, ctx);
                        }
                    }
                }),
                ModalButton::for_view(
                    rust_i18n::t!("dialog_cancel"),
                    |_: &mut Self, _: &mut ViewContext<Self>| {},
                ),
            ],
            |_: &mut Self, _: &mut ViewContext<Self>| {},
        ));
    }

    /// reuse 换文件时当前编辑器 dirty 的三按钮确认（保存/不保存→换内容 / 取消保持当前）。
    /// handle/meta 是「换进来的新文件」的来源（本地 None / 远程 Some），透传给换内容。
    fn confirm_discard_code_viewer_reuse(
        &mut self,
        idx: usize,
        content: String,
        path: String,
        handle: Option<SshHandle>,
        meta: Option<RemoteMeta>,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(tab_id) = self.terminal_tabs.get(idx).map(|t| t.id.clone()) else {
            return;
        };
        let name = self.code_viewer_unsaved_name(&tab_id);
        ctx.show_native_platform_modal(AlertDialogWithCallbacks::for_view(
            rust_i18n::t!("code_viewer_unsaved_title"),
            rust_i18n::t!("code_viewer_unsaved_message", name = name.as_str()).to_string(),
            vec![
                ModalButton::for_view(rust_i18n::t!("code_viewer_unsaved_save"), {
                    let (tab_id, content, path, handle, meta) =
                        (tab_id.clone(), content.clone(), path.clone(), handle.clone(), meta);
                    move |view: &mut Self, ctx: &mut ViewContext<Self>| {
                        if let Some(idx) = view.terminal_tabs.iter().position(|t| t.id == tab_id) {
                            // 先存当前内容（本地同步 / 远程异步），成功后换进新文件。
                            view.start_code_viewer_save(
                                idx,
                                PostSave::Reuse {
                                    content: content.clone(),
                                    path: path.clone(),
                                    handle: handle.clone(),
                                    meta,
                                },
                                ctx,
                            );
                        }
                    }
                }),
                ModalButton::for_view(rust_i18n::t!("code_viewer_unsaved_discard"), {
                    let (tab_id, content, path, handle, meta) =
                        (tab_id.clone(), content.clone(), path.clone(), handle.clone(), meta);
                    move |view: &mut Self, ctx: &mut ViewContext<Self>| {
                        if let Some(idx) = view.terminal_tabs.iter().position(|t| t.id == tab_id) {
                            view.apply_code_viewer_reuse(
                                idx,
                                content.clone(),
                                path.clone(),
                                handle.clone(),
                                meta,
                                ctx,
                            );
                        }
                    }
                }),
                ModalButton::for_view(
                    rust_i18n::t!("dialog_cancel"),
                    |_: &mut Self, _: &mut ViewContext<Self>| {},
                ),
            ],
            |_: &mut Self, _: &mut ViewContext<Self>| {},
        ));
    }

    /// reuse 命中 tab 的实际换内容：重建 view + 刷新基线/标签/来源(handle,meta) + 激活。
    fn apply_code_viewer_reuse(
        &mut self,
        idx: usize,
        content: String,
        path: String,
        handle: Option<SshHandle>,
        meta: Option<RemoteMeta>,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(tab_id) = self.terminal_tabs.get(idx).map(|t| t.id.clone()) else {
            return;
        };
        let view = self.build_code_viewer_view(&content, &path, &tab_id, ctx);
        // 基线用归一化文本（同 place_code_viewer_content，避免混合行尾「打开即脏」，审查 #3）。
        let baseline = view.as_ref(ctx).text(ctx).into_string();
        if let Some(tab) = self.terminal_tabs.get_mut(idx) {
            tab.code_viewer = Some(view);
            tab.code_viewer_path = Some(path.clone());
            tab.code_viewer_saved_content = Some(baseline);
            tab.code_viewer_dirty = false;
            // 换文件即新会话：清旧文件残留的保存中态，旧在途保存回来会因路径不符作废（review F）。
            tab.code_viewer_saving = false;
            tab.code_viewer_ssh_handle = handle;
            tab.code_viewer_remote_meta = meta;
            tab.fallback_label = code_viewer_tab_label(&path);
            tab.custom_label = None;
        }
        self.code_viewer_pending_post.remove(&tab_id);
        self.activate_terminal_tab(idx, ctx);
    }

    /// 是否存在未保存或保存中的编辑器标签（关窗 / 退出 app 前检测）。
    /// 含 code_viewer_saving：远程异步写在途时退出会无声打断（review A）。
    pub(crate) fn has_unsaved_code_viewer(&self) -> bool {
        self.terminal_tabs.iter().any(|t| {
            matches!(t.kind, TerminalSessionKind::CodeViewer)
                && (t.code_viewer_dirty || t.code_viewer_saving)
        })
    }

    /// 给定一组 tab index，收集其中 dirty CodeViewer 的 tab_id（批量关闭未保存检测，审查 #2）。
    pub(in crate::root_view) fn dirty_code_viewer_ids_in(&self, indices: &[usize]) -> Vec<String> {
        indices
            .iter()
            .filter_map(|&i| self.terminal_tabs.get(i))
            .filter(|t| matches!(t.kind, TerminalSessionKind::CodeViewer) && t.code_viewer_dirty)
            .map(|t| t.id.clone())
            .collect()
    }

    /// 批量关闭（关闭其他 / 关闭右侧）含 dirty CodeViewer 时弹一次汇总三按钮确认（审查 #2）。
    /// close_right=true → 关闭右侧，否则关闭其他；回调按 anchor tab_id 重定位再执行批量关闭。
    pub(in crate::root_view) fn confirm_discard_code_viewer_batch(
        &mut self,
        dirty_ids: Vec<String>,
        anchor: String,
        close_right: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let count = dirty_ids.len();
        ctx.show_native_platform_modal(AlertDialogWithCallbacks::for_view(
            rust_i18n::t!("code_viewer_unsaved_title"),
            rust_i18n::t!("code_viewer_unsaved_batch_message", count = count).to_string(),
            vec![
                ModalButton::for_view(rust_i18n::t!("code_viewer_unsaved_save_all"), {
                    let (dirty_ids, anchor) = (dirty_ids.clone(), anchor.clone());
                    move |view: &mut Self, ctx: &mut ViewContext<Self>| {
                        // 待关集合先取 id（关闭会移动 index）。脏编辑器各自存（本地同步关 / 远程异步存成功自关，
                        // 失败保留标签）；其余非脏 tab 立即关（ADR 0005）。
                        let close_ids = view.batch_close_target_ids(&anchor, close_right);
                        for id in &dirty_ids {
                            if let Some(idx) = view.terminal_tabs.iter().position(|t| t.id == *id) {
                                view.start_code_viewer_save(idx, PostSave::CloseTab, ctx);
                            }
                        }
                        for id in &close_ids {
                            if dirty_ids.contains(id) {
                                continue;
                            }
                            if let Some(idx) = view.terminal_tabs.iter().position(|t| t.id == *id) {
                                view.close_terminal_tab_inner(idx, ctx);
                            }
                        }
                        // 焦点对齐到 anchor（与「不保存」路径一致；逐个 close_inner 的位移逻辑不保证落在 anchor，review G）。
                        if let Some(idx) = view.terminal_tabs.iter().position(|t| t.id == anchor) {
                            view.activate_terminal_tab(idx, ctx);
                        }
                    }
                }),
                ModalButton::for_view(rust_i18n::t!("code_viewer_unsaved_discard"), {
                    let anchor = anchor.clone();
                    move |view: &mut Self, ctx: &mut ViewContext<Self>| {
                        view.run_code_viewer_batch_close(&anchor, close_right, ctx);
                    }
                }),
                ModalButton::for_view(
                    rust_i18n::t!("dialog_cancel"),
                    |_: &mut Self, _: &mut ViewContext<Self>| {},
                ),
            ],
            |_: &mut Self, _: &mut ViewContext<Self>| {},
        ));
    }

    /// 按 anchor tab_id 重定位 index 后执行对应批量关闭 inner（保存不改标签集合，index 关系稳定）。
    fn run_code_viewer_batch_close(
        &mut self,
        anchor: &str,
        close_right: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(index) = self.terminal_tabs.iter().position(|t| t.id == anchor) else {
            return;
        };
        if close_right {
            self.close_terminal_tabs_right_inner(index, ctx);
        } else {
            self.close_other_terminal_tabs_inner(index, ctx);
        }
    }

    /// 批量关闭的目标 tab id 集（不含 anchor）。先取 id 因关闭会移动 index（ADR 0005 save-all 用）。
    fn batch_close_target_ids(&self, anchor: &str, close_right: bool) -> Vec<String> {
        let Some(anchor_idx) = self.terminal_tabs.iter().position(|t| t.id == anchor) else {
            return Vec::new();
        };
        self.terminal_tabs
            .iter()
            .enumerate()
            .filter(|(i, _)| if close_right { *i > anchor_idx } else { *i != anchor_idx })
            .map(|(_, t)| t.id.clone())
            .collect()
    }

    /// 未保存确认弹窗里展示的标签名（找不到回退空串）。
    fn code_viewer_unsaved_name(&self, tab_id: &str) -> String {
        self.terminal_tabs
            .iter()
            .find(|t| t.id == tab_id)
            .map(|t| t.label())
            .unwrap_or_default()
    }

    /// 远程编辑标签的断线重连横幅：源终端断开时返回可点击提示，否则 None（中断重连体验）。
    fn code_viewer_reconnect_banner(
        &self,
        tab: &TerminalSessionTab,
        colors: &HostOverviewColors,
    ) -> Option<Box<dyn Element>> {
        // 仅远程编辑标签（本地文件无连接概念）。
        if tab.code_viewer_ssh_handle.is_none() {
            return None;
        }
        let source = tab
            .host_id
            .as_ref()
            .and_then(|hid| self.terminal_tabs.iter().position(|t| &t.id == hid));
        // 源终端找不到（已关）或未连通都视为断开。
        let connected = source
            .map(|i| Self::terminal_tab_is_connected(&self.terminal_tabs[i]))
            .unwrap_or(false);
        if connected {
            return None;
        }
        let text = if tab.code_viewer_dirty {
            rust_i18n::t!("code_viewer_reconnect_banner_dirty")
        } else {
            rust_i18n::t!("code_viewer_reconnect_banner")
        };
        let label = Container::new(
            Text::new_inline(text.to_string(), self.ui_font, 11.0)
                .with_color(colors.warning)
                .finish(),
        )
        .with_horizontal_padding(12.0)
        .with_vertical_padding(6.0)
        .with_border(Border::bottom(1.0).with_border_color(colors.panel_border))
        .finish();
        match source {
            Some(index) => Some(
                EventHandler::new(label)
                    .on_left_mouse_down(move |ctx, _, _| {
                        ctx.dispatch_typed_action(TerminalGridAction::ReconnectTab(index));
                        DispatchEventResult::StopPropagation
                    })
                    .finish(),
            ),
            None => Some(label),
        }
    }

    /// 渲染只读代码查看器主体（不含侧栏）。侧栏由 render_active_tab_body_with_side_panels 包裹，
    /// 与 GitDiff 同属「内容 + 可选文件/git 侧栏」类，故打开文件后文件面板按钮仍可用。
    pub(in crate::root_view) fn render_code_viewer_page(&self, _app: &AppContext) -> Box<dyn Element> {
        let colors = HostOverviewColors::from_theme(&self.cached_warp_theme);
        let Some(tab) = self.terminal_tabs.get(self.active_tab_index) else {
            return file_panel_message(
                &rust_i18n::t!("code_viewer_loading"),
                self.ui_font,
                colors.text_muted,
            );
        };

        let header = Container::new(
            Clipped::new(
                Text::new_inline(
                    tab.code_viewer_path
                        .clone()
                        .unwrap_or_else(|| tab.label()),
                    self.monospace_font,
                    11.0,
                )
                .with_color(colors.text_muted)
                .finish(),
            )
            .finish(),
        )
        .with_horizontal_padding(12.0)
        .with_vertical_padding(8.0)
        .with_border(Border::bottom(1.0).with_border_color(colors.panel_border))
        .finish();

        let body: Box<dyn Element> = if let Some(view) = tab.code_viewer.as_ref() {
            ChildView::new(view).finish()
        } else {
            file_panel_message(
                &rust_i18n::t!("code_viewer_loading"),
                self.ui_font,
                colors.text_muted,
            )
        };

        // 远程编辑标签 + 源终端断开：顶部条引导重连，避免改了存不回却无提示（缺陷1/3 体验）。
        let reconnect_banner = self.code_viewer_reconnect_banner(tab, &colors);

        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(header);
        if let Some(banner) = reconnect_banner {
            column = column.with_child(banner);
        }
        let column = column.with_child(Expanded::new(1.0, body).finish());

        Container::new(column.finish())
            .with_background_color(colors.panel_bg)
            .finish()
    }
}
