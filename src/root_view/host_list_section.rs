// host_list_section — 终端页左侧常驻主机列表（ADR 0023）：推入推出装配、搜索框、单击 / 折叠 /
// 右键菜单 handler。
// 本文件只含 impl RootView，无自由函数；分组、「已打开」、单击目标等纯函数在 host_list_helpers.rs，
// 行渲染在 host_management_view/host_list_nav.rs。

use nexshell::menu::{MenuItem, MenuItemFields};
use nexshell::text_editor::Event as EditorEvent;
use warp_core::ui::appearance::Appearance;
use warpui::elements::{CrossAxisAlignment, Expanded, Flex, MainAxisSize, ParentElement};
use warpui::{AppContext, Element, SingletonEntity as _, ViewContext};

use crate::host_list_helpers::{
    current_host_id, filter_host_list_sections, host_list_click_target, host_list_sections,
    host_open_states, HostListClick, HostListSection, TabHostRef, UNGROUPED_SECTION_KEY,
};
use crate::host_management_view::constants::{HostUiColors, SIDEBAR_WIDTH};
use crate::host_management_view::host_list_nav::{
    render_host_list_nav, HostListNavChrome, HostListNavRow, HostListNavSection,
};
use crate::host_management_view::search_bar::render_search_input;
use crate::sidebar_helpers::SidebarContent;
use crate::sidebar_push::SidebarPush;
use crate::terminal_grid_element::TerminalGridAction;
use crate::RootView;

