// host_library_section — RootView 的主机库页面（卡片列表 / 编辑 / 分组标签 / 导入导出）。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView 渲染入口
// 与子模块声明，无自由函数。子模块边界：
//   actions     — handle_host_* action handler（由 root_view/mod.rs handle_action 分发）
//   editors     — 改名 / 搜索内联编辑器
//   operations  — CRUD / 剪贴板 / 删除恢复 / 卡片拖拽排序
//   transfer    — 加密导入导出 + 密码输入栏
//   edit_window — 主机编辑窗 + 分组/标签管理窗 + draft 转换
//
// 渲染主体已下放到 host_management_view/ 子组件，本入口只做装配 + 叠加密码栏。

use crate::host_management_view::constants::HostUiColors;
use crate::host_management_view::render_host_management_panel;
use crate::RootView;
use warp::appearance::Appearance;
use warpui::elements::{CrossAxisAlignment, Expanded, Flex, MainAxisSize, ParentElement};
use warpui::{AppContext, Element, SingletonEntity as _};

mod actions;
mod edit_window;
mod editors;
mod operations;
mod transfer;

impl RootView {
    pub(in crate::root_view) fn render_host_management_page(&self, app: &AppContext) -> Box<dyn Element> {
        let mut view_states = self.host_view_states.borrow_mut();
        let hc = HostUiColors::from_theme(&self.cached_warp_theme);
        let panel = render_host_management_panel(
            &self.host_state,
            &mut view_states,
            &self.host_search_editor,
            self.host_rename_target.as_deref(),
            &self.host_rename_editor,
            self.ui_font,
            Appearance::as_ref(app),
            self.sidebar_open,
            &self.host_status_fleet,
            &self.host_keys,
            self.host_state.selected_key_id.as_deref(),
            self.host_selected_key_public.as_deref(),
            self.host_state.copy_cmd_expanded,
            self.host_key_edit_target.is_some()
                && self.host_key_edit_target.as_deref() == self.host_state.selected_key_id.as_deref(),
            self.host_state.key_delete_confirming,
            &self.host_key_name_editor,
            &self.host_key_passphrase_editor,
            &self.host_recent,
            &hc,
        );
        if let Some(bar) = self.render_host_password_bar() {
            Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(bar)
                .with_child(Expanded::new(1.0, panel).finish())
                .finish()
        } else {
            panel
        }
    }
}
