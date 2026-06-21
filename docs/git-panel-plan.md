# 本地终端 git 面板 — 技术方案

目标：给 native-shell 本地终端加一个 git 状态面板，作用域 = 当前 tab cwd。
架构：纯 Rust / warpui，**不涉及已废弃的 nexshell/src React 与 Tauri 桥**。
原则：每一步参考 warp 源码（仓库已 mirror 在 `sshtool/warp/`），不造轮子。

## 已确认决策

- **作用域**：只跟当前 tab 的本地 cwd 联动；切目录自动刷新。
- **远程主机**：SSH tab 不出现 git 面板；按钮 disabled / 隐藏。
- **数据采集**：全程 subprocess 调 `git` 二进制，**不引入 git2-rs / libgit2**
  （Warp 自己也这么做，原因之一：需要 `GIT_OPTIONAL_LOCKS=0`、`-c diff.autoRefreshIndex=false`
  这种细粒度 env，libgit2 给不了）。
- **UI 框架**：全 warpui（GPUI）。commit / push 对话框照搬
  `warp/app/src/code_review/git_dialog/commit.rs` 的 GPUI Element 写法。
- **依赖增量**：仅 `notify-debouncer-full`（fs watcher，Warp 同款）。

## 整体流向

```
shell hook (OSC 7 emit)
        │
        ▼
PTY 输出字节流
        │
        ▼
terminal_runtime::scan_osc7  ─────── (P0)
        │ Some(PathBuf)
        ▼
Tab.local_cwd  ──────────► UI: 路径条 / git 面板订阅
        │ 变化时通知
        ▼
GitPanelWorker (mpsc) ─────── (P2)
        │ git status v2 / branch / log
        ▼
GitPanelState  ──────────► GPUI 面板渲染 (P3)
        ▲
        │ fs 事件 throttle 5s
RepoWatcher (notify-debouncer-full) ─── (P4)
```

## 实施阶段

### P0 — OSC 7 cwd 追踪（基础设施，独立 PR）

**为什么先做**：日后 prompt 显示、`cd` 同步打开文件管理器、远端 cwd 对比等都依赖它。
**为什么走 OSC 7 而非 Warp 的 OSC 9278**：标准协议（VTE/iTerm/WezTerm 均识别），
hook 字符串短，不需要 JSON 编解码。Warp 走 9278 是因为它一次性还要传 session_id、
exit_code 等十几个字段，我们用不上。

| 改动 | 文件 | 参考 |
|---|---|---|
| zsh / bash / fish wrapper 追加 OSC 7 emit | `src/shell_integration.rs` | warp `bootstrap/zsh_body.sh:372` |
| 扫描 `\x1b]7;...\x07` / `\x1b]7;...\x1b\\`，提取 `file://host/path`，url-decode | `src/terminal_runtime.rs`（仿 `scan_bootstrap_marker` 2288） | warp `ansi/mod.rs:1092` 的 OSC 分发 |
| Tab 上新增 `local_cwd: Option<PathBuf>` 字段，扫到就写入 | `src/main.rs` Tab struct | — |

**hook 片段示例（zsh）**：

```sh
__nexshell_emit_osc7() {
  printf '\e]7;file://%s%s\a' "${HOST:-localhost}" "${PWD}"
}
typeset -ga chpwd_functions precmd_functions
chpwd_functions+=(__nexshell_emit_osc7)
precmd_functions+=(__nexshell_emit_osc7)  # 首屏 prompt 也发一次
```

bash 用 `PROMPT_COMMAND` 追加；fish 用 `--on-variable PWD`。

**扫描器实现要点**：仿 `scan_bootstrap_marker` 的 scan_buf 跨调用拼接策略，
但终结符是 `\x07` 或 `\x1b\\`（ST），状态机两个分支。

### P1 — `git_ops.rs`（subprocess wrapper）

照抄 warp `app/src/util/git.rs:14` `run_git_command` 的 env 配置：

```rust
let mut cmd = Command::new("git");
cmd.arg("-c").arg("diff.autoRefreshIndex=false")
   .args(args)
   .current_dir(repo_path)
   .env("GIT_OPTIONAL_LOCKS", "0")
   .kill_on_drop(true);
```

需要的命令集合：

| 用途 | git 命令 | 参考 warp |
|---|---|---|
| 当前分支 | `rev-parse --abbrev-ref HEAD` 失败回落 `branch --show-current` | `util/git.rs:116` |
| detached HEAD 显示 | `rev-parse --short HEAD` | `util/git.rs:133` |
| 主分支检测 | `symbolic-ref refs/remotes/origin/HEAD` → 回落 `origin/main/master/develop` | `util/git.rs:157` |
| 工作区状态（branch + ahead/behind + 文件分组）| `status --porcelain=v2 -b --untracked-files=all` | — |
| 最近 commit | `log -n 20 --oneline --decorate` | — |
| 是否 git 仓库 | `rev-parse --show-toplevel` | — |

