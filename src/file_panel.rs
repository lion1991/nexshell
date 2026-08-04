//! 文件面板（右侧 SFTP 浏览器）状态 + 后台 worker。
//! 状态机参考 warp/app/src/terminal/view/ssh_file_upload.rs（FileUploadStatus）。
//! Worker 模式参考 host_overview::spawn_host_overview_monitor：单独 OS 线程跑
//! current-thread tokio runtime，请求走 mpsc，事件走 async_channel。

use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use notify_debouncer_full::{
    new_debouncer_opt,
    notify::{Config, EventKind, PollWatcher, RecursiveMode, Watcher},
    DebounceEventResult, Debouncer, NoCache,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::sftp_ops::{self, RemoteEntry};
use crate::ssh_session::SshHandle;

/// worker / UI 共享的"取消令牌表"。UI 拿 Arc 即可在 worker 跑传输时随时翻牌。
type CancelMap = Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>;

type LocalFileWatcher = Debouncer<PollWatcher, NoCache>;

const LOCAL_FILE_WATCHER_DEBOUNCE: Duration = Duration::from_millis(250);
const LOCAL_FILE_WATCHER_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// 单个 tab 的文件面板状态。
#[derive(Debug, Default)]
pub struct FilePanelState {
    pub cwd: String,
    pub entries: Vec<RemoteEntry>,
    pub loading: bool,
    pub error: Option<String>,
    /// 当前选中的文件 / 目录名集合。BTreeSet 保证迭代顺序稳定。
    pub selected_names: BTreeSet<String>,
    /// shift 范围选的锚点：最近一次"普通点击 / cmd 点击"落点。
    /// 切换目录或彻底清空时一并清空。
    pub selection_anchor: Option<String>,
    pub follow_cwd: bool,
    pub transfers: Vec<TransferRow>,
    /// 本地 Project explorer 用的树根。远程 SSH 面板继续只读 `entries`。
    pub tree_root: Option<String>,
    /// 本地树：目录 path -> 已加载的直接子项。
    pub tree_children: HashMap<String, Vec<RemoteEntry>>,
    /// 本地树：已展开的目录 path。
    pub tree_expanded_dirs: BTreeSet<String>,
    /// 本地树：正在懒加载的目录 path。
    pub tree_loading_dirs: BTreeSet<String>,
    /// 本地树：目录 path -> 读取子目录失败信息。
    pub tree_child_errors: HashMap<String, String>,
}

/// 点击 entry 时根据修饰键决定的选择模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilePanelSelectMode {
    /// 无修饰键：清空已有选择并只选中目标，更新 anchor。
    Replace,
    /// cmd / ctrl：在已有选择基础上 toggle 目标，更新 anchor 为目标。
    Toggle,
    /// shift：从 anchor 到目标之间的所有 entry 都选中；anchor 不变。
    Range,
}

#[derive(Clone, Debug)]
pub struct FilePanelTreeRow {
    pub path: String,
    pub name: String,
    pub kind: sftp_ops::EntryKind,
    pub size: u64,
    pub modified: Option<std::time::SystemTime>,
    pub permissions: Option<u32>,
    pub depth: usize,
    pub is_expanded: bool,
    pub is_loading: bool,
    pub error: Option<String>,
}

impl FilePanelTreeRow {
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, sftp_ops::EntryKind::Dir)
    }

    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilePanelTreeToggle {
    Collapsed,
    ExpandedCached,
    ExpandedNeedsLoad,
}

