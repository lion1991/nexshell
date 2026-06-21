//! 本地终端 git 面板状态 + 后台 worker。
//! 架构参考 file_panel.rs 同款：独立 OS 线程跑 current-thread tokio runtime，
//! 请求走 mpsc，事件走 async_channel。git 调用本身全同步阻塞，因此 worker 内
//! 也只是顺序跑 [`git_ops`] 里的函数。
//!
//! 数据语义参考 warp/app/src/code_review/git_status_update.rs 的简化版：
//! 我们没有 fs watcher（P4 才上），刷新触发点是 cwd 变化 / 手动按钮 / 操作完成。

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use notify_debouncer_full::{
    new_debouncer,
    notify::{RecommendedWatcher, RecursiveMode, Watcher},
    DebounceEventResult, DebouncedEvent, Debouncer, FileIdMap,
};
use tokio::sync::mpsc;

use crate::git_ops::{self, CommitRow, GitDiffSelection, GitFileDiff, GitStatusSnapshot};

/// fs watcher debounce 时长。Warp git_status_update.rs:204 用 5s throttle，
/// 我们用 2s 来兼顾响应度——nexshell 不像 Warp 还要做 AI/diff，开销小。
const WATCHER_DEBOUNCE: Duration = Duration::from_secs(2);
pub const GIT_HISTORY_PAGE_SIZE: usize = 20;
pub const GIT_HISTORY_HEIGHT_DEFAULT: f32 = 220.0;
pub const GIT_HISTORY_HEIGHT_MIN: f32 = 120.0;
pub const GIT_HISTORY_HEIGHT_MAX: f32 = 520.0;

pub fn clamp_git_history_height(height: f32) -> f32 {
    height.clamp(GIT_HISTORY_HEIGHT_MIN, GIT_HISTORY_HEIGHT_MAX)
}

/// UI 持有的 git 面板状态。每个本地 tab 一份。
#[derive(Clone, Debug, Default)]
pub struct GitPanelState {
    /// 当前生效的仓库根目录。None = cwd 未知或不在 git 仓库内。
    pub repo_root: Option<PathBuf>,
    /// 主分支名（origin/main 之类）。
    pub main_branch: Option<String>,
    /// 最近一次 `git status` 结果。
    pub status: GitStatusSnapshot,
    /// 最近 N 条 commit。
    pub recent_commits: Vec<CommitRow>,
    /// 最近 commit 是否还有下一页可加载。
    pub history_has_more: bool,
    /// 正在加载 commit 下一页。
    pub history_loading_more: bool,
    /// worker 正在跑命令的标识；UI 用来灰化按钮。
    pub loading: bool,
    /// 最近一次操作 / 刷新的错误信息（非 NotARepo）。
    pub error: Option<String>,
    /// 当前 diff 预览选择；kind 用于区分 staged / unstaged 的同一路径。
    pub selected_diff: Option<GitDiffSelection>,
    pub diff_preview: Option<GitFileDiff>,
    pub diff_loading: bool,
    pub diff_error: Option<String>,
    /// 当前 Git 文件列表多选集合。用 GitDiffSelection 区分同一路径的 staged/worktree 行。
    pub selected_entries: BTreeSet<GitDiffSelection>,
    /// shift 范围选择锚点。
    pub selection_anchor: Option<GitDiffSelection>,
    /// worker 上次刷新时的 cwd。用于 UI 决定是否需要再发 SetCwd。
    pub last_cwd: Option<PathBuf>,
}

impl GitPanelState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前是否处于 git 仓库内。
    pub fn in_repo(&self) -> bool {
        self.repo_root.is_some()
    }
}

#[derive(Clone, Debug)]
pub enum GitRequest {
    /// 终端 OSC 7 上报新 cwd → worker 探测 repo_root 并刷新。
    SetCwd(PathBuf),
    /// 手动刷新（按钮 / 操作完成后）。worker 从上次 cwd 重新探测 repo_root。
    Refresh,
    /// `git add -- <paths>`。
    Stage(Vec<String>),
    /// `git restore --staged -- <paths>`。
    Unstage(Vec<String>),
    /// `git restore -- <paths>`，丢弃 tracked 文件的未暂存工作区改动。
    DiscardWorktreeChanges(Vec<String>),
    /// `git clean -ff -d -- <paths>`，删除 untracked 文件（含内嵌 git 仓库）。
    DeleteUntracked(Vec<String>),
    /// 将路径追加到 `.gitignore`。
    AddToGitignore(Vec<String>),
    /// 从当前仓库的历史列表继续加载一页。
    LoadMoreHistory {
        offset: usize,
    },
    /// 加载单个文件的 diff 预览。
    LoadDiff(GitDiffSelection),
    /// `git commit -m <message>`（可 amend）。
    Commit {
        message: String,
        amend: bool,
    },
    /// `git push` 到当前分支 upstream。
    Push {
        accept_new_ssh_host: bool,
    },
    Shutdown,
}

#[derive(Clone, Debug)]
pub enum GitEvent {
    /// worker 开始跑命令；UI 灰化按钮。
    Loading,
    /// 一次完整刷新的结果。
    Snapshot {
        repo_root: PathBuf,
        main_branch: Option<String>,
        status: GitStatusSnapshot,
        commits: Vec<CommitRow>,
        history_has_more: bool,
    },
    /// commit 历史的一页增量。
    HistoryPage {
        repo_root: PathBuf,
        offset: usize,
        commits: Vec<CommitRow>,
        has_more: bool,
    },
    DiffLoading {
        selection: GitDiffSelection,
    },
    DiffLoaded {
        repo_root: PathBuf,
        selection: GitDiffSelection,
        diff: GitFileDiff,
    },
    DiffFailed {
        selection: GitDiffSelection,
        message: String,
    },
    /// cwd 不在任何 git 仓库内。UI 显示提示。
    NotARepo {
        cwd: PathBuf,
    },
    /// stage/unstage/commit 等操作失败。Snapshot 类失败也走这里。
    OpFailed(String),
    /// SSH 首次连接需要用户确认 host key；UI 确认后再重试 push。
    SshHostKeyPrompt {
        prompt: git_ops::SshHostKeyPrompt,
    },
    /// commit 操作已经结束；UI 用它复位提交按钮，成功时清空输入框。
    CommitFinished {
        success: bool,
    },
    /// push 操作已经结束；UI 用它复位推送按钮。
    PushFinished {
        success: bool,
    },
}