impl RootView {
    fn tab_host_refs(&self) -> Vec<TabHostRef<'_>> {
        self.terminal_tabs
            .iter()
            .map(|tab| TabHostRef {
                kind: tab.kind,
                host_id: tab.host_id.as_deref(),
                disconnected: tab.is_disconnected(),
            })
            .collect()
    }

    /// 正文左边推入主机列表（开关沿用 sidebar_open，正文按最终宽度布局，终端只 resize 一次）。
    /// 不显示时把推入状态落到当前开关上：切标签 / 换页回来时直接出现，不凭空滑一下。
    pub(in crate::root_view) fn render_body_with_host_list(
        &self,
        body: Box<dyn Element>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        if self.sidebar_content() != SidebarContent::HostList {
            self.host_list_slide.snap(self.sidebar_open, SIDEBAR_WIDTH);
            return body;
        }
        let reveal = self
            .host_list_slide
            .reveal(self.sidebar_open, SIDEBAR_WIDTH);
        if reveal <= 0.0 {
            return body;
        }
        let nav = self.render_host_list_nav(app);
        if self.sidebar_open && reveal >= 1.0 {
            return Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(nav)
                .with_child(Expanded::new(1.0, body).finish())
                .finish();
        }
        SidebarPush::new(Some(nav), body, reveal, self.sidebar_open).finish()
    }

    fn host_list_searching(&self) -> bool {
        !self.host_list_query.trim().is_empty()
    }

    /// 按搜索词过滤后的分区（渲染和回车共用）。
    fn host_list_visible_sections(&self) -> Vec<HostListSection<'_>> {
        let snapshot = &self.host_state.snapshot;
        filter_host_list_sections(
            host_list_sections(&snapshot.groups, &snapshot.hosts),
            &self.host_list_query,
        )
    }

    fn render_host_list_nav(&self, app: &AppContext) -> Box<dyn Element> {
        let tabs = self.tab_host_refs();
        let open = host_open_states(&tabs);
        let current = current_host_id(
            &tabs,
            self.active_tab_index,
            self.source_terminal_tab_index(),
        );
        let menu_target = self
            .show_host_card_context_menu
            .and(self.host_state.context_menu_target.as_deref());
        let searching = self.host_list_searching();
        let sections = self.host_list_visible_sections();
        // 全部主机都没分组时不画分组标题，直接平铺。
        let show_headers = !matches!(
            sections.as_slice(),
            [only] if only.key == UNGROUPED_SECTION_KEY
        );
        let nav_sections: Vec<HostListNavSection<'_>> = sections
            .iter()
            .map(|section| HostListNavSection {
                key: section.key,
                label: section.label.map_or_else(
                    || rust_i18n::t!("host_list_ungrouped").to_string(),
                    str::to_string,
                ),
                // 搜索时分组全部展开，免得结果藏在折叠的分组里。
                collapsed: !searching && self.host_list_collapsed.contains(section.key),
                rows: section
                    .hosts
                    .iter()
                    .map(|host| HostListNavRow {
                        host,
                        open: open.get(host.id.as_str()).copied(),
                        current: current == Some(host.id.as_str()),
                        menu_target: menu_target == Some(host.id.as_str()),
                    })
                    .collect(),
            })
            .collect();
        let hc = HostUiColors::from_theme(&self.cached_warp_theme);
        let uc = self.ui_colors();
        let mut view_states = self.host_view_states.borrow_mut();
        let search_box = render_search_input(
            &view_states.host_list_nav.search_input_state,
            &self.host_list_search_editor,
            Appearance::as_ref(app),
            &hc,
        );
        render_host_list_nav(
            &nav_sections,
            show_headers,
            HostListNavChrome {
                search_box,
                searching,
                privacy_mode: self.host_state.privacy_mode,
                tooltip_bg: uc.tooltip_bg,
                tooltip_text: uc.tooltip_text,
            },
            &mut view_states.host_list_nav,
            self.ui_font,
            &hc,
        )
    }

    /// 搜索框：输入即过滤；回车打开第一个结果；Esc 清空。回车和 Esc 都把焦点还给终端。
    pub(in crate::root_view) fn handle_host_list_search_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            EditorEvent::Edited(_) => {
                self.host_list_query = self.host_list_search_editor.as_ref(ctx).buffer_text(ctx);
                ctx.notify();
            }
            EditorEvent::Enter => {
                let first = self
                    .host_list_searching()
                    .then(|| self.host_list_visible_sections())
                    .and_then(|sections| {
                        sections
                            .first()
                            .and_then(|section| section.hosts.first())
                            .map(|host| host.id.clone())
                    });
                match first {
                    Some(host_id) => self.handle_host_list_open(&host_id, ctx),
                    None => ctx.focus_self(),
                }
            }
            EditorEvent::Escape => {
                self.host_list_search_editor.update(ctx, |editor, ctx| {
                    if !editor.buffer_text(ctx).is_empty() {
                        editor.system_reset_buffer_text("", ctx);
                    }
                });
                self.host_list_query.clear();
                ctx.focus_self();
                ctx.notify();
            }
            _ => {}
        }
    }

    /// 列表收起或切到不显示列表的页面时，焦点不能留在看不见的搜索框里（终端会一直收不到键盘）。
    pub(in crate::root_view) fn release_host_list_search_focus(&self, ctx: &mut ViewContext<Self>) {
        if self.host_list_search_editor.is_focused(ctx) {
            ctx.focus_self();
        }
    }

    /// 主机列表的右键菜单：主页卡片菜单的精简版（改名 / 剪贴板 / 删除等依赖主页，不放）。
    pub(in crate::root_view) fn host_list_context_menu_items(
        &self,
        host_id: &str,
    ) -> Vec<MenuItem<TerminalGridAction>> {
        let endpoint = self
            .host_state
            .host_by_id(host_id)
            .map(|h| h.endpoint.clone())
            .unwrap_or_default();
        vec![
            MenuItemFields::new(rust_i18n::t!("host_list_ctx_new_session"))
                .with_on_select_action(TerminalGridAction::HostQuickConnect(host_id.to_string()))
                .into_item(),
            MenuItemFields::new(rust_i18n::t!("host_ctx_edit"))
                .with_on_select_action(TerminalGridAction::HostEditOne(host_id.to_string()))
                .into_item(),
            MenuItem::Separator,
            MenuItemFields::new(rust_i18n::t!("host_ctx_copy_address"))
                .with_disabled(endpoint.is_empty())
                .with_on_select_action(TerminalGridAction::CopyHostAddress(endpoint))
                .into_item(),
        ]
    }

    /// 单击主机行：开着就跳到它的标签（已在它上面就轮到下一个），没开就新开连接。
    pub(in crate::root_view) fn handle_host_list_open(
        &mut self,
        host_id: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        let anchor = self
            .source_terminal_tab_index()
            .unwrap_or(self.active_tab_index);
        let target = host_list_click_target(&self.tab_host_refs(), anchor, host_id);
        match target {
            HostListClick::Activate(index) if index == self.active_tab_index => ctx.focus_self(),
            HostListClick::Activate(index) => self.activate_terminal_tab(index, ctx),
            HostListClick::Connect => self.connect_host(host_id, ctx),
        }
        // 从搜索框回车打开：连接失败或没切走焦点时，别把键盘留在搜索框。
        self.release_host_list_search_focus(ctx);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_list_toggle_section(
        &mut self,
        key: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        // 搜索时分组强制展开，这时点标题不改折叠状态，免得清空搜索后分组「莫名」收起。
        if self.host_list_searching() {
            return;
        }
        if !self.host_list_collapsed.remove(key) {
            self.host_list_collapsed.insert(key.to_string());
        }
        self.save_ui_settings();
        ctx.notify();
    }
}