impl FilePanelState {
    pub fn new() -> Self {
        Self {
            cwd: String::from("."),
            follow_cwd: true,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub struct TransferRow {
    pub transfer_id: u64,
    pub file_name: String,
    pub total: Option<u64>,
    pub transferred: u64,
    pub status: TransferStatus,
    pub is_upload: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferStatus {
    Active,
    Done,
    Failed(String),
    Cancelled,
}

/// UI → worker 的请求。
#[derive(Debug)]
pub enum SftpRequest {
    /// 列出 path 目录；worker 会先 canonicalize 再 read_dir。
    List(String),
    /// 列出树状面板的子目录，不改变 worker 的刷新基准目录。
    ListTreeChild(String),
    /// 列出 cwd（worker 复用上次 path）。
    Refresh,
    /// 上传一组本地文件到远端目录。完成后会触发一次 Refresh。
    Upload {
        locals: Vec<PathBuf>,
        remote_dir: String,
    },
    /// 下载远端文件 / 目录到本地指定路径。
    /// 目录时 `local` 是要创建的本地目标目录（已含 file_name）。
    Download {
        remote: String,
        local: PathBuf,
        file_name: String,
        is_dir: bool,
    },
    /// 删除远端文件或目录（dir 用递归删）。
    Delete { path: String, is_dir: bool },
    /// 批量删除：worker 串行处理后再统一 refresh，避免 N 次刷新。
    DeleteMany { items: Vec<(String, bool)> },
    /// 远端 rename。
    Rename { from: String, to: String },
    /// 在 parent 下创建子目录。
    Mkdir { parent: String, name: String },
    /// 在 parent 下创建空文件。
    Touch { parent: String, name: String },
    /// 关闭 worker（drop handle 时也会触发）。
    Shutdown,
}

/// 单次传输任务进度，按 transfer_id 关联。
#[derive(Clone, Debug)]
pub struct TransferProgress {
    pub transfer_id: u64,
    pub file_name: String,
    pub total: Option<u64>,
    pub transferred: u64,
}

// 旧名兼容：上传场景沿用历史命名
pub type UploadProgress = TransferProgress;

/// Worker → UI 的事件。
#[derive(Clone, Debug)]
pub enum SftpEvent {
    Loading {
        path: String,
    },
    DirListed {
        path: String,
        entries: Vec<RemoteEntry>,
    },
    ListFailed {
        path: String,
        message: String,
    },
    Error {
        message: String,
    },
    UploadStarted {
        transfer_id: u64,
        file_name: String,
        total: Option<u64>,
    },
    UploadProgress(TransferProgress),
    UploadCompleted {
        transfer_id: u64,
        file_name: String,
    },
    UploadFailed {
        transfer_id: u64,
        file_name: String,
        message: String,
    },
    DownloadStarted {
        transfer_id: u64,
        file_name: String,
        total: Option<u64>,
    },
    DownloadProgress(TransferProgress),
    DownloadCompleted {
        transfer_id: u64,
        file_name: String,
        local: PathBuf,
    },
    DownloadFailed {
        transfer_id: u64,
        file_name: String,
        message: String,
    },
}

/// POSIX 风格路径父目录。根目录返回自身。
pub fn parent_path(path: &str) -> String {
    if path.is_empty() || path == "/" {
        return "/".to_string();
    }
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".to_string(),
        Some(idx) => trimmed[..idx].to_string(),
        None => ".".to_string(),
    }
}

/// 拼接 cwd/name；name 已经是绝对路径时直接用。
pub fn join_path(cwd: &str, name: &str) -> String {
    if name.starts_with('/') {
        return name.to_string();
    }
    if cwd.is_empty() {
        return name.to_string();
    }
    if cwd == "/" {
        return format!("/{name}");
    }
    format!("{}/{}", cwd.trim_end_matches('/'), name)
}

/// 根据修饰键模式更新选择集合。
/// - Replace：清空选择，只选 target，anchor = target。
/// - Toggle：toggle target 的选中状态，anchor = target。
/// - Range：anchor 缺省时退化到 Replace；否则用 entries 顺序选中 anchor..=target 区间。
pub fn apply_file_panel_selection(
    state: &mut FilePanelState,
    target: &str,
    mode: FilePanelSelectMode,
) {
    match mode {
        FilePanelSelectMode::Replace => {
            state.selected_names.clear();
            state.selected_names.insert(target.to_string());
            state.selection_anchor = Some(target.to_string());
        }
        FilePanelSelectMode::Toggle => {
            if !state.selected_names.remove(target) {
                state.selected_names.insert(target.to_string());
            }
            state.selection_anchor = Some(target.to_string());
        }
        FilePanelSelectMode::Range => {
            let Some(anchor) = state.selection_anchor.clone() else {
                state.selected_names.clear();
                state.selected_names.insert(target.to_string());
                state.selection_anchor = Some(target.to_string());
                return;
            };
            let anchor_idx = state.entries.iter().position(|e| e.name == anchor);
            let target_idx = state.entries.iter().position(|e| e.name == target);
            let (Some(ai), Some(ti)) = (anchor_idx, target_idx) else {
                state.selected_names.clear();
                state.selected_names.insert(target.to_string());
                state.selection_anchor = Some(target.to_string());
                return;
            };
            let (lo, hi) = if ai <= ti { (ai, ti) } else { (ti, ai) };
            state.selected_names.clear();
            for entry in &state.entries[lo..=hi] {
                state.selected_names.insert(entry.name.clone());
            }
            // Range 不更新 anchor，方便连续 shift 调整
        }
    }
}

pub fn apply_file_panel_tree_selection(
    state: &mut FilePanelState,
    target_path: &str,
    mode: FilePanelSelectMode,
) {
    match mode {
        FilePanelSelectMode::Replace => {
            state.selected_names.clear();
            state.selected_names.insert(target_path.to_string());
            state.selection_anchor = Some(target_path.to_string());
        }
        FilePanelSelectMode::Toggle => {
            if !state.selected_names.remove(target_path) {
                state.selected_names.insert(target_path.to_string());
            }
            state.selection_anchor = Some(target_path.to_string());
        }
        FilePanelSelectMode::Range => {
            let Some(anchor) = state.selection_anchor.clone() else {
                state.selected_names.clear();
                state.selected_names.insert(target_path.to_string());
                state.selection_anchor = Some(target_path.to_string());
                return;
            };
            let rows = flatten_file_panel_tree(state);
            let anchor_idx = rows.iter().position(|row| row.path == anchor);
            let target_idx = rows.iter().position(|row| row.path == target_path);
            let (Some(ai), Some(ti)) = (anchor_idx, target_idx) else {
                state.selected_names.clear();
                state.selected_names.insert(target_path.to_string());
                state.selection_anchor = Some(target_path.to_string());
                return;
            };
            let (lo, hi) = if ai <= ti { (ai, ti) } else { (ti, ai) };
            state.selected_names.clear();
            for row in &rows[lo..=hi] {
                state.selected_names.insert(row.path.clone());
            }
        }
    }
}

/// 本地 Project explorer 的目录展开/折叠。返回值告诉 UI 是否需要向 worker 发起 List。
pub fn toggle_file_panel_tree_dir(state: &mut FilePanelState, path: &str) -> FilePanelTreeToggle {
    if state.tree_expanded_dirs.remove(path) {
        state.tree_loading_dirs.remove(path);
        return FilePanelTreeToggle::Collapsed;
    }

    state.tree_expanded_dirs.insert(path.to_string());
    state.tree_child_errors.remove(path);
    if state.tree_children.contains_key(path) {
        FilePanelTreeToggle::ExpandedCached
    } else {
        state.tree_loading_dirs.insert(path.to_string());
        FilePanelTreeToggle::ExpandedNeedsLoad
    }
}

/// 本地 Project explorer 的事件应用路径。
///
/// 远程 SSH 面板继续使用 `apply_sftp_event`，所以它的 cwd/entries/选择行为不变。
pub fn apply_local_file_panel_event(state: &mut FilePanelState, event: SftpEvent) {
    match event {
        SftpEvent::Loading { path } => {
            state.error = None;
            // tree_loading_dirs 只由用户展开树节点(toggle_file_panel_tree_dir)写入；
            // cwd 跟随的 List 不在其中，一律当根目录切换——不再按路径前缀猜测。
            if state.tree_loading_dirs.contains(&path) {
                state.tree_child_errors.remove(&path);
            } else {
                reset_local_tree_root_if_changed(state, &path);
                state.loading = true;
                state.cwd = path;
            }
        }
        SftpEvent::DirListed { path, entries } => {
            let is_child = state.tree_root.as_deref().is_some_and(|root| root != path);
            let child_load = is_child
                && (state.tree_loading_dirs.remove(&path)
                    || state.tree_children.contains_key(&path));
            if child_load {
                state.tree_child_errors.remove(&path);
                state.tree_children.insert(path, entries);
                state.loading = false;
                state.error = None;
                return;
            }

            reset_local_tree_root_if_changed(state, &path);
            state.cwd = path.clone();
            state.entries = entries.clone();
            state.loading = false;
            state.error = None;
            state.selected_names.clear();
            state.selection_anchor = None;
            state.tree_root = Some(path.clone());
            state.tree_children.insert(path.clone(), entries);
            state.tree_expanded_dirs.insert(path);
        }
        SftpEvent::ListFailed { path, message } => {
            let is_child = state.tree_root.as_deref().is_some_and(|root| root != path);
            let child_load = is_child
                && (state.tree_loading_dirs.contains(&path)
                    || state.tree_children.contains_key(&path));
            state.tree_loading_dirs.remove(&path);
            if child_load {
                state.tree_children.remove(&path);
                state.tree_child_errors.insert(path, message);
                state.loading = false;
                state.error = None;
                return;
            }

            state.loading = false;
            state.error = Some(message);
        }
        SftpEvent::Error { message } => {
            state.loading = false;
            state.tree_loading_dirs.clear();
            state.error = Some(message);
        }
        other => apply_sftp_event(state, other),
    }
}

pub fn flatten_file_panel_tree(state: &FilePanelState) -> Vec<FilePanelTreeRow> {
    let Some(root) = state.tree_root.as_deref() else {
        return Vec::new();
    };
    let entries = state
        .tree_children
        .get(root)
        .map(Vec::as_slice)
        .unwrap_or(state.entries.as_slice());
    let mut rows = Vec::new();
    append_tree_rows(state, root, entries, 0, &mut rows);
    rows
}

fn append_tree_rows(
    state: &FilePanelState,
    parent: &str,
    entries: &[RemoteEntry],
    depth: usize,
    rows: &mut Vec<FilePanelTreeRow>,
) {
    for entry in entries {
        let path = join_path(parent, &entry.name);
        let is_dir = matches!(entry.kind, sftp_ops::EntryKind::Dir);
        let is_expanded = is_dir && state.tree_expanded_dirs.contains(&path);
        let is_loading = is_dir && state.tree_loading_dirs.contains(&path);
        rows.push(FilePanelTreeRow {
            path: path.clone(),
            name: entry.name.clone(),
            kind: entry.kind,
            size: entry.size,
            modified: entry.modified,
            permissions: entry.permissions,
            depth,
            is_expanded,
            is_loading,
            error: None,
        });
        if is_expanded {
            if let Some(children) = state.tree_children.get(&path) {
                append_tree_rows(state, &path, children, depth + 1, rows);
            } else if let Some(message) = state.tree_child_errors.get(&path) {
                rows.push(FilePanelTreeRow {
                    path: format!("{path}/.nexshell-tree-error"),
                    name: message.clone(),
                    kind: sftp_ops::EntryKind::Other,
                    size: 0,
                    modified: None,
                    permissions: None,
                    depth: depth + 1,
                    is_expanded: false,
                    is_loading: false,
                    error: Some(message.clone()),
                });
            }
        }
    }
}

fn reset_local_tree_root_if_changed(state: &mut FilePanelState, path: &str) {
    if state.tree_root.as_deref() == Some(path) {
        return;
    }
    state.tree_root = Some(path.to_string());
    state.tree_children.clear();
    state.tree_expanded_dirs.clear();
    state.tree_loading_dirs.clear();
    state.tree_child_errors.clear();
}

/// 把 worker 事件应用到 panel state；UI 层 callback 直接调用即可。
pub fn apply_sftp_event(state: &mut FilePanelState, event: SftpEvent) {
    match event {
        SftpEvent::Loading { path } => {
            state.loading = true;
            state.error = None;
            state.cwd = path;
        }
        SftpEvent::DirListed { path, entries } => {
            state.cwd = path;
            state.entries = entries;
            state.loading = false;
            state.error = None;
            state.selected_names.clear();
            state.selection_anchor = None;
        }
        SftpEvent::ListFailed { message, .. } => {
            state.loading = false;
            state.error = Some(message);
        }
        SftpEvent::Error { message } => {
            state.loading = false;
            state.error = Some(message);
        }
        SftpEvent::UploadStarted {
            transfer_id,
            file_name,
            total,
        } => {
            upsert_transfer_started(state, transfer_id, file_name, total, true);
        }
        SftpEvent::UploadProgress(p) => {
            apply_transfer_progress(state, &p);
        }
        SftpEvent::UploadCompleted { transfer_id, .. } => {
            mark_transfer_done(state, transfer_id);
        }
        SftpEvent::UploadFailed {
            transfer_id,
            message,
            ..
        } => {
            if message == "cancelled" {
                mark_transfer_cancelled(state, transfer_id);
            } else {
                mark_transfer_failed(state, transfer_id, message);
            }
        }
        SftpEvent::DownloadStarted {
            transfer_id,
            file_name,
            total,
        } => {
            upsert_transfer_started(state, transfer_id, file_name, total, false);
        }
        SftpEvent::DownloadProgress(p) => {
            apply_transfer_progress(state, &p);
        }
        SftpEvent::DownloadCompleted { transfer_id, .. } => {
            mark_transfer_done(state, transfer_id);
        }
        SftpEvent::DownloadFailed {
            transfer_id,
            message,
            ..
        } => {
            if message == "cancelled" {
                mark_transfer_cancelled(state, transfer_id);
            } else {
                mark_transfer_failed(state, transfer_id, message);
            }
        }
    }
}

fn upsert_transfer_started(
    state: &mut FilePanelState,
    transfer_id: u64,
    file_name: String,
    total: Option<u64>,
    is_upload: bool,
) {
    state.transfers.retain(|t| t.transfer_id != transfer_id);
    state.transfers.push(TransferRow {
        transfer_id,
        file_name,
        total,
        transferred: 0,
        status: TransferStatus::Active,
        is_upload,
    });
    // 历史记录只增不减会随传输次数无限累积；UI 只显示最近 5 条，
    // 超上限时从头淘汰最老的已结束记录（Active 保留，取消按钮仍要用）。
    const MAX_TRANSFER_ROWS: usize = 64;
    while state.transfers.len() > MAX_TRANSFER_ROWS {
        let Some(pos) = state
            .transfers
            .iter()
            .position(|t| !matches!(t.status, TransferStatus::Active))
        else {
            break;
        };
        state.transfers.remove(pos);
    }
}

fn apply_transfer_progress(state: &mut FilePanelState, p: &TransferProgress) {
    if let Some(row) = state
        .transfers
        .iter_mut()
        .find(|t| t.transfer_id == p.transfer_id)
    {
        row.transferred = p.transferred;
        if row.total.is_none() {
            row.total = p.total;
        }
    }
}

fn mark_transfer_done(state: &mut FilePanelState, transfer_id: u64) {
    if let Some(row) = state
        .transfers
        .iter_mut()
        .find(|t| t.transfer_id == transfer_id)
    {
        row.status = TransferStatus::Done;
        if let Some(total) = row.total {
            row.transferred = total;
        }
    }
}

fn mark_transfer_failed(state: &mut FilePanelState, transfer_id: u64, message: String) {
    if let Some(row) = state
        .transfers
        .iter_mut()
        .find(|t| t.transfer_id == transfer_id)
    {
        row.status = TransferStatus::Failed(message);
    }
}

fn mark_transfer_cancelled(state: &mut FilePanelState, transfer_id: u64) {
    if let Some(row) = state
        .transfers
        .iter_mut()
        .find(|t| t.transfer_id == transfer_id)
    {
        row.status = TransferStatus::Cancelled;
    }
}

#[derive(Debug)]
pub struct SftpWorkerHandle {
    tx: mpsc::UnboundedSender<SftpRequest>,
    shutdown: Arc<AtomicBool>,
    cancels: CancelMap,
    _thread: Option<JoinHandle<()>>,
}

impl SftpWorkerHandle {
    pub fn send(&self, req: SftpRequest) -> bool {
        self.tx.send(req).is_ok()
    }

    /// UI 触发：把对应 transfer 的取消令牌置 true。worker 正在跑的 chunk
    /// 循环会在下一次迭代退出，发出 Failed("cancelled") → UI 渲染为 Cancelled。
    pub fn cancel(&self, transfer_id: u64) {
        if let Ok(map) = self.cancels.lock() {
            if let Some(flag) = map.get(&transfer_id) {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }
}

impl Drop for SftpWorkerHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // drop 时让所有未完成传输也尽快退出
        if let Ok(map) = self.cancels.lock() {
            for flag in map.values() {
                flag.store(true, Ordering::Relaxed);
            }
        }
        let _ = self.tx.send(SftpRequest::Shutdown);
    }
}

#[derive(Debug)]
pub enum FilePanelWorkerHandle {
    Sftp(SftpWorkerHandle),
    Local(LocalFileWorkerHandle),
}

impl FilePanelWorkerHandle {
    pub fn send(&self, req: SftpRequest) -> bool {
        match self {
            Self::Sftp(worker) => worker.send(req),
            Self::Local(worker) => worker.send(req),
        }
    }

    pub fn cancel(&self, transfer_id: u64) {
        match self {
            Self::Sftp(worker) => worker.cancel(transfer_id),
            Self::Local(worker) => worker.cancel(transfer_id),
        }
    }
}

#[derive(Debug)]
pub struct LocalFileWorkerHandle {
    tx: mpsc::UnboundedSender<SftpRequest>,
    shutdown: Arc<AtomicBool>,
    cancels: CancelMap,
    _thread: Option<JoinHandle<()>>,
}

impl LocalFileWorkerHandle {
    pub fn send(&self, req: SftpRequest) -> bool {
        self.tx.send(req).is_ok()
    }

    pub fn cancel(&self, transfer_id: u64) {
        if let Ok(map) = self.cancels.lock() {
            if let Some(flag) = map.get(&transfer_id) {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }
}

impl Drop for LocalFileWorkerHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Ok(map) = self.cancels.lock() {
            for flag in map.values() {
                flag.store(true, Ordering::Relaxed);
            }
        }
        let _ = self.tx.send(SftpRequest::Shutdown);
    }
}

/// 在独立 OS 线程上起一个 SFTP worker。
/// 返回 handle + 事件 receiver。worker 内部会先 open_sftp_on_handle，然后
/// 循环消费请求。任何 send 失败、shutdown、SFTP 错误都会让 worker 退出。
pub fn spawn_sftp_worker(
    handle: SshHandle,
    label: &str,
) -> Result<(SftpWorkerHandle, async_channel::Receiver<SftpEvent>), String> {
    let (req_tx, req_rx) = mpsc::unbounded_channel::<SftpRequest>();
    let (evt_tx, evt_rx) = async_channel::bounded::<SftpEvent>(32);
    let shutdown = Arc::new(AtomicBool::new(false));
    let thread_shutdown = Arc::clone(&shutdown);
    let cancels: CancelMap = Arc::new(Mutex::new(HashMap::new()));
    let thread_cancels = Arc::clone(&cancels);

    let thread = thread::Builder::new()
        .name(format!("nexshell-sftp-{}", label.replace(['/', ':'], "_")))
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    let _ = evt_tx.try_send(SftpEvent::Error {
                        message: format!("failed to start sftp runtime: {error}"),
                    });
                    return;
                }
            };
            runtime.block_on(run_sftp_worker(
                handle,
                req_rx,
                evt_tx,
                thread_shutdown,
                thread_cancels,
            ));
        })
        .map_err(|error| format!("spawn sftp thread: {error}"))?;

    Ok((
        SftpWorkerHandle {
            tx: req_tx,
            shutdown,
            cancels,
            _thread: Some(thread),
        },
        evt_rx,
    ))
}

