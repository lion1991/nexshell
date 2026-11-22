// host_list_section — 终端页左侧常驻主机列表（ADR 0023）：推入推出装配 + 单击 / 折叠 handler。
// 本文件只含 impl RootView，无自由函数；分组、「已打开」、单击目标等纯函数在 host_list_helpers.rs，
// 行渲染在 host_management_view/host_list_nav.rs。

use warpui::elements::{CrossAxisAlignment, Expanded, Flex, MainAxisSize, ParentElement};
use warpui::{Element, ViewContext};

use crate::host_list_helpers::{
    current_host_id, host_list_click_target, host_list_sections, host_open_states, HostListClick,
    TabHostRef, UNGROUPED_SECTION_KEY,
};
use crate::host_management_view::constants::{HostUiColors, SIDEBAR_WIDTH};
use crate::host_management_view::host_list_nav::{
    render_host_list_nav, HostListNavRow, HostListNavSection,
};
use crate::sidebar_helpers::SidebarContent;
use crate::sidebar_push::SidebarPush;
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
        let nav = self.render_host_list_nav();
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

    fn render_host_list_nav(&self) -> Box<dyn Element> {
        let tabs = self.tab_host_refs();
        let open = host_open_states(&tabs);
        let current = current_host_id(
            &tabs,
            self.active_tab_index,
            self.source_terminal_tab_index(),
        );
        let snapshot = &self.host_state.snapshot;
        let sections = host_list_sections(&snapshot.groups, &snapshot.hosts);
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
                collapsed: self.host_list_collapsed.contains(section.key),
                rows: section
                    .hosts
                    .iter()
                    .map(|host| HostListNavRow {
                        host,
                        open: open.get(host.id.as_str()).copied(),
                        current: current == Some(host.id.as_str()),
                    })
                    .collect(),
            })
            .collect();
        let hc = HostUiColors::from_theme(&self.cached_warp_theme);
        let mut view_states = self.host_view_states.borrow_mut();
        render_host_list_nav(
            &nav_sections,
            show_headers,
            &mut view_states.host_list_nav,
            self.ui_font,
            &hc,
        )
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
            HostListClick::Activate(index) if index == self.active_tab_index => {}
            HostListClick::Activate(index) => self.activate_terminal_tab(index, ctx),
            HostListClick::Connect => self.connect_host(host_id, ctx),
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_host_list_toggle_section(
        &mut self,
        key: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        if !self.host_list_collapsed.remove(key) {
            self.host_list_collapsed.insert(key.to_string());
        }
        self.save_ui_settings();
        ctx.notify();
    }
}