/// 把 worker 事件应用到 state。
pub fn apply_git_event(state: &mut GitPanelState, event: GitEvent) {
    match event {
        GitEvent::Loading => {
            state.loading = true;
            state.error = None;
        }
        GitEvent::Snapshot {
            repo_root,
            main_branch,
            status,
            commits,
            history_has_more,
        } => {
            state.last_cwd = Some(repo_root.clone());
            state.repo_root = Some(repo_root);
            state.main_branch = main_branch;
            state.status = status;
            state.recent_commits = commits;
            state.history_has_more = history_has_more;
            state.history_loading_more = false;
            state.loading = false;
            state.error = None;
            clear_stale_diff_selection(state);
            prune_stale_git_panel_selection(state);
        }
        GitEvent::HistoryPage {
            repo_root,
            offset,
            commits,
            has_more,
        } => {
            if state.repo_root.as_ref() == Some(&repo_root) && offset <= state.recent_commits.len()
            {
                state.recent_commits.truncate(offset);
                state.recent_commits.extend(commits);
                state.history_has_more = has_more;
            }
            state.history_loading_more = false;
            state.error = None;
        }
        GitEvent::NotARepo { cwd } => {
            state.last_cwd = Some(cwd);
            state.repo_root = None;
            state.main_branch = None;
            state.status = GitStatusSnapshot::default();
            state.recent_commits.clear();
            state.history_has_more = false;
            state.history_loading_more = false;
            clear_diff_preview(state);
            clear_git_panel_selection(state);
            state.loading = false;
            state.error = None;
        }
        GitEvent::OpFailed(message) => {
            state.loading = false;
            state.history_loading_more = false;
            state.error = Some(message);
        }
        GitEvent::SshHostKeyPrompt { .. } => {
            state.loading = false;
            state.history_loading_more = false;
            state.error = None;
        }
        GitEvent::DiffLoading { selection } => {
            state.selected_diff = Some(selection);
            state.diff_preview = None;
            state.diff_loading = true;
            state.diff_error = None;
        }
        GitEvent::DiffLoaded {
            repo_root,
            selection,
            diff,
        } => {
            if state.repo_root.as_ref() == Some(&repo_root)
                && state.selected_diff.as_ref() == Some(&selection)
            {
                state.diff_preview = Some(diff);
                state.diff_loading = false;
                state.diff_error = None;
            }
        }
        GitEvent::DiffFailed { selection, message } => {
            if state.selected_diff.as_ref() == Some(&selection) {
                state.diff_preview = None;
                state.diff_loading = false;
                state.diff_error = Some(message);
            }
        }
        GitEvent::CommitFinished { .. } | GitEvent::PushFinished { .. } => {}
    }
}

fn clear_diff_preview(state: &mut GitPanelState) {
    state.selected_diff = None;
    state.diff_preview = None;
    state.diff_loading = false;
    state.diff_error = None;
}

fn clear_git_panel_selection(state: &mut GitPanelState) {
    state.selected_entries.clear();
    state.selection_anchor = None;
}

pub fn clear_stale_diff_selection(state: &mut GitPanelState) {
    let Some(selection) = state.selected_diff.as_ref() else {
        return;
    };
    if diff_selection_exists(&state.status, selection) {
        return;
    }
    clear_diff_preview(state);
}

pub fn diff_selection_exists(status: &GitStatusSnapshot, selection: &GitDiffSelection) -> bool {
    let entries = match selection.kind {
        git_ops::GitDiffKind::Staged => &status.staged,
        git_ops::GitDiffKind::Unstaged => &status.unstaged,
        git_ops::GitDiffKind::Untracked => &status.untracked,
    };
    entries.iter().any(|entry| entry.path == selection.path)
        || (selection.kind == git_ops::GitDiffKind::Unstaged
            && status
                .unmerged
                .iter()
                .any(|entry| entry.path == selection.path))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitPanelSelectMode {
    Replace,
    Toggle,
    Range,
}

pub fn apply_git_panel_selection(
    state: &mut GitPanelState,
    target: GitDiffSelection,
    mode: GitPanelSelectMode,
) {
    match mode {
        GitPanelSelectMode::Replace => {
            state.selected_entries.clear();
            state.selected_entries.insert(target.clone());
            state.selection_anchor = Some(target);
        }
        GitPanelSelectMode::Toggle => {
            if !state.selected_entries.remove(&target) {
                state.selected_entries.insert(target.clone());
            }
            state.selection_anchor = Some(target);
        }
        GitPanelSelectMode::Range => {
            let Some(anchor) = state.selection_anchor.clone() else {
                apply_git_panel_selection(state, target, GitPanelSelectMode::Replace);
                return;
            };
            if anchor.kind != target.kind {
                apply_git_panel_selection(state, target, GitPanelSelectMode::Replace);
                return;
            }
            let ordered = git_panel_selection_order(&state.status, target.kind);
            let anchor_idx = ordered.iter().position(|entry| entry == &anchor);
            let target_idx = ordered.iter().position(|entry| entry == &target);
            let (Some(ai), Some(ti)) = (anchor_idx, target_idx) else {
                apply_git_panel_selection(state, target, GitPanelSelectMode::Replace);
                return;
            };
            let (lo, hi) = if ai <= ti { (ai, ti) } else { (ti, ai) };
            state.selected_entries.clear();
            for selection in &ordered[lo..=hi] {
                state.selected_entries.insert(selection.clone());
            }
        }
    }
}

fn prune_stale_git_panel_selection(state: &mut GitPanelState) {
    state
        .selected_entries
        .retain(|selection| diff_selection_exists(&state.status, selection));
    if state
        .selection_anchor
        .as_ref()
        .is_some_and(|selection| !diff_selection_exists(&state.status, selection))
    {
        state.selection_anchor = None;
    }
}

fn git_panel_selection_order(
    status: &GitStatusSnapshot,
    kind: git_ops::GitDiffKind,
) -> Vec<GitDiffSelection> {
    let entries = match kind {
        git_ops::GitDiffKind::Staged => &status.staged,
        git_ops::GitDiffKind::Unstaged => &status.unstaged,
        git_ops::GitDiffKind::Untracked => &status.untracked,
    };
    let mut selections: Vec<GitDiffSelection> = entries
        .iter()
        .map(|entry| GitDiffSelection {
            path: entry.path.clone(),
            kind,
        })
        .collect();
    if kind == git_ops::GitDiffKind::Unstaged {
        selections.extend(status.unmerged.iter().map(|entry| GitDiffSelection {
            path: entry.path.clone(),
            kind,
        }));
    }
    selections
}

#[derive(Debug)]
pub struct GitWorkerHandle {
    tx: mpsc::UnboundedSender<GitRequest>,
    shutdown: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl GitWorkerHandle {
    pub fn send(&self, req: GitRequest) -> bool {
        self.tx.send(req).is_ok()
    }
}

impl Drop for GitWorkerHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = self.tx.send(GitRequest::Shutdown);
    }
}

