use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OpenFlags};

const HOST_DATABASE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS groups (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS tags (
    name TEXT PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS host_access_history (
    host_id     TEXT PRIMARY KEY,
    accessed_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS hosts (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    host         TEXT NOT NULL,
    port         INTEGER NOT NULL DEFAULT 22,
    username     TEXT NOT NULL DEFAULT 'root',
    protocol     TEXT NOT NULL DEFAULT 'ssh',
    auth_method  TEXT NOT NULL DEFAULT 'password',
    password     TEXT,
    private_key  TEXT,
    key_passphrase TEXT,
    ca_cert      TEXT,
    serial_port  TEXT,
    serial_baud_rate INTEGER NOT NULL DEFAULT 115200,
    serial_data_bits INTEGER NOT NULL DEFAULT 8,
    serial_stop_bits INTEGER NOT NULL DEFAULT 1,
    serial_parity TEXT NOT NULL DEFAULT 'none',
    serial_flow_control TEXT NOT NULL DEFAULT 'none',
    serial_dtr INTEGER NOT NULL DEFAULT 0,
    serial_rts INTEGER NOT NULL DEFAULT 0,
    group_id     TEXT,
    sort_order   INTEGER NOT NULL DEFAULT 0,
    keep_alive_enabled INTEGER NOT NULL DEFAULT 1,
    keep_alive_interval INTEGER NOT NULL DEFAULT 30,
    keep_alive_max_failures INTEGER NOT NULL DEFAULT 3,
    tcp_connect_timeout INTEGER NOT NULL DEFAULT 15,
    auth_timeout INTEGER NOT NULL DEFAULT 30,
    term_encoding TEXT NOT NULL DEFAULT 'utf-8',
    tags         TEXT NOT NULL DEFAULT '[]',
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    FOREIGN KEY (group_id) REFERENCES groups(id) ON DELETE SET NULL
);
"#;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostManagementSnapshot {
    pub title: &'static str,
    pub top_actions: [&'static str; 4],
    pub groups: Vec<HostGroupSnapshot>,
    pub search_placeholder: &'static str,
    pub protocol_filter_label: &'static str,
    pub available_tags: Vec<String>,
    pub hosts: Vec<HostCardSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostGroupSnapshot {
    pub id: String,
    pub label: String,
    pub count: usize,
    pub selected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentHostSnapshot {
    pub host_id: String,
    pub name: String,
    pub group_name: Option<String>,
    pub accessed_at: i64,
    pub protocol: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HostCardSnapshot {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub endpoint: String,
    pub description: String,
    pub connection: HostConnectionConfig,
    pub group_id: Option<String>,
    pub tags: Vec<String>,
    pub system: HostSystemIcon,
    pub sort_order: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HostConnectionConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub key_passphrase: Option<String>,
    pub ca_cert: Option<String>,
    pub serial_port: Option<String>,
    pub serial_baud_rate: u32,
    pub serial_data_bits: u8,
    pub serial_stop_bits: u8,
    pub serial_parity: String,
    pub serial_flow_control: String,
    pub serial_dtr: bool,
    pub serial_rts: bool,
    pub keep_alive_enabled: bool,
    pub keep_alive_interval: u16,
    pub keep_alive_max_failures: u8,
    pub tcp_connect_timeout: u16,
    pub auth_timeout: u16,
    pub term_encoding: String,
    #[serde(default)]
    pub key_id: Option<String>,
    // RDP 显示质量：标准（逻辑像素）/ 高清（物理像素）。仅 RDP 协议使用。
    #[serde(default)]
    pub rdp_display_quality: RdpDisplayQuality,
}

// RDP 显示质量二选一：标准=逻辑像素（默认），高清=物理像素（HiDPI）。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RdpDisplayQuality {
    #[default]
    Standard,
    Hidpi,
}

impl RdpDisplayQuality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Hidpi => "hidpi",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "hidpi" => Self::Hidpi,
            _ => Self::Standard,
        }
    }
}

impl HostConnectionConfig {
    pub fn ssh(host: impl Into<String>, port: u16, username: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port,
            username: username.into(),
            auth_method: "password".to_string(),
            password: None,
            private_key: None,
            key_passphrase: None,
            ca_cert: None,
            serial_port: None,
            serial_baud_rate: 115_200,
            serial_data_bits: 8,
            serial_stop_bits: 1,
            serial_parity: "none".to_string(),
            serial_flow_control: "none".to_string(),
            serial_dtr: false,
            serial_rts: false,
            keep_alive_enabled: true,
            keep_alive_interval: 30,
            keep_alive_max_failures: 3,
            tcp_connect_timeout: 15,
            auth_timeout: 30,
            term_encoding: "utf-8".to_string(),
            key_id: None,
            rdp_display_quality: RdpDisplayQuality::Standard,
        }
    }

    pub fn rdp(host: impl Into<String>, port: u16, username: impl Into<String>) -> Self {
        let mut config = Self::ssh(host, port, username);
        config.serial_baud_rate = 115_200;
        config
    }

    pub fn serial(serial_port: impl Into<String>, serial_baud_rate: u32) -> Self {
        let serial_port = serial_port.into();
        Self {
            host: serial_port.clone(),
            port: 22,
            username: "serial".to_string(),
            auth_method: "password".to_string(),
            password: None,
            private_key: None,
            key_passphrase: None,
            ca_cert: None,
            serial_port: Some(serial_port),
            serial_baud_rate,
            serial_data_bits: 8,
            serial_stop_bits: 1,
            serial_parity: "none".to_string(),
            serial_flow_control: "none".to_string(),
            serial_dtr: false,
            serial_rts: false,
            keep_alive_enabled: true,
            keep_alive_interval: 30,
            keep_alive_max_failures: 3,
            tcp_connect_timeout: 15,
            auth_timeout: 30,
            term_encoding: "utf-8".to_string(),
            key_id: None,
            rdp_display_quality: RdpDisplayQuality::Standard,
        }
    }

    pub fn endpoint(&self, protocol: &str) -> String {
        if protocol.eq_ignore_ascii_case("serial") {
            format!(
                "{} @ {}",
                self.serial_port.as_deref().unwrap_or(self.host.as_str()),
                self.serial_baud_rate
            )
        } else {
            format!("{}@{}:{}", self.username, self.host, self.port)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PtyCommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub status: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostConnectionPlan {
    SavedSsh {
        session_id: String,
        title: String,
        config: HostConnectionConfig,
    },
    DirectPty {
        session_id: String,
        title: String,
        command: PtyCommandSpec,
    },
    Serial {
        session_id: String,
        title: String,
        config: HostConnectionConfig,
    },
    Rdp {
        session_id: String,
        title: String,
        config: HostConnectionConfig,
    },
    Unsupported {
        title: String,
        reason: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostEditDraft {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub description: String,
}

impl HostEditDraft {
    pub fn from_card(host: &HostCardSnapshot) -> Option<Self> {
        if host.protocol != "SSH" {
            return None;
        }
        Some(Self {
            id: host.id.clone(),
            name: host.name.clone(),
            host: host.connection.host.clone(),
            port: host.connection.port,
            username: host.connection.username.clone(),
            description: host.description.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HostSystemIcon {
    Terminal,
    Linux,
    Serial,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolFilter {
    All,
    Ssh,
    Serial,
    Rdp,
}

fn all_hosts_group_label() -> String {
    rust_i18n::t!("host_group_all").to_string()
}

impl ProtocolFilter {
    pub fn label(self) -> String {
        match self {
            Self::All => rust_i18n::t!("host_protocol_all").to_string(),
            Self::Ssh => "SSH".to_string(),
            Self::Serial => "Serial".to_string(),
            Self::Rdp => "RDP".to_string(),
        }
    }

    fn next(self) -> Self {
        match self {
            Self::All => Self::Ssh,
            Self::Ssh => Self::Serial,
            Self::Serial => Self::Rdp,
            Self::Rdp => Self::All,
        }
    }

    fn matches(self, host: &HostCardSnapshot) -> bool {
        match self {
            Self::All => true,
            Self::Ssh => host.protocol == "SSH",
            Self::Serial => host.protocol == "Serial",
            Self::Rdp => host.protocol == "RDP",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostViewMode {
    Grid,
    List,
    Status,
    Keys,
}

// 右键菜单复制/剪切的剪贴板操作类型（会话内内存态）
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostClipboardOp {
    Copy,
    Cut,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostManagementState {
    pub snapshot: HostManagementSnapshot,
    pub query: String,
    pub selected_group_id: Option<String>,
    pub selected_tags: BTreeSet<String>,
    pub protocol_filter: ProtocolFilter,
    pub view_mode: HostViewMode,
    pub selected_host_ids: BTreeSet<String>,
    pub privacy_mode: bool,
    pub notice: Option<String>,
    draft_host_count: usize,
    pub refresh_count: u64,
    pub reorder_mode: bool,
    pub host_drag_in_progress: bool,
    // 右键菜单：复制/剪切的源主机快照；剪切待粘贴后删源
    pub host_clipboard: Option<(HostCardSnapshot, HostClipboardOp)>,
    // 右键菜单：最近一次删除的主机备份，供"恢复"
    pub deleted_host_backup: Vec<HostCardSnapshot>,
    // 右键菜单目标：仅用于卡片高亮与编辑/删除作用对象，不进 selected_host_ids
    pub context_menu_target: Option<String>,
    // 密钥页：当前选中查看详情的密钥 id
    pub selected_key_id: Option<String>,
    // 密钥页：详情面板「复制公钥到服务器」命令是否已展开（点击才展开 + 复制）
    pub copy_cmd_expanded: bool,
    // 密钥页：删除按钮是否处于二次确认态
    pub key_delete_confirming: bool,
}

impl HostManagementState {
    pub fn new(snapshot: HostManagementSnapshot) -> Self {
        Self {
            snapshot,
            query: String::new(),
            selected_group_id: None,
            selected_tags: BTreeSet::new(),
            protocol_filter: ProtocolFilter::All,
            view_mode: HostViewMode::Grid,
            selected_host_ids: BTreeSet::new(),
            privacy_mode: false,
            notice: None,
            draft_host_count: 0,
            refresh_count: 0,
            reorder_mode: false,
            host_drag_in_progress: false,
            host_clipboard: None,
            deleted_host_backup: Vec::new(),
            context_menu_target: None,
            selected_key_id: None,
            copy_cmd_expanded: false,
            key_delete_confirming: false,
        }
    }

    pub fn filtered_hosts(&self) -> Vec<HostCardSnapshot> {
        let query = self.query.trim().to_lowercase();

        self.snapshot
            .hosts
            .iter()
            .filter(|host| {
                self.selected_group_id
                    .as_deref()
                    .map_or(true, |id| host.group_id.as_deref() == Some(id))
            })
            .filter(|host| {
                self.selected_tags.is_empty()
                    || host.tags.iter().any(|tag| self.selected_tags.contains(tag))
            })
            .filter(|host| self.protocol_filter.matches(host))
            .filter(|host| {
                if query.is_empty() {
                    return true;
                }

                host.name.to_lowercase().contains(&query)
                    || host.endpoint.to_lowercase().contains(&query)
                    || host.description.to_lowercase().contains(&query)
                    || host
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&query))
            })
            .cloned()
            .collect()
    }

    pub fn groups_for_render(&self) -> Vec<HostGroupSnapshot> {
        self.snapshot
            .groups
            .iter()
            .map(|group| {
                let selected = if group.id == "all" {
                    self.selected_group_id.is_none()
                } else {
                    self.selected_group_id.as_deref() == Some(group.id.as_str())
                };
                let count = if group.id == "all" {
                    self.snapshot.hosts.len()
                } else {
                    self.snapshot
                        .hosts
                        .iter()
                        .filter(|host| host.group_id.as_deref() == Some(group.id.as_str()))
                        .count()
                };

                HostGroupSnapshot {
                    id: group.id.clone(),
                    label: if group.id == "all" {
                        all_hosts_group_label()
                    } else {
                        group.label.clone()
                    },
                    count,
                    selected,
                }
            })
            .collect()
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.retain_visible_selection();
    }

    pub fn push_search_text(&mut self, text: &str) {
        self.query.push_str(text);
        self.retain_visible_selection();
    }

    pub fn backspace_search(&mut self) {
        self.query.pop();
        self.retain_visible_selection();
    }

    pub fn clear_search(&mut self) {
        self.query.clear();
        self.retain_visible_selection();
    }

    pub fn select_group(&mut self, id: &str) {
        self.selected_group_id = (id != "all").then(|| id.to_string());
        self.retain_visible_selection();
    }

    pub fn toggle_tag(&mut self, tag: &str) {
        if !self.selected_tags.remove(tag) {
            self.selected_tags.insert(tag.to_string());
        }
        self.retain_visible_selection();
    }

    pub fn cycle_protocol_filter(&mut self) {
        self.protocol_filter = self.protocol_filter.next();
        self.retain_visible_selection();
    }

    pub fn set_protocol_filter(&mut self, filter: ProtocolFilter) {
        self.protocol_filter = filter;
        self.retain_visible_selection();
    }

    pub fn set_view_mode(&mut self, mode: HostViewMode) {
        self.view_mode = mode;
    }

    pub fn toggle_privacy_mode(&mut self) {
        self.privacy_mode = !self.privacy_mode;
    }

    pub fn replace_snapshot(&mut self, snapshot: HostManagementSnapshot) {
        self.snapshot = snapshot;
        self.selected_tags
            .retain(|tag| self.snapshot.available_tags.contains(tag));
        self.retain_visible_selection();
    }

    pub fn refresh(&mut self) {
        self.refresh_count = self.refresh_count.wrapping_add(1);
        self.notice = Some("已刷新本地主机视图".to_string());
    }

    pub fn toggle_select_host(&mut self, host_id: &str) {
        if !self.selected_host_ids.remove(host_id) {
            self.selected_host_ids.insert(host_id.to_string());
        }
    }

    // 单选：若当前唯一选中即此项则取消，否则清空并只选中目标
    pub fn select_single_host(&mut self, host_id: &str) {
        if self.selected_host_ids.len() == 1 && self.selected_host_ids.contains(host_id) {
            self.selected_host_ids.clear();
            return;
        }
        self.selected_host_ids.clear();
        self.selected_host_ids.insert(host_id.to_string());
    }

    pub fn toggle_select_all_filtered(&mut self) {
        let visible_ids: BTreeSet<_> = self
            .filtered_hosts()
            .into_iter()
            .map(|host| host.id)
            .collect();
        if visible_ids.is_empty() {
            self.selected_host_ids.clear();
            return;
        }

        if visible_ids
            .iter()
            .all(|host_id| self.selected_host_ids.contains(host_id))
        {
            for host_id in visible_ids {
                self.selected_host_ids.remove(&host_id);
            }
        } else {
            self.selected_host_ids.extend(visible_ids);
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected_host_ids.clear();
    }

    pub fn selected_count(&self) -> usize {
        self.selected_host_ids.len()
    }

    pub fn all_filtered_selected(&self) -> bool {
        let visible_ids: Vec<_> = self
            .filtered_hosts()
            .into_iter()
            .map(|host| host.id)
            .collect();
        !visible_ids.is_empty()
            && visible_ids
                .iter()
                .all(|host_id| self.selected_host_ids.contains(host_id))
    }

    pub fn delete_selected(&mut self) {
        let deleted = self.selected_host_ids.len();
        if deleted == 0 {
            return;
        }

        self.snapshot
            .hosts
            .retain(|host| !self.selected_host_ids.contains(&host.id));
        self.selected_host_ids.clear();
        self.notice = Some(format!("已删除 {deleted} 台本地主机"));
    }

    pub fn add_draft_host(&mut self) {
        self.draft_host_count += 1;
        let id = format!("draft-host-{}", self.draft_host_count);
        let name = if self.draft_host_count == 1 {
            "新建主机".to_string()
        } else {
            format!("新建主机 {}", self.draft_host_count)
        };

        let connection = HostConnectionConfig::ssh("127.0.0.1", 22, "root");
        self.snapshot.hosts.insert(
            0,
            HostCardSnapshot {
                id,
                name,
                protocol: "SSH".to_string(),
                endpoint: connection.endpoint("SSH"),
                description: "本地草稿".to_string(),
                connection,
                group_id: Some("default".to_string()),
                tags: Vec::new(),
                system: HostSystemIcon::Terminal,
                sort_order: 0,
            },
        );
        self.notice = Some("已新增本地草稿主机".to_string());
    }

    pub fn apply_edit_draft_fields(
        &mut self,
        id: &str,
        name: &str,
        protocol: &str,
        description: &str,
        connection: HostConnectionConfig,
        group_id: Option<String>,
        tags: Vec<String>,
        system: HostSystemIcon,
        is_new: bool,
    ) {
        if is_new {
            self.snapshot.hosts.insert(
                0,
                HostCardSnapshot {
                    id: id.to_string(),
                    name: name.to_string(),
                    protocol: protocol.to_string(),
                    endpoint: connection.endpoint(protocol),
                    description: description.to_string(),
                    connection,
                    group_id,
                    tags,
                    system,
                    sort_order: 0,
                },
            );
            self.notice = Some("已新增主机".to_string());
        } else if let Some(host) = self.snapshot.hosts.iter_mut().find(|h| h.id == id) {
            host.name = name.to_string();
            host.protocol = protocol.to_string();
            host.endpoint = connection.endpoint(protocol);
            host.description = description.to_string();
            host.connection = connection;
            host.group_id = group_id;
            host.tags = tags;
            host.system = system;
            self.notice = Some("已更新主机".to_string());
        }
    }

    pub fn host_by_id(&self, id: &str) -> Option<&HostCardSnapshot> {
        self.snapshot.hosts.iter().find(|host| host.id == id)
    }

    pub fn connect_command_for(&self, id: &str) -> Option<Vec<u8>> {
        let host = self.host_by_id(id)?;
        match host.protocol.as_str() {
            "SSH" => {
                let command = ssh_command_for_host(host);
                (!command.is_empty()).then(|| command.into_bytes())
            }
            "Serial" => None,
            _ => None,
        }
    }

    pub fn connection_plan_for(&self, id: &str) -> Option<HostConnectionPlan> {
        let host = self.host_by_id(id)?;
        Some(match host.protocol.as_str() {
            "SSH" => match ssh_saved_connection_error(&host.connection) {
                None => HostConnectionPlan::SavedSsh {
                    session_id: session_id_for_host(&host.id),
                    title: host.name.clone(),
                    config: host.connection.clone(),
                },
                Some(reason) => HostConnectionPlan::Unsupported {
                    title: host.name.clone(),
                    reason,
                },
            },
            "Serial" => match serial_saved_connection_error(&host.connection) {
                None => HostConnectionPlan::Serial {
                    session_id: session_id_for_host(&host.id),
                    title: host.name.clone(),
                    config: host.connection.clone(),
                },
                Some(reason) => HostConnectionPlan::Unsupported {
                    title: host.name.clone(),
                    reason,
                },
            },
            "RDP" => match rdp_saved_connection_error(&host.connection) {
                None => HostConnectionPlan::Rdp {
                    session_id: session_id_for_host(&host.id),
                    title: host.name.clone(),
                    config: host.connection.clone(),
                },
                Some(reason) => HostConnectionPlan::Unsupported {
                    title: host.name.clone(),
                    reason,
                },
            },
            protocol => HostConnectionPlan::Unsupported {
                title: host.name.clone(),
                reason: format!("暂不支持 {protocol} 直连"),
            },
        })
    }

    pub fn edit_draft_for(&self, id: &str) -> Option<HostEditDraft> {
        HostEditDraft::from_card(self.host_by_id(id)?)
    }

    fn retain_visible_selection(&mut self) {
        let visible_ids: BTreeSet<_> = self
            .filtered_hosts()
            .into_iter()
            .map(|host| host.id)
            .collect();
        self.selected_host_ids
            .retain(|host_id| visible_ids.contains(host_id));
    }
}

pub fn load_host_management_snapshot() -> Result<HostManagementSnapshot, String> {
    let db_path =
        default_database_path().ok_or_else(|| "cannot resolve NexShell db path".to_string())?;
    load_host_management_snapshot_from_db_path(&db_path)
}

pub fn load_or_initialize_host_management_snapshot_from_db_path(
    db_path: &Path,
) -> Result<HostManagementSnapshot, String> {
    initialize_host_database(db_path)?;
    load_host_management_snapshot_from_db_path(db_path)
}

pub fn load_host_management_snapshot_from_db_path(
    db_path: &Path,
) -> Result<HostManagementSnapshot, String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("open NexShell db {}: {error}", db_path.display()))?;

    let groups = load_groups(&conn)?;
    let mut hosts = load_hosts(&conn)?;
    let mut tags = BTreeSet::new();
    for host in &hosts {
        tags.extend(host.tags.iter().cloned());
    }
    if let Ok(mut stmt) = conn.prepare("SELECT name FROM tags") {
        if let Ok(rows) = stmt.query_map([], |row| row.get(0)) {
            let db_tags: Vec<String> = rows.filter_map(|r| r.ok()).collect();
            tags.extend(db_tags);
        }
    }

    let mut groups_for_render = Vec::with_capacity(groups.len() + 1);
    groups_for_render.push(HostGroupSnapshot {
        id: "all".to_string(),
        label: all_hosts_group_label(),
        count: hosts.len(),
        selected: true,
    });
    groups_for_render.extend(groups.into_iter().map(|(id, label)| {
        let count = hosts
            .iter()
            .filter(|host| host.group_id.as_deref() == Some(id.as_str()))
            .count();
        HostGroupSnapshot {
            id,
            label,
            count,
            selected: false,
        }
    }));

    if hosts.is_empty() {
        hosts = Vec::new();
    }

    Ok(HostManagementSnapshot {
        title: "主机管理",
        top_actions: ["隐私", "导入", "云同步", "新建主机"],
        groups: groups_for_render,
        search_placeholder: "搜索主机或分组...",
        protocol_filter_label: "所有协议",
        available_tags: tags.into_iter().collect(),
        hosts,
    })
}

pub fn initialize_host_database(db_path: &Path) -> Result<(), String> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create NexShell db dir {}: {error}", parent.display()))?;
    }

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )
    .map_err(|error| format!("open NexShell db {}: {error}", db_path.display()))?;
    conn.execute_batch(HOST_DATABASE_SCHEMA)
        .map_err(|error| format!("initialize NexShell host db: {error}"))?;
    migrate_add_sort_order(&conn);
    migrate_add_rdp_display_quality(&conn);
    migrate_seed_tags_table(&conn);
    crate::ssh_key_store::ensure_schema(&conn)?;
    Ok(())
}

// 给 hosts 补 rdp_display_quality 列（幂等；已存在则 ALTER 失败被忽略）。
fn migrate_add_rdp_display_quality(conn: &Connection) {
    let _ = conn.execute(
        "ALTER TABLE hosts ADD COLUMN rdp_display_quality TEXT NOT NULL DEFAULT 'standard'",
        [],
    );
}

fn migrate_add_sort_order(conn: &Connection) {
    let added = conn
        .execute(
            "ALTER TABLE hosts ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .is_ok();
    if added {
        let _ = conn.execute_batch(
            "UPDATE hosts SET sort_order = (
                SELECT COUNT(*) FROM hosts h2 WHERE h2.created_at > hosts.created_at
            )",
        );
    }
}

fn migrate_seed_tags_table(conn: &Connection) {
    let mut stmt = match conn.prepare("SELECT tags FROM hosts") {
        Ok(s) => s,
        Err(_) => return,
    };
    let rows: Vec<String> = match stmt.query_map([], |row| row.get(0)) {
        Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
        Err(_) => return,
    };
    for tags_json in &rows {
        for tag in parse_tags(tags_json) {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
                params![tag],
            );
        }
    }
}

// 记录主机访问时间（最近访问用）。host_id 主键，每主机只存最后一次。
pub fn record_host_access_in_db(db_path: &Path, host_id: &str) -> Result<(), String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e| format!("open db: {e}"))?;
    conn.execute(
        "INSERT INTO host_access_history (host_id, accessed_at) VALUES (?1, ?2)
         ON CONFLICT(host_id) DO UPDATE SET accessed_at = excluded.accessed_at",
        params![host_id, unix_ts_seconds()],
    )
    .map_err(|e| format!("record host access: {e}"))?;
    Ok(())
}

// 最近访问的主机 (host_id, accessed_at)，按时间倒序。表缺失/出错时返回空。
pub fn get_recent_access(db_path: &Path, limit: usize) -> Vec<(String, i64)> {
    let conn = match Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut stmt = match conn.prepare(
        "SELECT host_id, accessed_at FROM host_access_history ORDER BY accessed_at DESC LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    });
    match rows {
        Ok(r) => r.filter_map(|x| x.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

pub fn create_tag_in_db(db_path: &Path, name: &str) -> Result<(), String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e| format!("open db: {e}"))?;
    conn.execute(
        "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
        params![name],
    )
    .map_err(|e| format!("insert tag: {e}"))?;
    Ok(())
}

pub fn update_host_sort_orders(db_path: &Path, updates: &[(String, i64)]) -> Result<(), String> {
    if updates.is_empty() {
        return Ok(());
    }
    let mut conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| format!("open NexShell db {}: {error}", db_path.display()))?;
    let tx = conn
        .transaction()
        .map_err(|error| format!("sort order transaction: {error}"))?;
    for (id, order) in updates {
        tx.execute(
            "UPDATE hosts SET sort_order = ?1 WHERE id = ?2",
            params![order, id],
        )
        .map_err(|error| format!("update sort_order for {id}: {error}"))?;
    }
    tx.commit()
        .map_err(|error| format!("sort order commit: {error}"))
}

pub fn unavailable_host_management_snapshot() -> HostManagementSnapshot {
    HostManagementSnapshot {
        title: "主机管理",
        top_actions: ["隐私", "导入", "云同步", "新建主机"],
        groups: vec![HostGroupSnapshot {
            id: "all".to_string(),
            label: all_hosts_group_label(),
            count: 0,
            selected: true,
        }],
        search_placeholder: "搜索主机或分组...",
        protocol_filter_label: "所有协议",
        available_tags: Vec::new(),
        hosts: Vec::new(),
    }
}

pub fn create_draft_host_in_db_path(
    db_path: &Path,
    group_id: Option<&str>,
) -> Result<String, String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| format!("open NexShell db {}: {error}", db_path.display()))?;
    let id = draft_host_id();
    let now = unix_ts_seconds();

    let next_sort_order: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM hosts",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    conn.execute(
        "INSERT INTO hosts (
            id, name, description, host, port, username, protocol, auth_method,
            password, private_key, key_passphrase, ca_cert, serial_port,
            serial_baud_rate, serial_data_bits, serial_stop_bits, serial_parity,
            serial_flow_control, serial_dtr, serial_rts, group_id, sort_order,
            keep_alive_enabled, keep_alive_interval, keep_alive_max_failures,
            tcp_connect_timeout, auth_timeout, term_encoding, tags, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
            NULL, NULL, NULL, NULL, NULL,
            ?9, ?10, ?11, ?12,
            ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20,
            ?21, ?22, ?23, ?24, ?25, ?26
        )",
        params![
            id,
            "新建主机",
            "本地草稿",
            "127.0.0.1",
            22_i64,
            "root",
            "ssh",
            "password",
            115_200_i64,
            8_i64,
            1_i64,
            "none",
            "none",
            0_i64,
            0_i64,
            group_id,
            next_sort_order,
            1_i64,
            30_i64,
            3_i64,
            15_i64,
            30_i64,
            "utf-8",
            "[]",
            now,
            now,
        ],
    )
    .map_err(|error| format!("create draft host: {error}"))?;

    Ok(id)
}

pub fn delete_hosts_from_db_path(
    db_path: &Path,
    host_ids: &BTreeSet<String>,
) -> Result<usize, String> {
    if host_ids.is_empty() {
        return Ok(0);
    }

    let mut conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| format!("open NexShell db {}: {error}", db_path.display()))?;
    let tx = conn
        .transaction()
        .map_err(|error| format!("delete hosts transaction: {error}"))?;

    let mut deleted = 0usize;
    for id in host_ids {
        deleted += tx
            .execute("DELETE FROM hosts WHERE id = ?1", params![id])
            .map_err(|error| format!("delete host {id}: {error}"))?;
    }

    tx.commit()
        .map_err(|error| format!("delete hosts commit: {error}"))?;
    Ok(deleted)
}

// ── Group / Tag CRUD ──

pub fn create_group_in_db(db_path: &Path, name: &str) -> Result<String, String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| format!("open db: {error}"))?;
    let id = format!("group-{}", epoch_millis());
    let max_order: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM groups",
            [],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    conn.execute(
        "INSERT INTO groups (id, name, sort_order) VALUES (?1, ?2, ?3)",
        params![id, name, max_order + 1],
    )
    .map_err(|error| format!("insert group: {error}"))?;
    Ok(id)
}

