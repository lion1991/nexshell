//! 终端页左侧主机列表的纯函数 helper：分组、「已打开」映射、当前主机、单击目标。无 &self。

use std::collections::HashMap;

use nexshell::host_management::{host_matches_query, HostCardSnapshot, HostGroupSnapshot};

use crate::TerminalSessionKind;

/// 未分组主机（`group_id` 为空或指向已删分组）的分区 key；也用作折叠状态的持久化 key。
pub(crate) const UNGROUPED_SECTION_KEY: &str = "__ungrouped__";

/// 主页分组导航里的「所有主机」伪分组，列表里不单列。
const ALL_HOSTS_GROUP_ID: &str = "all";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HostListSection<'a> {
    /// 分组 id；未分组为 `UNGROUPED_SECTION_KEY`。
    pub key: &'a str,
    /// None 表示未分组（标题由调用方取文案）。
    pub label: Option<&'a str>,
    pub hosts: Vec<&'a HostCardSnapshot>,
}

/// 按主页的分组顺序把主机分区，组内保持快照顺序；空分组不列，未分组放最后。
pub(crate) fn host_list_sections<'a>(
    groups: &'a [HostGroupSnapshot],
    hosts: &'a [HostCardSnapshot],
) -> Vec<HostListSection<'a>> {
    let mut sections: Vec<HostListSection<'a>> = groups
        .iter()
        .filter(|group| group.id != ALL_HOSTS_GROUP_ID)
        .map(|group| HostListSection {
            key: group.id.as_str(),
            label: Some(group.label.as_str()),
            hosts: Vec::new(),
        })
        .collect();
    let mut ungrouped = Vec::new();
    for host in hosts {
        let section = host
            .group_id
            .as_deref()
            .and_then(|id| sections.iter_mut().find(|section| section.key == id));
        match section {
            Some(section) => section.hosts.push(host),
            None => ungrouped.push(host),
        }
    }
    sections.retain(|section| !section.hosts.is_empty());
    if !ungrouped.is_empty() {
        sections.push(HostListSection {
            key: UNGROUPED_SECTION_KEY,
            label: None,
            hosts: ungrouped,
        });
    }
    sections
}

/// 按搜索词过滤分区：只留匹配的主机，删掉过滤后为空的分区。搜索词空白时原样返回。
pub(crate) fn filter_host_list_sections<'a>(
    sections: Vec<HostListSection<'a>>,
    query: &str,
) -> Vec<HostListSection<'a>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return sections;
    }
    sections
        .into_iter()
        .filter_map(|mut section| {
            section
                .hosts
                .retain(|host| host_matches_query(host, &query));
            (!section.hosts.is_empty()).then_some(section)
        })
        .collect()
}

/// 标签里与主机相关的那部分信息。
#[derive(Clone, Copy, Debug)]
pub(crate) struct TabHostRef<'a> {
    pub kind: TerminalSessionKind,
    /// 会话 / 详情页标签存主机 id；Git Diff / 内置编辑器标签存的是来源标签 id。
    pub host_id: Option<&'a str>,
    pub disconnected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostOpenState {
    /// 至少一个会话还连着。
    Connected,
    /// 有会话但全断开了。
    Disconnected,
}

/// 连到某台主机的会话标签（详情页不算「已打开」，点了仍要新开连接）。
fn is_host_session(kind: TerminalSessionKind) -> bool {
    matches!(
        kind,
        TerminalSessionKind::Remote
            | TerminalSessionKind::Serial
            | TerminalSessionKind::Telnet
            | TerminalSessionKind::Direct
            | TerminalSessionKind::Rdp
    )
}

/// 主机 id → 「已打开」状态。
pub(crate) fn host_open_states<'a>(tabs: &[TabHostRef<'a>]) -> HashMap<&'a str, HostOpenState> {
    let mut states = HashMap::new();
    for tab in tabs.iter().filter(|tab| is_host_session(tab.kind)) {
        let Some(host_id) = tab.host_id else {
            continue;
        };
        let state = if tab.disconnected {
            HostOpenState::Disconnected
        } else {
            HostOpenState::Connected
        };
        states
            .entry(host_id)
            .and_modify(|existing| {
                if state == HostOpenState::Connected {
                    *existing = HostOpenState::Connected;
                }
            })
            .or_insert(state);
    }
    states
}

