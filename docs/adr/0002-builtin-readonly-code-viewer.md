# 内置只读代码查看器：复用 Warp CodeEditorView，承载为一种标签种类

Status: accepted (2026-05-29)

## 背景

文件面板要能「打开/查看」文件。NexShell **本身没有任何编辑能力**——这是刻意的范围。
Warp 有自己的代码面板 `CodeEditorView`（行号 + tree-sitter 语法高亮，可编辑），但：

- `code` 模块在 `warp/app/src/lib.rs` 是私有的（`mod code;`），外部 crate 用不了；
- 完整的 `CodeView` 与 pane 系统、`GlobalBufferModel` 单例、LSP、workspace、telemetry 深度耦合，
  无法单独拎出来渲染。

nexshell 已依赖全部相关 crate（`warp`/`warp_editor`/`warp_core`/`warpui`），地基具备。

## 决策

提供一个**只读**代码查看器，**复用（调用，而非复刻）** Warp 的核心渲染引擎 `CodeEditorView`：

- **调用**：在 `warp/app/src/lib.rs` 加最小 `pub use` 导出 `CodeEditorView` 等；buffer 用
  `warp_editor` 的 `Buffer::from_plain_text` 直接构造，**绕过** `GlobalBufferModel` / pane / LSP。
- **承载为标签种类** `TerminalSessionKind::CodeViewer`，沿用 ProcessList/NetworkList/SystemInfo/GitDiff
  这些**非终端整页标签**的现成先例（在 `render` 分发链里单独渲染整页），**不进** `pane_tree`。
- **只读**：经 `set_interaction_state` 设为不可编辑。要真正*编辑*一律「用外部程序打开」（系统默认
  关联程序 / 配置的外部编辑器）。
- **仅本地标签**；远程文件只能「下载」（见 0001 范围）。二进制文件不进查看器，直接「用外部程序打开」。

### 标签复用：diff 与代码查看器共用「单标签」开关

`git diff` 与代码查看器同属**非终端查看标签**。现状：git 面板每点一个文件就 `open_git_diff_tab`
新开一个标签（标签 id 按 `(源终端标签, repo, 具体文件选择)` 精确区分），文件多时标签爆炸。

决策：增设**统一设置项**控制「复用单标签」，**默认开启**，**同时管 git diff 与代码查看器**：

- **开启（默认）**：同一源终端标签下，diff 共用一个标签、代码查看器共用一个标签——匹配时
  **忽略具体文件**，命中就换内容重载，不新建标签。
- **关闭**：恢复「每文件一个标签」的旧行为。

实现落在 `open_git_diff_tab`（及新增的 `open_code_viewer_tab`）：reuse 开启时按 `(源终端标签[, repo])`
匹配复用、忽略 selection；关闭时维持现状的按文件精确匹配。

## Considered Options

- **直接调用 Warp `CodeView`**：私有 + pane/单例深耦合，不可行。
- **复刻**（把 `code/editor` 子模块拷进 nexshell）：几千行 + tree-sitter 集成，维护成本高且与 Warp 脱节。
- **给 NexShell 做自有编辑器**：无内置编辑能力是刻意范围；「只读查看 + 外部编辑」已覆盖需求。
- **分屏 pane 承载**：`pane_tree` 目前仅放终端，新增非终端 pane 类型侵入大；tab kind 有现成先例，更省。

## Consequences

- 对 vendored Warp 有**极小**改动（`lib.rs` 的 `pub use` 导出）——升级 Warp 时需留意这一处。
- nexshell 需注册 `FontSettings` 单例（`CodeEditorView::new` 依赖；`Appearance`/`AppEditorSettings`/`FeatureFlag` 已就绪）。
- 只读是刻意取舍：用户改文件须经外部程序，符合「NexShell 无编辑能力」的定位。
- 新增统一设置项「diff / 查看器复用单标签」，**默认开启**；关闭可回到「每文件新标签」。