/// 起一个 git worker。返回 handle + 事件 receiver。每个本地 tab 一个 worker
/// 即可——git 命令足够轻，不必跨 tab 共享（参考 file_panel：每 tab 一个 sftp worker）。
pub fn spawn_git_worker(
    label: &str,
) -> Result<(GitWorkerHandle, async_channel::Receiver<GitEvent>), String> {
    let (req_tx, req_rx) = mpsc::unbounded_channel::<GitRequest>();
    let (evt_tx, evt_rx) = async_channel::bounded::<GitEvent>(32);
    let shutdown = Arc::new(AtomicBool::new(false));
    let thread_shutdown = Arc::clone(&shutdown);
    let req_tx_for_watcher = req_tx.clone();

    let thread = thread::Builder::new()
        .name(format!("nexshell-git-{}", label.replace(['/', ':'], "_")))
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    let _ =
                        evt_tx.try_send(GitEvent::OpFailed(format!("git worker runtime: {error}")));
                    return;
                }
            };
            runtime.block_on(run_worker(
                req_rx,
                req_tx_for_watcher,
                evt_tx,
                thread_shutdown,
            ));
        })
        .map_err(|error| format!("spawn git thread: {error}"))?;

    Ok((
        GitWorkerHandle {
            tx: req_tx,
            shutdown,
            _thread: Some(thread),
        },
        evt_rx,
    ))
}

async fn run_worker(
    mut req_rx: mpsc::UnboundedReceiver<GitRequest>,
    req_tx_self: mpsc::UnboundedSender<GitRequest>,
    evt_tx: async_channel::Sender<GitEvent>,
    shutdown: Arc<AtomicBool>,
) {
    let mut current_repo: Option<PathBuf> = None;
    let mut current_cwd: Option<PathBuf> = None;
    // 拥有 watcher：drop 时自动停止线程。repo root 变化时重建。
    let mut _watcher: Option<Debouncer<RecommendedWatcher, FileIdMap>> = None;
    let mut history_limit = GIT_HISTORY_PAGE_SIZE;

    while let Some(req) = req_rx.recv().await {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match req {
            GitRequest::Shutdown => break,
            GitRequest::SetCwd(cwd) => {
                current_cwd = Some(cwd.clone());
                let _ = evt_tx.send(GitEvent::Loading).await;
                match git_ops::repo_root(&cwd) {
                    Ok(root) => {
                        // repo 变了才重建 watcher，避免无谓 tear-down/spawn
                        let need_new_watcher =
                            current_repo.as_ref().map(|p| p != &root).unwrap_or(true);
                        current_repo = Some(root.clone());
                        if need_new_watcher {
                            history_limit = GIT_HISTORY_PAGE_SIZE;
                            _watcher = build_repo_watcher(&root, req_tx_self.clone());
                        }
                        emit_snapshot(&evt_tx, &root, history_limit).await;
                    }
                    Err(_) => {
                        current_repo = None;
                        _watcher = None;
                        let _ = evt_tx.send(GitEvent::NotARepo { cwd }).await;
                    }
                }
            }
            GitRequest::Refresh => {
                let root = match current_cwd
                    .as_ref()
                    .and_then(|cwd| git_ops::repo_root(cwd).ok())
                {
                    Some(root) => {
                        let need_new_watcher =
                            current_repo.as_ref().map(|p| p != &root).unwrap_or(true);
                        current_repo = Some(root.clone());
                        if need_new_watcher {
                            history_limit = GIT_HISTORY_PAGE_SIZE;
                            _watcher = build_repo_watcher(&root, req_tx_self.clone());
                        }
                        root
                    }
                    None => {
                        if let Some(cwd) = current_cwd.clone() {
                            current_repo = None;
                            _watcher = None;
                            let _ = evt_tx.send(GitEvent::NotARepo { cwd }).await;
                        }
                        continue;
                    }
                };

                // index.lock 抑制：用户正在跑 commit / merge 等，
                // 读到的 HEAD / index 是中间态。Warp git_status_update.rs:367 同款。
                if index_lock_present(&root) {
                    continue;
                }
                let _ = evt_tx.send(GitEvent::Loading).await;
                emit_snapshot(&evt_tx, &root, history_limit).await;
            }
            GitRequest::Stage(paths) => {
                if let Some(root) = current_repo.clone() {
                    run_modifying(&evt_tx, &root, history_limit, |r| git_ops::stage(r, &paths))
                        .await;
                }
            }
            GitRequest::Unstage(paths) => {
                if let Some(root) = current_repo.clone() {
                    run_modifying(&evt_tx, &root, history_limit, |r| {
                        git_ops::unstage(r, &paths)
                    })
                    .await;
                }
            }
            GitRequest::DiscardWorktreeChanges(paths) => {
                if let Some(root) = current_repo.clone() {
                    run_modifying(&evt_tx, &root, history_limit, |r| {
                        git_ops::discard_worktree_changes(r, &paths)
                    })
                    .await;
                }
            }
            GitRequest::DeleteUntracked(paths) => {
                if let Some(root) = current_repo.clone() {
                    run_modifying(&evt_tx, &root, history_limit, |r| {
                        git_ops::delete_untracked(r, &paths)
                    })
                    .await;
                }
            }
            GitRequest::AddToGitignore(paths) => {
                if let Some(root) = current_repo.clone() {
                    run_modifying(&evt_tx, &root, history_limit, |r| {
                        git_ops::add_to_gitignore(r, &paths)
                    })
                    .await;
                }
            }
            GitRequest::LoadMoreHistory { offset } => {
                if let Some(root) = current_repo.clone() {
                    match commit_page(&root, offset, GIT_HISTORY_PAGE_SIZE) {
                        Ok((commits, has_more)) => {
                            history_limit = history_limit.max(offset + commits.len());
                            let _ = evt_tx
                                .send(GitEvent::HistoryPage {
                                    repo_root: root,
                                    offset,
                                    commits,
                                    has_more,
                                })
                                .await;
                        }
                        Err(message) => {
                            let _ = evt_tx.send(GitEvent::OpFailed(message)).await;
                        }
                    }
                }
            }
            GitRequest::LoadDiff(selection) => {
                if let Some(root) = current_repo.clone() {
                    let _ = evt_tx
                        .send(GitEvent::DiffLoading {
                            selection: selection.clone(),
                        })
                        .await;
                    match git_ops::file_diff(&root, &selection.path, selection.kind) {
                        Ok(diff) => {
                            let _ = evt_tx
                                .send(GitEvent::DiffLoaded {
                                    repo_root: root,
                                    selection,
                                    diff,
                                })
                                .await;
                        }
                        Err(message) => {
                            let _ = evt_tx
                                .send(GitEvent::DiffFailed { selection, message })
                                .await;
                        }
                    }
                }
            }
            GitRequest::Commit { message, amend } => {
                if let Some(root) = current_repo.clone() {
                    run_commit(&evt_tx, &root, history_limit, &message, amend).await;
                }
            }
            GitRequest::Push {
                accept_new_ssh_host,
            } => {
                if let Some(root) = current_repo.clone() {
                    run_push(&evt_tx, &root, history_limit, accept_new_ssh_host).await;
                }
            }
        }
    }
}

