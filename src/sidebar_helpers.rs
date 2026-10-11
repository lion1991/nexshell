//! 标题栏侧栏按钮与左侧栏的纯函数 helper：按页面和当前标签求出侧栏内容。无 &self。

use crate::terminal_view_helpers::terminal_tab_kind_uses_side_panel_layout;
use crate::{AppPage, TerminalSessionKind};

/// 左侧栏该显示什么。按钮显隐、侧栏渲染、概览采集三处都按它判断，
/// 不会出现「按钮亮着却没有内容」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarContent {
    /// 没有左侧栏，按钮留同尺寸空位。
    None,
    /// 主机管理页的分组导航。
    HostLibraryNav,
    /// SSH 主机概览；`source_tab` 是持有概览数据的终端 tab
    /// （Git Diff / 代码查看器回溯到来源终端）。
    HostOverview { source_tab: usize },
}

/// `ssh_source_tab`：当前 tab 的来源终端（普通 tab 即自身），且其主机是已保存的 SSH 主机时为 Some。
pub(crate) fn sidebar_content(
    page: AppPage,
    active_kind: Option<TerminalSessionKind>,
    ssh_source_tab: Option<usize>,
) -> SidebarContent {
    match page {
        AppPage::HostManagement => SidebarContent::HostLibraryNav,
        AppPage::Settings => SidebarContent::None,
        // 进程 / 网络 / 系统信息详情页与 RDP 不画侧栏。
        AppPage::Terminal => match (active_kind, ssh_source_tab) {
            (Some(kind), Some(source_tab)) if terminal_tab_kind_uses_side_panel_layout(kind) => {
                SidebarContent::HostOverview { source_tab }
            }
            _ => SidebarContent::None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{sidebar_content, SidebarContent};
    use crate::{AppPage, TerminalSessionKind};

    #[test]
    fn host_management_page_shows_the_group_nav() {
        assert_eq!(
            sidebar_content(AppPage::HostManagement, None, None),
            SidebarContent::HostLibraryNav
        );
    }

    #[test]
    fn settings_page_has_no_sidebar() {
        assert_eq!(
            sidebar_content(
                AppPage::Settings,
                Some(TerminalSessionKind::Remote),
                Some(0)
            ),
            SidebarContent::None
        );
    }

    #[test]
    fn ssh_terminal_and_its_viewer_tabs_show_the_source_overview() {
        for kind in [
            TerminalSessionKind::Remote,
            TerminalSessionKind::GitDiff,
            TerminalSessionKind::CodeViewer,
        ] {
            assert_eq!(
                sidebar_content(AppPage::Terminal, Some(kind), Some(2)),
                SidebarContent::HostOverview { source_tab: 2 },
                "{kind:?}"
            );
        }
    }

    #[test]
    fn tabs_without_an_ssh_source_have_no_sidebar() {
        for kind in [
            TerminalSessionKind::Local,
            TerminalSessionKind::Serial,
            TerminalSessionKind::Telnet,
            TerminalSessionKind::Rdp,
            TerminalSessionKind::GitDiff,
        ] {
            assert_eq!(
                sidebar_content(AppPage::Terminal, Some(kind), None),
                SidebarContent::None,
                "{kind:?}"
            );
        }
        assert_eq!(
            sidebar_content(AppPage::Terminal, None, None),
            SidebarContent::None
        );
    }

    #[test]
    fn detail_pages_never_show_the_sidebar() {
        for kind in [
            TerminalSessionKind::ProcessList,
            TerminalSessionKind::NetworkList,
            TerminalSessionKind::SystemInfo,
            TerminalSessionKind::Rdp,
        ] {
            assert_eq!(
                sidebar_content(AppPage::Terminal, Some(kind), Some(0)),
                SidebarContent::None,
                "{kind:?}"
            );
        }
    }
}