pub fn rename_group_in_db(db_path: &Path, id: &str, new_name: &str) -> Result<(), String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| format!("open db: {error}"))?;
    conn.execute(
        "UPDATE groups SET name = ?1 WHERE id = ?2",
        params![new_name, id],
    )
    .map_err(|error| format!("rename group: {error}"))?;
    Ok(())
}

pub fn delete_group_from_db(db_path: &Path, id: &str) -> Result<(), String> {
    let mut conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| format!("open db: {error}"))?;
    // 显式在同一事务里把成员置空再删组：不依赖 SQLite foreign_keys 默认值（各环境/CI 可能为 OFF）
    let tx = conn.transaction().map_err(|e| format!("tx: {e}"))?;
    tx.execute(
        "UPDATE hosts SET group_id = NULL WHERE group_id = ?1",
        params![id],
    )
    .map_err(|e| format!("clear host group: {e}"))?;
    tx.execute("DELETE FROM groups WHERE id = ?1", params![id])
        .map_err(|e| format!("delete group: {e}"))?;
    tx.commit().map_err(|e| format!("commit: {e}"))?;
    Ok(())
}

/// 导入时按 id 重建分组：存在则更新名称/排序，不存在则插入。
pub fn upsert_group_in_db(
    db_path: &Path,
    id: &str,
    name: &str,
    sort_order: i64,
) -> Result<(), String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e| format!("open db: {e}"))?;
    conn.execute(
        "INSERT INTO groups (id, name, sort_order) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, sort_order = excluded.sort_order",
        params![id, name, sort_order],
    )
    .map_err(|e| format!("upsert group: {e}"))?;
    Ok(())
}