/// 构造 repo 的 fs watcher。监听 root（recursive），事件回调过滤噪声路径
/// 后向 worker 自我发送 Refresh。Drop debouncer 即停止 watch 线程。
fn build_repo_watcher(
    root: &Path,
    tx: mpsc::UnboundedSender<GitRequest>,
) -> Option<Debouncer<RecommendedWatcher, FileIdMap>> {
    let root_buf = root.to_path_buf();
    let mut debouncer = new_debouncer(
        WATCHER_DEBOUNCE,
        None,
        move |result: DebounceEventResult| {
            let Ok(events) = result else { return };
            // 任何一个事件不是噪声 → 触发 Refresh
            if events.iter().any(|ev| !is_noisy_event(&root_buf, ev)) {
                let _ = tx.send(GitRequest::Refresh);
            }
        },
    )
    .ok()?;
    debouncer
        .watcher()
        .watch(root, RecursiveMode::Recursive)
        .ok()?;
    Some(debouncer)
}

/// 判断 watcher 事件是否纯噪声（应忽略）。
/// - `.git/objects` / `logs` / `info` / `hooks` / `lfs` 内的变更：git 内部 plumbing，
///   工作树状态不会改变。
/// - 任何 `index.lock`：用户正在跑 git 命令，等它结束再说。
/// - 事件所有路径都满足上述条件才认定为噪声；只要有一条干净路径就当真事件。
fn is_noisy_event(root: &Path, ev: &DebouncedEvent) -> bool {
    let git_dir = root.join(".git");
    if ev.paths.is_empty() {
        return true;
    }
    ev.paths.iter().all(|p| is_noisy_path(&git_dir, p))
}

fn is_noisy_path(git_dir: &Path, p: &Path) -> bool {
    if p.file_name().map(|f| f == "index.lock").unwrap_or(false) {
        return true;
    }
    if let Ok(rel) = p.strip_prefix(git_dir) {
        if let Some(first) = rel.components().next() {
            if let Some(s) = first.as_os_str().to_str() {
                return matches!(
                    s,
                    "objects" | "logs" | "info" | "hooks" | "lfs" | "FETCH_HEAD" | "ORIG_HEAD"
                );
            }
        }
    }
    false
}

fn index_lock_present(root: &Path) -> bool {
    root.join(".git").join("index.lock").exists()
}

fn commit_page(
    root: &std::path::Path,
    offset: usize,
    limit: usize,
) -> Result<(Vec<CommitRow>, bool), String> {
    if limit == 0 {
        return Ok((Vec::new(), false));
    }
    let mut commits = git_ops::recent_commits_page(root, offset, limit.saturating_add(1))?;
    let has_more = commits.len() > limit;
    if has_more {
        commits.truncate(limit);
    }
    Ok((commits, has_more))
}

async fn emit_snapshot(
    evt_tx: &async_channel::Sender<GitEvent>,
    root: &std::path::Path,
    history_limit: usize,
) {
    let status = git_ops::status(root);
    let commits = commit_page(root, 0, history_limit.max(GIT_HISTORY_PAGE_SIZE));
    let main_branch = git_ops::detect_main_branch(root);
    match status {
        Ok(status) => {
            let (commits, history_has_more) = commits.unwrap_or_default();
            let _ = evt_tx
                .send(GitEvent::Snapshot {
                    repo_root: root.to_path_buf(),
                    main_branch,
                    status,
                    commits,
                    history_has_more,
                })
                .await;
        }
        Err(message) => {
            let _ = evt_tx.send(GitEvent::OpFailed(message)).await;
        }
    }
}