/// 在独立 OS 线程上起一个本地文件 worker。
/// 复用 SftpRequest/SftpEvent，UI 层可以继续使用同一套文件面板状态机。
pub fn spawn_local_file_worker(
    label: &str,
    initial_cwd: PathBuf,
) -> Result<(LocalFileWorkerHandle, async_channel::Receiver<SftpEvent>), String> {
    let (req_tx, req_rx) = mpsc::unbounded_channel::<SftpRequest>();
    let req_tx_for_watcher = req_tx.clone();
    let (evt_tx, evt_rx) = async_channel::bounded::<SftpEvent>(32);
    let shutdown = Arc::new(AtomicBool::new(false));
    let thread_shutdown = Arc::clone(&shutdown);
    let cancels: CancelMap = Arc::new(Mutex::new(HashMap::new()));
    let thread_cancels = Arc::clone(&cancels);

    let thread = thread::Builder::new()
        .name(format!(
            "nexshell-local-files-{}",
            label.replace(['/', ':'], "_")
        ))
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    let _ = evt_tx.try_send(SftpEvent::Error {
                        message: format!("failed to start local file runtime: {error}"),
                    });
                    return;
                }
            };
            runtime.block_on(run_local_file_worker(
                initial_cwd,
                req_rx,
                req_tx_for_watcher,
                evt_tx,
                thread_shutdown,
                thread_cancels,
            ));
        })
        .map_err(|error| format!("spawn local file thread: {error}"))?;

    Ok((
        LocalFileWorkerHandle {
            tx: req_tx,
            shutdown,
            cancels,
            _thread: Some(thread),
        },
        evt_rx,
    ))
}

async fn run_sftp_worker(
    handle: SshHandle,
    mut req_rx: mpsc::UnboundedReceiver<SftpRequest>,
    evt_tx: async_channel::Sender<SftpEvent>,
    shutdown: Arc<AtomicBool>,
    cancels: CancelMap,
) {
    let sftp = match sftp_ops::open_sftp_on_handle(&handle).await {
        Ok(s) => s,
        Err(error) => {
            let _ = evt_tx.try_send(SftpEvent::Error { message: error });
            return;
        }
    };

    let mut last_path = String::from(".");
    let mut next_transfer_id: u64 = 1;
    while let Some(req) = req_rx.recv().await {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match req {
            SftpRequest::Shutdown => break,
            SftpRequest::List(path) => {
                last_path = path.clone();
                run_list(&sftp, &path, &evt_tx).await;
            }
            SftpRequest::ListTreeChild(path) => {
                run_list(&sftp, &path, &evt_tx).await;
            }
            SftpRequest::Refresh => {
                let path = last_path.clone();
                run_list(&sftp, &path, &evt_tx).await;
            }
            SftpRequest::Upload { locals, remote_dir } => {
                for local in locals {
                    let id = next_transfer_id;
                    next_transfer_id += 1;
                    let token = register_cancel(&cancels, id);
                    run_upload(&sftp, &local, &remote_dir, id, &evt_tx, &token).await;
                    unregister_cancel(&cancels, id);
                }
                run_list(&sftp, &last_path, &evt_tx).await;
            }
            SftpRequest::Delete { path, is_dir } => {
                let result = if is_dir {
                    sftp_ops::remove_dir_recursive(&sftp, &path).await
                } else {
                    sftp_ops::remove_file(&sftp, &path).await
                };
                if let Err(error) = result {
                    let _ = evt_tx.send(SftpEvent::Error { message: error }).await;
                }
                run_list(&sftp, &last_path, &evt_tx).await;
            }
            SftpRequest::DeleteMany { items } => {
                for (path, is_dir) in items {
                    let result = if is_dir {
                        sftp_ops::remove_dir_recursive(&sftp, &path).await
                    } else {
                        sftp_ops::remove_file(&sftp, &path).await
                    };
                    if let Err(error) = result {
                        let _ = evt_tx
                            .send(SftpEvent::Error {
                                message: format!("{path}: {error}"),
                            })
                            .await;
                    }
                }
                run_list(&sftp, &last_path, &evt_tx).await;
            }
            SftpRequest::Rename { from, to } => {
                if sftp_ops::path_has_dotdot(&from) || sftp_ops::path_has_dotdot(&to) {
                    let _ = evt_tx
                        .send(SftpEvent::Error {
                            message: "重命名路径包含非法的 .. 段".to_string(),
                        })
                        .await;
                } else if let Err(error) = sftp_ops::rename(&sftp, &from, &to).await {
                    let _ = evt_tx.send(SftpEvent::Error { message: error }).await;
                }
                run_list(&sftp, &last_path, &evt_tx).await;
            }
            SftpRequest::Mkdir { parent, name } => {
                if !sftp_ops::is_safe_path_segment(&name) {
                    let _ = evt_tx
                        .send(SftpEvent::Error {
                            message: format!("非法目录名: {name:?}"),
                        })
                        .await;
                } else {
                    let abs = join_path(&parent, &name);
                    if let Err(error) = sftp_ops::create_dir(&sftp, &abs).await {
                        let _ = evt_tx.send(SftpEvent::Error { message: error }).await;
                    }
                }
                run_list(&sftp, &last_path, &evt_tx).await;
            }
            SftpRequest::Touch { parent, name } => {
                if !sftp_ops::is_safe_path_segment(&name) {
                    let _ = evt_tx
                        .send(SftpEvent::Error {
                            message: format!("非法文件名: {name:?}"),
                        })
                        .await;
                } else {
                    let abs = join_path(&parent, &name);
                    if let Err(error) = sftp_ops::create_empty_file(&sftp, &abs).await {
                        let _ = evt_tx.send(SftpEvent::Error { message: error }).await;
                    }
                }
                run_list(&sftp, &last_path, &evt_tx).await;
            }
            SftpRequest::Download {
                remote,
                local,
                file_name,
                is_dir,
            } => {
                let id = next_transfer_id;
                next_transfer_id += 1;
                let token = register_cancel(&cancels, id);
                if is_dir {
                    run_download_dir(&sftp, &remote, &local, &file_name, id, &evt_tx, &token).await;
                } else {
                    run_download(&sftp, &remote, &local, &file_name, id, &evt_tx, &token).await;
                }
                unregister_cancel(&cancels, id);
            }
        }
    }
}