pub fn delete_tag_from_all_hosts_in_db(db_path: &Path, tag: &str) -> Result<(), String> {
    let mut conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| format!("open db: {error}"))?;
    let tx = conn.transaction().map_err(|e| format!("tx: {e}"))?;
    {
        let mut stmt = tx
            .prepare("SELECT id, tags FROM hosts WHERE tags LIKE ?1")
            .map_err(|e| format!("prepare: {e}"))?;
        let pattern = format!("%{tag}%");
        let rows: Vec<(String, String)> = stmt
            .query_map([&pattern], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| format!("query: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        for (id, tags_json) in &rows {
            let mut tags: Vec<String> = parse_tags(tags_json);
            tags.retain(|t| t != tag);
            let new_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());
            tx.execute(
                "UPDATE hosts SET tags = ?1 WHERE id = ?2",
                params![new_json, id],
            )
            .map_err(|e| format!("update tags: {e}"))?;
        }
        tx.execute("DELETE FROM tags WHERE name = ?1", params![tag])
            .map_err(|e| format!("delete tag row: {e}"))?;
    }
    tx.commit().map_err(|e| format!("commit: {e}"))
}

pub fn rename_tag_in_all_hosts_in_db(
    db_path: &Path,
    old_name: &str,
    new_name: &str,
) -> Result<(), String> {
    let mut conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| format!("open db: {error}"))?;
    let tx = conn.transaction().map_err(|e| format!("tx: {e}"))?;
    {
        let mut stmt = tx
            .prepare("SELECT id, tags FROM hosts WHERE tags LIKE ?1")
            .map_err(|e| format!("prepare: {e}"))?;
        let pattern = format!("%{old_name}%");
        let rows: Vec<(String, String)> = stmt
            .query_map([&pattern], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| format!("query: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        for (id, tags_json) in &rows {
            let tags: Vec<String> = parse_tags(tags_json)
                .into_iter()
                .map(|t| {
                    if t == old_name {
                        new_name.to_string()
                    } else {
                        t
                    }
                })
                .collect();
            let new_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());
            tx.execute(
                "UPDATE hosts SET tags = ?1 WHERE id = ?2",
                params![new_json, id],
            )
            .map_err(|e| format!("update tags: {e}"))?;
        }
    }
    tx.commit().map_err(|e| format!("commit: {e}"))
}