`status --porcelain=v2` 输出已结构化，自己写个解析器（约 80 行）即可：
- `# branch.head <name>` / `# branch.ab +N -M`
- `1 XY ...` 已跟踪文件，XY 为 staged/unstaged 状态字符
- `2 XY ...` 重命名/复制
- `u XY ...` unmerged
- `? path` untracked

### P2 — `git_panel.rs`（state + worker）

完全仿 `file_panel.rs` 结构。

```rust
#[derive(Debug, Default)]
pub struct GitPanelState {
    pub repo_root: Option<PathBuf>,      // 非 git 目录时 None，UI 显示提示
    pub branch: String,                  // 当前分支或短 SHA
    pub main_branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: Vec<GitFileEntry>,
    pub unstaged: Vec<GitFileEntry>,
    pub untracked: Vec<GitFileEntry>,
    pub recent_commits: Vec<CommitRow>,
    pub loading: bool,
    pub error: Option<String>,
    pub selected_paths: BTreeSet<String>,
}

pub enum GitRequest {
    Refresh(PathBuf),         // cwd 变化 / 用户手动刷新
    Stage(Vec<String>),
    Unstage(Vec<String>),
    Commit { message: String, amend: bool },
    // push / pull 后续阶段
}

pub enum GitEvent {
    Snapshot(GitPanelState),
    OpFailed(String),
}
```

worker 模式照抄 `file_panel::spawn_sftp_worker`（current-thread tokio runtime
+ mpsc 请求 / async_channel 事件）。

**关键差异**：file_panel 走 SFTP（异步阻塞），git_panel 走本地 subprocess，
延迟低得多，可以省去 transfer / cancel 机制。

### P3 — GPUI 面板渲染

挂载方式仿 file_panel：右侧 divider 拖宽 + 顶部 chrome 按钮切换。

| 元素 | 仿 file_panel | 仿 warp |
|---|---|---|
| 切换按钮 | `FILE_PANEL_BUTTON_POSITION_ID` 同源新增 `GIT_PANEL_BUTTON_POSITION_ID` | — |
| divider | `FILE_PANEL_DIVIDER_POSITION_ID` 复制 | — |
| 分支 header | — | warp `code_review_view.rs` git chip |
| 文件分组折叠 | 自实现 | `code_review_view.rs` |
| 右键菜单 | `file_panel_context_menu` 模式 | — |

**布局**：

```
┌─ ⎇ main  ↑2 ↓0  [⟳] ────────────┐
│ ▼ Staged (2)                     │
│    M  src/foo.rs        [-]      │
│ ▼ Changes (5)                    │
│    M  src/bar.rs   [+] [diff]    │
│    ?? new.txt       [+]          │
│ ▼ Recent commits                 │
│    acd9d63 Fix remote panel...   │
├──────────────────────────────────┤
│ [Commit…] [Push] [Pull] [Fetch]  │
└──────────────────────────────────┘
```

**非 git 目录 / cwd 未知 时**：面板内只显示提示文字（"当前目录不在 git 仓库中"），
不报错。

**远程 tab**：直接不实例化 `git_panel_state`；按钮 disabled。

### P4 — `repo_watcher.rs`（fs 自动刷新）

**何时上**：P3 验证完 UI 之后。P0-P3 阶段用三个触发点已够：
- cwd 变化（OSC 7）
- 面板打开
- 用户点 ⟳ 按钮

P4 加 watcher 后再砍掉手动刷新按钮（或保留作 fallback）。

照抄 warp `crates/repo_metadata/src/watcher.rs` + `git_status_update.rs:147`：
- `notify-debouncer-full` 监听 `<repo_root>/.git/` 内 `HEAD` / `index` / `refs/heads/*`
  + 工作区（排除 `.git/objects` 等噪声目录）
- **5 秒 throttle**（warp `git_status_update.rs:204`）
- **`.git/index.lock` 抑制**：检测到锁文件就跳过本轮 commit_updated 事件
  （warp `git_status_update.rs:367`，避免用户跑 commit 时读到中间态）
- 单例 `GitWatcherRegistry`，多 tab 同 repo 共享一个 watcher
  （warp `git_status_update.rs:76` `subscribe`）

### P5 — commit / stage / unstage 交互

**stage / unstage**：直接调 `git add <paths>` / `git restore --staged <paths>`，
worker 跑完发 `Refresh` 自更新。