async fn run_local_file_worker(
    initial_cwd: PathBuf,
    mut req_rx: mpsc::UnboundedReceiver<SftpRequest>,
    req_tx_self: mpsc::UnboundedSender<SftpRequest>,
    evt_tx: async_channel::Sender<SftpEvent>,
    shutdown: Arc<AtomicBool>,
    cancels: CancelMap,
) {
    let mut last_path = normalize_local_initial_path(initial_cwd);
    let mut listed_paths = BTreeSet::new();
    let mut watcher: Option<LocalFileWatcher> = None;
    let mut next_transfer_id: u64 = 1;
    while let Some(req) = req_rx.recv().await {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match req {
            SftpRequest::Shutdown => break,
            SftpRequest::List(path) => {
                last_path = local_path_from_panel_string(&path);
                listed_paths.clear();
                listed_paths.insert(last_path.clone());
                watcher = build_local_file_watcher(&last_path, req_tx_self.clone());
                run_local_list(&last_path, &evt_tx).await;
            }
            SftpRequest::ListTreeChild(path) => {
                let path = local_path_from_panel_string(&path);
                if listed_paths.insert(path.clone()) {
                    watch_local_file_directory(watcher.as_mut(), &path);
                }
                run_local_list(&path, &evt_tx).await;
            }
            SftpRequest::Refresh => {
                run_local_list(&last_path, &evt_tx).await;
                let child_paths: Vec<PathBuf> = listed_paths
                    .iter()
                    .filter(|path| *path != &last_path)
                    .cloned()
                    .collect();
                for path in child_paths {
                    run_local_list_without_loading(&path, &evt_tx).await;
                }
            }
            SftpRequest::Upload { locals, remote_dir } => {
                let target_dir = local_path_from_panel_string(&remote_dir);
                for local in locals {
                    let id = next_transfer_id;
                    next_transfer_id += 1;
                    let token = register_cancel(&cancels, id);
                    run_local_copy_into_dir(&local, &target_dir, id, true, &evt_tx, &token).await;
                    unregister_cancel(&cancels, id);
                }
                run_local_list(&last_path, &evt_tx).await;
            }
            SftpRequest::Download {
                remote,
                local,
                file_name,
                is_dir: _,
            } => {
                let id = next_transfer_id;
                next_transfer_id += 1;
                let token = register_cancel(&cancels, id);
                let source = local_path_from_panel_string(&remote);
                run_local_copy(&source, &local, &file_name, id, false, &evt_tx, &token).await;
                unregister_cancel(&cancels, id);
                run_local_list(&last_path, &evt_tx).await;
            }
            SftpRequest::Delete { path, is_dir } => {
                if let Err(message) =
                    remove_local_path(&local_path_from_panel_string(&path), is_dir).await
                {
                    let _ = evt_tx.send(SftpEvent::Error { message }).await;
                }
                run_local_list(&last_path, &evt_tx).await;
            }
            SftpRequest::DeleteMany { items } => {
                for (path, is_dir) in items {
                    if let Err(error) =
                        remove_local_path(&local_path_from_panel_string(&path), is_dir).await
                    {
                        let _ = evt_tx
                            .send(SftpEvent::Error {
                                message: format!("{path}: {error}"),
                            })
                            .await;
                    }
                }
                run_local_list(&last_path, &evt_tx).await;
            }
            SftpRequest::Rename { from, to } => {
                let from = local_path_from_panel_string(&from);
                let to = local_path_from_panel_string(&to);
                if local_path_has_parent_dir(&from) || local_path_has_parent_dir(&to) {
                    let _ = evt_tx
                        .send(SftpEvent::Error {
                            message: "重命名路径包含非法的 .. 段".to_string(),
                        })
                        .await;
                } else if let Err(error) = tokio::fs::rename(&from, &to).await {
                    let _ = evt_tx
                        .send(SftpEvent::Error {
                            message: format!(
                                "rename({} -> {}) failed: {error}",
                                from.display(),
                                to.display()
                            ),
                        })
                        .await;
                }
                run_local_list(&last_path, &evt_tx).await;
            }
            SftpRequest::Mkdir { parent, name } => {
                if !sftp_ops::is_safe_path_segment(&name) {
                    let _ = evt_tx
                        .send(SftpEvent::Error {
                            message: format!("非法目录名: {name:?}"),
                        })
                        .await;
                } else {
                    let path = local_path_from_panel_string(&parent).join(&name);
                    if let Err(error) = tokio::fs::create_dir(&path).await {
                        let _ = evt_tx
                            .send(SftpEvent::Error {
                                message: format!("create_dir({}) failed: {error}", path.display()),
                            })
                            .await;
                    }
                }
                run_local_list(&last_path, &evt_tx).await;
            }
            SftpRequest::Touch { parent, name } => {
                if !sftp_ops::is_safe_path_segment(&name) {
                    let _ = evt_tx
                        .send(SftpEvent::Error {
                            message: format!("非法文件名: {name:?}"),
                        })
                        .await;
                } else {
                    let path = local_path_from_panel_string(&parent).join(&name);
                    if let Err(error) = tokio::fs::File::create(&path).await {
                        let _ = evt_tx
                            .send(SftpEvent::Error {
                                message: format!("create file {} failed: {error}", path.display()),
                            })
                            .await;
                    }
                }
                run_local_list(&last_path, &evt_tx).await;
            }
        }
    }
}

fn build_local_file_watcher(
    path: &Path,
    tx: mpsc::UnboundedSender<SftpRequest>,
) -> Option<LocalFileWatcher> {
    let mut debouncer = match new_debouncer_opt::<_, PollWatcher, NoCache>(
        LOCAL_FILE_WATCHER_DEBOUNCE,
        None,
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                if events
                    .iter()
                    .any(|event| !matches!(event.kind, EventKind::Access(_)))
                {
                    let _ = tx.send(SftpRequest::Refresh);
                }
            }
            Err(errors) => log::warn!("local file watcher error: {errors:?}"),
        },
        NoCache,
        Config::default().with_poll_interval(LOCAL_FILE_WATCHER_POLL_INTERVAL),
    ) {
        Ok(debouncer) => debouncer,
        Err(error) => {
            log::warn!("failed to create local file watcher: {error}");
            return None;
        }
    };
    if let Err(error) = debouncer.watcher().watch(path, RecursiveMode::NonRecursive) {
        log::warn!(
            "failed to watch local directory {}: {error}",
            path.display()
        );
        return None;
    }
    Some(debouncer)
}

fn watch_local_file_directory(watcher: Option<&mut LocalFileWatcher>, path: &Path) {
    let Some(watcher) = watcher else {
        return;
    };
    if let Err(error) = watcher.watcher().watch(path, RecursiveMode::NonRecursive) {
        log::warn!(
            "failed to watch local directory {}: {error}",
            path.display()
        );
    }
}

fn register_cancel(cancels: &CancelMap, id: u64) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut map) = cancels.lock() {
        map.insert(id, Arc::clone(&flag));
    }
    flag
}

fn unregister_cancel(cancels: &CancelMap, id: u64) {
    if let Ok(mut map) = cancels.lock() {
        map.remove(&id);
    }
}

fn normalize_local_initial_path(path: PathBuf) -> PathBuf {
    if path.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        path
    }
}

fn local_path_from_panel_string(path: &str) -> PathBuf {
    if path.trim().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(path)
    }
}

fn local_panel_path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn local_path_has_parent_dir(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

async fn run_local_list(path: &Path, evt_tx: &async_channel::Sender<SftpEvent>) {
    let _ = evt_tx
        .send(SftpEvent::Loading {
            path: local_panel_path_string(path),
        })
        .await;
    run_local_list_without_loading(path, evt_tx).await;
}

async fn run_local_list_without_loading(path: &Path, evt_tx: &async_channel::Sender<SftpEvent>) {
    // 不 canonicalize：直接列请求的逻辑路径，保持与 OSC 7 上报的 local_cwd 一致。
    // canonicalize 会把符号链接 / firmlink / File Provider（如 Synology Drive）目录解析成
    // 物理路径，使 file_panel cwd ≠ local_cwd（逻辑），导致 follow_cwd 反复同步、甚至物理
    // 路径 read_dir 失败而切换不过去。read_dir 本身已跟随符号链接，无需 canonicalize。
    match list_local_dir_entries(path).await {
        Ok(entries) => {
            let _ = evt_tx
                .send(SftpEvent::DirListed {
                    path: local_panel_path_string(path),
                    entries,
                })
                .await;
        }
        Err(message) => {
            let _ = evt_tx
                .send(SftpEvent::ListFailed {
                    path: local_panel_path_string(path),
                    message,
                })
                .await;
        }
    }
}

async fn list_local_dir_entries(path: &Path) -> Result<Vec<RemoteEntry>, String> {
    let mut rd = tokio::fs::read_dir(path)
        .await
        .map_err(|error| format!("read_dir({}) failed: {error}", path.display()))?;
    let mut entries = Vec::new();
    while let Some(entry) = rd
        .next_entry()
        .await
        .map_err(|error| format!("read_dir next: {error}"))?
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "." || name == ".." {
            continue;
        }
        let fallback_file_type = entry.file_type().await.ok();
        let meta = tokio::fs::symlink_metadata(entry.path()).await.ok();
        entries.push(local_entry_from_metadata(
            name,
            meta.as_ref(),
            fallback_file_type.as_ref(),
        ));
    }
    sort_file_panel_entries(&mut entries);
    Ok(entries)
}

