// host_library_section::actions — RootView 的主机库 action handler。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。
// 每个 handle_host_* 由 root_view/mod.rs handle_action match arm 单行分发；handle_action 按引用匹配，
// 故各 handler 取 owned 形参（String / Copy enum），调用方传 host_id.clone() / *mode 等。
// 操作实现散在同 section 的 operations / transfer / edit_window / editors，跨文件 self.xxx() 调用。

use crate::host_edit_window::HostEditDraft;
use crate::RootView;
use nexshell::host_management::{
    HostClipboardOp, HostConnectionConfig, HostViewMode, ProtocolFilter,
};
use pathfinder_geometry::rect::RectF;
use warpui::ViewContext;

impl RootView {
    pub(in crate::root_view) fn handle_host_clipboard_copy(
        &mut self,
        host_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(snap) = self.host_state.host_by_id(&host_id).cloned() {
            self.host_state.host_clipboard = Some((snap, HostClipboardOp::Copy));
            self.host_state.notice = Some(rust_i18n::t!("toast_host_copied").to_string());
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn handle_host_clipboard_cut(
        &mut self,
        host_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(snap) = self.host_state.host_by_id(&host_id).cloned() {
            self.host_state.host_clipboard = Some((snap, HostClipboardOp::Cut));
            self.host_state.notice = Some(rust_i18n::t!("toast_host_cut").to_string());
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn handle_host_clipboard_paste(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_clipboard_paste(ctx);
    }

    pub(in crate::root_view) fn handle_host_restore_deleted(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_restore_deleted(ctx);
    }

    pub(in crate::root_view) fn handle_host_rename_inline(
        &mut self,
        host_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        self.show_host_card_context_menu = None;
        self.start_host_rename(host_id, ctx);
    }

    pub(in crate::root_view) fn handle_host_quick_connect(
        &mut self,
        host_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        self.connect_host(&host_id, ctx);
    }

    pub(in crate::root_view) fn handle_host_toggle_select(
        &mut self,
        host_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.toggle_select_host(&host_id);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_select_single(
        &mut self,
        host_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.select_single_host(&host_id);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_toggle_select_all(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.toggle_select_all_filtered();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_select_group(
        &mut self,
        group_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.select_group(&group_id);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_toggle_tag(
        &mut self,
        tag: String,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.toggle_tag(&tag);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_toggle_protocol_dropdown(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        let mut view_states = self.host_view_states.borrow_mut();
        view_states.search_bar.protocol_dropdown_open =
            !view_states.search_bar.protocol_dropdown_open;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_set_protocol_filter(
        &mut self,
        filter: ProtocolFilter,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.set_protocol_filter(filter);
        self.host_view_states
            .borrow_mut()
            .search_bar
            .protocol_dropdown_open = false;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_set_view_mode(
        &mut self,
        mode: HostViewMode,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.set_view_mode(mode);
        // 切换视图复位密钥页瞬时态，避免再回来停在确认 / 展开 / 编辑态
        self.host_state.copy_cmd_expanded = false;
        self.host_state.key_delete_confirming = false;
        self.host_key_edit_target = None;
        if mode == HostViewMode::Status {
            self.start_host_status_fleet(ctx);
        } else {
            self.host_status_fleet.stop_all();
        }
        if mode == HostViewMode::Keys {
            self.reload_host_keys();
        }
        ctx.notify();
    }

    /// 对当前筛选下的 SSH 主机启动状态监控舰队（幂等），事件流接回 fleet。
    fn start_host_status_fleet(&mut self, ctx: &mut ViewContext<Self>) {
        let hosts: Vec<(String, HostConnectionConfig)> = self
            .host_state
            .filtered_hosts()
            .into_iter()
            .filter(|host| host.protocol.eq_ignore_ascii_case("SSH"))
            .map(|host| (host.id, host.connection))
            .collect();
        for (host_id, receiver) in self.host_status_fleet.start(&hosts) {
            ctx.spawn_stream_local(
                receiver,
                move |view, event, ctx| {
                    view.host_status_fleet.apply_event(&host_id, event);
                    ctx.notify();
                },
                |_, _| {},
            );
        }
    }

    /// 重新从库加载密钥列表（含关联主机数）到缓存。
    pub(super) fn reload_host_keys(&mut self) {
        if let Some(db_path) = nexshell::host_management::default_database_path() {
            if let Ok(keys) = nexshell::ssh_key_store::list_ssh_keys_with_usage(&db_path) {
                self.host_keys = keys;
            }
        }
    }

    /// 重算选中密钥的 openssh 公钥缓存（选中 / 编辑保存后共用，避免每帧解密私钥）。
    fn recompute_selected_key_public(&mut self, id: &str) {
        self.host_selected_key_public = self
            .host_keys
            .iter()
            .find(|(record, _)| record.id == id)
            .and_then(|(record, _)| {
                nexshell::ssh_key_store::derive_public_key(
                    &record.content,
                    record.passphrase.as_deref(),
                )
            });
    }

    /// 记录主机访问时间并刷新最近访问缓存。
    pub(in crate::root_view) fn record_host_access(&mut self, host_id: &str) {
        if let Some(db_path) = nexshell::host_management::default_database_path() {
            let _ = nexshell::host_management::record_host_access_in_db(&db_path, host_id);
        }
        self.reload_host_recent();
    }

    /// 从库加载最近访问主机，映射成 名称 / 分组名 / 时间。
    pub(in crate::root_view) fn reload_host_recent(&mut self) {
        let db_path = match nexshell::host_management::default_database_path() {
            Some(p) => p,
            None => return,
        };
        let raw = nexshell::host_management::get_recent_access(&db_path, 6);
        let recent = raw
            .into_iter()
            .filter_map(|(hid, ts)| {
                let host = self.host_state.host_by_id(&hid)?;
                let group_name = host.group_id.as_ref().and_then(|gid| {
                    self.host_state
                        .snapshot
                        .groups
                        .iter()
                        .find(|g| &g.id == gid)
                        .map(|g| g.label.clone())
                });
                Some(nexshell::host_management::RecentHostSnapshot {
                    host_id: hid,
                    name: host.name.clone(),
                    group_name,
                    accessed_at: ts,
                    protocol: host.protocol.clone(),
                })
            })
            .collect();
        self.host_recent = recent;
    }

    /// 导入私钥文件：读内容入库（名称取文件名），刷新缓存。本地文件之后变动不影响连接。
    pub(in crate::root_view) fn handle_host_import_key_file(
        &mut self,
        path: String,
        ctx: &mut ViewContext<Self>,
    ) {
        let db_path = match nexshell::host_management::default_database_path() {
            Some(path) => path,
            None => return,
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                self.host_state.notice = Some(format!("读取私钥失败: {error}"));
                ctx.notify();
                return;
            }
        };
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "private_key".to_string());
        let record = nexshell::ssh_key_store::build_record(&name, &content, None);
        if let Err(error) = nexshell::ssh_key_store::upsert_ssh_key(&db_path, &record) {
            self.host_state.notice = Some(format!("导入密钥失败: {error}"));
            ctx.notify();
            return;
        }
        self.reload_host_keys();
        self.host_state.notice = Some(format!("已导入密钥 {name}"));
        ctx.notify();
    }

    /// 删除密钥：引用它的主机 key_id 会在 ssh_key_store 内被置空。删的是选中项则清详情。
    pub(in crate::root_view) fn handle_host_delete_key(
        &mut self,
        key_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(db_path) = nexshell::host_management::default_database_path() else {
            ctx.notify();
            return;
        };
        match nexshell::ssh_key_store::delete_ssh_key(&db_path, &key_id) {
            Ok(()) => {
                self.reload_host_keys();
                self.host_state.key_delete_confirming = false;
                if self.host_state.selected_key_id.as_deref() == Some(key_id.as_str()) {
                    self.host_state.selected_key_id = None;
                    self.host_selected_key_public = None;
                }
            }
            Err(error) => {
                self.host_state.notice = Some(format!("删除密钥失败: {error}"));
            }
        }
        ctx.notify();
    }

    /// 选中密钥看详情：记录选中 id，并推导 openssh 公钥缓存（加密无口令则 None）。
    pub(in crate::root_view) fn handle_host_select_key(
        &mut self,
        key_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        self.recompute_selected_key_public(&key_id);
        self.host_state.selected_key_id = Some(key_id);
        self.host_state.copy_cmd_expanded = false;
        self.host_state.key_delete_confirming = false;
        self.host_key_edit_target = None;
        ctx.notify();
    }

    /// 复制公钥到服务器：生成 echo >> authorized_keys 命令写入剪贴板，并展开命令展示。
    pub(in crate::root_view) fn handle_host_copy_key_to_server(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(pubkey) = self.host_selected_key_public.clone() {
            let command = format!("echo '{}' >> ~/.ssh/authorized_keys", pubkey.trim());
            ctx.clipboard()
                .write(warpui::clipboard::ClipboardContent::plain_text(command));
            self.host_state.copy_cmd_expanded = true;
        }
        ctx.notify();
    }

    /// 进入编辑态：预填当前选中密钥的名称 / 口令，聚焦名称输入框。
    pub(in crate::root_view) fn handle_host_edit_key(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(id) = self.host_state.selected_key_id.clone() else {
            return;
        };
        let Some((name, pass)) = self
            .host_keys
            .iter()
            .find(|(record, _)| record.id == id)
            .map(|(record, _)| {
                (
                    record.name.clone(),
                    record.passphrase.clone().unwrap_or_default(),
                )
            })
        else {
            return;
        };
        self.host_key_name_editor.update(ctx, move |editor, ctx| {
            editor.clear_buffer_and_reset_undo_stack(ctx);
            if !name.is_empty() {
                editor.insert_selected_text(&name, ctx);
            }
        });
        self.host_key_passphrase_editor
            .update(ctx, move |editor, ctx| {
                editor.clear_buffer_and_reset_undo_stack(ctx);
                if !pass.is_empty() {
                    editor.insert_selected_text(&pass, ctx);
                }
            });
        self.host_key_edit_target = Some(id);
        self.host_state.copy_cmd_expanded = false;
        self.host_state.key_delete_confirming = false;
        ctx.focus(&self.host_key_name_editor);
        ctx.notify();
    }

    /// 保存编辑：写回名称 / 口令（重测类型），刷新缓存与公钥后退出编辑态。
    pub(in crate::root_view) fn handle_host_key_edit_save(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(id) = self.host_key_edit_target.clone() else {
            return;
        };
        let name = self
            .host_key_name_editor
            .as_ref(ctx)
            .buffer_text(ctx)
            .trim()
            .to_string();
        let passphrase = self.host_key_passphrase_editor.as_ref(ctx).buffer_text(ctx);
        let Some(db_path) = nexshell::host_management::default_database_path() else {
            ctx.notify();
            return;
        };
        // 写库成功才退出编辑态并刷新缓存；失败保留编辑态，避免「假成功」误导用户。
        match nexshell::ssh_key_store::update_ssh_key_meta(&db_path, &id, &name, Some(passphrase)) {
            Ok(()) => {
                self.host_key_edit_target = None;
                self.reload_host_keys();
                self.recompute_selected_key_public(&id);
                ctx.focus_self();
            }
            Err(error) => {
                self.host_state.notice = Some(format!("保存密钥失败: {error}"));
            }
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_key_edit_cancel(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_key_edit_target = None;
        ctx.focus_self();
        ctx.notify();
    }

    /// 点「删除密钥」：先进入二次确认态，不立即删除。
    pub(in crate::root_view) fn handle_host_delete_key_prompt(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        if self.host_state.selected_key_id.is_some() {
            self.host_state.key_delete_confirming = true;
            ctx.notify();
        }
    }

    pub(in crate::root_view) fn handle_host_delete_key_cancel(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.key_delete_confirming = false;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_toggle_privacy(&mut self, ctx: &mut ViewContext<Self>) {
        self.host_state.toggle_privacy_mode();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_refresh(&mut self, ctx: &mut ViewContext<Self>) {
        match self.load_host_snapshot_from_db() {
            Ok(()) => {
                self.host_state.notice = Some(rust_i18n::t!("toast_hosts_refreshed").to_string());
            }
            Err(error) => {
                self.host_state.notice = Some(
                    rust_i18n::t!("toast_refresh_failed", error = error.to_string()).to_string(),
                );
            }
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_new_host(&mut self, ctx: &mut ViewContext<Self>) {
        self.open_host_edit_window(HostEditDraft::new_ssh(), true, ctx);
    }

    pub(in crate::root_view) fn handle_host_edit_selected(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(host_id) = self.host_state.selected_host_ids.iter().next().cloned() {
            if let Some(card) = self.host_state.host_by_id(&host_id) {
                let draft = HostEditDraft::from_card(card);
                self.open_host_edit_window(draft, false, ctx);
            }
        }
    }

    pub(in crate::root_view) fn handle_host_edit_one(
        &mut self,
        host_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(card) = self.host_state.host_by_id(&host_id) {
            let draft = HostEditDraft::from_card(card);
            self.open_host_edit_window(draft, false, ctx);
        }
    }

    pub(in crate::root_view) fn handle_host_delete_one(
        &mut self,
        host_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_delete_one(host_id);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_delete_selected(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.delete_selected_hosts();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_connect_selected(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(first_id) = self.host_state.selected_host_ids.iter().next().cloned() {
            self.host_state.clear_selection();
            self.connect_host(&first_id, ctx);
        }
    }

    pub(in crate::root_view) fn handle_host_clear_selection(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.clear_selection();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_enter_reorder_mode(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.reorder_mode = true;
        self.host_state.clear_selection();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_exit_reorder_mode(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.reorder_mode = false;
        self.host_state.host_drag_in_progress = false;
        self.save_host_sort_orders();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_start_card_drag(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host_state.host_drag_in_progress = true;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_drag_card(
        &mut self,
        host_id: String,
        card_position: RectF,
        ctx: &mut ViewContext<Self>,
    ) {
        self.on_host_card_drag(&host_id, card_position, ctx);
    }

    pub(in crate::root_view) fn handle_host_drop_card(&mut self, ctx: &mut ViewContext<Self>) {
        self.host_state.host_drag_in_progress = false;
        self.last_host_swap_time = None;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_manage_groups_tags(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.open_group_tag_manage_window(ctx);
    }

    pub(in crate::root_view) fn handle_host_cloud_sync(&mut self, ctx: &mut ViewContext<Self>) {
        self.host_state.notice = Some(rust_i18n::t!("toast_feature_wip").to_string());
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_import(&mut self, ctx: &mut ViewContext<Self>) {
        self.start_host_import(ctx);
    }

    pub(in crate::root_view) fn handle_host_export(&mut self, ctx: &mut ViewContext<Self>) {
        self.start_host_export(ctx);
    }

    pub(in crate::root_view) fn handle_host_password_confirm(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.commit_host_password(ctx);
    }

    pub(in crate::root_view) fn handle_host_password_cancel(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.cancel_host_password(ctx);
    }
}