/// 当前标签对应的主机（列表里高亮）。Git Diff / 内置编辑器回溯到来源标签；本地终端没有主机。
/// `source_tab`：当前标签的来源标签 index（普通标签即自身），来源已关时为 None。
pub(crate) fn current_host_id<'a>(
    tabs: &[TabHostRef<'a>],
    active: usize,
    source_tab: Option<usize>,
) -> Option<&'a str> {
    let tab = tabs.get(active)?;
    let tab = match tab.kind {
        TerminalSessionKind::GitDiff | TerminalSessionKind::CodeViewer => tabs.get(source_tab?)?,
        _ => tab,
    };
    match tab.kind {
        TerminalSessionKind::ProcessList
        | TerminalSessionKind::NetworkList
        | TerminalSessionKind::SystemInfo => tab.host_id,
        kind if is_host_session(kind) => tab.host_id,
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostListClick {
    /// 主机已开着：切到这个标签。
    Activate(usize),
    /// 没开着：新开连接。
    Connect,
}

/// 单击主机行：没开就连；开着就跳过去。当前已在它的某个会话上时，跳到它的下一个会话（循环），
/// 只有一个会话时停在原地。`anchor`：当前标签的来源标签（Git Diff / 内置编辑器回溯到来源终端，
/// 其余即当前标签），轮换从它往后数。
pub(crate) fn host_list_click_target(
    tabs: &[TabHostRef<'_>],
    anchor: usize,
    host_id: &str,
) -> HostListClick {
    let sessions: Vec<usize> = tabs
        .iter()
        .enumerate()
        .filter(|(_, tab)| is_host_session(tab.kind) && tab.host_id == Some(host_id))
        .map(|(index, _)| index)
        .collect();
    let Some(&first) = sessions.first() else {
        return HostListClick::Connect;
    };
    let next = sessions
        .iter()
        .position(|&index| index == anchor)
        .map_or(first, |pos| sessions[(pos + 1) % sessions.len()]);
    HostListClick::Activate(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexshell::host_management::{HostConnectionConfig, HostSystemIcon};

    fn group(id: &str, label: &str) -> HostGroupSnapshot {
        HostGroupSnapshot {
            id: id.to_string(),
            label: label.to_string(),
            count: 0,
            selected: false,
        }
    }

    fn host(id: &str, group_id: Option<&str>) -> HostCardSnapshot {
        HostCardSnapshot {
            id: id.to_string(),
            name: id.to_string(),
            protocol: "SSH".to_string(),
            endpoint: String::new(),
            description: String::new(),
            connection: HostConnectionConfig::ssh("example.invalid", 22, "root"),
            group_id: group_id.map(str::to_string),
            tags: Vec::new(),
            system: HostSystemIcon::Linux,
            sort_order: 0,
        }
    }

    fn tab(kind: TerminalSessionKind, host_id: Option<&str>) -> TabHostRef<'_> {
        TabHostRef {
            kind,
            host_id,
            disconnected: false,
        }
    }

    fn ids(section: &HostListSection<'_>) -> Vec<String> {
        section.hosts.iter().map(|h| h.id.clone()).collect()
    }

    #[test]
    fn sections_follow_group_order_and_put_ungrouped_last() {
        let groups = vec![
            group("all", "所有主机"),
            group("g2", "生产"),
            group("g1", "测试"),
        ];
        let hosts = vec![
            host("a", Some("g1")),
            host("b", None),
            host("c", Some("g2")),
            host("d", Some("deleted-group")),
            host("e", Some("g1")),
        ];
        let sections = host_list_sections(&groups, &hosts);
        let keys: Vec<&str> = sections.iter().map(|s| s.key).collect();
        assert_eq!(keys, vec!["g2", "g1", UNGROUPED_SECTION_KEY]);
        assert_eq!(ids(&sections[1]), vec!["a", "e"]);
        assert_eq!(ids(&sections[2]), vec!["b", "d"]);
        assert_eq!(sections[2].label, None);
    }

    #[test]
    fn empty_groups_are_not_listed() {
        let groups = vec![group("all", "所有主机"), group("g1", "空分组")];
        let hosts = vec![host("a", None)];
        let sections = host_list_sections(&groups, &hosts);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].key, UNGROUPED_SECTION_KEY);
    }

    #[test]
    fn search_keeps_matching_hosts_and_drops_empty_sections() {
        let groups = vec![group("g1", "生产"), group("g2", "测试")];
        let mut web = host("web-01", Some("g1"));
        web.endpoint = "root@10.0.0.5:22".to_string();
        let hosts = vec![web, host("db-01", Some("g1")), host("dev", Some("g2"))];
        let sections = filter_host_list_sections(host_list_sections(&groups, &hosts), " WEB ");
        assert_eq!(sections.len(), 1);
        assert_eq!(ids(&sections[0]), vec!["web-01"]);
        let by_address = filter_host_list_sections(host_list_sections(&groups, &hosts), "10.0.0");
        assert_eq!(ids(&by_address[0]), vec!["web-01"]);
        let all = filter_host_list_sections(host_list_sections(&groups, &hosts), "  ");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn open_state_is_connected_if_any_session_is_connected() {
        let mut down = tab(TerminalSessionKind::Remote, Some("h1"));
        down.disconnected = true;
        let up = tab(TerminalSessionKind::Remote, Some("h1"));
        let mut only_down = tab(TerminalSessionKind::Telnet, Some("h2"));
        only_down.disconnected = true;
        let states = host_open_states(&[down, up, only_down]);
        assert_eq!(states.get("h1"), Some(&HostOpenState::Connected));
        assert_eq!(states.get("h2"), Some(&HostOpenState::Disconnected));
    }

    #[test]
    fn detail_and_viewer_tabs_do_not_count_as_open() {
        // Git Diff / 内置编辑器的 host_id 是来源标签 id，和主机 id 撞上也不能算打开。
        let states = host_open_states(&[
            tab(TerminalSessionKind::ProcessList, Some("h1")),
            tab(TerminalSessionKind::GitDiff, Some("h2")),
            tab(TerminalSessionKind::CodeViewer, Some("h2")),
            tab(TerminalSessionKind::Local, None),
        ]);
        assert!(states.is_empty());
    }

    #[test]
    fn current_host_follows_viewer_tabs_back_to_their_source() {
        let tabs = [
            tab(TerminalSessionKind::Remote, Some("h1")),
            tab(TerminalSessionKind::CodeViewer, Some("tab-0")),
            tab(TerminalSessionKind::SystemInfo, Some("h2")),
            tab(TerminalSessionKind::Local, None),
        ];
        assert_eq!(current_host_id(&tabs, 0, Some(0)), Some("h1"));
        assert_eq!(current_host_id(&tabs, 1, Some(0)), Some("h1"));
        assert_eq!(current_host_id(&tabs, 1, None), None);
        assert_eq!(current_host_id(&tabs, 2, Some(2)), Some("h2"));
        assert_eq!(current_host_id(&tabs, 3, Some(3)), None);
    }

    #[test]
    fn click_connects_when_host_has_no_session() {
        let tabs = [
            tab(TerminalSessionKind::Local, None),
            tab(TerminalSessionKind::ProcessList, Some("h1")),
        ];
        assert_eq!(
            host_list_click_target(&tabs, 0, "h1"),
            HostListClick::Connect
        );
    }

    #[test]
    fn click_jumps_to_open_session_and_cycles_from_the_current_one() {
        let tabs = [
            tab(TerminalSessionKind::Local, None),
            tab(TerminalSessionKind::Remote, Some("h1")),
            tab(TerminalSessionKind::Remote, Some("h2")),
            tab(TerminalSessionKind::Remote, Some("h1")),
        ];
        assert_eq!(
            host_list_click_target(&tabs, 0, "h1"),
            HostListClick::Activate(1)
        );
        assert_eq!(
            host_list_click_target(&tabs, 1, "h1"),
            HostListClick::Activate(3)
        );
        assert_eq!(
            host_list_click_target(&tabs, 3, "h1"),
            HostListClick::Activate(1)
        );
        assert_eq!(
            host_list_click_target(&tabs, 2, "h2"),
            HostListClick::Activate(2)
        );
        // 在 tab 3 的内置编辑器上点：调用方传来源 tab 3 作锚点，轮到下一个会话。
        assert_eq!(
            host_list_click_target(&tabs, 3, "h1"),
            HostListClick::Activate(1)
        );
    }
}