fn epoch_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn upsert_host_card_in_db_path(db_path: &Path, host: &HostCardSnapshot) -> Result<(), String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| format!("open NexShell db {}: {error}", db_path.display()))?;
    let now = unix_ts_seconds();
    let tags_json = serde_json::to_string(&host.tags)
        .map_err(|error| format!("serialize host {} tags: {error}", host.id))?;
    let protocol = if host.protocol.eq_ignore_ascii_case("serial") {
        "serial"
    } else if host.protocol.eq_ignore_ascii_case("rdp") {
        "rdp"
    } else {
        "ssh"
    };
    let config = &host.connection;

    conn.execute(
        "INSERT INTO hosts (
            id, name, description, host, port, username, protocol, auth_method,
            password, private_key, key_passphrase, ca_cert, serial_port,
            serial_baud_rate, serial_data_bits, serial_stop_bits, serial_parity,
            serial_flow_control, serial_dtr, serial_rts, group_id,
            keep_alive_enabled, keep_alive_interval, keep_alive_max_failures,
            tcp_connect_timeout, auth_timeout, term_encoding, tags, created_at, updated_at, key_id,
            rdp_display_quality
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
            ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17,
            ?18, ?19, ?20, ?21,
            ?22, ?23, ?24,
            ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32
        )
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            description = excluded.description,
            host = excluded.host,
            port = excluded.port,
            username = excluded.username,
            protocol = excluded.protocol,
            auth_method = excluded.auth_method,
            password = excluded.password,
            private_key = excluded.private_key,
            key_passphrase = excluded.key_passphrase,
            ca_cert = excluded.ca_cert,
            serial_port = excluded.serial_port,
            serial_baud_rate = excluded.serial_baud_rate,
            serial_data_bits = excluded.serial_data_bits,
            serial_stop_bits = excluded.serial_stop_bits,
            serial_parity = excluded.serial_parity,
            serial_flow_control = excluded.serial_flow_control,
            serial_dtr = excluded.serial_dtr,
            serial_rts = excluded.serial_rts,
            group_id = excluded.group_id,
            keep_alive_enabled = excluded.keep_alive_enabled,
            keep_alive_interval = excluded.keep_alive_interval,
            keep_alive_max_failures = excluded.keep_alive_max_failures,
            tcp_connect_timeout = excluded.tcp_connect_timeout,
            auth_timeout = excluded.auth_timeout,
            term_encoding = excluded.term_encoding,
            tags = excluded.tags,
            updated_at = excluded.updated_at,
            key_id = excluded.key_id,
            rdp_display_quality = excluded.rdp_display_quality",
        params![
            host.id,
            host.name.trim(),
            host.description.trim(),
            config.host.trim(),
            i64::from(config.port),
            config.username.trim(),
            protocol,
            config.auth_method.trim(),
            config.password.as_deref().map(str::trim),
            config.private_key.as_deref().map(str::trim),
            config.key_passphrase.as_deref().map(str::trim),
            config.ca_cert.as_deref().map(str::trim),
            config.serial_port.as_deref().map(str::trim),
            i64::from(config.serial_baud_rate),
            i64::from(config.serial_data_bits),
            i64::from(config.serial_stop_bits),
            config.serial_parity.trim(),
            config.serial_flow_control.trim(),
            if config.serial_dtr { 1_i64 } else { 0_i64 },
            if config.serial_rts { 1_i64 } else { 0_i64 },
            host.group_id.as_deref(),
            if config.keep_alive_enabled {
                1_i64
            } else {
                0_i64
            },
            i64::from(config.keep_alive_interval),
            i64::from(config.keep_alive_max_failures),
            i64::from(config.tcp_connect_timeout),
            i64::from(config.auth_timeout),
            config.term_encoding.trim(),
            tags_json,
            now,
            now,
            config.key_id.as_deref().map(str::trim),
            config.rdp_display_quality.as_str(),
        ],
    )
    .map_err(|error| format!("upsert host {}: {error}", host.id))?;

    Ok(())
}