fn local_entry_from_metadata(
    name: String,
    meta: Option<&std::fs::Metadata>,
    fallback_file_type: Option<&std::fs::FileType>,
) -> RemoteEntry {
    let Some(meta) = meta else {
        let kind = fallback_file_type
            .map(local_entry_kind_from_file_type)
            .unwrap_or(sftp_ops::EntryKind::Other);
        return RemoteEntry {
            name,
            kind,
            size: 0,
            modified: None,
            permissions: None,
        };
    };
    let kind = local_entry_kind_from_file_type(&meta.file_type());
    RemoteEntry {
        name,
        kind,
        size: meta.len(),
        modified: meta.modified().ok(),
        permissions: local_permissions(meta),
    }
}

fn local_entry_kind_from_file_type(file_type: &std::fs::FileType) -> sftp_ops::EntryKind {
    if file_type.is_dir() {
        sftp_ops::EntryKind::Dir
    } else if file_type.is_file() {
        sftp_ops::EntryKind::File
    } else if file_type.is_symlink() {
        sftp_ops::EntryKind::Symlink
    } else {
        sftp_ops::EntryKind::Other
    }
}

fn sort_file_panel_entries(entries: &mut [RemoteEntry]) {
    entries.sort_by(|a, b| {
        let ak = matches!(a.kind, sftp_ops::EntryKind::Dir);
        let bk = matches!(b.kind, sftp_ops::EntryKind::Dir);
        bk.cmp(&ak).then_with(|| a.name.cmp(&b.name))
    });
}

fn local_permissions(meta: &std::fs::Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

async fn remove_local_path(path: &Path, is_dir: bool) -> Result<(), String> {
    let meta = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| format!("metadata({}) failed: {error}", path.display()))?;
    if is_dir && meta.file_type().is_dir() {
        tokio::fs::remove_dir_all(path)
            .await
            .map_err(|error| format!("remove_dir_all({}) failed: {error}", path.display()))
    } else {
        tokio::fs::remove_file(path)
            .await
            .map_err(|error| format!("remove_file({}) failed: {error}", path.display()))
    }
}

async fn run_local_copy_into_dir(
    source: &Path,
    target_dir: &Path,
    transfer_id: u64,
    is_upload: bool,
    evt_tx: &async_channel::Sender<SftpEvent>,
    cancel: &AtomicBool,
) {
    let file_name = source
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.display().to_string());
    let destination = target_dir.join(&file_name);
    run_local_copy(
        source,
        &destination,
        &file_name,
        transfer_id,
        is_upload,
        evt_tx,
        cancel,
    )
    .await;
}

async fn run_local_copy(
    source: &Path,
    destination: &Path,
    file_name: &str,
    transfer_id: u64,
    is_upload: bool,
    evt_tx: &async_channel::Sender<SftpEvent>,
    cancel: &AtomicBool,
) {
    let result = async {
        let meta = tokio::fs::metadata(source)
            .await
            .map_err(|error| format!("metadata({}) failed: {error}", source.display()))?;
        if meta.is_dir() {
            copy_local_dir(
                source,
                destination,
                file_name,
                transfer_id,
                is_upload,
                evt_tx,
                cancel,
            )
            .await
        } else {
            send_transfer_started(evt_tx, transfer_id, file_name, Some(meta.len()), is_upload)
                .await;
            copy_local_file_stream(
                source,
                destination,
                transfer_id,
                file_name,
                Some(meta.len()),
                0,
                is_upload,
                evt_tx,
                cancel,
            )
            .await
            .map(|_| ())
        }
    }
    .await;

    match result {
        Ok(()) => send_transfer_completed(evt_tx, transfer_id, file_name, is_upload).await,
        Err(message) => {
            send_transfer_failed(evt_tx, transfer_id, file_name, message, is_upload).await
        }
    }
}

async fn copy_local_dir(
    source: &Path,
    destination: &Path,
    file_name: &str,
    transfer_id: u64,
    is_upload: bool,
    evt_tx: &async_channel::Sender<SftpEvent>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let display_name = format!("{file_name}/");
    let source_canon = tokio::fs::canonicalize(source)
        .await
        .unwrap_or_else(|_| source.to_path_buf());
    if destination.starts_with(&source_canon) {
        return Err("cannot copy a directory into itself".to_string());
    }
    let tree = sftp_ops::walk_local_dir(source).await?;
    send_transfer_started(
        evt_tx,
        transfer_id,
        &display_name,
        Some(tree.total_bytes),
        is_upload,
    )
    .await;

    tokio::fs::create_dir_all(destination)
        .await
        .map_err(|error| format!("create local {} failed: {error}", destination.display()))?;
    let mut dirs = tree.dirs.clone();
    dirs.sort_by_key(|d| d.matches('/').count());
    for dir in &dirs {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let abs = destination.join(dir.replace('/', std::path::MAIN_SEPARATOR_STR));
        tokio::fs::create_dir_all(&abs)
            .await
            .map_err(|error| format!("create local {} failed: {error}", abs.display()))?;
    }

    let mut base: u64 = 0;
    for file in &tree.files {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let target = destination.join(file.rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let copied = copy_local_file_stream(
            &file.abs,
            &target,
            transfer_id,
            &display_name,
            Some(tree.total_bytes),
            base,
            is_upload,
            evt_tx,
            cancel,
        )
        .await
        .map_err(|message| format!("{}: {message}", file.rel))?;
        base = base.saturating_add(copied);
    }
    Ok(())
}

const LOCAL_COPY_CHUNK_SIZE: usize = 64 * 1024;

async fn copy_local_file_stream(
    source: &Path,
    destination: &Path,
    transfer_id: u64,
    file_name: &str,
    total: Option<u64>,
    progress_base: u64,
    is_upload: bool,
    evt_tx: &async_channel::Sender<SftpEvent>,
    cancel: &AtomicBool,
) -> Result<u64, String> {
    if source == destination {
        return Err("source and destination are the same file".to_string());
    }
    if let Some(parent) = destination.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| format!("create local {} failed: {error}", parent.display()))?;
        }
    }

    let mut input = tokio::fs::File::open(source)
        .await
        .map_err(|error| format!("open local {} failed: {error}", source.display()))?;
    let mut output = tokio::fs::File::create(destination)
        .await
        .map_err(|error| format!("create local {} failed: {error}", destination.display()))?;

    let result = async {
        let mut buf = vec![0u8; LOCAL_COPY_CHUNK_SIZE];
        let mut copied: u64 = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".to_string());
            }
            let n = input
                .read(&mut buf)
                .await
                .map_err(|error| format!("local read failed: {error}"))?;
            if n == 0 {
                break;
            }
            output
                .write_all(&buf[..n])
                .await
                .map_err(|error| format!("local write failed: {error}"))?;
            copied = copied.saturating_add(n as u64);
            send_transfer_progress(
                evt_tx,
                transfer_id,
                file_name,
                total,
                progress_base.saturating_add(copied),
                is_upload,
            )
            .await;
        }
        output
            .flush()
            .await
            .map_err(|error| format!("local flush failed: {error}"))?;
        Ok(copied)
    }
    .await;

    if result.is_err() {
        drop(output);
        let _ = tokio::fs::remove_file(destination).await;
    }
    result
}

async fn send_transfer_started(
    evt_tx: &async_channel::Sender<SftpEvent>,
    transfer_id: u64,
    file_name: &str,
    total: Option<u64>,
    is_upload: bool,
) {
    let event = if is_upload {
        SftpEvent::UploadStarted {
            transfer_id,
            file_name: file_name.to_string(),
            total,
        }
    } else {
        SftpEvent::DownloadStarted {
            transfer_id,
            file_name: file_name.to_string(),
            total,
        }
    };
    let _ = evt_tx.send(event).await;
}

async fn send_transfer_progress(
    evt_tx: &async_channel::Sender<SftpEvent>,
    transfer_id: u64,
    file_name: &str,
    total: Option<u64>,
    transferred: u64,
    is_upload: bool,
) {
    let progress = TransferProgress {
        transfer_id,
        file_name: file_name.to_string(),
        total,
        transferred,
    };
    let event = if is_upload {
        SftpEvent::UploadProgress(progress)
    } else {
        SftpEvent::DownloadProgress(progress)
    };
    let _ = evt_tx.send(event).await;
}

async fn send_transfer_completed(
    evt_tx: &async_channel::Sender<SftpEvent>,
    transfer_id: u64,
    file_name: &str,
    is_upload: bool,
) {
    let event = if is_upload {
        SftpEvent::UploadCompleted {
            transfer_id,
            file_name: file_name.to_string(),
        }
    } else {
        SftpEvent::DownloadCompleted {
            transfer_id,
            file_name: file_name.to_string(),
            local: PathBuf::new(),
        }
    };
    let _ = evt_tx.send(event).await;
}

async fn send_transfer_failed(
    evt_tx: &async_channel::Sender<SftpEvent>,
    transfer_id: u64,
    file_name: &str,
    message: String,
    is_upload: bool,
) {
    let event = if is_upload {
        SftpEvent::UploadFailed {
            transfer_id,
            file_name: file_name.to_string(),
            message,
        }
    } else {
        SftpEvent::DownloadFailed {
            transfer_id,
            file_name: file_name.to_string(),
            message,
        }
    };
    let _ = evt_tx.send(event).await;
}

