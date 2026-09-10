# herdr 多路复用器：git / 文件面板跟随其焦点 workspace 的 cwd

Status: accepted (2026-09-10)

## 背景

nexshell 的 git 面板与文件面板靠 shell 的 **OSC 7** 上报 cwd（`TerminalRuntimeState.local_cwd`）。当 tab 前台跑的是 **herdr**（终端多路复用器）client 时这条链路整个失效：

- herdr client 自己是个 TUI，**吞掉**被复用 shell 的 OSC 7，不向宿主 pty 转发，`local_cwd` 停在 herdr 启动那一刻。
- pane 里的真实 shell 挂在 **herdr server** 进程下，与 nexshell 的 pty 子进程**进程树不相连**（`pbi_comm` 只看得到 `herdr` 这个 client），拿不到它的 cwd。
- Warp 无可参考实现：Warp 只做了 tmux control mode（`tmux -CC`），herdr 是另一套私有 Unix socket 协议，上游没有对应代码可抄。

herdr 0.9.0 提供 Unix socket JSON-line API（`~/.config/herdr/herdr.sock`），是唯一可用的信息通道。

## 决策

新增 `src/herdr_bridge/`（socket 客户端 + 快照索引），前台是 herdr 时接管面板 cwd：

- **进程检测**：`query_foreground_kind` 由 tcgetpgrp + proc_pidinfo 取 `pbi_comm`，分类成 `Shell / Herdr / Other`（`src/foreground_kind.rs`）。`shell_is_foreground` 语义逐字节不变。
- **数据源 `session.snapshot`**：一次拿回全部 workspaces / tabs / panes / layouts 与各自的 cwd，索引成 `SnapshotIndex`（`workspace.active_tab_id → layouts.focused_pane_id → panes.cwd`，回退 `foreground_cwd`）。
- **标题反查 workspace**：herdr 焦点是 **per-client** 的（官方 concepts：多 client 时各看各的 workspace），socket API 的 `focused_*` 只反映「前台 client」。可用的 per-client 信号只有 herdr client 写给宿主 pty 的 OSC 0/2 窗口标题（默认模板 `{hostname}: {workspace}`，nexshell 已解析进 `TerminalRuntimeState.title`）。于是按第一个 `": "` 切出 workspace label 反查。label 是目录 basename、可能重名，重名时先比对候选的焦点目录：**同目录直接用，目录确实不同才回退全局焦点**。
- **事件只当脏标记**：订阅 workspace/tab/pane/layout 共 13 类事件，收到任意一条 → 标脏 → 缓冲读空后拉一次 snapshot（同批连发只拉一次）。不解析事件负载——per-client 焦点让事件里的 `focused_*` 不可信。
- **`panel_cwd` 与 `local_cwd` 分离**：snapshot 新增 `panel_cwd = herdr_cwd.or(local_cwd)`，`local_cwd` 与 OSC 7 通路**一字未改**。只有 `dispatch_git_cwd_updates` / `dispatch_file_panel_cwd_updates` 改读 `panel_cwd`，非 herdr 用户行为逐字节不变。
- **单例 + RAII 租约**：进程级 `HerdrBridge`，任一 tab 前台是 herdr 就 `acquire()`，租约 drop 即 release，归零后停线程清状态。cwd 变化经 wakeup 通道的 **WeakSender** 戳 UI 重绘（不延长通道寿命，子进程退出后照旧关闭）。断线按 1s→2s→…→30s 退避，会话稳定活过 10s 才归零。全部错误静默降级为「没有 cwd」，退化成 OSC 7 行为。

## Considered Options

- **`pane.current` 拿全局焦点**：最初实现。但 herdr 焦点是 per-client 的，用户在别的终端还挂着一个 client 时，server 报的一直是那个 client 的 workspace；nexshell 里切 workspace 时全局焦点根本不动。真机复测直接失败。否。
- **事件关联法**（订 `workspace.focused` 等，从事件负载推断本 client 焦点）：已核实 server 对第二个 client 的切换**不向订阅端广播任何焦点事件**，只写自己的日志。无信号可关联。否。
- **herdr 插件 / 钩子**：要 herdr 侧回调 nexshell，得再造一条 nexshell IPC 入口 + 安装配置，用户零配置的前提没了，且强耦合 herdr 内部扩展点。否。
- **进程树遍历找真实 shell**：pane 里的 shell 挂在 herdr server 下，与我们的 pty 子进程不相连，遍历不到。技术上不可行。否。
- **解析 herdr TUI 屏幕内容**：脆弱（依赖渲染布局与主题），违反「不造轮子」。否。

