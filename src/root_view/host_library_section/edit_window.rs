// host_library_section::edit_window — RootView 的主机编辑窗 + 分组/标签管理窗 + draft 转换。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。

#[cfg(target_os = "macos")]
use crate::macos_window_util;
use crate::group_tag_manage_window::{GroupTagManageEvent, GroupTagManageModel, GroupTagManageView};
use crate::host_edit_window::{HostEditDraft, HostEditEvent, HostEditModel, HostEditView};
use crate::terminal_view_helpers::optional_text;
use crate::RootView;
use nexshell::host_management::{
    default_database_path, upsert_host_card_in_db_path, HostCardSnapshot, HostConnectionConfig,
};
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::vec2f;
use warpui::platform::WindowBounds;
use warpui::{
    AddWindowOptions, ModelAsRef, ModelHandle, NextNewWindowsHasThisWindowsBoundsUponClose,
    ViewContext,
};

impl RootView {
    fn save_host_edit_draft(
        &mut self,
        draft: &HostEditDraft,
        is_new: bool,
        _ctx: &mut ViewContext<Self>,
    ) -> bool {
        if draft.name.trim().is_empty()
            || draft.host.trim().is_empty()
            || (draft.protocol == "SSH" && draft.port == 0)
            || (draft.protocol == "Serial" && draft.serial_baud_rate == 0)
            || (draft.protocol == "SSH" && draft.username.trim().is_empty())
        {
            self.host_state.notice = Some(rust_i18n::t!("toast_form_required").to_string());
            return false;
        }

        let connection = Self::connection_config_from_draft(draft);
        let existing_sort_order = self
            .host_state
            .host_by_id(&draft.id)
            .map(|h| h.sort_order)
            .unwrap_or(0);
        let card = HostCardSnapshot {
            id: draft.id.clone(),
            name: draft.name.trim().to_string(),
            protocol: draft.protocol.clone(),
            endpoint: connection.endpoint(&draft.protocol),
            description: draft.description.trim().to_string(),
            connection,
            group_id: draft.group_id.clone(),
            tags: draft.tags.clone(),
            system: draft.system,
            sort_order: existing_sort_order,
        };

        let Some(db_path) = default_database_path() else {
            self.host_state.notice =
                Some(rust_i18n::t!("toast_host_library_unavailable_save").to_string());
            return false;
        };

        match upsert_host_card_in_db_path(&db_path, &card) {
            Ok(()) => match self.load_host_snapshot_from_db() {
                Ok(()) => {
                    self.host_state.selected_host_ids.clear();
                    self.host_state.selected_host_ids.insert(card.id.clone());
                    self.host_state.notice = Some(if is_new {
                        rust_i18n::t!("toast_host_created").to_string()
                    } else {
                        rust_i18n::t!("toast_host_saved").to_string()
                    });
                    true
                }
                Err(error) => {
                    self.host_state.apply_edit_draft_fields(
                        &card.id,
                        &card.name,
                        &card.protocol,
                        &card.description,
                        card.connection,
                        card.group_id,
                        card.tags,
                        card.system,
                        is_new,
                    );
                    self.host_state.notice =
                        Some(rust_i18n::t!("toast_saved_refresh_fail", error = error).to_string());
                    true
                }
            },
            Err(error) => {
                self.host_state.notice =
                    Some(rust_i18n::t!("toast_save_failed", error = error).to_string());
                false
            }
        }
    }

    pub(super) fn open_host_edit_window(
        &mut self,
        draft: HostEditDraft,
        is_new: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let group_options: Vec<_> = self
            .host_state
            .snapshot
            .groups
            .iter()
            .filter(|group| group.id != "all")
            .map(|group| (group.id.clone(), group.label.clone()))
            .collect();
        let available_tags = self.host_state.snapshot.available_tags.clone();
        let key_options: Vec<(String, String)> =
            nexshell::host_management::default_database_path()
                .and_then(|db| {
                    nexshell::ssh_key_store::list_ssh_keys_with_usage(&db).ok()
                })
                .map(|keys| keys.into_iter().map(|(k, _)| (k.id, k.name)).collect())
                .unwrap_or_default();
        let model = ctx.add_model(move |_| HostEditModel {
            draft,
            is_new,
            group_options,
            key_options,
            available_tags,
        });
        ctx.subscribe_to_model(&model, Self::on_host_edit_event);

        let edit_size = vec2f(540.0, 620.0);
        let main_window_id = ctx.window_id();
        let bounds = if let Some(main_win) = ctx.windows().platform_window(main_window_id) {
            let origin = main_win.origin();
            let size = main_win.size();
            let x = origin.x() + (size.x() - edit_size.x()) / 2.0;
            let y = origin.y() + (size.y() - edit_size.y()) / 2.0;
            WindowBounds::ExactPosition(RectF::new(vec2f(x, y), edit_size))
        } else {
            WindowBounds::ExactSize(edit_size)
        };

        let (window_id, edit_view_handle) = ctx.add_window(
            AddWindowOptions {
                title: Some(if is_new {
                    rust_i18n::t!("form_new_host").to_string()
                } else {
                    rust_i18n::t!("form_edit_host").to_string()
                }),
                window_bounds: bounds,
                anchor_new_windows_from_closed_position:
                    NextNewWindowsHasThisWindowsBoundsUponClose::No,
                ..Default::default()
            },
            {
                let model = model.clone();
                move |view_ctx| HostEditView::new(model, view_ctx)
            },
        );
        ctx.focus(&edit_view_handle);

        // 隐藏红绿灯
        #[cfg(target_os = "macos")]
        {
            if let Some(edit_win) = ctx.windows().platform_window(window_id) {
                use warpui::platform::current::WindowExt;
                edit_win.as_ref().set_window_buttons(false);
            }
        }
        // 设为主窗口子窗口（始终在前）
        #[cfg(target_os = "macos")]
        macos_window_util::raise_window_level();

        self.active_edit_model = Some(model);
        self.edit_window_id = Some(window_id);
    }

