//! 标题栏侧栏 / 概览按钮与左侧栏、右侧概览面板的纯函数 helper：按页面和当前标签求出显示内容。无 &self。

use crate::terminal_view_helpers::terminal_tab_kind_uses_side_panel_layout;
use crate::{AppPage, TerminalSessionKind};

/// 右侧主机概览面板的内容宽度（左边框另计 1px）。
pub(crate) const OVERVIEW_PANEL_WIDTH: f32 = 248.0;

/// 左侧栏该显示什么。按钮显隐和侧栏渲染都按它判断，不会出现「按钮亮着却没有内容」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarContent {
    /// 没有左侧栏，按钮槽收起。
    None,
    /// 主机管理页的分组导航。
    HostLibraryNav,
    /// 终端页的常驻主机列表（ADR 0023）。
    HostList,
}

pub(crate) fn sidebar_content(
    page: AppPage,
    active_kind: Option<TerminalSessionKind>,
) -> SidebarContent {
    match page {
        AppPage::HostManagement => SidebarContent::HostLibraryNav,
        // RDP 远端分辨率按整窗宽算，开关侧栏就要重设一次，第一版不显示。
        AppPage::Terminal => match active_kind {
            Some(TerminalSessionKind::Rdp) | None => SidebarContent::None,
            Some(_) => SidebarContent::HostList,
        },
        AppPage::Settings => SidebarContent::None,
    }
}

/// 右侧概览面板显示哪个标签的概览（Git Diff / 代码查看器回溯到来源终端）；None 表示当前没有概览面板。
/// 概览按钮显隐、面板渲染、概览采集三处都按它判断。
/// `ssh_source_tab`：当前 tab 的来源终端（普通 tab 即自身），且其主机是已保存的 SSH 主机时为 Some。
pub(crate) fn overview_panel_source(
    page: AppPage,
    active_kind: Option<TerminalSessionKind>,
    ssh_source_tab: Option<usize>,
) -> Option<usize> {
    if page != AppPage::Terminal {
        return None;
    }
    // 进程 / 网络 / 系统信息详情页与 RDP 不画侧边面板。
    match (active_kind, ssh_source_tab) {
        (Some(kind), Some(source_tab)) if terminal_tab_kind_uses_side_panel_layout(kind) => {
            Some(source_tab)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{overview_panel_source, sidebar_content, SidebarContent};
    use crate::{AppPage, TerminalSessionKind};

    #[test]
    fn left_sidebar_content_by_page() {
        assert_eq!(
            sidebar_content(AppPage::HostManagement, None),
            SidebarContent::HostLibraryNav
        );
        assert_eq!(
            sidebar_content(AppPage::Settings, Some(TerminalSessionKind::Remote)),
            SidebarContent::None
        );
    }

    #[test]
    fn terminal_page_shows_host_list_except_on_rdp() {
        for kind in [
            TerminalSessionKind::Local,
            TerminalSessionKind::Remote,
            TerminalSessionKind::Serial,
            TerminalSessionKind::Telnet,
            TerminalSessionKind::ProcessList,
            TerminalSessionKind::NetworkList,
            TerminalSessionKind::SystemInfo,
            TerminalSessionKind::GitDiff,
            TerminalSessionKind::CodeViewer,
        ] {
            assert_eq!(
                sidebar_content(AppPage::Terminal, Some(kind)),
                SidebarContent::HostList,
                "{kind:?}"
            );
        }
        assert_eq!(
            sidebar_content(AppPage::Terminal, Some(TerminalSessionKind::Rdp)),
            SidebarContent::None
        );
        assert_eq!(
            sidebar_content(AppPage::Terminal, None),
            SidebarContent::None
        );
    }

    #[test]
    fn non_terminal_pages_have_no_overview_panel() {
        for page in [AppPage::HostManagement, AppPage::Settings] {
            assert_eq!(
                overview_panel_source(page, Some(TerminalSessionKind::Remote), Some(0)),
                None,
                "{page:?}"
            );
        }
    }

    #[test]
    fn ssh_terminal_and_its_viewer_tabs_show_the_source_overview() {
        for kind in [
            TerminalSessionKind::Remote,
            TerminalSessionKind::GitDiff,
            TerminalSessionKind::CodeViewer,
        ] {
            assert_eq!(
                overview_panel_source(AppPage::Terminal, Some(kind), Some(2)),
                Some(2),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn tabs_without_an_ssh_source_have_no_overview_panel() {
        for kind in [
            TerminalSessionKind::Local,
            TerminalSessionKind::Serial,
            TerminalSessionKind::Telnet,
            TerminalSessionKind::Rdp,
            TerminalSessionKind::GitDiff,
        ] {
            assert_eq!(
                overview_panel_source(AppPage::Terminal, Some(kind), None),
                None,
                "{kind:?}"
            );
        }
        assert_eq!(overview_panel_source(AppPage::Terminal, None, None), None);
    }

    #[test]
    fn detail_pages_never_show_the_overview_panel() {
        for kind in [
            TerminalSessionKind::ProcessList,
            TerminalSessionKind::NetworkList,
            TerminalSessionKind::SystemInfo,
            TerminalSessionKind::Rdp,
        ] {
            assert_eq!(
                overview_panel_source(AppPage::Terminal, Some(kind), Some(0)),
                None,
                "{kind:?}"
            );
        }
    }
}