## 已知局限

- **重名 label 且目录不同**才回退 server 全局焦点（同目录的重名已能正确跟随）。这种情况没有可用信号可解——需要 herdr 提供 per-client 焦点查询，或标题模板支持 workspace id 占位符。可作为诉求反馈上游。
- **只支持单一 socket 路径**：`HERDR_SOCKET_PATH` → `~/.config/herdr/herdr.sock`。命名 session / 多 server 实例未支持。
- **依赖默认标题模板**：用户把 `ui.window_title` 改成不含 `{workspace}` 时反查不出来，回退全局焦点（不劣于改造前）。
- **分屏 / 新 tab 继承仍用 `local_cwd`**（`terminal_section/split.rs`）：新 shell 是 nexshell 的子进程、与 herdr pane 无关，落在 herdr 启动目录才合理。让分屏跟随 herdr 焦点目录是另一个取舍，本次范围外。
- **只刷本地 tab**：`refresh_local_foreground_status` 每帧遍历所有 Local tab；远程 / 串口 runtime 没有 pty_fd，不参与。

## Consequences

- 新增 `src/herdr_bridge/{mod,protocol,client,snapshot,focus_state}.rs` + `testdata/session_snapshot.json`（真机 `herdr api snapshot` 裁剪，含重名 label、焦点 pane 非 p1、cwd 为 null 三种坑），以及 `src/foreground_kind.rs`。
- `pty_event_loop::EventLoopHandle` 新增 `weak_wakeup_tx`，供 bridge 在无 pty 输出时唤醒 UI。
- 纯函数全部可单测：snapshot 索引与 label 反查、事件→标脏→合并拉取、重连退避。另有两个 `#[ignore]` 真机用例（`session.snapshot` 建索引、13 类订阅被 server 接受）。
- 测试自基线 388/195 增至 **433 passed / 0 failed / 3 ignored**（lib）与 **195 passed / 0 failed**（bin）；clippy 与 build warning 数与基线持平（0 error / 177 warning、7 warning），新文件零告警。
- 非 herdr 用户零影响：`local_cwd` 与 OSC 7 路径未动，bridge 线程仅在前台出现 herdr 时启动。

## 验证方式

真机步骤：

1. nexshell 开本地 tab 跑 `herdr`，git 栏 / 文件面板应跳到当前 pane 的仓库。
2. 在 herdr **TUI** 里（不是 CLI）切到一个 label 唯一的 workspace，**不敲任何命令**——面板应立即跟随（验证 waker 生效，不依赖 pty 输出）。
3. 另开一个终端再挂一个 herdr client 停在别的 workspace，重复步骤 2——这是 per-client 盲区的复现场景，应仍然正确。
4. pane 内 `cd` 到别的目录 → 面板跟随。
5. 退出 herdr 回到裸 shell → `cd` 几次，确认 OSC 7 重新接管。
6. 关掉 herdr server → 面板静默退回 OSC 7，无卡顿无报错。

观测（`log::debug!` 经 tracing-log 桥接，设了 `RUST_LOG` 才有出口）：

```
RUST_LOG=nexshell::terminal_runtime=debug,nexshell::herdr_bridge=debug ./target/debug/nexshell
```

关键行：

- `herdr panel cwd -> Some("/…/taskops") (title Some("Matts-MacBook-Pro: taskops"))` —— 仅在跟随目录**真变化**时打印。title 为 `None` 说明标题没解析到；title 有值但 cwd 落在全局焦点说明 label 重名或模板不匹配。
- `herdr bridge: subscribed to …` / `snapshot updated` / `subscription closed` —— 连接、拉取、断线。
