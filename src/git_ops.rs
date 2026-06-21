//! git 子进程包装。所有命令同步阻塞调用，调用方应在独立 worker 线程里跑。
//! env / arg 配置参考 warp/app/src/util/git.rs:14 `run_git_command`：
//! `-c diff.autoRefreshIndex=false` 避免改 index、`GIT_OPTIONAL_LOCKS=0`
//! 防止与用户操作竞争锁。

use std::{
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// 通用 git 调用：成功 = stdout trim 后的字符串；失败 = stderr + stdout 合并的错误。
/// `git diff` 单独有一类"退出码 1 = 有差异"，调用方按需放宽（本模块仅对 diff 命令做）。
pub fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    run_git_configured(repo, args, |_| {})
}

fn run_git_configured<F>(repo: &Path, args: &[&str], configure: F) -> Result<String, String>
where
    F: FnOnce(&mut Command),
{
    run_git_program_configured("git", repo, args, configure)
}

fn run_git_diff(repo: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg("diff.autoRefreshIndex=false")
        .arg("-c")
        .arg("core.quotePath=false")
        .args(args)
        .current_dir(repo)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = command
        .output()
        .map_err(|e| format!("spawn git failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() || (output.status.code() == Some(1) && !stdout.trim().is_empty()) {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git {}: {}{}", args.join(" "), stderr, stdout))
    }
}