    fn close_edit_window(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(wid) = self.edit_window_id.take() {
            #[cfg(target_os = "macos")]
            macos_window_util::reset_window_level();
            ctx.windows().hide_window(wid);
        }
        self.active_edit_model = None;
    }

    pub(super) fn open_group_tag_manage_window(&mut self, ctx: &mut ViewContext<Self>) {
        let groups: Vec<(String, String)> = self
            .host_state
            .snapshot
            .groups
            .iter()
            .filter(|g| g.id != "all")
            .map(|g| (g.id.clone(), g.label.clone()))
            .collect();
        let tags = self.host_state.snapshot.available_tags.clone();
        let db_path = nexshell::host_management::default_database_path()
            .unwrap_or_default();

        let model = ctx.add_model(move |_| GroupTagManageModel {
            groups,
            tags,
            db_path,
            changed: false,
        });
        ctx.subscribe_to_model(&model, Self::on_group_tag_manage_event);

        let win_size = vec2f(400.0, 500.0);
        let main_window_id = ctx.window_id();
        let bounds = if let Some(main_win) = ctx.windows().platform_window(main_window_id) {
            let origin = main_win.origin();
            let size = main_win.size();
            let x = origin.x() + (size.x() - win_size.x()) / 2.0;
            let y = origin.y() + (size.y() - win_size.y()) / 2.0;
            WindowBounds::ExactPosition(RectF::new(vec2f(x, y), win_size))
        } else {
            WindowBounds::ExactSize(win_size)
        };

        let (window_id, view_handle) = ctx.add_window(
            AddWindowOptions {
                title: Some(rust_i18n::t!("manage_title").to_string()),
                window_bounds: bounds,
                anchor_new_windows_from_closed_position:
                    NextNewWindowsHasThisWindowsBoundsUponClose::No,
                ..Default::default()
            },
            {
                let model = model.clone();
                move |view_ctx| GroupTagManageView::new(model, view_ctx)
            },
        );
        ctx.focus(&view_handle);

        #[cfg(target_os = "macos")]
        {
            if let Some(win) = ctx.windows().platform_window(window_id) {
                use warpui::platform::current::WindowExt;
                win.as_ref().set_window_buttons(false);
            }
        }
        #[cfg(target_os = "macos")]
        macos_window_util::raise_window_level();

        self.active_manage_model = Some(model);
        self.manage_window_id = Some(window_id);
    }

    fn close_manage_window(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(wid) = self.manage_window_id.take() {
            #[cfg(target_os = "macos")]
            macos_window_util::reset_window_level();
            ctx.windows().hide_window(wid);
        }
        self.active_manage_model = None;
    }

    fn on_group_tag_manage_event(
        &mut self,
        _model: ModelHandle<GroupTagManageModel>,
        event: &GroupTagManageEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            GroupTagManageEvent::Closed { changed } => {
                if *changed {
                    let _ = self.load_host_snapshot_from_db();
                }
                self.close_manage_window(ctx);
                ctx.notify();
            }
        }
    }

    fn on_host_edit_event(
        &mut self,
        _model: ModelHandle<HostEditModel>,
        event: &HostEditEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            HostEditEvent::Saved(draft) => {
                let is_new = ctx.model(&_model).is_new;
                if self.save_host_edit_draft(draft, is_new, ctx) {
                    self.close_edit_window(ctx);
                }
                ctx.notify();
            }
            HostEditEvent::Cancelled => {
                self.close_edit_window(ctx);
                ctx.notify();
            }
        }
    }

    fn connection_config_from_draft(draft: &HostEditDraft) -> HostConnectionConfig {
        if draft.protocol == "Serial" {
            let mut connection =
                HostConnectionConfig::serial(draft.host.trim(), draft.serial_baud_rate);
            connection.serial_data_bits = draft.serial_data_bits;
            connection.serial_stop_bits = draft.serial_stop_bits;
            connection.serial_parity = draft.serial_parity.clone();
            connection.serial_flow_control = draft.serial_flow_control.clone();
            connection.serial_dtr = draft.serial_dtr;
            connection.serial_rts = draft.serial_rts;
            return connection;
        }

        let mut connection =
            HostConnectionConfig::ssh(draft.host.trim(), draft.port, draft.username.trim());
        connection.auth_method = draft.auth_method.clone();
        connection.password = optional_text(&draft.password);
        connection.private_key = optional_text(&draft.private_key);
        connection.key_passphrase = optional_text(&draft.key_passphrase);
        connection.ca_cert = optional_text(&draft.ca_cert);
        connection.key_id = draft.key_id.clone();
        connection.keep_alive_enabled = draft.keep_alive_enabled;
        connection.keep_alive_interval = draft.keep_alive_interval;
        connection.keep_alive_max_failures = draft.keep_alive_max_failures.clamp(1, 10) as u8;
        connection.tcp_connect_timeout = draft.tcp_connect_timeout;
        connection.auth_timeout = draft.auth_timeout;
        connection.term_encoding = draft.term_encoding.clone();
        connection
    }
}
