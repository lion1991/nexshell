// host_library_section::operations — RootView 的主机库 CRUD / 剪贴板 / 删除恢复 / 卡片拖拽排序。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。

use crate::RootView;
use nexshell::host_management::{
    default_database_path, delete_hosts_from_db_path, draft_host_id,
    load_or_initialize_host_management_snapshot_from_db_path, update_host_sort_orders,
    upsert_host_card_in_db_path, HostCardSnapshot, HostClipboardOp, HostViewMode,
};
use pathfinder_geometry::rect::RectF;
use warpui::ViewContext;

impl RootView {
    pub(super) fn load_host_snapshot_from_db(&mut self) -> Result<(), String> {
        let db_path = default_database_path()
            .ok_or_else(|| rust_i18n::t!("toast_db_path_unavailable").to_string())?;
        let snapshot = load_or_initialize_host_management_snapshot_from_db_path(&db_path)?;
        self.host_state.replace_snapshot(snapshot);
        self.reload_host_recent();
        Ok(())
    }

    pub(super) fn delete_selected_hosts(&mut self) {
        if self.host_state.selected_host_ids.is_empty() {
            return;
        }

        let Some(db_path) = default_database_path() else {
            self.host_state.notice =
                Some(rust_i18n::t!("toast_host_library_unavailable_delete").to_string());
            return;
        };

        let selected = self.host_state.selected_host_ids.clone();
        // 删除前备份完整快照，供右键菜单"恢复"（会话内内存态）
        let backup: Vec<HostCardSnapshot> = selected
            .iter()
            .filter_map(|id| self.host_state.host_by_id(id).cloned())
            .collect();
        match delete_hosts_from_db_path(&db_path, &selected) {
            Ok(deleted) => match self.load_host_snapshot_from_db() {
                Ok(()) => {
                    self.host_state.selected_host_ids.clear();
                    self.host_state.deleted_host_backup = backup;
                    self.host_state.notice =
                        Some(rust_i18n::t!("toast_hosts_deleted", count = deleted).to_string());
                }
                Err(error) => {
                    self.host_state.selected_host_ids.clear();
                    self.host_state.notice = Some(
                        rust_i18n::t!(
                            "toast_hosts_deleted_refresh_fail",
                            count = deleted,
                            error = error
                        )
                        .to_string(),
                    );
                }
            },
            Err(error) => {
                self.host_state.notice =
                    Some(rust_i18n::t!("toast_delete_failed", error = error).to_string());
            }
        }
    }

    pub(super) fn host_delete_one(&mut self, host_id: String) {
        let Some(db_path) = default_database_path() else {
            self.host_state.notice =
                Some(rust_i18n::t!("toast_host_library_unavailable_delete").to_string());
            return;
        };
        let backup: Vec<HostCardSnapshot> = self
            .host_state
            .host_by_id(&host_id)
            .cloned()
            .into_iter()
            .collect();
        let mut ids = std::collections::BTreeSet::new();
        ids.insert(host_id);
        match delete_hosts_from_db_path(&db_path, &ids) {
            Ok(deleted) => match self.load_host_snapshot_from_db() {
                Ok(()) => {
                    self.host_state.context_menu_target = None;
                    self.host_state.deleted_host_backup = backup;
                    self.host_state.notice =
                        Some(rust_i18n::t!("toast_hosts_deleted", count = deleted).to_string());
                }
                Err(error) => {
                    self.host_state.context_menu_target = None;
                    self.host_state.notice = Some(
                        rust_i18n::t!(
                            "toast_hosts_deleted_refresh_fail",
                            count = deleted,
                            error = error
                        )
                        .to_string(),
                    );
                }
            },
            Err(error) => {
                self.host_state.notice =
                    Some(rust_i18n::t!("toast_delete_failed", error = error).to_string());
            }
        }
    }

    pub(super) fn host_clipboard_paste(&mut self, ctx: &mut ViewContext<Self>) {
        let Some((src, op)) = self.host_state.host_clipboard.clone() else {
            return;
        };
        let Some(db_path) = default_database_path() else {
            self.host_state.notice =
                Some(rust_i18n::t!("toast_host_library_unavailable_save").to_string());
            return;
        };
        let mut copy = src.clone();
        copy.id = draft_host_id();
        copy.name = format!("{} {}", src.name, rust_i18n::t!("host_copy_suffix"));
        match upsert_host_card_in_db_path(&db_path, &copy) {
            Ok(()) => {
                if op == HostClipboardOp::Cut {
                    let mut ids = std::collections::BTreeSet::new();
                    ids.insert(src.id.clone());
                    let _ = delete_hosts_from_db_path(&db_path, &ids);
                    self.host_state.host_clipboard = None;
                }
                match self.load_host_snapshot_from_db() {
                    Ok(()) => {
                        self.host_state.notice =
                            Some(rust_i18n::t!("toast_host_pasted").to_string());
                    }
                    Err(error) => {
                        self.host_state.notice =
                            Some(rust_i18n::t!("toast_paste_failed", error = error).to_string());
                    }
                }
            }
            Err(error) => {
                self.host_state.notice =
                    Some(rust_i18n::t!("toast_paste_failed", error = error).to_string());
            }
        }
        ctx.notify();
    }