fn run_git_program_configured<P, F>(
    program: P,
    repo: &Path,
    args: &[&str],
    configure: F,
) -> Result<String, String>
where
    P: AsRef<OsStr>,
    F: FnOnce(&mut Command),
{
    let mut command = Command::new(program);
    command
        .arg("-c")
        .arg("diff.autoRefreshIndex=false")
        .arg("-c")
        .arg("core.quotePath=false")
        .args(args)
        .current_dir(repo)
        .env("GIT_OPTIONAL_LOCKS", "0");
    configure(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = command
        .output()
        .map_err(|e| format!("spawn git failed: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!("git {}: {}{}", args.join(" "), stderr, stdout))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SshHostKeyPolicy {
    Ask,
    AcceptNew,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SshHostKeyPrompt {
    pub message: String,
    pub host: Option<String>,
    pub fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitPushError {
    Failed(String),
    SshHostKeyPrompt(SshHostKeyPrompt),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SshEndpoint {
    host: String,
    port: Option<u16>,
}

impl SshEndpoint {
    fn known_hosts_target(&self) -> String {
        match self.port {
            Some(port) if port != 22 => format!("[{}]:{}", self.host, port),
            _ => self.host.clone(),
        }
    }
}

pub fn ssh_host_key_prompt_from_git_error(message: &str) -> Option<SshHostKeyPrompt> {
    let normalized = message.replace("\r\n", "\n");
    if normalized.contains("REMOTE HOST IDENTIFICATION HAS CHANGED") {
        return None;
    }

    let is_new_host_prompt = normalized.contains("The authenticity of host ")
        && normalized.contains("can't be established")
        && normalized.contains("key fingerprint is ")
        && normalized.contains("Are you sure you want to continue connecting");
    if !is_new_host_prompt {
        return None;
    }

    Some(SshHostKeyPrompt {
        host: normalized
            .lines()
            .find_map(extract_ssh_host_key_prompt_host),
        fingerprint: normalized
            .lines()
            .find_map(extract_ssh_host_key_prompt_fingerprint),
        message: normalized.trim().to_string(),
    })
}

fn extract_ssh_host_key_prompt_host(line: &str) -> Option<String> {
    let marker = "The authenticity of host '";
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let (host, _) = rest.split_once('\'')?;
    Some(host.to_string())
}

fn extract_ssh_host_key_prompt_fingerprint(line: &str) -> Option<String> {
    let marker = "key fingerprint is ";
    let start = line.find(marker)? + marker.len();
    Some(line[start..].trim().trim_end_matches('.').to_string())
}

fn git_ssh_command(existing: Option<&str>, host_key_policy: SshHostKeyPolicy) -> String {
    let base = existing
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ssh");
    let strict_host_key_checking = match host_key_policy {
        SshHostKeyPolicy::Ask => "ask",
        SshHostKeyPolicy::AcceptNew => "accept-new",
    };
    format!(
        "{base} -o BatchMode=yes -o NumberOfPasswordPrompts=0 -o StrictHostKeyChecking={strict_host_key_checking}"
    )
}

fn push_ssh_host_key_prompt(repo: &Path) -> Option<SshHostKeyPrompt> {
    let endpoint = push_ssh_endpoint(repo)?;
    if ssh_known_host_exists(&endpoint) {
        return None;
    }

    let display_host = endpoint.known_hosts_target();
    let fingerprint = scan_ssh_host_key_fingerprint(&endpoint);
    let mut message = format!("The authenticity of host '{display_host}' can't be established.");
    if let Some(fingerprint) = fingerprint.as_deref() {
        message.push_str(&format!("\nHost key fingerprint is {fingerprint}."));
    }
    message.push_str(
        "\nThis key is not known by any other names.\nAre you sure you want to continue connecting?",
    );

    Some(SshHostKeyPrompt {
        message,
        host: Some(display_host),
        fingerprint,
    })
}

fn push_ssh_endpoint(repo: &Path) -> Option<SshEndpoint> {
    let branch = detect_current_branch(repo).ok()?;
    if branch == "HEAD" || branch.is_empty() {
        return None;
    }
    let remote_key = format!("branch.{branch}.remote");
    let remote = run_git(repo, &["config", "--get", &remote_key]).ok()?;
    let remote = remote.trim();
    if remote.is_empty() {
        return None;
    }
    let url = run_git(repo, &["remote", "get-url", "--push", remote]).ok()?;
    parse_ssh_endpoint(url.trim())
}

fn parse_ssh_endpoint(url: &str) -> Option<SshEndpoint> {
    if let Some(rest) = url.strip_prefix("ssh://") {
        let authority = rest
            .split('/')
            .next()?
            .split('?')
            .next()?
            .split('#')
            .next()?;
        return parse_ssh_authority(authority);
    }
    if url.contains("://") {
        return None;
    }

    let (authority, path) = url.split_once(':')?;
    if authority.is_empty() || path.is_empty() || authority.contains('/') {
        return None;
    }
    parse_ssh_authority(authority)
}

fn parse_ssh_authority(authority: &str) -> Option<SshEndpoint> {
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    if let Some(rest) = host_port.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        let port = tail
            .strip_prefix(':')
            .and_then(|port| port.parse::<u16>().ok());
        return nonempty_host(host, port);
    }

    if let Some((host, port_text)) = host_port.rsplit_once(':') {
        if let Ok(port) = port_text.parse::<u16>() {
            return nonempty_host(host, Some(port));
        }
    }
    nonempty_host(host_port, None)
}

fn nonempty_host(host: &str, port: Option<u16>) -> Option<SshEndpoint> {
    let host = host.trim();
    if host.is_empty() {
        None
    } else {
        Some(SshEndpoint {
            host: host.to_string(),
            port,
        })
    }
}

fn ssh_known_host_exists(endpoint: &SshEndpoint) -> bool {
    Command::new("ssh-keygen")
        .args(["-F", &endpoint.known_hosts_target()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn scan_ssh_host_key_fingerprint(endpoint: &SshEndpoint) -> Option<String> {
    let mut keyscan = Command::new("ssh-keyscan");
    keyscan
        .args(["-T", "5", "-t", "ed25519,ecdsa,rsa"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(port) = endpoint.port {
        keyscan.args(["-p", &port.to_string()]);
    }
    let keyscan_output = keyscan.arg(&endpoint.host).output().ok()?;
    if keyscan_output.stdout.is_empty() {
        return None;
    }

    let mut keygen = Command::new("ssh-keygen")
        .args(["-lf", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    keygen
        .stdin
        .take()?
        .write_all(&keyscan_output.stdout)
        .ok()?;
    let keygen_output = keygen.wait_with_output().ok()?;
    if !keygen_output.status.success() {
        return None;
    }
    let fingerprints = String::from_utf8_lossy(&keygen_output.stdout);
    fingerprints
        .lines()
        .find_map(|line| line.split_whitespace().nth(1).map(str::to_string))
}

/// 仓库根目录（rev-parse --show-toplevel）。非 git 目录返回 Err。
pub fn repo_root(path: &Path) -> Result<PathBuf, String> {
    let out = run_git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(out.trim()))
}

/// 当前分支名（detached HEAD 时返回字面量 "HEAD"）。
/// warp util/git.rs:116 同款回落策略。
pub fn detect_current_branch(repo: &Path) -> Result<String, String> {
    match run_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(out) => Ok(out.trim().to_owned()),
        Err(_) => run_git(repo, &["branch", "--show-current"]).map(|out| out.trim().to_owned()),
    }
}

/// 展示用分支名：detached HEAD 时返回短 SHA 而非字面量 "HEAD"。
/// warp util/git.rs:133。
pub fn detect_current_branch_display(repo: &Path) -> Result<String, String> {
    let branch = detect_current_branch(repo)?;
    if branch == "HEAD" {
        run_git(repo, &["rev-parse", "--short", "HEAD"]).map(|s| s.trim().to_owned())
    } else {
        Ok(branch)
    }
}

/// 主分支检测：origin/HEAD → 回落候选列表。warp util/git.rs:157。
pub fn detect_main_branch(repo: &Path) -> Option<String> {
    if let Ok(out) = run_git(repo, &["symbolic-ref", "refs/remotes/origin/HEAD"]) {
        if let Some(name) = out.trim().strip_prefix("refs/remotes/") {
            return Some(name.to_owned());
        }
    }
    for cand in ["origin/main", "origin/master", "main", "master", "develop"] {
        if run_git(
            repo,
            &["rev-parse", "--verify", &format!("{cand}^{{commit}}")],
        )
        .is_ok()
        {
            return Some(cand.to_owned());
        }
    }
    None
}

// ── 状态采集 ────────────────────────────────────────────────────────────────

/// 单个文件的工作区状态分类。porcelain v2 XY 字段：
/// X = 暂存区相对 HEAD 的状态，Y = 工作区相对暂存区的状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitFileStage {
    /// 仅 staged（Y == '.'）
    Staged,
    /// 仅 unstaged（X == '.'）
    Unstaged,
    /// 同时 staged + unstaged（X 和 Y 都非 '.'）
    Both,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitFileEntry {
    pub path: String,
    /// 重命名 / 复制时的源路径。
    pub original_path: Option<String>,
    /// porcelain v2 X 字符（暂存区状态）。
    pub index_status: char,
    /// porcelain v2 Y 字符（工作区状态）。
    pub worktree_status: char,
    pub stage: GitFileStage,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GitStatusSnapshot {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// 已跟踪文件中暂存区有变化的。
    pub staged: Vec<GitFileEntry>,
    /// 已跟踪文件中工作区有变化的（含 X 和 Y 都非空的双重状态）。
    pub unstaged: Vec<GitFileEntry>,
    /// `?` 行。
    pub untracked: Vec<GitFileEntry>,
    /// `u` 行（merge conflict）。
    pub unmerged: Vec<GitFileEntry>,
}

/// 一次 `git status --porcelain=v2 -b --untracked-files=all` 的完整快照。
pub fn status(repo: &Path) -> Result<GitStatusSnapshot, String> {
    let out = run_git(
        repo,
        &["status", "--porcelain=v2", "-b", "--untracked-files=all"],
    )?;
    Ok(parse_porcelain_v2(&out))
}

/// 解析 porcelain v2 输出。格式见 git-status(1) "Porcelain Format Version 2"。
pub fn parse_porcelain_v2(text: &str) -> GitStatusSnapshot {
    let mut snap = GitStatusSnapshot::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            let name = rest.trim();
            snap.branch = if name == "(detached)" {
                None
            } else {
                Some(name.to_owned())
            };
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            snap.upstream = Some(rest.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // 形式："+N -M"
            let mut parts = rest.split_whitespace();
            if let Some(a) = parts.next() {
                snap.ahead = a.trim_start_matches('+').parse().unwrap_or(0);
            }
            if let Some(b) = parts.next() {
                snap.behind = b.trim_start_matches('-').parse().unwrap_or(0);
            }
        } else if let Some(rest) = line.strip_prefix("1 ") {
            if let Some(entry) = parse_ordinary_entry(rest) {
                classify_and_push(&mut snap, entry);
            }
        } else if let Some(rest) = line.strip_prefix("2 ") {
            if let Some(entry) = parse_renamed_entry(rest) {
                classify_and_push(&mut snap, entry);
            }
        } else if let Some(rest) = line.strip_prefix("u ") {
            if let Some(entry) = parse_unmerged_entry(rest) {
                snap.unmerged.push(entry);
            }
        } else if let Some(rest) = line.strip_prefix("? ") {
            snap.untracked.push(GitFileEntry {
                path: rest.trim().to_owned(),
                original_path: None,
                index_status: '?',
                worktree_status: '?',
                stage: GitFileStage::Unstaged,
            });
        }
    }
    snap
}

fn classify_and_push(snap: &mut GitStatusSnapshot, entry: GitFileEntry) {
    match entry.stage {
        GitFileStage::Staged => snap.staged.push(entry),
        GitFileStage::Unstaged => snap.unstaged.push(entry),
        GitFileStage::Both => {
            snap.staged.push(entry.clone());
            snap.unstaged.push(entry);
        }
    }
}

/// `1 XY <sub> <mH> <mI> <mW> <hH> <hI> <path>`
fn parse_ordinary_entry(rest: &str) -> Option<GitFileEntry> {
    let mut parts = rest.splitn(8, ' ');
    let xy = parts.next()?;
    let _sub = parts.next()?;
    let _mh = parts.next()?;
    let _mi = parts.next()?;
    let _mw = parts.next()?;
    let _hh = parts.next()?;
    let _hi = parts.next()?;
    let path = parts.next()?.to_owned();
    let (x, y) = xy_chars(xy)?;
    Some(GitFileEntry {
        path,
        original_path: None,
        index_status: x,
        worktree_status: y,
        stage: classify_xy(x, y),
    })
}

/// `2 XY <sub> <mH> <mI> <mW> <hH> <hI> <Rscore> <path>\t<orig>`
fn parse_renamed_entry(rest: &str) -> Option<GitFileEntry> {
    let mut parts = rest.splitn(9, ' ');
    let xy = parts.next()?;
    for _ in 0..7 {
        parts.next()?;
    }
    // 第 9 段是 "<path>\t<orig>"
    let tail = parts.next()?;
    let mut paths = tail.splitn(2, '\t');
    let path = paths.next()?.to_owned();
    let orig = paths.next().map(|s| s.to_owned());
    let (x, y) = xy_chars(xy)?;
    Some(GitFileEntry {
        path,
        original_path: orig,
        index_status: x,
        worktree_status: y,
        stage: classify_xy(x, y),
    })
}

/// `u XY <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>`
/// 共 10 个空格分隔字段（前缀 "u " 已被剥）：XY + sub + 4 mode + 3 hash + path。
fn parse_unmerged_entry(rest: &str) -> Option<GitFileEntry> {
    let mut parts = rest.splitn(10, ' ');
    let xy = parts.next()?;
    for _ in 0..8 {
        parts.next()?;
    }
    let path = parts.next()?.to_owned();
    let (x, y) = xy_chars(xy)?;
    Some(GitFileEntry {
        path,
        original_path: None,
        index_status: x,
        worktree_status: y,
        stage: GitFileStage::Both,
    })
}

fn xy_chars(xy: &str) -> Option<(char, char)> {
    let mut it = xy.chars();
    Some((it.next()?, it.next()?))
}

fn classify_xy(x: char, y: char) -> GitFileStage {
    match (x, y) {
        ('.', _) => GitFileStage::Unstaged,
        (_, '.') => GitFileStage::Staged,
        _ => GitFileStage::Both,
    }
}

// ── 最近 commit ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitFileChange {
    pub path: String,
    pub original_path: Option<String>,
    pub insertions: Option<u32>,
    pub deletions: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitRow {
    pub sha: String,
    pub full_sha: String,
    pub author: String,
    pub authored_at: String,
    pub decorations: String,
    pub summary: String,
    pub body: String,
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
    pub file_changes: Vec<CommitFileChange>,
}

/// `git log` 分页取 commit；使用 record separator 让正文可以安全包含换行。
pub fn recent_commits_page(repo: &Path, skip: usize, n: usize) -> Result<Vec<CommitRow>, String> {
    let arg_n = format!("-n{n}");
    let skip_arg = (skip > 0).then(|| format!("--skip={skip}"));
    let mut args = vec!["log", arg_n.as_str()];
    if let Some(skip_arg) = skip_arg.as_deref() {
        args.push(skip_arg);
    }
    args.extend([
        "--date=iso-strict",
        "--pretty=format:%x1f%h%x09%H%x09%an%x09%aI%x09%D%x09%s%x09%b",
        "--numstat",
    ]);
    let out = run_git(repo, &args)?;
    Ok(parse_recent_commits(&out))
}

/// `git log` 取最近若干 commit。
pub fn recent_commits(repo: &Path, n: usize) -> Result<Vec<CommitRow>, String> {
    recent_commits_page(repo, 0, n)
}

// ── Diff preview ───────────────────────────────────────────────────────────

pub const MAX_DIFF_PREVIEW_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GitDiffKind {
    Staged,
    Unstaged,
    Untracked,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GitDiffSelection {
    pub path: String,
    pub kind: GitDiffKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitDiffLineType {
    Context,
    Add,
    Delete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitDiffLine {
    pub line_type: GitDiffLineType,
    pub old_line_number: Option<usize>,
    pub new_line_number: Option<usize>,
    pub text: String,
    pub no_trailing_newline: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitDiffHunk {
    pub header: String,
    pub old_start_line: usize,
    pub old_line_count: usize,
    pub new_start_line: usize,
    pub new_line_count: usize,
    pub lines: Vec<GitDiffLine>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitFileDiff {
    pub path: String,
    pub kind: GitDiffKind,
    pub hunks: Vec<GitDiffHunk>,
    pub additions: usize,
    pub deletions: usize,
    pub is_binary: bool,
    pub is_too_large: bool,
    pub raw_size: usize,
    pub binary_message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UnifiedDiffHeader {
    old_start_line: usize,
    old_line_count: usize,
    new_start_line: usize,
    new_line_count: usize,
}

pub fn file_diff(repo: &Path, path: &str, kind: GitDiffKind) -> Result<GitFileDiff, String> {
    // pathspec 防 glob：文件名里的 * ? [ ] 不被 git 当通配匹配到别的文件。
    // Untracked 走 --no-index，path 是字面文件系统路径而非 pathspec，故不加前缀。
    let literal = format!(":(literal){path}");
    let args = match kind {
        GitDiffKind::Staged => vec![
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--cached",
            "--",
            &literal,
        ],
        GitDiffKind::Unstaged => vec!["diff", "--no-ext-diff", "--no-color", "--", &literal],
        GitDiffKind::Untracked => vec![
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--no-index",
            "--",
            "/dev/null",
            path,
        ],
    };
    let diff_output = run_git_diff(repo, &args)?;
    parse_unified_file_diff(path, kind, &diff_output)
}

pub fn parse_unified_file_diff(
    path: &str,
    kind: GitDiffKind,
    diff_output: &str,
) -> Result<GitFileDiff, String> {
    let raw_size = diff_output.len();
    if raw_size > MAX_DIFF_PREVIEW_BYTES {
        return Ok(GitFileDiff {
            path: path.to_string(),
            kind,
            hunks: Vec::new(),
            additions: 0,
            deletions: 0,
            is_binary: false,
            is_too_large: true,
            raw_size,
            binary_message: None,
        });
    }

    if let Some(message) = binary_diff_message(diff_output) {
        return Ok(GitFileDiff {
            path: path.to_string(),
            kind,
            hunks: Vec::new(),
            additions: 0,
            deletions: 0,
            is_binary: true,
            is_too_large: false,
            raw_size,
            binary_message: Some(message.to_string()),
        });
    }

    let lines: Vec<&str> = diff_output.lines().collect();
    let mut hunks = Vec::new();
    let mut additions = 0;
    let mut deletions = 0;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if !line.starts_with("@@") {
            i += 1;
            continue;
        }

        let header = parse_unified_diff_header(line)?;
        let mut hunk_lines: Vec<GitDiffLine> = Vec::new();
        let mut old_line = header.old_start_line;
        let mut new_line = header.new_start_line;
        i += 1;

        while i < lines.len() && !lines[i].starts_with("@@") && !lines[i].starts_with("diff --git")
        {
            let content_line = lines[i];
            if content_line == "\\ No newline at end of file" {
                if let Some(last) = hunk_lines.last_mut() {
                    last.no_trailing_newline = true;
                }
                i += 1;
                continue;
            }

            let Some(prefix) = content_line.as_bytes().first().copied() else {
                i += 1;
                continue;
            };
            let text = content_line.get(1..).unwrap_or_default().to_string();
            match prefix {
                b' ' => {
                    hunk_lines.push(GitDiffLine {
                        line_type: GitDiffLineType::Context,
                        old_line_number: Some(old_line),
                        new_line_number: Some(new_line),
                        text,
                        no_trailing_newline: false,
                    });
                    old_line += 1;
                    new_line += 1;
                }
                b'+' => {
                    hunk_lines.push(GitDiffLine {
                        line_type: GitDiffLineType::Add,
                        old_line_number: None,
                        new_line_number: Some(new_line),
                        text,
                        no_trailing_newline: false,
                    });
                    additions += 1;
                    new_line += 1;
                }
                b'-' => {
                    hunk_lines.push(GitDiffLine {
                        line_type: GitDiffLineType::Delete,
                        old_line_number: Some(old_line),
                        new_line_number: None,
                        text,
                        no_trailing_newline: false,
                    });
                    deletions += 1;
                    old_line += 1;
                }
                _ => {}
            }
            i += 1;
        }

        hunks.push(GitDiffHunk {
            header: line.to_string(),
            old_start_line: header.old_start_line,
            old_line_count: header.old_line_count,
            new_start_line: header.new_start_line,
            new_line_count: header.new_line_count,
            lines: hunk_lines,
        });
    }

    Ok(GitFileDiff {
        path: path.to_string(),
        kind,
        hunks,
        additions,
        deletions,
        is_binary: false,
        is_too_large: false,
        raw_size,
        binary_message: None,
    })
}

fn binary_diff_message(diff_output: &str) -> Option<&str> {
    diff_output.lines().find(|line| {
        (line.starts_with("Binary files ") && line.contains(" differ"))
            || *line == "GIT binary patch"
    })
}

fn parse_unified_diff_header(header_line: &str) -> Result<UnifiedDiffHeader, String> {
    let header_parts: Vec<&str> = header_line.split_whitespace().take(3).collect();
    if header_parts.len() < 3 {
        return Err(format!("invalid unified diff header: {header_line}"));
    }
    let old_range = header_parts[1]
        .strip_prefix('-')
        .ok_or_else(|| format!("invalid old range in diff header: {header_line}"))?;
    let new_range = header_parts[2]
        .strip_prefix('+')
        .ok_or_else(|| format!("invalid new range in diff header: {header_line}"))?;
    let (old_start_line, old_line_count) = parse_diff_range(old_range)?;
    let (new_start_line, new_line_count) = parse_diff_range(new_range)?;
    Ok(UnifiedDiffHeader {
        old_start_line,
        old_line_count,
        new_start_line,
        new_line_count,
    })
}

fn parse_diff_range(range: &str) -> Result<(usize, usize), String> {
    if let Some((start, count)) = range.split_once(',') {
        Ok((
            start
                .parse()
                .map_err(|_| format!("invalid diff range start: {range}"))?,
            count
                .parse()
                .map_err(|_| format!("invalid diff range count: {range}"))?,
        ))
    } else {
        Ok((
            range
                .parse()
                .map_err(|_| format!("invalid diff range: {range}"))?,
            1,
        ))
    }
}

fn parse_recent_commits(text: &str) -> Vec<CommitRow> {
    if text.contains('\x1f') {
        return text
            .split('\x1f')
            .filter_map(parse_recent_commit_record)
            .collect();
    }

    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let sha = parts.next()?.trim();
            let author = parts.next().unwrap_or("").trim();
            let decorations = parts.next().unwrap_or("").trim();
            let summary = parts.next().unwrap_or("").trim();
            if sha.is_empty() {
                None
            } else {
                Some(CommitRow {
                    sha: sha.to_owned(),
                    full_sha: sha.to_owned(),
                    author: author.to_owned(),
                    authored_at: String::new(),
                    decorations: decorations.to_owned(),
                    summary: summary.to_owned(),
                    body: String::new(),
                    files_changed: 0,
                    insertions: 0,
                    deletions: 0,
                    file_changes: Vec::new(),
                })
            }
        })
        .collect()
}

fn parse_recent_commit_record(record: &str) -> Option<CommitRow> {
    let record = record.trim_matches('\n');
    if record.trim().is_empty() {
        return None;
    }
    let mut parts = record.splitn(7, '\t');
    let sha = parts.next()?.trim();
    let full_sha = parts.next().unwrap_or("").trim();
    let author = parts.next().unwrap_or("").trim();
    let authored_at = parts.next().unwrap_or("").trim();
    let decorations = parts.next().unwrap_or("").trim();
    let summary = parts.next().unwrap_or("").trim();
    let body_and_stat = parts.next().unwrap_or("");
    if sha.is_empty() {
        return None;
    }
    let (body, files_changed, insertions, deletions, file_changes) =
        parse_commit_body_and_stats(body_and_stat);
    Some(CommitRow {
        sha: sha.to_owned(),
        full_sha: if full_sha.is_empty() {
            sha.to_owned()
        } else {
            full_sha.to_owned()
        },
        author: author.to_owned(),
        authored_at: authored_at.to_owned(),
        decorations: decorations.to_owned(),
        summary: summary.to_owned(),
        body,
        files_changed,
        insertions,
        deletions,
        file_changes,
    })
}

fn parse_commit_body_and_stats(text: &str) -> (String, u32, u32, u32, Vec<CommitFileChange>) {
    let mut body_lines = Vec::new();
    let mut file_changes = Vec::new();
    let mut numstat_insertions = 0;
    let mut numstat_deletions = 0;
    let mut shortstat = None;

    for line in text.lines() {
        if let Some(change) = parse_numstat_line(line) {
            if let Some(insertions) = change.insertions {
                numstat_insertions += insertions;
            }
            if let Some(deletions) = change.deletions {
                numstat_deletions += deletions;
            }
            file_changes.push(change);
        } else if let Some(parsed_shortstat) = parse_shortstat_line(line) {
            shortstat = Some(parsed_shortstat);
        } else {
            body_lines.push(line);
        }
    }

    let body = body_lines.join("\n").trim().to_owned();
    if file_changes.is_empty() {
        let (files_changed, insertions, deletions) = shortstat.unwrap_or_default();
        (body, files_changed, insertions, deletions, file_changes)
    } else {
        (
            body,
            file_changes.len() as u32,
            numstat_insertions,
            numstat_deletions,
            file_changes,
        )
    }
}

fn parse_numstat_line(line: &str) -> Option<CommitFileChange> {
    let mut parts = line.splitn(3, '\t');
    let insertions = parse_numstat_count(parts.next()?)?;
    let deletions = parse_numstat_count(parts.next()?)?;
    let path = parts.next()?.trim();
    if path.is_empty() {
        return None;
    }
    Some(CommitFileChange {
        path: path.to_string(),
        original_path: None,
        insertions,
        deletions,
    })
}

fn parse_numstat_count(text: &str) -> Option<Option<u32>> {
    let text = text.trim();
    if text == "-" {
        Some(None)
    } else {
        text.parse::<u32>().ok().map(Some)
    }
}

fn parse_shortstat_line(line: &str) -> Option<(u32, u32, u32)> {
    let mut files_changed = 0;
    let mut insertions = 0;
    let mut deletions = 0;
    let mut saw_stat = false;
    for part in line.split(',').map(str::trim) {
        if part.contains("file changed") || part.contains("files changed") {
            files_changed = leading_u32(part)?;
            saw_stat = true;
        } else if part.contains("insertion") {
            insertions = leading_u32(part)?;
            saw_stat = true;
        } else if part.contains("deletion") {
            deletions = leading_u32(part)?;
            saw_stat = true;
        }
    }
    saw_stat.then_some((files_changed, insertions, deletions))
}

fn leading_u32(text: &str) -> Option<u32> {
    let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

// ── 修改操作 ────────────────────────────────────────────────────────────────

/// 跑一条以 pathspec 收尾的 git 命令。每个 path 包 `:(literal)` 前缀，
/// 防止文件名里的 glob 元字符(* ? [])被 git 当通配解释而误伤其它文件。
fn run_git_with_paths(repo: &Path, prefix: &[&str], paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let literals: Vec<String> = paths.iter().map(|p| format!(":(literal){p}")).collect();
    let mut args: Vec<&str> = prefix.to_vec();
    args.extend(literals.iter().map(String::as_str));
    run_git(repo, &args).map(|_| ())
}

/// `git add -- <paths>...`。
pub fn stage(repo: &Path, paths: &[String]) -> Result<(), String> {
    run_git_with_paths(repo, &["add", "--"], paths)
}

/// `git restore --staged -- <paths>...`。
pub fn unstage(repo: &Path, paths: &[String]) -> Result<(), String> {
    run_git_with_paths(repo, &["restore", "--staged", "--"], paths)
}

/// `git restore -- <paths>...`，丢弃 tracked 文件的未暂存工作区改动。
pub fn discard_worktree_changes(repo: &Path, paths: &[String]) -> Result<(), String> {
    run_git_with_paths(repo, &["restore", "--"], paths)
}

/// `git clean -ff -d -- <paths>...`，从磁盘删除未跟踪文件/目录（-ff 连同内嵌 git 仓库）。
pub fn delete_untracked(repo: &Path, paths: &[String]) -> Result<(), String> {
    run_git_with_paths(repo, &["clean", "-ff", "-d", "--"], paths)
}

/// 追加路径到仓库根目录 `.gitignore`。已存在的规则不会重复写入。
pub fn add_to_gitignore(repo: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let gitignore_path = repo.join(".gitignore");
    let existing = match fs::read_to_string(&gitignore_path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(format!("read .gitignore failed: {err}")),
    };
    let existing_lines: std::collections::BTreeSet<&str> = existing.lines().collect();
    let mut to_append = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for path in paths
        .iter()
        .map(|path| path.trim())
        .filter(|path| !path.is_empty())
    {
        if existing_lines.contains(path) || !seen.insert(path.to_string()) {
            continue;
        }
        to_append.push(path.to_string());
    }
    if to_append.is_empty() {
        return Ok(());
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&gitignore_path)
        .map_err(|err| format!("open .gitignore failed: {err}"))?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        file.write_all(b"\n")
            .map_err(|err| format!("write .gitignore failed: {err}"))?;
    }
    for path in to_append {
        writeln!(file, "{path}").map_err(|err| format!("write .gitignore failed: {err}"))?;
    }
    Ok(())
}

/// `git commit -m <message>`，可选 `--amend`。
pub fn commit(repo: &Path, message: &str, amend: bool) -> Result<(), String> {
    let mut args: Vec<&str> = vec!["commit", "-m", message];
    if amend {
        args.push("--amend");
    }
    run_git(repo, &args).map(|_| ())
}

/// `git push` 到当前分支配置的 upstream。
pub fn push(repo: &Path, ssh_host_key_policy: SshHostKeyPolicy) -> Result<(), GitPushError> {
    if ssh_host_key_policy == SshHostKeyPolicy::Ask {
        if let Some(prompt) = push_ssh_host_key_prompt(repo) {
            return Err(GitPushError::SshHostKeyPrompt(prompt));
        }
    }

    let existing = std::env::var("GIT_SSH_COMMAND").ok();
    let ssh_command = git_ssh_command(existing.as_deref(), ssh_host_key_policy);
    let result = run_git_configured(repo, &["push"], |command| {
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_SSH_COMMAND", ssh_command);
    });

    result.map(|_| ()).map_err(|message| {
        if ssh_host_key_policy == SshHostKeyPolicy::Ask {
            if let Some(prompt) = ssh_host_key_prompt_from_git_error(&message) {
                return GitPushError::SshHostKeyPrompt(prompt);
            }
        }
        GitPushError::Failed(message)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_branch_header_and_ab() {
        let out = "\
# branch.oid abc123
# branch.head feature/x
# branch.upstream origin/feature/x
# branch.ab +3 -1
";
        let s = parse_porcelain_v2(out);
        assert_eq!(s.branch.as_deref(), Some("feature/x"));
        assert_eq!(s.upstream.as_deref(), Some("origin/feature/x"));
        assert_eq!(s.ahead, 3);
        assert_eq!(s.behind, 1);
    }

    #[test]
    fn parse_detached_head() {
        let out = "# branch.head (detached)\n";
        let s = parse_porcelain_v2(out);
        assert!(s.branch.is_none());
    }

    #[test]
    fn detects_ssh_host_key_prompt_from_git_error() {
        let message = "\
git push: The authenticity of host '[192.0.2.1]:2222 ([192.0.2.1]:2222)' can't be established.
ED25519 key fingerprint is SHA256:l64TJvF7FIzVXIX4By9bAG7HZLFfmY2KtjNcH4amaJs.
This key is not known by any other names.
Are you sure you want to continue connecting (yes/no/[fingerprint])?
";

        let prompt = ssh_host_key_prompt_from_git_error(message).expect("should detect prompt");

        assert_eq!(
            prompt.host.as_deref(),
            Some("[192.0.2.1]:2222 ([192.0.2.1]:2222)")
        );
        assert_eq!(
            prompt.fingerprint.as_deref(),
            Some("SHA256:l64TJvF7FIzVXIX4By9bAG7HZLFfmY2KtjNcH4amaJs")
        );
        assert!(prompt.message.contains("Are you sure"));
    }

    #[test]
    fn does_not_offer_accept_new_for_changed_ssh_host_key() {
        let message = "\
git push: @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
Host key verification failed.
";

        assert!(ssh_host_key_prompt_from_git_error(message).is_none());
    }

    #[test]
    #[cfg(unix)]
    fn git_command_does_not_inherit_stdin_from_launch_terminal() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let fake_git = tmp.path().join("git");
        fs::write(
            &fake_git,
            "#!/bin/sh\nif IFS= read -r line; then echo \"read stdin: $line\" >&2; exit 7; fi\necho no stdin\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&fake_git).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_git, perms).unwrap();

        let stdin_path = tmp.path().join("stdin.txt");
        fs::write(&stdin_path, "yes\n").unwrap();
        let stdin_file = fs::File::open(stdin_path).unwrap();

        let output = run_git_program_configured(&fake_git, tmp.path(), &["push"], |command| {
            command.stdin(Stdio::from(stdin_file));
        })
        .expect("git command should see closed stdin, not inherited terminal input");

        assert_eq!(output.trim(), "no stdin");
    }

    #[test]
    fn parse_ssh_endpoint_supports_ssh_url_ports_and_scp_style() {
        assert_eq!(
            parse_ssh_endpoint("ssh://git@192.0.2.1:2222/repo.git"),
            Some(SshEndpoint {
                host: "192.0.2.1".into(),
                port: Some(2222),
            })
        );
        assert_eq!(
            parse_ssh_endpoint("git@example.com:owner/repo.git"),
            Some(SshEndpoint {
                host: "example.com".into(),
                port: None,
            })
        );
        assert_eq!(
            parse_ssh_endpoint("https://example.com/owner/repo.git"),
            None
        );
    }

    #[test]
    fn ssh_endpoint_formats_known_hosts_target_for_custom_ports() {
        assert_eq!(
            SshEndpoint {
                host: "192.0.2.1".into(),
                port: Some(2222),
            }
            .known_hosts_target(),
            "[192.0.2.1]:2222"
        );
        assert_eq!(
            SshEndpoint {
                host: "example.com".into(),
                port: Some(22),
            }
            .known_hosts_target(),
            "example.com"
        );
    }

    #[test]
    fn git_ssh_command_disables_terminal_prompts() {
        let command = git_ssh_command(Some("ssh -i key"), SshHostKeyPolicy::Ask);
        assert!(command.contains("ssh -i key"));
        assert!(command.contains("BatchMode=yes"));
        assert!(command.contains("NumberOfPasswordPrompts=0"));
        assert!(command.contains("StrictHostKeyChecking=ask"));

        let command = git_ssh_command(None, SshHostKeyPolicy::AcceptNew);
        assert!(command.contains("StrictHostKeyChecking=accept-new"));
    }

    #[test]
    fn parse_ordinary_staged_and_unstaged() {
        // 1 M. N... 100644 100644 100644 hash1 hash2 src/foo.rs  → staged
        // 1 .M N... 100644 100644 100644 hash1 hash2 src/bar.rs  → unstaged
        // 1 MM N... 100644 100644 100644 hash1 hash2 src/baz.rs  → both
        let out = "\
1 M. N... 100644 100644 100644 h1 h2 src/foo.rs
1 .M N... 100644 100644 100644 h1 h2 src/bar.rs
1 MM N... 100644 100644 100644 h1 h2 src/baz.rs
";
        let s = parse_porcelain_v2(out);
        assert_eq!(s.staged.len(), 2, "foo + baz");
        assert_eq!(s.unstaged.len(), 2, "bar + baz");
        assert!(s.staged.iter().any(|e| e.path == "src/foo.rs"));
        assert!(s.staged.iter().any(|e| e.path == "src/baz.rs"));
        assert!(s.unstaged.iter().any(|e| e.path == "src/bar.rs"));
    }

    #[test]
    fn parse_untracked_and_unmerged() {
        let out = "\
? newfile.txt
u UU N... 100644 100644 100644 100644 h1 h2 h3 conflict.rs
";
        let s = parse_porcelain_v2(out);
        assert_eq!(s.untracked.len(), 1);
        assert_eq!(s.untracked[0].path, "newfile.txt");
        assert_eq!(s.unmerged.len(), 1);
        assert_eq!(s.unmerged[0].path, "conflict.rs");
    }

    #[test]
    fn parse_renamed_entry_path_and_orig() {
        // 2 R. N... 100644 100644 100644 h1 h2 R100 newpath\toldpath
        let out = "2 R. N... 100644 100644 100644 h1 h2 R100 newpath\toldpath\n";
        let s = parse_porcelain_v2(out);
        assert_eq!(s.staged.len(), 1);
        assert_eq!(s.staged[0].path, "newpath");
        assert_eq!(s.staged[0].original_path.as_deref(), Some("oldpath"));
    }

    #[test]
    fn parse_recent_commits_includes_author_and_decorations() {
        let out = "\
abc123\tmatt\tHEAD -> main, origin/main\tfeat: add git history
def456\tlee\t\tfix: refresh status
";
        let rows = parse_recent_commits(out);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].sha, "abc123");
        assert_eq!(rows[0].author, "matt");
        assert_eq!(rows[0].decorations, "HEAD -> main, origin/main");
        assert_eq!(rows[0].summary, "feat: add git history");
        assert_eq!(rows[1].author, "lee");
        assert!(rows[1].decorations.is_empty());
    }

    #[test]
    fn parse_recent_commits_includes_hover_details_and_shortstat() {
        let out = "\
\x1fabc123\tabc123ffffffffffffffffffffffffffffffffffff\tmatt\t2026-05-16T09:18:00-07:00\tHEAD -> main\tfix: use native serial transport\tReplace external screen/cu serial sessions with an in-process serialport runtime.

Also keep remote/serial tab labels stable.

 7 files changed, 919 insertions(+), 533 deletions(-)

\x1fdef456\tdef456ffffffffffffffffffffffffffffffffffff\tlee\t2026-05-17T10:30:00-07:00\torigin/main\tfix: refresh status\t
 1 file changed, 2 insertions(+)
";
        let rows = parse_recent_commits(out);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].sha, "abc123");
        assert_eq!(
            rows[0].full_sha,
            "abc123ffffffffffffffffffffffffffffffffffff"
        );
        assert_eq!(rows[0].authored_at, "2026-05-16T09:18:00-07:00");
        assert_eq!(
            rows[0].body,
            "Replace external screen/cu serial sessions with an in-process serialport runtime.\n\nAlso keep remote/serial tab labels stable."
        );
        assert_eq!(rows[0].files_changed, 7);
        assert_eq!(rows[0].insertions, 919);
        assert_eq!(rows[0].deletions, 533);
        assert_eq!(rows[1].decorations, "origin/main");
        assert_eq!(rows[1].files_changed, 1);
        assert_eq!(rows[1].insertions, 2);
        assert_eq!(rows[1].deletions, 0);
    }

    #[test]
    fn parse_recent_commits_includes_changed_files_from_numstat() {
        let out = "\
\x1fabc123\tabc123ffffffffffffffffffffffffffffffffffff\tmatt\t2026-05-16T09:18:00-07:00\tHEAD -> main\tfix: show commit files\tExplain the change.

12\t3\tsrc/main.rs
0\t4\tlocales/en.yml
-\t-\tassets/AppIcon.icns
";
        let rows = parse_recent_commits(out);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].files_changed, 3);
        assert_eq!(rows[0].insertions, 12);
        assert_eq!(rows[0].deletions, 7);
        assert_eq!(
            rows[0]
                .file_changes
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/main.rs", "locales/en.yml", "assets/AppIcon.icns"]
        );
    }

    #[test]
    fn parse_unified_diff_hunks_tracks_line_numbers_and_no_newline_marker() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
index 1111111..2222222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,3 +10,4 @@ fn main() {
 context
-old
+new
+extra
\\ No newline at end of file
";

        let file_diff = parse_unified_file_diff("src/main.rs", GitDiffKind::Unstaged, diff)
            .expect("diff should parse");

        assert_eq!(file_diff.hunks.len(), 1);
        let hunk = &file_diff.hunks[0];
        assert_eq!(hunk.old_start_line, 10);
        assert_eq!(hunk.old_line_count, 3);
        assert_eq!(hunk.new_start_line, 10);
        assert_eq!(hunk.new_line_count, 4);
        assert_eq!(hunk.lines.len(), 4);
        assert_eq!(hunk.lines[0].line_type, GitDiffLineType::Context);
        assert_eq!(hunk.lines[0].old_line_number, Some(10));
        assert_eq!(hunk.lines[0].new_line_number, Some(10));
        assert_eq!(hunk.lines[1].line_type, GitDiffLineType::Delete);
        assert_eq!(hunk.lines[1].old_line_number, Some(11));
        assert_eq!(hunk.lines[1].new_line_number, None);
        assert_eq!(hunk.lines[2].line_type, GitDiffLineType::Add);
        assert_eq!(hunk.lines[2].old_line_number, None);
        assert_eq!(hunk.lines[2].new_line_number, Some(11));
        assert!(hunk.lines[3].no_trailing_newline);
        assert_eq!(file_diff.additions, 2);
        assert_eq!(file_diff.deletions, 1);
    }

    #[test]
    fn parse_unified_diff_marks_binary_output_without_hunks() {
        let diff = "Binary files a/assets/icon.png and b/assets/icon.png differ\n";

        let file_diff = parse_unified_file_diff("assets/icon.png", GitDiffKind::Unstaged, diff)
            .expect("binary diff should parse as placeholder");

        assert!(file_diff.is_binary);
        assert!(file_diff.hunks.is_empty());
        assert_eq!(file_diff.binary_message.as_deref(), Some(diff.trim()));
    }

    #[test]
    fn untracked_file_diff_uses_no_index_and_allows_exit_one() {
        use std::fs;
        use std::process::Command;

        let tmp = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(tmp.path())
            .status()
            .unwrap();
        fs::write(tmp.path().join("new.txt"), "hello\n").unwrap();

        let diff = file_diff(tmp.path(), "new.txt", GitDiffKind::Untracked)
            .expect("untracked diff should be returned despite git diff --no-index exit 1");

        assert_eq!(diff.path, "new.txt");
        assert_eq!(diff.kind, GitDiffKind::Untracked);
        assert_eq!(diff.additions, 1);
        assert_eq!(diff.deletions, 0);
        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.hunks[0].lines[0].line_type, GitDiffLineType::Add);
        assert_eq!(diff.hunks[0].lines[0].text, "hello");
    }

    #[test]
    fn discard_worktree_changes_restores_tracked_file() {
        use std::fs;
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }

        let tmp = tempfile::tempdir().expect("mktempdir");
        let repo = tmp.path();
        assert!(Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init")
            .success());
        for (k, v) in [("user.email", "t@t"), ("user.name", "t")] {
            assert!(Command::new("git")
                .args(["config", k, v])
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("a.txt"), "hello\n").unwrap();
        stage(repo, &["a.txt".to_string()]).expect("stage a");
        commit(repo, "init", false).expect("commit");

        fs::write(repo.join("a.txt"), "changed\n").unwrap();
        assert!(
            status(repo)
                .expect("status before discard")
                .unstaged
                .iter()
                .any(|entry| entry.path == "a.txt"),
            "a.txt should start with an unstaged worktree change"
        );

        discard_worktree_changes(repo, &["a.txt".to_string()]).expect("discard change");

        assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "hello\n");
        assert!(
            status(repo)
                .expect("status after discard")
                .unstaged
                .is_empty(),
            "tracked worktree change should be gone"
        );
    }

    #[test]
    fn discard_does_not_glob_match_sibling_with_same_prefix() {
        use std::fs;
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }

        let tmp = tempfile::tempdir().expect("mktempdir");
        let repo = tmp.path();
        assert!(Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init")
            .success());
        for (k, v) in [("user.email", "t@t"), ("user.name", "t")] {
            assert!(Command::new("git")
                .args(["config", k, v])
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        // "a*.txt" 含 glob 元字符；若被当通配，会连 "ab.txt" 一起丢弃。
        fs::write(repo.join("a*.txt"), "orig\n").unwrap();
        fs::write(repo.join("ab.txt"), "orig\n").unwrap();
        stage(repo, &["a*.txt".to_string(), "ab.txt".to_string()]).expect("stage");
        commit(repo, "init", false).expect("commit");
        fs::write(repo.join("a*.txt"), "changed\n").unwrap();
        fs::write(repo.join("ab.txt"), "changed\n").unwrap();

        discard_worktree_changes(repo, &["a*.txt".to_string()]).expect("discard");

        assert_eq!(fs::read_to_string(repo.join("a*.txt")).unwrap(), "orig\n");
        assert_eq!(
            fs::read_to_string(repo.join("ab.txt")).unwrap(),
            "changed\n",
            "同前缀的 ab.txt 不该被 glob 误丢弃"
        );
    }

    #[test]
    fn delete_untracked_removes_only_the_literal_path() {
        use std::fs;
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }

        let tmp = tempfile::tempdir().expect("mktempdir");
        let repo = tmp.path();
        assert!(Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init")
            .success());
        // 两个 untracked 文件，"del*.txt" 含 glob；若被当通配，会连 "del1.txt" 一起删。
        fs::write(repo.join("del*.txt"), "x").unwrap();
        fs::write(repo.join("del1.txt"), "y").unwrap();

        delete_untracked(repo, &["del*.txt".to_string()]).expect("delete untracked");

        assert!(!repo.join("del*.txt").exists(), "目标 untracked 应被删盘");
        assert!(
            repo.join("del1.txt").exists(),
            "同前缀的 del1.txt 不该被 glob 误删"
        );
        let untracked = status(repo).expect("status").untracked;
        assert!(untracked.iter().any(|e| e.path == "del1.txt"));
        assert!(!untracked.iter().any(|e| e.path == "del*.txt"));
    }

    #[test]
    fn file_diff_does_not_glob_match_sibling_with_same_prefix() {
        use std::fs;
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }

        let tmp = tempfile::tempdir().expect("mktempdir");
        let repo = tmp.path();
        assert!(Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init")
            .success());
        for (k, v) in [("user.email", "t@t"), ("user.name", "t")] {
            assert!(Command::new("git")
                .args(["config", k, v])
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("g*.txt"), "base\n").unwrap();
        fs::write(repo.join("g1.txt"), "base\n").unwrap();
        stage(repo, &["g*.txt".to_string(), "g1.txt".to_string()]).expect("stage");
        commit(repo, "init", false).expect("commit");
        // 两文件各加一条可识别行；glob 误匹配会把 g1.txt 的 "ONE" 混进 g*.txt 的 diff。
        fs::write(repo.join("g*.txt"), "base\nSTAR\n").unwrap();
        fs::write(repo.join("g1.txt"), "base\nONE\n").unwrap();

        let diff = file_diff(repo, "g*.txt", GitDiffKind::Unstaged).expect("diff");
        let texts: Vec<&str> = diff
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter().map(|l| l.text.as_str()))
            .collect();
        assert!(texts.iter().any(|t| t.contains("STAR")), "应含目标文件的改动");
        assert!(
            !texts.iter().any(|t| t.contains("ONE")),
            "同前缀的 g1.txt 改动不该混进来"
        );
    }

    #[test]
    fn add_to_gitignore_appends_missing_untracked_paths_once() {
        use std::fs;
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }

        let tmp = tempfile::tempdir().expect("mktempdir");
        let repo = tmp.path();
        assert!(Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init")
            .success());

        fs::write(repo.join(".gitignore"), "target/\n\n").unwrap();

        add_to_gitignore(
            repo,
            &[
                "target/".to_string(),
                "未跟踪.log".to_string(),
                "nested/file.txt".to_string(),
            ],
        )
        .expect("append gitignore entries");

        let text = fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert_eq!(text, "target/\n\n未跟踪.log\nnested/file.txt\n");
    }

    /// 端到端：临时建一个 git 仓库，跑全套 git_ops 流程。
    /// 系统没装 git 时跳过。
    #[test]
    fn smoke_real_repo_status_and_stage() {
        use std::fs;
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let tmp = tempfile::tempdir().expect("mktempdir");
        let repo = tmp.path();
        // 用低级命令初始化，避免 user.name/email 缺失阻断 commit
        let init = Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init");
        assert!(init.success());
        for (k, v) in [("user.email", "t@t"), ("user.name", "t")] {
            Command::new("git")
                .args(["config", k, v])
                .current_dir(repo)
                .status()
                .unwrap();
        }
        fs::write(repo.join("a.txt"), "hello\n").unwrap();
        stage(repo, &["a.txt".to_string()]).expect("stage a");
        commit(repo, "init", false).expect("commit");

        fs::write(repo.join("a.txt"), "hello\nchanged\n").unwrap();
        fs::write(repo.join("b.txt"), "new\n").unwrap();
        stage(repo, &["b.txt".to_string()]).expect("stage b");

        let snap = status(repo).expect("status");
        assert_eq!(snap.branch.as_deref(), Some("main"));
        assert!(
            snap.staged.iter().any(|e| e.path == "b.txt"),
            "b.txt 应在 staged"
        );
        assert!(
            snap.unstaged.iter().any(|e| e.path == "a.txt"),
            "a.txt 应在 unstaged"
        );

        let log = recent_commits(repo, 5).expect("log");
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].summary, "init");
        assert_eq!(log[0].author, "t");

        let branch = detect_current_branch_display(repo).unwrap();
        assert_eq!(branch, "main");
    }

    #[test]
    fn status_preserves_non_ascii_paths_when_git_quote_path_is_enabled() {
        use std::fs;
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }

        let tmp = tempfile::tempdir().expect("mktempdir");
        let repo = tmp.path();
        assert!(Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init")
            .success());
        for args in [
            ["config", "user.email", "t@t"],
            ["config", "user.name", "t"],
            ["config", "core.quotePath", "true"],
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }

        let tracked = "中文路径.md";
        let untracked = "未跟踪.md";
        fs::write(repo.join(tracked), "hello\n").unwrap();
        stage(repo, &[tracked.to_string()]).expect("stage tracked");
        commit(repo, "init", false).expect("commit");

        fs::write(repo.join(tracked), "hello\nchanged\n").unwrap();
        fs::write(repo.join(untracked), "new\n").unwrap();

        let snap = status(repo).expect("status");
        assert!(
            snap.unstaged.iter().any(|entry| entry.path == tracked),
            "tracked Chinese path should not be octal-quoted: {:?}",
            snap.unstaged
        );
        assert!(
            snap.untracked.iter().any(|entry| entry.path == untracked),
            "untracked Chinese path should not be octal-quoted: {:?}",
            snap.untracked
        );
    }
}