pub fn update_host_basic_in_db_path(db_path: &Path, draft: &HostEditDraft) -> Result<(), String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| format!("open NexShell db {}: {error}", db_path.display()))?;
    let changed = conn
        .execute(
            "UPDATE hosts SET
                name = ?2,
                description = ?3,
                host = ?4,
                port = ?5,
                username = ?6,
                updated_at = ?7
             WHERE id = ?1",
            params![
                draft.id,
                draft.name.trim(),
                draft.description.trim(),
                draft.host.trim(),
                i64::from(draft.port),
                draft.username.trim(),
                unix_ts_seconds(),
            ],
        )
        .map_err(|error| format!("update host {}: {error}", draft.id))?;

    if changed == 0 {
        Err(format!("host {} not found", draft.id))
    } else {
        Ok(())
    }
}

pub fn default_database_path() -> Option<PathBuf> {
    database_path_from_env(
        env::consts::OS,
        env::var_os("NEXSHELL_DB_PATH").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
        env::var_os("APPDATA").map(PathBuf::from),
        env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
    )
}

fn database_path_from_env(
    target_os: &str,
    explicit_path: Option<PathBuf>,
    home: Option<PathBuf>,
    appdata: Option<PathBuf>,
    xdg_config_home: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(path) = explicit_path {
        return Some(path);
    }

    match target_os {
        "macos" => home.map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("com.matt.nexshell")
                .join("nexshell.db")
        }),
        "windows" => appdata.map(|base| base.join("com.matt.nexshell").join("nexshell.db")),
        _ => {
            let home = home?;
            let config_home = xdg_config_home.unwrap_or_else(|| home.join(".config"));
            Some(config_home.join("com.matt.nexshell").join("nexshell.db"))
        }
    }
}