    pub(super) fn host_restore_deleted(&mut self, ctx: &mut ViewContext<Self>) {
        if self.host_state.deleted_host_backup.is_empty() {
            return;
        }
        let Some(db_path) = default_database_path() else {
            self.host_state.notice =
                Some(rust_i18n::t!("toast_host_library_unavailable_save").to_string());
            return;
        };
        let backup = self.host_state.deleted_host_backup.clone();
        for snap in &backup {
            if let Err(error) = upsert_host_card_in_db_path(&db_path, snap) {
                self.host_state.notice =
                    Some(rust_i18n::t!("toast_restore_failed", error = error).to_string());
                ctx.notify();
                return;
            }
        }
        self.host_state.deleted_host_backup.clear();
        match self.load_host_snapshot_from_db() {
            Ok(()) => {
                self.host_state.notice = Some(rust_i18n::t!("toast_host_restored").to_string());
            }
            Err(error) => {
                self.host_state.notice =
                    Some(rust_i18n::t!("toast_restore_failed", error = error).to_string());
            }
        }
        ctx.notify();
    }

    pub(super) fn on_host_card_drag(
        &mut self,
        host_id: &str,
        drag_position: RectF,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(t) = self.last_host_swap_time {
            if t.elapsed() < std::time::Duration::from_millis(150) {
                return;
            }
        }
        let hosts = self.host_state.filtered_hosts();
        let Some(current_index) = hosts.iter().position(|h| h.id == host_id) else {
            return;
        };
        let total = hosts.len();
        let grid_columns = if self.host_state.view_mode == HostViewMode::Grid {
            3
        } else {
            1
        };
        let new_index = self.calculate_updated_host_index(
            current_index,
            drag_position,
            total,
            grid_columns,
            ctx,
        );
        if new_index != current_index {
            self.last_host_swap_time = Some(std::time::Instant::now());
            // 过滤态下 current/new_index 是过滤后下标，需带 id 让 swap 定位全量列表
            let id_a = hosts[current_index].id.clone();
            let id_b = hosts[new_index].id.clone();
            self.swap_host_sort_order(current_index, new_index, &id_a, &id_b);
            ctx.notify();
        }
    }

    fn calculate_updated_host_index(
        &self,
        current_index: usize,
        drag_position: RectF,
        total: usize,
        grid_columns: usize,
        ctx: &mut ViewContext<Self>,
    ) -> usize {
        let mid_x = (drag_position.min_x() + drag_position.max_x()) / 2.0;
        let mid_y = (drag_position.min_y() + drag_position.max_y()) / 2.0;

        // Center-point comparison: only swap when dragged card's midpoint
        // passes the neighbor's center. Stronger hysteresis than edge comparison.
        if current_index > 0 {
            let id = format!("host_card_position_{}", current_index - 1);
            if let Some(pos) = ctx.element_position_by_id(&id) {
                let cx = (pos.min_x() + pos.max_x()) / 2.0;
                if mid_x < cx && mid_y >= pos.min_y() && mid_y <= pos.max_y() {
                    return current_index - 1;
                }
            }
        }

        if current_index + 1 < total {
            let id = format!("host_card_position_{}", current_index + 1);
            if let Some(pos) = ctx.element_position_by_id(&id) {
                let cx = (pos.min_x() + pos.max_x()) / 2.0;
                if mid_x > cx && mid_y >= pos.min_y() && mid_y <= pos.max_y() {
                    return current_index + 1;
                }
            }
        }

        if current_index >= grid_columns {
            let id = format!("host_card_position_{}", current_index - grid_columns);
            if let Some(pos) = ctx.element_position_by_id(&id) {
                let cy = (pos.min_y() + pos.max_y()) / 2.0;
                if mid_y < cy && mid_x >= pos.min_x() && mid_x <= pos.max_x() {
                    return current_index - grid_columns;
                }
            }
        }

        if current_index + grid_columns < total {
            let id = format!("host_card_position_{}", current_index + grid_columns);
            if let Some(pos) = ctx.element_position_by_id(&id) {
                let cy = (pos.min_y() + pos.max_y()) / 2.0;
                if mid_y > cy && mid_x >= pos.min_x() && mid_x <= pos.max_x() {
                    return current_index + grid_columns;
                }
            }
        }

        current_index
    }

    fn swap_host_sort_order(&mut self, from: usize, to: usize, id_a: &str, id_b: &str) {
        let hosts = &mut self.host_state.snapshot.hosts;
        // from/to 是过滤后下标，必须按 id 定位全量列表真实位置，否则会交换错误主机
        let pa = hosts.iter().position(|h| h.id == id_a);
        let pb = hosts.iter().position(|h| h.id == id_b);
        let (Some(pa), Some(pb)) = (pa, pb) else {
            return;
        };
        let tmp = hosts[pa].sort_order;
        hosts[pa].sort_order = hosts[pb].sort_order;
        hosts[pb].sort_order = tmp;
        hosts.swap(pa, pb);

        // 卡片状态向量与过滤后列表同长，仍用过滤下标
        let mut view_states = self.host_view_states.borrow_mut();
        let cards = &mut view_states.host_cards;
        if from < cards.draggable_states.len() && to < cards.draggable_states.len() {
            cards.draggable_states.swap(from, to);
            cards.card_states.swap(from, to);
            cards.connect_states.swap(from, to);
        }
    }

    pub(super) fn save_host_sort_orders(&mut self) {
        let Some(db_path) = default_database_path() else {
            return;
        };
        let updates: Vec<(String, i64)> = self
            .host_state
            .snapshot
            .hosts
            .iter()
            .enumerate()
            .map(|(i, h)| (h.id.clone(), i as i64))
            .collect();
        if let Err(e) = update_host_sort_orders(&db_path, &updates) {
            eprintln!("[nexshell] 保存主机排序失败: {e}");
            self.host_state.notice = Some(format!("保存主机排序失败: {e}"));
        }
    }
}