async fn run_upload(
    sftp: &russh_sftp::client::SftpSession,
    local: &std::path::Path,
    remote_dir: &str,
    transfer_id: u64,
    evt_tx: &async_channel::Sender<SftpEvent>,
    cancel: &AtomicBool,
) {
    let file_name = local
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| local.display().to_string());

    if local.is_dir() {
        run_upload_dir(
            sftp,
            local,
            remote_dir,
            &file_name,
            transfer_id,
            evt_tx,
            cancel,
        )
        .await;
        return;
    }

    let total = tokio::fs::metadata(local).await.ok().map(|m| m.len());
    let _ = evt_tx
        .send(SftpEvent::UploadStarted {
            transfer_id,
            file_name: file_name.clone(),
            total,
        })
        .await;

    let remote_path = join_path(remote_dir, &file_name);
    let (prog_tx, mut prog_rx) = mpsc::unbounded_channel::<crate::sftp_ops::TransferEvent>();
    let evt_tx_progress = evt_tx.clone();
    let name_progress = file_name.clone();
    let progress_task = tokio::spawn(async move {
        while let Some(ev) = prog_rx.recv().await {
            if let crate::sftp_ops::TransferEvent::Progress { transferred } = ev {
                let _ = evt_tx_progress
                    .send(SftpEvent::UploadProgress(UploadProgress {
                        transfer_id,
                        file_name: name_progress.clone(),
                        total,
                        transferred,
                    }))
                    .await;
            }
        }
    });

    let result = async {
        sftp_ops::put_file_stream(sftp, local, &remote_path, &prog_tx, 0, cancel).await?;
        Ok::<(), String>(())
    }
    .await;
    drop(prog_tx);
    let _ = progress_task.await;

    match result {
        Ok(()) => {
            let _ = evt_tx
                .send(SftpEvent::UploadCompleted {
                    transfer_id,
                    file_name,
                })
                .await;
        }
        Err(message) => {
            let _ = evt_tx
                .send(SftpEvent::UploadFailed {
                    transfer_id,
                    file_name,
                    message,
                })
                .await;
        }
    }
}

/// 递归上传整个本地目录到 remote_dir/dir_name。
/// 单条 transfer，total = 全部文件总字节，按文件累计 progress。
/// 参考 Warp ssh_file_upload.rs:223 的 `put -r` 语义，但我们用 SFTP 协议手动遍历。
async fn run_upload_dir(
    sftp: &russh_sftp::client::SftpSession,
    local_root: &std::path::Path,
    remote_dir: &str,
    dir_name: &str,
    transfer_id: u64,
    evt_tx: &async_channel::Sender<SftpEvent>,
    cancel: &AtomicBool,
) {
    let display_name = format!("{dir_name}/");
    let tree = match sftp_ops::walk_local_dir(local_root).await {
        Ok(t) => t,
        Err(message) => {
            let _ = evt_tx
                .send(SftpEvent::UploadFailed {
                    transfer_id,
                    file_name: display_name,
                    message,
                })
                .await;
            return;
        }
    };

    let total = Some(tree.total_bytes);
    let _ = evt_tx
        .send(SftpEvent::UploadStarted {
            transfer_id,
            file_name: display_name.clone(),
            total,
        })
        .await;

    let remote_root = join_path(remote_dir, dir_name);
    if let Err(message) = sftp_ops::ensure_remote_dir(sftp, &remote_root).await {
        let _ = evt_tx
            .send(SftpEvent::UploadFailed {
                transfer_id,
                file_name: display_name,
                message,
            })
            .await;
        return;
    }
    // 远端先按深度顺序建好子目录
    let mut dirs = tree.dirs.clone();
    dirs.sort_by_key(|d| d.matches('/').count());
    for d in &dirs {
        let abs = format!("{}/{}", remote_root.trim_end_matches('/'), d);
        if let Err(message) = sftp_ops::ensure_remote_dir(sftp, &abs).await {
            let _ = evt_tx
                .send(SftpEvent::UploadFailed {
                    transfer_id,
                    file_name: display_name,
                    message,
                })
                .await;
            return;
        }
    }

    let (prog_tx, mut prog_rx) = mpsc::unbounded_channel::<crate::sftp_ops::TransferEvent>();
    let evt_tx_progress = evt_tx.clone();
    let name_progress = display_name.clone();
    let progress_task = tokio::spawn(async move {
        while let Some(ev) = prog_rx.recv().await {
            if let crate::sftp_ops::TransferEvent::Progress { transferred } = ev {
                let _ = evt_tx_progress
                    .send(SftpEvent::UploadProgress(UploadProgress {
                        transfer_id,
                        file_name: name_progress.clone(),
                        total,
                        transferred,
                    }))
                    .await;
            }
        }
    });

    let mut base: u64 = 0;
    let mut failure: Option<String> = None;
    for file in &tree.files {
        if cancel.load(Ordering::Relaxed) {
            failure = Some("cancelled".to_string());
            break;
        }
        let remote_path = format!("{}/{}", remote_root.trim_end_matches('/'), file.rel);
        match sftp_ops::put_file_stream(sftp, &file.abs, &remote_path, &prog_tx, base, cancel).await
        {
            Ok(n) => base += n,
            Err(message) => {
                failure = Some(if message == "cancelled" {
                    "cancelled".to_string()
                } else {
                    format!("{}: {message}", file.rel)
                });
                break;
            }
        }
    }
    drop(prog_tx);
    let _ = progress_task.await;

    match failure {
        None => {
            let _ = evt_tx
                .send(SftpEvent::UploadCompleted {
                    transfer_id,
                    file_name: display_name,
                })
                .await;
        }
        Some(message) => {
            let _ = evt_tx
                .send(SftpEvent::UploadFailed {
                    transfer_id,
                    file_name: display_name,
                    message,
                })
                .await;
        }
    }
}

async fn run_download(
    sftp: &russh_sftp::client::SftpSession,
    remote: &str,
    local: &std::path::Path,
    file_name: &str,
    transfer_id: u64,
    evt_tx: &async_channel::Sender<SftpEvent>,
    cancel: &AtomicBool,
) {
    // 进度先转一遍：sftp_ops::TransferEvent → SftpEvent::DownloadProgress
    let (prog_tx, mut prog_rx) = mpsc::unbounded_channel::<crate::sftp_ops::TransferEvent>();
    let evt_tx_progress = evt_tx.clone();
    let evt_tx_started = evt_tx.clone();
    let name_started = file_name.to_string();
    let name_progress = file_name.to_string();
    let progress_task = tokio::spawn(async move {
        let mut total_seen: Option<u64> = None;
        while let Some(ev) = prog_rx.recv().await {
            match ev {
                crate::sftp_ops::TransferEvent::Started { total } => {
                    total_seen = total;
                    let _ = evt_tx_started
                        .send(SftpEvent::DownloadStarted {
                            transfer_id,
                            file_name: name_started.clone(),
                            total,
                        })
                        .await;
                }
                crate::sftp_ops::TransferEvent::Progress { transferred } => {
                    let _ = evt_tx_progress
                        .send(SftpEvent::DownloadProgress(TransferProgress {
                            transfer_id,
                            file_name: name_progress.clone(),
                            total: total_seen,
                            transferred,
                        }))
                        .await;
                }
                crate::sftp_ops::TransferEvent::Completed { .. }
                | crate::sftp_ops::TransferEvent::Failed { .. } => {}
            }
        }
    });

    let total = sftp.metadata(remote).await.ok().map(|m| m.len());
    let _ = prog_tx.send(crate::sftp_ops::TransferEvent::Started { total });
    let result = sftp_ops::get_file_stream(sftp, remote, local, &prog_tx, 0, cancel)
        .await
        .map(|_| ());
    drop(prog_tx);
    let _ = progress_task.await;

    match result {
        Ok(()) => {
            let _ = evt_tx
                .send(SftpEvent::DownloadCompleted {
                    transfer_id,
                    file_name: file_name.to_string(),
                    local: local.to_path_buf(),
                })
                .await;
        }
        Err(message) => {
            let _ = evt_tx
                .send(SftpEvent::DownloadFailed {
                    transfer_id,
                    file_name: file_name.to_string(),
                    message,
                })
                .await;
        }
    }
}