pub fn legacy_host_management_snapshot() -> HostManagementSnapshot {
    let hosts = legacy_hosts();

    HostManagementSnapshot {
        title: "主机管理",
        top_actions: ["隐私", "导入", "云同步", "新建主机"],
        groups: vec![
            HostGroupSnapshot {
                id: "all".to_string(),
                label: all_hosts_group_label(),
                count: hosts.len(),
                selected: true,
            },
            HostGroupSnapshot {
                id: "default".to_string(),
                label: "默认分组".to_string(),
                count: 1,
                selected: false,
            },
        ],
        search_placeholder: "搜索主机或分组...",
        protocol_filter_label: "所有协议",
        available_tags: vec!["测试标签".to_string()],
        hosts,
    }
}

fn load_groups(conn: &Connection) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name FROM groups ORDER BY sort_order ASC")
        .map_err(|error| format!("query groups: {error}"))?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| format!("query groups: {error}"))?;

    let mut groups = Vec::new();
    for row in rows {
        groups.push(row.map_err(|error| format!("read group row: {error}"))?);
    }
    Ok(groups)
}

fn load_hosts(conn: &Connection) -> Result<Vec<HostCardSnapshot>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, COALESCE(description, ''), host, COALESCE(port, 22), \
             COALESCE(username, 'root'), COALESCE(protocol, 'ssh'), \
             COALESCE(auth_method, 'password'), password, private_key, key_passphrase, ca_cert, \
             serial_port, COALESCE(serial_baud_rate, 115200), COALESCE(serial_data_bits, 8), \
             COALESCE(serial_stop_bits, 1), COALESCE(serial_parity, 'none'), \
             COALESCE(serial_flow_control, 'none'), COALESCE(serial_dtr, 0), \
             COALESCE(serial_rts, 0), group_id, COALESCE(keep_alive_enabled, 1), \
             COALESCE(keep_alive_interval, 30), COALESCE(keep_alive_max_failures, 3), \
             COALESCE(tcp_connect_timeout, 15), COALESCE(auth_timeout, 30), \
             COALESCE(term_encoding, 'utf-8'), COALESCE(tags, '[]'), \
             COALESCE(sort_order, 0), key_id, \
             COALESCE(rdp_display_quality, 'standard') \
             FROM hosts ORDER BY sort_order ASC, created_at DESC",
        )
        .map_err(|error| format!("query hosts: {error}"))?;
    let rows = stmt
        .query_map([], |row| {
            let protocol_raw: String = row.get(6)?;
            let is_serial = protocol_raw.eq_ignore_ascii_case("serial");
            let is_rdp = protocol_raw.eq_ignore_ascii_case("rdp");
            let protocol = if is_serial {
                "Serial".to_string()
            } else if is_rdp {
                "RDP".to_string()
            } else {
                "SSH".to_string()
            };
            let tags_json: String = row.get(27)?;
            let connection = HostConnectionConfig {
                host: row.get(3)?,
                port: row.get::<_, i32>(4)? as u16,
                username: row.get(5)?,
                auth_method: row.get(7)?,
                password: row.get(8)?,
                private_key: row.get(9)?,
                key_passphrase: row.get(10)?,
                ca_cert: row.get(11)?,
                serial_port: row.get(12)?,
                serial_baud_rate: row.get::<_, i64>(13)? as u32,
                serial_data_bits: row.get::<_, i64>(14)? as u8,
                serial_stop_bits: row.get::<_, i64>(15)? as u8,
                serial_parity: row.get(16)?,
                serial_flow_control: row.get(17)?,
                serial_dtr: row.get::<_, i64>(18)? != 0,
                serial_rts: row.get::<_, i64>(19)? != 0,
                keep_alive_enabled: row.get::<_, i64>(21)? != 0,
                keep_alive_interval: row.get::<_, i64>(22)? as u16,
                keep_alive_max_failures: row.get::<_, i64>(23)? as u8,
                tcp_connect_timeout: row.get::<_, i64>(24)? as u16,
                auth_timeout: row.get::<_, i64>(25)? as u16,
                term_encoding: row.get(26)?,
                key_id: row.get(29)?,
                rdp_display_quality: RdpDisplayQuality::from_db(&row.get::<_, String>(30)?),
            };
            let endpoint = connection.endpoint(&protocol);

            Ok(HostCardSnapshot {
                id: row.get(0)?,
                name: row.get(1)?,
                description: empty_description(row.get::<_, String>(2)?),
                protocol,
                endpoint,
                connection,
                group_id: row.get(20)?,
                tags: parse_tags(&tags_json),
                system: if is_serial {
                    HostSystemIcon::Serial
                } else {
                    HostSystemIcon::Terminal
                },
                sort_order: row.get(28)?,
            })
        })
        .map_err(|error| format!("query hosts: {error}"))?;

    let mut hosts = Vec::new();
    for row in rows {
        hosts.push(row.map_err(|error| format!("read host row: {error}"))?);
    }
    Ok(hosts)
}

// 空描述保持为空：卡片/编辑窗对空值各自处理，不再注入占位文案。
fn empty_description(description: String) -> String {
    description.trim().to_string()
}

fn parse_tags(tags_json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(tags_json).unwrap_or_default()
}

fn ssh_command_for_host(host: &HostCardSnapshot) -> String {
    match ssh_pty_command_for_host(host) {
        Ok(command) => {
            let args = command
                .args
                .iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{} {args}\r", command.program)
        }
        Err(_) => String::new(),
    }
}

fn ssh_saved_connection_error(config: &HostConnectionConfig) -> Option<String> {
    if config.host.trim().is_empty() {
        return Some("主机地址为空".to_string());
    }
    if config.username.trim().is_empty() {
        return Some("用户名为空".to_string());
    }
    if config.auth_method.eq_ignore_ascii_case("key") {
        // 私钥本体或 key_id 引用任一有即可（引用走 resolve_private_key_source 取库内容）
        let no_inline = config
            .private_key
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty();
        let no_keyref = config
            .key_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty();
        if no_inline && no_keyref {
            return Some("密钥认证未保存私钥".to_string());
        }
    } else if config
        .password
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        return Some("密码认证未保存密码".to_string());
    }
    None
}

// RDP 快连门禁：host/username/password 缺一即给具体 reason，不静默落 Unsupported。
fn rdp_saved_connection_error(config: &HostConnectionConfig) -> Option<String> {
    if config.host.trim().is_empty() {
        return Some("主机地址为空".to_string());
    }
    if config.username.trim().is_empty() {
        return Some("用户名为空".to_string());
    }
    if config
        .password
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        return Some("RDP 需要保存密码".to_string());
    }
    None
}

fn serial_saved_connection_error(config: &HostConnectionConfig) -> Option<String> {
    let port = config
        .serial_port
        .as_deref()
        .unwrap_or(config.host.as_str())
        .trim();
    if port.is_empty() {
        return Some("串口为空".to_string());
    }
    if config.serial_baud_rate == 0 {
        return Some("串口波特率为空".to_string());
    }
    None
}