/// 执行一个修改性操作，成功后自动再发一次 Snapshot；失败 → OpFailed。
async fn run_modifying<F>(
    evt_tx: &async_channel::Sender<GitEvent>,
    root: &std::path::Path,
    history_limit: usize,
    op: F,
) where
    F: FnOnce(&std::path::Path) -> Result<(), String>,
{
    let _ = evt_tx.send(GitEvent::Loading).await;
    match op(root) {
        Ok(()) => emit_snapshot(evt_tx, root, history_limit).await,
        Err(message) => {
            let _ = evt_tx.send(GitEvent::OpFailed(message)).await;
        }
    }
}

async fn run_commit(
    evt_tx: &async_channel::Sender<GitEvent>,
    root: &std::path::Path,
    history_limit: usize,
    message: &str,
    amend: bool,
) {
    let _ = evt_tx.send(GitEvent::Loading).await;
    let success = match git_ops::commit(root, message, amend) {
        Ok(()) => {
            emit_snapshot(evt_tx, root, history_limit).await;
            true
        }
        Err(message) => {
            let _ = evt_tx.send(GitEvent::OpFailed(message)).await;
            false
        }
    };
    let _ = evt_tx.send(GitEvent::CommitFinished { success }).await;
}

async fn run_push(
    evt_tx: &async_channel::Sender<GitEvent>,
    root: &std::path::Path,
    history_limit: usize,
    accept_new_ssh_host: bool,
) {
    let policy = if accept_new_ssh_host {
        git_ops::SshHostKeyPolicy::AcceptNew
    } else {
        git_ops::SshHostKeyPolicy::Ask
    };
    let success = match git_ops::push(root, policy) {
        Ok(()) => {
            emit_snapshot(evt_tx, root, history_limit).await;
            true
        }
        Err(git_ops::GitPushError::SshHostKeyPrompt(prompt)) => {
            let _ = evt_tx.send(GitEvent::SshHostKeyPrompt { prompt }).await;
            return;
        }
        Err(git_ops::GitPushError::Failed(message)) => {
            let _ = evt_tx.send(GitEvent::OpFailed(message)).await;
            false
        }
    };
    let _ = evt_tx.send(GitEvent::PushFinished { success }).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit_row(sha: &str) -> CommitRow {
        CommitRow {
            sha: sha.into(),
            full_sha: sha.into(),
            author: "matt".into(),
            authored_at: String::new(),
            decorations: String::new(),
            summary: "x".into(),
            body: String::new(),
            files_changed: 0,
            insertions: 0,
            deletions: 0,
            file_changes: Vec::new(),
        }
    }

    #[test]
    fn clamp_git_history_height_keeps_splitter_in_usable_range() {
        assert_eq!(clamp_git_history_height(40.0), GIT_HISTORY_HEIGHT_MIN);
        assert_eq!(clamp_git_history_height(260.0), 260.0);
        assert_eq!(clamp_git_history_height(900.0), GIT_HISTORY_HEIGHT_MAX);
    }

    #[test]
    fn apply_snapshot_marks_in_repo() {
        let mut s = GitPanelState::new();
        apply_git_event(
            &mut s,
            GitEvent::Snapshot {
                repo_root: PathBuf::from("/tmp/r"),
                main_branch: Some("main".into()),
                status: GitStatusSnapshot {
                    branch: Some("feat".into()),
                    ..Default::default()
                },
                commits: vec![],
                history_has_more: false,
            },
        );
        assert!(s.in_repo());
        assert_eq!(s.status.branch.as_deref(), Some("feat"));
        assert!(!s.loading);
        assert!(s.error.is_none());
    }

    #[test]
    fn apply_not_a_repo_clears_state() {
        let mut s = GitPanelState {
            repo_root: Some(PathBuf::from("/tmp/r")),
            main_branch: Some("main".into()),
            status: GitStatusSnapshot {
                branch: Some("feat".into()),
                ..Default::default()
            },
            recent_commits: vec![CommitRow {
                sha: "abc".into(),
                full_sha: "abc".into(),
                author: "matt".into(),
                authored_at: String::new(),
                decorations: String::new(),
                summary: "x".into(),
                body: String::new(),
                files_changed: 0,
                insertions: 0,
                deletions: 0,
                file_changes: Vec::new(),
            }],
            ..Default::default()
        };
        apply_git_event(
            &mut s,
            GitEvent::NotARepo {
                cwd: PathBuf::from("/tmp/x"),
            },
        );
        assert!(!s.in_repo());
        assert!(s.main_branch.is_none());
        assert!(s.recent_commits.is_empty());
    }

    #[test]
    fn apply_history_page_appends_next_page_and_tracks_exhaustion() {
        let mut s = GitPanelState {
            repo_root: Some(PathBuf::from("/tmp/r")),
            recent_commits: vec![commit_row("a"), commit_row("b")],
            history_loading_more: true,
            history_has_more: true,
            ..Default::default()
        };
        apply_git_event(
            &mut s,
            GitEvent::HistoryPage {
                repo_root: PathBuf::from("/tmp/r"),
                offset: 2,
                commits: vec![commit_row("c")],
                has_more: false,
            },
        );
        assert_eq!(
            s.recent_commits
                .iter()
                .map(|commit| commit.sha.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert!(!s.history_loading_more);
        assert!(!s.history_has_more);
    }

    #[test]
    fn op_failed_does_not_clear_existing_snapshot() {
        let mut s = GitPanelState {
            repo_root: Some(PathBuf::from("/tmp/r")),
            history_loading_more: true,
            ..Default::default()
        };
        apply_git_event(&mut s, GitEvent::OpFailed("nope".into()));
        assert!(s.in_repo());
        assert_eq!(s.error.as_deref(), Some("nope"));
        assert!(!s.history_loading_more);
    }

    #[test]
    fn ssh_host_key_prompt_resets_loading_without_erroring_snapshot() {
        let mut s = GitPanelState {
            repo_root: Some(PathBuf::from("/tmp/r")),
            loading: true,
            history_loading_more: true,
            error: Some("old".into()),
            ..Default::default()
        };
        apply_git_event(
            &mut s,
            GitEvent::SshHostKeyPrompt {
                prompt: git_ops::SshHostKeyPrompt {
                    message: "prompt".into(),
                    host: Some("example.com".into()),
                    fingerprint: Some("SHA256:x".into()),
                },
            },
        );
        assert!(s.in_repo());
        assert!(!s.loading);
        assert!(!s.history_loading_more);
        assert!(s.error.is_none());
    }

    #[test]
    fn diff_events_update_preview_without_overwriting_panel_error() {
        let selection = git_ops::GitDiffSelection {
            path: "src/main.rs".into(),
            kind: git_ops::GitDiffKind::Unstaged,
        };
        let diff = git_ops::GitFileDiff {
            path: "src/main.rs".into(),
            kind: git_ops::GitDiffKind::Unstaged,
            hunks: vec![],
            additions: 0,
            deletions: 0,
            is_binary: false,
            is_too_large: false,
            raw_size: 0,
            binary_message: None,
        };
        let mut s = GitPanelState {
            repo_root: Some(PathBuf::from("/tmp/r")),
            error: Some("old panel error".into()),
            ..Default::default()
        };

        apply_git_event(
            &mut s,
            GitEvent::DiffLoading {
                selection: selection.clone(),
            },
        );
        assert_eq!(s.selected_diff.as_ref(), Some(&selection));
        assert!(s.diff_loading);
        assert!(s.diff_error.is_none());
        assert_eq!(s.error.as_deref(), Some("old panel error"));

        apply_git_event(
            &mut s,
            GitEvent::DiffLoaded {
                repo_root: PathBuf::from("/tmp/r"),
                selection: selection.clone(),
                diff: diff.clone(),
            },
        );
        assert!(!s.diff_loading);
        assert_eq!(s.diff_preview.as_ref(), Some(&diff));

        apply_git_event(
            &mut s,
            GitEvent::DiffFailed {
                selection,
                message: "diff failed".into(),
            },
        );
        assert!(!s.diff_loading);
        assert_eq!(s.diff_error.as_deref(), Some("diff failed"));
        assert_eq!(s.error.as_deref(), Some("old panel error"));
    }

    #[test]
    fn git_panel_selection_supports_replace_toggle_and_section_range() {
        let mut s = GitPanelState {
            status: GitStatusSnapshot {
                staged: vec![
                    git_file_entry("staged-a.rs", 'M', '.'),
                    git_file_entry("staged-b.rs", 'M', '.'),
                ],
                unstaged: vec![
                    git_file_entry("work-a.rs", '.', 'M'),
                    git_file_entry("work-b.rs", '.', 'M'),
                    git_file_entry("work-c.rs", '.', 'M'),
                ],
                ..Default::default()
            },
            ..Default::default()
        };

        let work_a = GitDiffSelection {
            path: "work-a.rs".into(),
            kind: git_ops::GitDiffKind::Unstaged,
        };
        let work_c = GitDiffSelection {
            path: "work-c.rs".into(),
            kind: git_ops::GitDiffKind::Unstaged,
        };
        let staged_b = GitDiffSelection {
            path: "staged-b.rs".into(),
            kind: git_ops::GitDiffKind::Staged,
        };

        apply_git_panel_selection(&mut s, work_a.clone(), GitPanelSelectMode::Replace);
        assert!(s.selected_entries.contains(&work_a));

        apply_git_panel_selection(&mut s, work_c.clone(), GitPanelSelectMode::Range);
        assert_eq!(
            s.selected_entries.iter().cloned().collect::<Vec<_>>(),
            vec![
                work_a.clone(),
                GitDiffSelection {
                    path: "work-b.rs".into(),
                    kind: git_ops::GitDiffKind::Unstaged,
                },
                work_c.clone(),
            ]
        );

        apply_git_panel_selection(&mut s, work_a.clone(), GitPanelSelectMode::Toggle);
        assert!(!s.selected_entries.contains(&work_a));
        assert!(s.selected_entries.contains(&work_c));

        apply_git_panel_selection(&mut s, staged_b.clone(), GitPanelSelectMode::Range);
        assert_eq!(
            s.selected_entries.iter().cloned().collect::<Vec<_>>(),
            vec![staged_b]
        );
    }

    #[test]
    fn snapshot_prunes_stale_git_panel_selection() {
        let mut s = GitPanelState {
            repo_root: Some(PathBuf::from("/tmp/r")),
            selected_entries: [
                GitDiffSelection {
                    path: "kept.rs".into(),
                    kind: git_ops::GitDiffKind::Unstaged,
                },
                GitDiffSelection {
                    path: "gone.rs".into(),
                    kind: git_ops::GitDiffKind::Unstaged,
                },
            ]
            .into_iter()
            .collect(),
            selection_anchor: Some(GitDiffSelection {
                path: "gone.rs".into(),
                kind: git_ops::GitDiffKind::Unstaged,
            }),
            ..Default::default()
        };

        apply_git_event(
            &mut s,
            GitEvent::Snapshot {
                repo_root: PathBuf::from("/tmp/r"),
                main_branch: Some("main".into()),
                status: GitStatusSnapshot {
                    unstaged: vec![git_file_entry("kept.rs", '.', 'M')],
                    ..Default::default()
                },
                commits: vec![],
                history_has_more: false,
            },
        );

        assert_eq!(
            s.selected_entries.iter().cloned().collect::<Vec<_>>(),
            vec![GitDiffSelection {
                path: "kept.rs".into(),
                kind: git_ops::GitDiffKind::Unstaged,
            }]
        );
        assert!(s.selection_anchor.is_none());
    }

    fn git_file_entry(
        path: &str,
        index_status: char,
        worktree_status: char,
    ) -> git_ops::GitFileEntry {
        git_ops::GitFileEntry {
            path: path.into(),
            original_path: None,
            index_status,
            worktree_status,
            stage: match (index_status, worktree_status) {
                ('.', _) => git_ops::GitFileStage::Unstaged,
                (_, '.') => git_ops::GitFileStage::Staged,
                _ => git_ops::GitFileStage::Both,
            },
        }
    }

    #[test]
    fn handle_send_after_drop_rx_reports_closed() {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let h = GitWorkerHandle {
            tx,
            shutdown: Arc::new(AtomicBool::new(false)),
            _thread: None,
        };
        assert!(!h.send(GitRequest::Refresh));
    }

    #[test]
    fn is_noisy_path_filters_git_internals() {
        let git = Path::new("/r/.git");
        assert!(is_noisy_path(git, Path::new("/r/.git/objects/ab/cdef")));
        assert!(is_noisy_path(git, Path::new("/r/.git/logs/HEAD")));
        assert!(is_noisy_path(git, Path::new("/r/.git/info/exclude")));
        assert!(is_noisy_path(git, Path::new("/r/.git/index.lock")));
        // 真事件：HEAD / index / refs/heads/main / 工作区文件
        assert!(!is_noisy_path(git, Path::new("/r/.git/HEAD")));
        assert!(!is_noisy_path(git, Path::new("/r/.git/index")));
        assert!(!is_noisy_path(git, Path::new("/r/.git/refs/heads/main")));
        assert!(!is_noisy_path(git, Path::new("/r/src/foo.rs")));
    }

    /// 端到端：临时建仓 → spawn worker → SetCwd → 收 Snapshot → Stage → 收新 Snapshot。
    #[test]
    fn worker_full_loop_against_real_repo() {
        use std::{fs, process::Command, time::Duration};
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "t"],
        ] {
            assert!(Command::new("git")
                .args(&args)
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("a.txt"), "hi\n").unwrap();
        // 先建一个 commit，避免空仓库 status 干扰
        Command::new("git")
            .args(["add", "a.txt"])
            .current_dir(repo)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "init"])
            .current_dir(repo)
            .status()
            .unwrap();
        fs::write(repo.join("b.txt"), "new\n").unwrap();

        let (handle, evt_rx) = spawn_git_worker("test").unwrap();
        assert!(handle.send(GitRequest::SetCwd(repo.to_path_buf())));

        // 收 events 直到拿到 Snapshot
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let snap = rt.block_on(async {
            let mut snapshot = None;
            for _ in 0..10 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await
                {
                    if let GitEvent::Snapshot { status, .. } = ev {
                        snapshot = Some(status);
                        break;
                    }
                }
            }
            snapshot
        });
        let snap = snap.expect("应收到至少一次 Snapshot");
        assert_eq!(snap.branch.as_deref(), Some("main"));
        assert!(snap.untracked.iter().any(|e| e.path == "b.txt"));

        // stage b.txt
        assert!(handle.send(GitRequest::Stage(vec!["b.txt".into()])));
        let snap2 = rt.block_on(async {
            let mut snapshot = None;
            for _ in 0..10 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await
                {
                    if let GitEvent::Snapshot { status, .. } = ev {
                        snapshot = Some(status);
                        break;
                    }
                }
            }
            snapshot
        });
        let snap2 = snap2.expect("stage 后应收到 Snapshot");
        assert!(
            snap2.staged.iter().any(|e| e.path == "b.txt"),
            "b.txt 应已 staged"
        );
    }

    #[test]
    fn worker_refresh_reprobes_cwd_after_nested_repo_init() {
        use std::{fs, process::Command, time::Duration};
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path();
        let child = parent.join("child");
        fs::create_dir(&child).unwrap();

        assert!(Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(parent)
            .status()
            .unwrap()
            .success());

        let (handle, evt_rx) = spawn_git_worker("nested-refresh-test").unwrap();
        assert!(handle.send(GitRequest::SetCwd(child.clone())));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let first_root = rt.block_on(async {
            for _ in 0..10 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await
                {
                    if let GitEvent::Snapshot { repo_root, .. } = ev {
                        return repo_root;
                    }
                }
            }
            panic!("未收到父仓库 Snapshot");
        });
        assert_eq!(
            fs::canonicalize(&first_root).unwrap(),
            fs::canonicalize(parent).unwrap()
        );

        assert!(Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&child)
            .status()
            .unwrap()
            .success());

        assert!(handle.send(GitRequest::Refresh));
        let refreshed_root = rt.block_on(async {
            for _ in 0..10 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await
                {
                    if let GitEvent::Snapshot { repo_root, .. } = ev {
                        return repo_root;
                    }
                }
            }
            panic!("刷新后未收到子仓库 Snapshot");
        });
        assert_eq!(
            fs::canonicalize(&refreshed_root).unwrap(),
            fs::canonicalize(&child).unwrap()
        );
    }

    #[test]
    fn worker_discards_worktree_change_and_refreshes_status() {
        use std::{fs, process::Command, time::Duration};
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "t"],
        ] {
            assert!(Command::new("git")
                .args(&args)
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("a.txt"), "hi\n").unwrap();
        Command::new("git")
            .args(["add", "a.txt"])
            .current_dir(repo)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "init"])
            .current_dir(repo)
            .status()
            .unwrap();
        fs::write(repo.join("a.txt"), "changed\n").unwrap();

        let (handle, evt_rx) = spawn_git_worker("discard-test").unwrap();
        assert!(handle.send(GitRequest::SetCwd(repo.to_path_buf())));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let first = rt.block_on(async {
            let mut snapshot = None;
            for _ in 0..10 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await
                {
                    if let GitEvent::Snapshot { status, .. } = ev {
                        snapshot = Some(status);
                        break;
                    }
                }
            }
            snapshot
        });
        assert!(
            first
                .expect("initial snapshot")
                .unstaged
                .iter()
                .any(|entry| entry.path == "a.txt"),
            "a.txt 应先显示为未暂存改动"
        );

        assert!(handle.send(GitRequest::DiscardWorktreeChanges(vec!["a.txt".into()])));
        let second = rt.block_on(async {
            let mut snapshot = None;
            for _ in 0..10 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await
                {
                    if let GitEvent::Snapshot { status, .. } = ev {
                        snapshot = Some(status);
                        break;
                    }
                }
            }
            snapshot
        });

        assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "hi\n");
        assert!(
            second.expect("discard snapshot").unstaged.is_empty(),
            "丢弃后未暂存列表应清空"
        );
    }

    #[test]
    fn worker_adds_untracked_file_to_gitignore_and_refreshes_status() {
        use std::{fs, process::Command, time::Duration};
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "t"],
        ] {
            assert!(Command::new("git")
                .args(&args)
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("ignored.log"), "debug\n").unwrap();

        let (handle, evt_rx) = spawn_git_worker("gitignore-test").unwrap();
        assert!(handle.send(GitRequest::SetCwd(repo.to_path_buf())));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let first = rt.block_on(async {
            for _ in 0..10 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await
                {
                    if let GitEvent::Snapshot { status, .. } = ev {
                        return status;
                    }
                }
            }
            panic!("initial snapshot")
        });
        assert!(first
            .untracked
            .iter()
            .any(|entry| entry.path == "ignored.log"));

        assert!(handle.send(GitRequest::AddToGitignore(vec!["ignored.log".into()])));
        let second = rt.block_on(async {
            for _ in 0..10 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await
                {
                    if let GitEvent::Snapshot { status, .. } = ev {
                        if !status
                            .untracked
                            .iter()
                            .any(|entry| entry.path == "ignored.log")
                        {
                            return status;
                        }
                    }
                }
            }
            panic!("gitignore refresh snapshot")
        });

        assert_eq!(
            fs::read_to_string(repo.join(".gitignore")).unwrap(),
            "ignored.log\n"
        );
        assert!(second
            .untracked
            .iter()
            .any(|entry| entry.path == ".gitignore"));
        assert!(!second
            .untracked
            .iter()
            .any(|entry| entry.path == "ignored.log"));
    }

    #[test]
    fn worker_pushes_clean_ahead_branch_and_refreshes_status() {
        use std::{fs, process::Command, time::Duration};
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("origin.git");
        assert!(Command::new("git")
            .args(["init", "--bare", "-q"])
            .arg(&remote)
            .status()
            .unwrap()
            .success());

        let repo = tmp.path().join("work");
        fs::create_dir(&repo).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "t"],
        ] {
            assert!(Command::new("git")
                .args(&args)
                .current_dir(&repo)
                .status()
                .unwrap()
                .success());
        }
        assert!(Command::new("git")
            .args(["remote", "add", "origin"])
            .arg(&remote)
            .current_dir(&repo)
            .status()
            .unwrap()
            .success());
        fs::write(repo.join("a.txt"), "init\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "a.txt"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-q", "-m", "init"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["push", "-q", "-u", "origin", "main"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success());

        fs::write(repo.join("a.txt"), "local\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "a.txt"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-q", "-m", "local"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success());

        let (handle, evt_rx) = spawn_git_worker("push-test").unwrap();
        assert!(handle.send(GitRequest::SetCwd(repo.clone())));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let ahead = rt.block_on(async {
            for _ in 0..10 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await
                {
                    if let GitEvent::Snapshot { status, .. } = ev {
                        return status.ahead;
                    }
                }
            }
            0
        });
        assert_eq!(ahead, 1);

        assert!(handle.send(GitRequest::Push {
            accept_new_ssh_host: false,
        }));
        let first_push_event = rt.block_on(async {
            tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .unwrap()
                .unwrap()
        });
        assert!(
            !matches!(first_push_event, GitEvent::Loading),
            "push 期间应保留当前 ahead/历史视图，只让推送按钮进入 busy"
        );
        let saw_clean_after_push = rt.block_on(async {
            let mut saw_clean = match &first_push_event {
                GitEvent::Snapshot { status, .. } => status.ahead == 0,
                _ => false,
            };
            let mut saw_success =
                matches!(first_push_event, GitEvent::PushFinished { success: true });
            for _ in 0..12 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await
                {
                    match ev {
                        GitEvent::Snapshot { status, .. } => {
                            saw_clean |= status.ahead == 0;
                        }
                        GitEvent::PushFinished { success } => {
                            saw_success |= success;
                        }
                        _ => {}
                    }
                    if saw_success && saw_clean {
                        return true;
                    }
                }
            }
            false
        });
        assert!(saw_clean_after_push, "push 后应刷新为 ahead=0");
    }

    /// 端到端验证 fs watcher：建仓 → SetCwd → 等首次 Snapshot → 外部写入新文件 →
    /// 等 watcher debounce → 验证自动收到含新文件的 Snapshot（无需手动 Refresh）。
    /// 用 ignore 标记是因为 2s+ debounce 会拖慢整套测试套件；CI 单跑 `--ignored` 验。
    #[test]
    #[ignore = "需要 ~6s 真实 fs watch；跑 `cargo test -- --ignored`"]
    fn watcher_triggers_auto_refresh_on_workspace_change() {
        use std::{fs, process::Command, time::Duration};
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "t"],
        ] {
            assert!(Command::new("git")
                .args(&args)
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("a.txt"), "hi\n").unwrap();
        Command::new("git")
            .args(["add", "a.txt"])
            .current_dir(repo)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "init"])
            .current_dir(repo)
            .status()
            .unwrap();

        let (handle, evt_rx) = spawn_git_worker("watcher-test").unwrap();
        assert!(handle.send(GitRequest::SetCwd(repo.to_path_buf())));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // 1) 收第一帧 Snapshot（干净状态）
        rt.block_on(async {
            for _ in 0..10 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await
                {
                    if matches!(ev, GitEvent::Snapshot { .. }) {
                        return;
                    }
                }
            }
            panic!("未收到初始 Snapshot");
        });

        // 2) 外部写入新文件 — 不主动 send Refresh，等 watcher 自动触发
        fs::write(repo.join("watched.txt"), "auto\n").unwrap();

        // 3) 等 watcher 自动派发的 Snapshot 含新文件（debounce 2s + emit 时间）
        let saw_new_file = rt.block_on(async {
            for _ in 0..15 {
                if let Ok(Ok(ev)) =
                    tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await
                {
                    if let GitEvent::Snapshot { status, .. } = ev {
                        if status.untracked.iter().any(|e| e.path == "watched.txt") {
                            return true;
                        }
                    }
                }
            }
            false
        });
        assert!(saw_new_file, "watcher 应自动触发 Refresh 并报告新文件");
    }
}