**commit dialog**：照抄 `warp/app/src/code_review/git_dialog/commit.rs`（684 行），
关键结构：
- `render_body` 整体布局
- `render_changes_section` 复用面板的文件列表（只读、勾选已 staged）
- `render_message_editor`：`warp_editor::EditorView` 多行输入
- `render_intent_buttons`：commit / commit & push 等

精简版可以省略 AI message generation / intent selector，只保留 textarea + 两个按钮。

## Warp 源码索引（一一对应）

| 我们的文件 / 模块 | 主要参考 warp 文件 | 行号 |
|---|---|---|
| `shell_integration.rs` OSC 7 hook | `bootstrap/zsh_body.sh` / `bash_body.sh` / `fish.sh` | 整体结构 |
| `terminal_runtime.rs` OSC 7 解析 | `app/src/terminal/model/ansi/mod.rs` `WARP_OSC_MARKER` 分发 | 1092 |
| `git_ops.rs` | `app/src/util/git.rs` | 14, 116, 133, 157 |
| `git_panel.rs` state + worker | `app/src/code_review/git_status_update.rs` 简化版 | 43, 123, 147 |
| `repo_watcher.rs` | `crates/repo_metadata/src/watcher.rs` + `repository.rs` | 全文 |
| 面板 UI 渲染 | `app/src/code_review/code_review_view.rs` | — |
| commit dialog | `app/src/code_review/git_dialog/commit.rs` | 532, 564, 626 |
| throttle 工具 | `app/src/throttle.rs`（warp 内部 5 秒 throttle） | — |

## 代码改动点汇总（按阶段）

**P0 改动**：
1. `src/shell_integration.rs` ZSH/BASH/FISH wrapper 末尾追加 OSC 7 emit。
2. `src/terminal_runtime.rs` 新增 `scan_osc7_marker`（仿 2288 `scan_bootstrap_marker`），
   命中后通过现有 event 通道发 `Event::CwdChanged(PathBuf)`。
3. `src/main.rs` Tab struct 新增 `local_cwd: Option<PathBuf>`；事件循环里写入。

**P1 改动**：
4. `src/git_ops.rs`（新文件，约 200 行）：`run_git`、`detect_branch`、`detect_main_branch`、
   `parse_porcelain_v2`、`recent_commits`。

**P2 改动**：
5. `src/git_panel.rs`（新文件，约 400 行）：`GitPanelState` / `GitRequest` / `GitEvent` /
   `spawn_git_worker`。
6. `src/main.rs` Tab struct 新增 `git_panel_state: GitPanelState` +
   `git_worker: GitWorkerHandle`；`local_cwd` 变化时入队 `Refresh`。

**P3 改动**：
7. `src/main.rs` ShellModel 新增 `git_panel_open / git_panel_width /
   git_panel_button_state / git_panel_divider_state / git_panel_context_menu`
   等字段（仿 file_panel 同名字段）。
8. `src/main.rs` render 路径新增 git panel 渲染 fn + divider + button overlay。
9. `terminal_grid_element.rs` 新增 actions：`GitPanelToggle / GitStageFiles /
   GitUnstageFiles / GitRefresh / GitOpenCommitDialog`。
10. `locales/zh-CN.yml` + `en.yml` 新增 `git_panel_*` key。

**P4 改动**：
11. `src/repo_watcher.rs`（新文件，约 300 行）：`GitWatcherRegistry` 单例 + 5s throttle。
12. `Cargo.toml` 加 `notify-debouncer-full`。

**P5 改动**：
13. `src/git_panel.rs` 扩展 `GitRequest` 加 stage/unstage/commit。
14. `src/main.rs` 新增 `git_commit_dialog: ViewHandle<...>`（仿 host_edit_window 模式）。

## 开放问题

1. **OSC 7 主机字段**：`file://host/path` 中 host 段在多机环境（同一 zsh hook
   被 ssh 远端复用时）会变。本地终端我们只看 path 部分，host 暂时忽略；
   若日后接入远端 cwd 追踪，再做 host 校验。
2. **submodule**：v1 不特殊处理，只对 top-level repo 报状态。
3. **大仓库性能**：`status --porcelain=v2 --untracked-files=all` 在巨型仓
   （如 chromium）可能慢；P1 实测后再决定是否加 `--untracked-files=normal`
   或 `--ignore-submodules` 开关。
4. **detached HEAD / rebase 中**：分支显示走 `detect_current_branch_display`
   返回短 SHA；rebase / merge 中的状态文件（`.git/MERGE_HEAD` 等）v1 不解析，
   只透传 porcelain v2 的 unmerged 行。
5. **`git pull` / `push` 走面板按钮还是塞到终端 stdin**：v1 走面板按钮 + worker
   后台调用；若需要看实时输出（如 push 进度），日后再考虑往当前 PTY 注入命令。