fn ssh_pty_command_for_host(host: &HostCardSnapshot) -> Result<PtyCommandSpec, String> {
    let config = &host.connection;
    let address = config.host.trim();
    if address.is_empty() {
        return Err("主机地址为空".to_string());
    }

    let username = config.username.trim();
    if username.is_empty() {
        return Err("用户名为空".to_string());
    }

    if config.auth_method == "key" {
        if let Some(private_key) = config.private_key.as_deref().map(str::trim) {
            if !private_key.is_empty() && private_key_path_arg(private_key).is_none() {
                return Err(
                    "native direct ssh 暂不支持内联私钥内容，后续需要接入 Tauri russh 连接后端"
                        .to_string(),
                );
            }
        }
    }

    let target = format!("{username}@{address}");
    let mut args = vec![
        "-p".to_string(),
        config.port.to_string(),
        "-o".to_string(),
        format!("ConnectTimeout={}", config.tcp_connect_timeout.clamp(5, 60)),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
    ];

    if config.keep_alive_enabled {
        args.extend([
            "-o".to_string(),
            format!(
                "ServerAliveInterval={}",
                config.keep_alive_interval.clamp(10, 300)
            ),
            "-o".to_string(),
            format!(
                "ServerAliveCountMax={}",
                config.keep_alive_max_failures.clamp(1, 10)
            ),
        ]);
    }

    if config.auth_method == "key" {
        if let Some(private_key) = config
            .private_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(private_key_path_arg)
        {
            args.extend([
                "-i".to_string(),
                private_key,
                "-o".to_string(),
                "IdentitiesOnly=yes".to_string(),
            ]);
        }
    }

    args.push(target.clone());

    Ok(PtyCommandSpec {
        program: "ssh".to_string(),
        args,
        status: format!("connecting SSH: {target}:{}", config.port),
    })
}

fn private_key_path_arg(private_key: &str) -> Option<String> {
    if private_key.contains('\n') || private_key.contains("BEGIN ") {
        return None;
    }

    let trimmed = private_key.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(expand_tilde(trimmed))
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

fn shell_quote(arg: &str) -> String {
    if arg.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '=' | '@')
    }) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

fn session_id_for_host(host_id: &str) -> String {
    let mut normalized = String::with_capacity(host_id.len() + 5);
    normalized.push_str("host-");
    for ch in host_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            normalized.push(ch);
        } else {
            normalized.push('-');
        }
    }
    normalized
}

fn unix_ts_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

pub fn draft_host_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("native-draft-{}-{nanos}", std::process::id())
}

fn legacy_ssh_host(
    id: &str,
    name: &str,
    endpoint: &str,
    group_id: Option<&str>,
    tags: Vec<String>,
    system: HostSystemIcon,
) -> HostCardSnapshot {
    let connection = connection_from_ssh_endpoint(endpoint);
    HostCardSnapshot {
        id: id.to_string(),
        name: name.to_string(),
        protocol: "SSH".to_string(),
        endpoint: connection.endpoint("SSH"),
        description: String::new(),
        connection,
        group_id: group_id.map(str::to_string),
        tags,
        system,
        sort_order: 0,
    }
}

fn legacy_serial_host(id: &str, name: &str, port: &str, baud_rate: u32) -> HostCardSnapshot {
    let connection = HostConnectionConfig::serial(port, baud_rate);
    HostCardSnapshot {
        id: id.to_string(),
        name: name.to_string(),
        protocol: "Serial".to_string(),
        endpoint: connection.endpoint("Serial"),
        description: String::new(),
        connection,
        group_id: None,
        tags: Vec::new(),
        system: HostSystemIcon::Serial,
        sort_order: 0,
    }
}

fn connection_from_ssh_endpoint(endpoint: &str) -> HostConnectionConfig {
    let trimmed = endpoint.trim();
    let Some((target, port)) = trimmed.rsplit_once(':') else {
        return HostConnectionConfig::ssh(trimmed, 22, "root");
    };
    let Some((username, host)) = target.split_once('@') else {
        return HostConnectionConfig::ssh(target, port.parse().unwrap_or(22), "root");
    };
    HostConnectionConfig::ssh(host, port.parse().unwrap_or(22), username)
}