/// 递归下载远端目录到 local_root（本地不存在时按需创建）。
/// 单条 transfer，total = 远端目录全部文件字节数。
async fn run_download_dir(
    sftp: &russh_sftp::client::SftpSession,
    remote_root: &str,
    local_root: &std::path::Path,
    dir_name: &str,
    transfer_id: u64,
    evt_tx: &async_channel::Sender<SftpEvent>,
    cancel: &AtomicBool,
) {
    let display_name = format!("{dir_name}/");
    let tree = match sftp_ops::walk_remote_dir(sftp, remote_root).await {
        Ok(t) => t,
        Err(message) => {
            let _ = evt_tx
                .send(SftpEvent::DownloadFailed {
                    transfer_id,
                    file_name: display_name,
                    message,
                })
                .await;
            return;
        }
    };

    let total = Some(tree.total_bytes);
    let _ = evt_tx
        .send(SftpEvent::DownloadStarted {
            transfer_id,
            file_name: display_name.clone(),
            total,
        })
        .await;

    if let Err(error) = tokio::fs::create_dir_all(local_root).await {
        let _ = evt_tx
            .send(SftpEvent::DownloadFailed {
                transfer_id,
                file_name: display_name,
                message: format!("create local {} failed: {error}", local_root.display()),
            })
            .await;
        return;
    }
    let mut dirs = tree.dirs.clone();
    dirs.sort_by_key(|d| d.matches('/').count());
    for d in &dirs {
        let abs = local_root.join(d.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Err(error) = tokio::fs::create_dir_all(&abs).await {
            let _ = evt_tx
                .send(SftpEvent::DownloadFailed {
                    transfer_id,
                    file_name: display_name,
                    message: format!("create local {} failed: {error}", abs.display()),
                })
                .await;
            return;
        }
    }

    let (prog_tx, mut prog_rx) = mpsc::unbounded_channel::<crate::sftp_ops::TransferEvent>();
    let evt_tx_progress = evt_tx.clone();
    let name_progress = display_name.clone();
    let progress_task = tokio::spawn(async move {
        while let Some(ev) = prog_rx.recv().await {
            if let crate::sftp_ops::TransferEvent::Progress { transferred } = ev {
                let _ = evt_tx_progress
                    .send(SftpEvent::DownloadProgress(TransferProgress {
                        transfer_id,
                        file_name: name_progress.clone(),
                        total,
                        transferred,
                    }))
                    .await;
            }
        }
    });

    let mut base: u64 = 0;
    let mut failure: Option<String> = None;
    for file in &tree.files {
        if cancel.load(Ordering::Relaxed) {
            failure = Some("cancelled".to_string());
            break;
        }
        let remote_path = format!("{}/{}", remote_root.trim_end_matches('/'), file.rel);
        let local_path = local_root.join(file.rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        match sftp_ops::get_file_stream(sftp, &remote_path, &local_path, &prog_tx, base, cancel)
            .await
        {
            Ok(n) => base += n,
            Err(message) => {
                failure = Some(if message == "cancelled" {
                    "cancelled".to_string()
                } else {
                    format!("{}: {message}", file.rel)
                });
                break;
            }
        }
    }
    drop(prog_tx);
    let _ = progress_task.await;

    match failure {
        None => {
            let _ = evt_tx
                .send(SftpEvent::DownloadCompleted {
                    transfer_id,
                    file_name: display_name,
                    local: local_root.to_path_buf(),
                })
                .await;
        }
        Some(message) => {
            let _ = evt_tx
                .send(SftpEvent::DownloadFailed {
                    transfer_id,
                    file_name: display_name,
                    message,
                })
                .await;
        }
    }
}

async fn run_list(
    sftp: &russh_sftp::client::SftpSession,
    path: &str,
    evt_tx: &async_channel::Sender<SftpEvent>,
) {
    let _ = evt_tx
        .send(SftpEvent::Loading {
            path: path.to_string(),
        })
        .await;
    let resolved = sftp_ops::canonicalize(sftp, path)
        .await
        .unwrap_or_else(|_| path.to_string());
    match sftp_ops::list_dir(sftp, &resolved).await {
        Ok(mut entries) => {
            // 目录优先、名字升序；隐藏 . 和 ..
            entries.retain(|e| e.name != "." && e.name != "..");
            sort_file_panel_entries(&mut entries);
            let _ = evt_tx
                .send(SftpEvent::DirListed {
                    path: resolved,
                    entries,
                })
                .await;
        }
        Err(error) => {
            let _ = evt_tx
                .send(SftpEvent::ListFailed {
                    path: resolved,
                    message: error,
                })
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_entry(name: &str, kind: crate::sftp_ops::EntryKind) -> RemoteEntry {
        RemoteEntry {
            name: name.to_string(),
            kind,
            size: 0,
            modified: None,
            permissions: None,
        }
    }

    fn recv_event(
        runtime: &tokio::runtime::Runtime,
        rx: &async_channel::Receiver<SftpEvent>,
    ) -> SftpEvent {
        runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(2), rx.recv())
                    .await
                    .expect("timed out waiting for file panel event")
            })
            .unwrap()
    }

    #[test]
    fn new_state_defaults_to_follow_cwd_on_and_dot_path() {
        let s = FilePanelState::new();
        assert!(s.follow_cwd);
        assert_eq!(s.cwd, ".");
        assert!(s.entries.is_empty());
        assert!(!s.loading);
        assert!(s.error.is_none());
    }

    #[test]
    fn parent_path_handles_root_and_relative() {
        assert_eq!(parent_path("/"), "/");
        assert_eq!(parent_path(""), "/");
        assert_eq!(parent_path("/home/matt"), "/home");
        assert_eq!(parent_path("/home"), "/");
        assert_eq!(parent_path("/home/"), "/");
        assert_eq!(parent_path("a/b/c"), "a/b");
        assert_eq!(parent_path("foo"), ".");
    }

    #[test]
    fn join_path_handles_absolute_and_root_cwd() {
        assert_eq!(join_path("/home", "matt"), "/home/matt");
        assert_eq!(join_path("/", "etc"), "/etc");
        assert_eq!(join_path("/home/", "matt"), "/home/matt");
        assert_eq!(join_path(".", "src"), "./src");
        assert_eq!(join_path("/home/matt", "/abs"), "/abs");
        assert_eq!(join_path("", "name"), "name");
    }

    #[test]
    fn local_tree_listing_keeps_child_loads_under_root_cwd() {
        let mut state = FilePanelState::new();
        apply_local_file_panel_event(
            &mut state,
            SftpEvent::DirListed {
                path: "/Users/example".to_string(),
                entries: vec![
                    test_entry(".codex", crate::sftp_ops::EntryKind::Dir),
                    test_entry("notes.txt", crate::sftp_ops::EntryKind::File),
                ],
            },
        );

        assert_eq!(state.cwd, "/Users/example");
        let rows = flatten_file_panel_tree(&state);
        assert_eq!(
            rows.iter()
                .map(|row| (row.path.as_str(), row.depth, row.is_expanded))
                .collect::<Vec<_>>(),
            vec![
                ("/Users/example/.codex", 0, false),
                ("/Users/example/notes.txt", 0, false)
            ]
        );

        let toggle = toggle_file_panel_tree_dir(&mut state, "/Users/example/.codex");
        assert_eq!(toggle, FilePanelTreeToggle::ExpandedNeedsLoad);
        apply_local_file_panel_event(
            &mut state,
            SftpEvent::DirListed {
                path: "/Users/example/.codex".to_string(),
                entries: vec![test_entry("config.toml", crate::sftp_ops::EntryKind::File)],
            },
        );

        assert_eq!(state.cwd, "/Users/example");
        let rows = flatten_file_panel_tree(&state);
        assert_eq!(
            rows.iter()
                .map(|row| (row.path.as_str(), row.depth, row.is_expanded))
                .collect::<Vec<_>>(),
            vec![
                ("/Users/example/.codex", 0, true),
                ("/Users/example/.codex/config.toml", 1, false),
                ("/Users/example/notes.txt", 0, false)
            ]
        );
    }

    #[test]
    fn local_tree_background_child_refresh_keeps_root_cwd() {
        let mut state = FilePanelState::new();
        apply_local_file_panel_event(
            &mut state,
            SftpEvent::DirListed {
                path: "/Users/example".to_string(),
                entries: vec![test_entry("project", crate::sftp_ops::EntryKind::Dir)],
            },
        );
        assert_eq!(
            toggle_file_panel_tree_dir(&mut state, "/Users/example/project"),
            FilePanelTreeToggle::ExpandedNeedsLoad
        );
        apply_local_file_panel_event(
            &mut state,
            SftpEvent::DirListed {
                path: "/Users/example/project".to_string(),
                entries: vec![test_entry("old.txt", crate::sftp_ops::EntryKind::File)],
            },
        );

        apply_local_file_panel_event(
            &mut state,
            SftpEvent::DirListed {
                path: "/Users/example/project".to_string(),
                entries: vec![
                    test_entry("new.txt", crate::sftp_ops::EntryKind::File),
                    test_entry("old.txt", crate::sftp_ops::EntryKind::File),
                ],
            },
        );

        assert_eq!(state.cwd, "/Users/example");
        assert_eq!(state.tree_root.as_deref(), Some("/Users/example"));
        assert_eq!(
            flatten_file_panel_tree(&state)
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/Users/example/project",
                "/Users/example/project/new.txt",
                "/Users/example/project/old.txt"
            ]
        );
    }

    #[test]
    fn local_cwd_follow_into_subdir_switches_root_not_tree_child() {
        // 回归：cd 进当前显示目录的子目录时，跟随的 List 不经过 toggle，
        // tree_loading_dirs 不含该路径，必须整体切换根目录——而非误当树节点展开
        // 导致 state.cwd 卡在父目录、面板不跟随（旧的路径前缀启发式 bug）。
        let mut state = FilePanelState::new();
        apply_local_file_panel_event(
            &mut state,
            SftpEvent::DirListed {
                path: "/Users/example".to_string(),
                entries: vec![test_entry("proj", crate::sftp_ops::EntryKind::Dir)],
            },
        );
        assert_eq!(state.cwd, "/Users/example");

        apply_local_file_panel_event(
            &mut state,
            SftpEvent::Loading {
                path: "/Users/example/proj".to_string(),
            },
        );
        apply_local_file_panel_event(
            &mut state,
            SftpEvent::DirListed {
                path: "/Users/example/proj".to_string(),
                entries: vec![test_entry("main.rs", crate::sftp_ops::EntryKind::File)],
            },
        );

        assert_eq!(state.cwd, "/Users/example/proj");
        assert_eq!(state.tree_root.as_deref(), Some("/Users/example/proj"));
        assert_eq!(
            flatten_file_panel_tree(&state)
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/Users/example/proj/main.rs"]
        );
    }

    #[test]
    fn remote_flat_event_path_does_not_enable_local_tree_state() {
        let mut state = FilePanelState::new();
        apply_sftp_event(
            &mut state,
            SftpEvent::DirListed {
                path: "/home/root".to_string(),
                entries: vec![test_entry("logs", crate::sftp_ops::EntryKind::Dir)],
            },
        );

        assert_eq!(state.cwd, "/home/root");
        assert_eq!(state.entries.len(), 1);
        assert!(state.tree_root.is_none());
        assert!(flatten_file_panel_tree(&state).is_empty());
    }

    #[test]
    fn local_entry_metadata_failure_keeps_visible_placeholder() {
        let entry = local_entry_from_metadata("Documents".to_string(), None, None);

        assert_eq!(entry.name, "Documents");
        assert!(matches!(entry.kind, crate::sftp_ops::EntryKind::Other));
        assert_eq!(entry.size, 0);
    }

    #[test]
    fn local_tree_child_read_error_does_not_replace_whole_panel() {
        let mut state = FilePanelState::new();
        apply_local_file_panel_event(
            &mut state,
            SftpEvent::DirListed {
                path: "/Users/example".to_string(),
                entries: vec![test_entry(".Trash", crate::sftp_ops::EntryKind::Dir)],
            },
        );

        assert_eq!(
            toggle_file_panel_tree_dir(&mut state, "/Users/example/.Trash"),
            FilePanelTreeToggle::ExpandedNeedsLoad
        );
        apply_local_file_panel_event(
            &mut state,
            SftpEvent::Loading {
                path: "/Users/example/.Trash".to_string(),
            },
        );
        apply_local_file_panel_event(
            &mut state,
            SftpEvent::ListFailed {
                path: "/Users/example/.Trash".to_string(),
                message: "read_dir(/Users/example/.Trash) failed: Operation not permitted"
                    .to_string(),
            },
        );

        assert_eq!(state.cwd, "/Users/example");
        assert!(state.error.is_none());
        assert!(!state.tree_loading_dirs.contains("/Users/example/.Trash"));
        assert_eq!(
            state
                .tree_child_errors
                .get("/Users/example/.Trash")
                .map(String::as_str),
            Some("read_dir(/Users/example/.Trash) failed: Operation not permitted")
        );
        let rows = flatten_file_panel_tree(&state);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].path, "/Users/example/.Trash");
        assert!(rows[1].is_error());
    }

    #[test]
    fn worker_send_reports_closed_request_channel() {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let worker = SftpWorkerHandle {
            tx,
            shutdown: Arc::new(AtomicBool::new(false)),
            cancels: Arc::new(Mutex::new(HashMap::new())),
            _thread: None,
        };

        assert!(!worker.send(SftpRequest::Refresh));
    }

    #[test]
    fn local_file_worker_lists_dirs_first_from_requested_path() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("b-dir")).unwrap();
        std::fs::write(tmp.path().join("a-file.txt"), "hello").unwrap();

        let (worker, rx) = spawn_local_file_worker("unit-local", tmp.path().to_path_buf()).unwrap();
        assert!(worker.send(SftpRequest::List(tmp.path().to_string_lossy().into_owned())));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let loading = runtime.block_on(rx.recv()).unwrap();
        assert!(matches!(loading, SftpEvent::Loading { .. }));
        let listed = runtime.block_on(rx.recv()).unwrap();
        let SftpEvent::DirListed { path, entries } = listed else {
            panic!("expected DirListed event");
        };

        assert_eq!(PathBuf::from(path), tmp.path());
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["b-dir", "a-file.txt"]
        );
        assert!(matches!(entries[0].kind, crate::sftp_ops::EntryKind::Dir));
        assert!(matches!(entries[1].kind, crate::sftp_ops::EntryKind::File));
    }

    #[test]
    fn local_file_worker_refreshes_when_file_is_created_externally() {
        let tmp = tempfile::tempdir().unwrap();
        let (worker, rx) =
            spawn_local_file_worker("unit-local-watch", tmp.path().to_path_buf()).unwrap();
        assert!(worker.send(SftpRequest::List(tmp.path().to_string_lossy().into_owned())));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _ = recv_event(&runtime, &rx);
        let initial = recv_event(&runtime, &rx);
        assert!(matches!(initial, SftpEvent::DirListed { .. }));

        std::fs::write(tmp.path().join("created.txt"), "new").unwrap();

        let refreshed = runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if let SftpEvent::DirListed { entries, .. } = rx.recv().await.unwrap() {
                        if entries.iter().any(|entry| entry.name == "created.txt") {
                            return true;
                        }
                    }
                }
            })
            .await
            .unwrap_or(false)
        });
        assert!(
            refreshed,
            "external create should trigger an automatic refresh"
        );
    }

    #[test]
    fn local_file_worker_refreshes_loaded_tree_child_automatically() {
        let tmp = tempfile::tempdir().unwrap();
        let child = tmp.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let (worker, rx) =
            spawn_local_file_worker("unit-local-child-watch", tmp.path().to_path_buf()).unwrap();
        assert!(worker.send(SftpRequest::List(tmp.path().to_string_lossy().into_owned())));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _ = recv_event(&runtime, &rx);
        let _ = recv_event(&runtime, &rx);
        assert!(worker.send(SftpRequest::ListTreeChild(
            child.to_string_lossy().into_owned()
        )));
        let _ = recv_event(&runtime, &rx);
        let _ = recv_event(&runtime, &rx);

        std::fs::write(child.join("created.txt"), "new").unwrap();

        let refreshed_child = runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let SftpEvent::DirListed { path, entries } = rx.recv().await.unwrap() else {
                        continue;
                    };
                    if PathBuf::from(path) == child
                        && entries.iter().any(|entry| entry.name == "created.txt")
                    {
                        return true;
                    }
                }
            })
            .await
            .unwrap_or(false)
        });
        assert!(
            refreshed_child,
            "external create should refresh an already loaded tree child"
        );
    }

    #[test]
    fn local_file_worker_tree_child_list_does_not_replace_refresh_path() {
        let tmp = tempfile::tempdir().unwrap();
        let child = tmp.path().join("child");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(tmp.path().join("root.txt"), "root").unwrap();
        std::fs::write(child.join("nested.txt"), "nested").unwrap();

        let (worker, rx) =
            spawn_local_file_worker("unit-local-tree", tmp.path().to_path_buf()).unwrap();
        assert!(worker.send(SftpRequest::List(tmp.path().to_string_lossy().into_owned())));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _ = recv_event(&runtime, &rx);
        let root_listed = recv_event(&runtime, &rx);
        assert!(matches!(root_listed, SftpEvent::DirListed { .. }));

        assert!(worker.send(SftpRequest::ListTreeChild(
            child.to_string_lossy().into_owned()
        )));
        let _ = recv_event(&runtime, &rx);
        let SftpEvent::DirListed {
            path: child_path, ..
        } = recv_event(&runtime, &rx)
        else {
            panic!("expected child DirListed event");
        };
        assert_eq!(PathBuf::from(child_path), child);

        assert!(worker.send(SftpRequest::Refresh));
        let _ = recv_event(&runtime, &rx);
        let SftpEvent::DirListed { path, entries } = recv_event(&runtime, &rx) else {
            panic!("expected refreshed root DirListed event");
        };
        assert_eq!(PathBuf::from(path), tmp.path());
        assert!(entries.iter().any(|entry| entry.name == "root.txt"));
    }

    #[test]
    fn local_file_worker_refresh_updates_loaded_tree_children() {
        let tmp = tempfile::tempdir().unwrap();
        let child = tmp.path().join("child");
        std::fs::create_dir(&child).unwrap();

        let (worker, rx) =
            spawn_local_file_worker("unit-local-tree-refresh", tmp.path().to_path_buf()).unwrap();
        assert!(worker.send(SftpRequest::List(tmp.path().to_string_lossy().into_owned())));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _ = recv_event(&runtime, &rx);
        let _ = recv_event(&runtime, &rx);

        assert!(worker.send(SftpRequest::ListTreeChild(
            child.to_string_lossy().into_owned()
        )));
        let _ = recv_event(&runtime, &rx);
        let _ = recv_event(&runtime, &rx);

        std::fs::write(child.join("created.txt"), "new").unwrap();
        assert!(worker.send(SftpRequest::Refresh));

        let refreshed_child = runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let SftpEvent::DirListed { path, entries } = rx.recv().await.unwrap() else {
                        continue;
                    };
                    if PathBuf::from(path) == child
                        && entries.iter().any(|entry| entry.name == "created.txt")
                    {
                        return true;
                    }
                }
            })
            .await
            .unwrap_or(false)
        });
        assert!(
            refreshed_child,
            "manual refresh should update loaded tree children"
        );
    }

    #[test]
    fn local_file_worker_copies_files_into_cwd_and_deletes_them() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();
        let source = src_dir.path().join("note.txt");
        std::fs::write(&source, "hello").unwrap();

        let (worker, rx) =
            spawn_local_file_worker("unit-local-copy", dst_dir.path().to_path_buf()).unwrap();
        assert!(worker.send(SftpRequest::Upload {
            locals: vec![source],
            remote_dir: dst_dir.path().to_string_lossy().into_owned(),
        }));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let copied = dst_dir.path().join("note.txt");
        let mut saw_completed = false;
        let mut saw_refreshed_listing = false;
        for _ in 0..8 {
            match recv_event(&runtime, &rx) {
                SftpEvent::UploadCompleted { file_name, .. } => {
                    assert_eq!(file_name, "note.txt");
                    saw_completed = true;
                }
                SftpEvent::DirListed { entries, .. } => {
                    saw_refreshed_listing = entries.iter().any(|entry| entry.name == "note.txt");
                    if saw_refreshed_listing {
                        break;
                    }
                }
                SftpEvent::UploadFailed { message, .. } | SftpEvent::Error { message } => {
                    panic!("unexpected local copy failure: {message}");
                }
                _ => {}
            }
        }
        assert!(saw_completed);
        assert!(saw_refreshed_listing);
        assert_eq!(std::fs::read_to_string(&copied).unwrap(), "hello");

        assert!(worker.send(SftpRequest::Delete {
            path: copied.to_string_lossy().into_owned(),
            is_dir: false,
        }));
        let mut saw_delete_listing = false;
        for _ in 0..4 {
            if let SftpEvent::DirListed { entries, .. } = recv_event(&runtime, &rx) {
                saw_delete_listing = !entries.iter().any(|entry| entry.name == "note.txt");
                if saw_delete_listing {
                    break;
                }
            }
        }
        assert!(saw_delete_listing);
        assert!(!copied.exists());
    }
}