fn legacy_hosts() -> Vec<HostCardSnapshot> {
    vec![
        legacy_ssh_host(
            "seven-province-x86",
            "7省-X86",
            "root@192.168.248.120:22",
            Some("default"),
            Vec::new(),
            HostSystemIcon::Terminal,
        ),
        legacy_ssh_host(
            "syno",
            "Syno",
            "lion1991@192.168.252.1:22",
            None,
            Vec::new(),
            HostSystemIcon::Linux,
        ),
        legacy_serial_host(
            "serial",
            "串口连接",
            "/dev/cu.Bluetooth-Incoming-Port",
            115_200,
        ),
        legacy_ssh_host(
            "ggy-us",
            "ggy-美国",
            "root@ggy-us.121221.xyz:22",
            None,
            Vec::new(),
            HostSystemIcon::Terminal,
        ),
        legacy_ssh_host(
            "dc9",
            "搬瓦工-DC9",
            "root@dc9.121221.xyz:22",
            None,
            Vec::new(),
            HostSystemIcon::Terminal,
        ),
        legacy_ssh_host(
            "ca-centos7",
            "CA-Centos7",
            "root@silkus.121221.xyz:22",
            None,
            Vec::new(),
            HostSystemIcon::Terminal,
        ),
        legacy_ssh_host(
            "company-hp",
            "公司 HP 服务器",
            "root@192.168.24.95:22",
            None,
            Vec::new(),
            HostSystemIcon::Terminal,
        ),
        legacy_ssh_host(
            "company-dell",
            "公司 Dell 服务器",
            "root@192.168.24.205:22",
            None,
            Vec::new(),
            HostSystemIcon::Terminal,
        ),
        legacy_ssh_host(
            "tencent-beijing",
            "腾讯云-北京",
            "root@tx.121221.xyz:22",
            None,
            vec!["测试标签".to_string()],
            HostSystemIcon::Linux,
        ),
        legacy_ssh_host(
            "local-test",
            "测试主机",
            "root@test.121221.xyz:22",
            None,
            Vec::new(),
            HostSystemIcon::Terminal,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_db() -> (PathBuf, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.keep().join("test.db");
        initialize_host_database(&path).unwrap();
        let conn = Connection::open(&path).unwrap();
        (path, conn)
    }

    fn insert_host_with_tags(conn: &Connection, id: &str, tags: &[&str]) {
        let tags_json = serde_json::to_string(&tags).unwrap();
        let now = epoch_millis();
        conn.execute(
            "INSERT INTO hosts (id, name, host, port, username, tags, created_at, updated_at) \
             VALUES (?1, ?1, '127.0.0.1', 22, 'root', ?2, ?3, ?3)",
            params![id, tags_json, now],
        )
        .unwrap();
    }

    #[test]
    fn windows_database_path_uses_appdata_without_home() {
        let path = database_path_from_env(
            "windows",
            None,
            None,
            Some(PathBuf::from(r"C:\Users\matt\AppData\Roaming")),
            None,
        )
        .expect("windows appdata path");

        assert_eq!(
            path,
            PathBuf::from(r"C:\Users\matt\AppData\Roaming")
                .join("com.matt.nexshell")
                .join("nexshell.db")
        );
    }

    #[test]
    fn create_group_inserts_and_returns_id() {
        let (db_path, conn) = temp_db();
        let id = create_group_in_db(&db_path, "Production").unwrap();
        assert!(!id.is_empty());
        let name: String = conn
            .query_row("SELECT name FROM groups WHERE id = ?1", [&id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(name, "Production");
    }

    #[test]
    fn rename_group_updates_name() {
        let (db_path, conn) = temp_db();
        let id = create_group_in_db(&db_path, "Old").unwrap();
        rename_group_in_db(&db_path, &id, "New").unwrap();
        let name: String = conn
            .query_row("SELECT name FROM groups WHERE id = ?1", [&id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(name, "New");
    }

    #[test]
    fn delete_group_removes_row_and_nullifies_hosts() {
        let (db_path, conn) = temp_db();
        let gid = create_group_in_db(&db_path, "ToDelete").unwrap();
        insert_host_with_tags(&conn, "h1", &[]);
        conn.execute("UPDATE hosts SET group_id = ?1 WHERE id = 'h1'", [&gid])
            .unwrap();

        delete_group_from_db(&db_path, &gid).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM groups WHERE id = ?1", [&gid], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        let group_id: Option<String> = conn
            .query_row("SELECT group_id FROM hosts WHERE id = 'h1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(group_id, None);
    }

    #[test]
    fn delete_tag_removes_from_all_hosts() {
        let (db_path, conn) = temp_db();
        insert_host_with_tags(&conn, "h1", &["prod", "us-east"]);
        insert_host_with_tags(&conn, "h2", &["prod", "staging"]);
        insert_host_with_tags(&conn, "h3", &["staging"]);

        delete_tag_from_all_hosts_in_db(&db_path, "prod").unwrap();

        let tags1 = read_host_tags(&conn, "h1");
        let tags2 = read_host_tags(&conn, "h2");
        let tags3 = read_host_tags(&conn, "h3");
        assert_eq!(tags1, vec!["us-east"]);
        assert_eq!(tags2, vec!["staging"]);
        assert_eq!(tags3, vec!["staging"]);
    }

    #[test]
    fn rename_tag_updates_all_hosts() {
        let (db_path, conn) = temp_db();
        insert_host_with_tags(&conn, "h1", &["old-name", "other"]);
        insert_host_with_tags(&conn, "h2", &["old-name"]);

        rename_tag_in_all_hosts_in_db(&db_path, "old-name", "new-name").unwrap();

        let tags1 = read_host_tags(&conn, "h1");
        let tags2 = read_host_tags(&conn, "h2");
        assert_eq!(tags1, vec!["new-name", "other"]);
        assert_eq!(tags2, vec!["new-name"]);
    }

    #[test]
    fn create_tag_persists_in_db() {
        let (db_path, conn) = temp_db();
        create_tag_in_db(&db_path, "production").unwrap();
        create_tag_in_db(&db_path, "staging").unwrap();
        // duplicate is ignored
        create_tag_in_db(&db_path, "production").unwrap();

        let names: Vec<String> = conn
            .prepare("SELECT name FROM tags ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(names, vec!["production", "staging"]);
    }

    #[test]
    fn tag_available_after_create_without_host() {
        let (db_path, _conn) = temp_db();
        create_tag_in_db(&db_path, "standalone-tag").unwrap();

        let snapshot = load_host_management_snapshot_from_db_path(&db_path).unwrap();
        assert!(snapshot
            .available_tags
            .contains(&"standalone-tag".to_string()));
    }

    #[test]
    fn delete_tag_removes_from_tags_table() {
        let (db_path, conn) = temp_db();
        create_tag_in_db(&db_path, "to-delete").unwrap();
        insert_host_with_tags(&conn, "h1", &["to-delete", "keep"]);

        delete_tag_from_all_hosts_in_db(&db_path, "to-delete").unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tags WHERE name = 'to-delete'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(read_host_tags(&conn, "h1"), vec!["keep"]);
    }

    #[test]
    fn migrate_seeds_tags_from_existing_hosts() {
        let (db_path, conn) = temp_db();
        insert_host_with_tags(&conn, "h1", &["alpha", "beta"]);
        insert_host_with_tags(&conn, "h2", &["beta", "gamma"]);
        // re-initialize triggers migrate_seed_tags_table
        initialize_host_database(&db_path).unwrap();

        let names: Vec<String> = conn
            .prepare("SELECT name FROM tags ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
    }

    fn rdp_card(
        username: &str,
        password: Option<&str>,
        quality: RdpDisplayQuality,
    ) -> HostCardSnapshot {
        let mut conn = HostConnectionConfig::rdp("10.0.0.5", 3389, username);
        conn.password = password.map(str::to_string);
        conn.rdp_display_quality = quality;
        HostCardSnapshot {
            id: "rdp-1".to_string(),
            name: "win-box".to_string(),
            protocol: "RDP".to_string(),
            endpoint: conn.endpoint("RDP"),
            description: "desc".to_string(),
            connection: conn,
            group_id: None,
            tags: Vec::new(),
            system: HostSystemIcon::Terminal,
            sort_order: 0,
        }
    }

    fn state_with_host(card: HostCardSnapshot) -> HostManagementState {
        let snapshot = HostManagementSnapshot {
            title: "t",
            top_actions: ["a", "b", "c", "d"],
            groups: Vec::new(),
            search_placeholder: "s",
            protocol_filter_label: "p",
            available_tags: Vec::new(),
            hosts: vec![card],
        };
        HostManagementState::new(snapshot)
    }

    #[test]
    fn rdp_plan_ok_with_credentials_and_quality() {
        let state = state_with_host(rdp_card(
            "Administrator",
            Some("pw"),
            RdpDisplayQuality::Hidpi,
        ));
        match state.connection_plan_for("rdp-1").unwrap() {
            HostConnectionPlan::Rdp { config, title, .. } => {
                assert_eq!(title, "win-box");
                assert_eq!(config.port, 3389);
                assert_eq!(config.rdp_display_quality, RdpDisplayQuality::Hidpi);
            }
            other => panic!("expected Rdp, got {other:?}"),
        }
    }

    #[test]
    fn rdp_plan_missing_username_is_unsupported() {
        let state = state_with_host(rdp_card("", Some("pw"), RdpDisplayQuality::Standard));
        assert!(matches!(
            state.connection_plan_for("rdp-1"),
            Some(HostConnectionPlan::Unsupported { .. })
        ));
    }

    #[test]
    fn rdp_plan_missing_password_is_unsupported() {
        let state = state_with_host(rdp_card("Administrator", None, RdpDisplayQuality::Standard));
        assert!(matches!(
            state.connection_plan_for("rdp-1"),
            Some(HostConnectionPlan::Unsupported { .. })
        ));
    }

    #[test]
    fn rdp_default_display_quality_is_standard() {
        let config = HostConnectionConfig::rdp("h", 3389, "u");
        assert_eq!(config.rdp_display_quality, RdpDisplayQuality::Standard);
    }

    #[test]
    fn rdp_host_roundtrips_through_db() {
        let (db_path, _conn) = temp_db();
        let card = rdp_card("Administrator", Some("pw"), RdpDisplayQuality::Hidpi);
        upsert_host_card_in_db_path(&db_path, &card).unwrap();

        let snapshot = load_host_management_snapshot_from_db_path(&db_path).unwrap();
        let host = snapshot.hosts.iter().find(|h| h.id == "rdp-1").unwrap();
        assert_eq!(host.protocol, "RDP");
        assert_eq!(
            host.connection.rdp_display_quality,
            RdpDisplayQuality::Hidpi
        );
        assert_eq!(host.connection.password.as_deref(), Some("pw"));
    }

    #[test]
    fn inserted_host_without_quality_column_defaults_standard() {
        // create_draft_host_in_db_path 的 INSERT 不含 rdp_display_quality，靠列默认值回退。
        let (db_path, _conn) = temp_db();
        let id = create_draft_host_in_db_path(&db_path, None).unwrap();
        let snapshot = load_host_management_snapshot_from_db_path(&db_path).unwrap();
        let host = snapshot.hosts.iter().find(|h| h.id == id).unwrap();
        assert_eq!(
            host.connection.rdp_display_quality,
            RdpDisplayQuality::Standard
        );
    }

    fn read_host_tags(conn: &Connection, id: &str) -> Vec<String> {
        let json: String = conn
            .query_row("SELECT tags FROM hosts WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        parse_tags(&json)
    }
}
