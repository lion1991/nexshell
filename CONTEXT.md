# nexshell

nexshell 的原生终端 Rust crate。本文件是术语表，约束相关概念的用词，不记录实现细节。

## Language

### 标签与会话

**文件面板 (File Panel)**：
终端标签侧边的目录浏览器，可浏览文件、上传/下载/重命名/删除。
_Avoid_: 文件树、资源管理器

**本地标签 (Local Tab)** / **远程标签 (Remote Tab)**：
终端标签的两类；本地路径是真实本地 fs 路径，远程经 SSH/SFTP。

**标签种类 (TerminalSessionKind)**：
一个标签的内容类型。除终端外，已有 ProcessList/NetworkList/SystemInfo/GitDiff 等**非终端**整页种类；**内置编辑器**是其中一种。

### 打开文件

**内置编辑器 (Built-in Editor)**：
NexShell **内置**的编辑器，复用 Warp 的 `CodeEditorView` 渲染（行号 + 语法高亮）。作为一个**编辑器标签**打开，与终端标签并排。**可编辑可保存**（`Cmd+S`），脏标记 + 未保存确认保护。编辑**文本**文件，二进制 / 超大除外。
_Avoid_: 代码查看器、只读（已反转，见 ADR 0003）、代码编辑器（不止编辑代码）

**编辑器标签 (Editor Tab)**：
承载内置编辑器的标签（代码标识符仍是 `TerminalSessionKind::CodeViewer`，未随术语重命名）。

**打开 (Open)**：
把文件**在内置编辑器标签中打开**（默认行为，双击触发）。本地标签经 fs 读写，远程标签经 SFTP 内存读写。
_Avoid_: 编辑、查看（口头同义，但 canonical 词是「打开」）

**用外部程序打开 (Open externally)**：
把文件交给**系统默认关联程序**或**配置的外部编辑器**。定位区分（ADR 0003）：内置编辑器=快速改存，外部=重度编辑（项目级、LSP、调试）。本地二进制 / 超大文件也走这里。
_Avoid_: 编辑（口头同义）

**外部编辑器 (ExternalEditor)** / **编辑器选择 (EditorChoice)**：
「用外部程序打开」可选的外部图形编辑器（VS Code、Cursor 等）及其设置项。与内置编辑器相对。

### 终端 / 备用屏

**备用屏 (Alternate Screen)**：
tmux / vim / less 等全屏程序使用的备用缓冲区（DECSET 1049）。与**主屏 (Primary Screen)** 相对。

**备用屏 scrollback (Alt-Screen Scrollback)**：
为备用屏保留「从滚动区顶部滚出」的历史行，使原生滚动条 / 滚轮能回看（参照 iTerm2，见 ADR 0006）。是**通用模拟器特性**，对 tmux / vim / less 等一律生效。
_Avoid_: 「tmux 历史」「tmux 滚动」「tmux 集成」——这不是 tmux 专属，更不是 tmux control mode。

**备用屏滚轮 (Alternate Mouse Scroll)**：
备用屏下、当前程序未请求鼠标上报 / alternate-scroll 时，滚轮去滚本地备用屏 scrollback，而非被吞掉或转发给程序。

**tmux control mode (`tmux -CC`)**：
iTerm2 把 tmux window→原生 tab、pane→原生 split 的深度集成。**当前未采用**（备用屏 scrollback 已满足「回看历史」目标），保留为未来可选高级集成。
_Avoid_: 与「备用屏 scrollback」混为一谈。

## Relationships

- **内置编辑器**对**本地与远程标签**的**文本文件**都生效：本地经 fs，远程经 SFTP 把内容读进编辑器、`Cmd+S` 写回原路径。
- **二进制 / 超大文件**不进内置编辑器：本地回退「用外部程序打开」；远程提示先「下载」（系统程序开不了远程路径）。
- **内置编辑器**复用 Warp 的 `CodeEditorView`（不复刻、调用导出），buffer 由 `Buffer::from_plain_text` 直接构造，不经 Warp 的 GlobalBufferModel / pane / LSP。
- **内置编辑器 ≠ 外部编辑器**：内置是 NexShell 自己的整页编辑器标签；外部是把文件交给独立 app。

## Example dialogue

> **Dev:** 双击文件是 NexShell 自己编辑吗？
> **Matt:** 是。双击在**内置编辑器**里打开，可改可存（`Cmd+S`）。重度编辑才**用外部程序打开**交给 VS Code 之类。

> **Dev:** 远程 SFTP 的文件能在内置编辑器里改吗？
> **Matt:** 能（文本文件）。经 SFTP 把内容读进编辑器、保存写回原路径。二进制 / 超大仍只能「下载」。

## Flagged ambiguities

- 「代码查看器 / 只读」是过时用词：ADR 0002 曾定为只读，**ADR 0003 已反转**为可编辑可保存，canonical 词改为**内置编辑器**。
- 「代码编辑器」也不取：它编辑任意**文本**文件（config、日志等），不限代码；语法高亮只是渲染特性。
- Warp 的 `CodeEditorView` 本就可编辑；NexShell 现复用其**可编辑**能力（0002 时仅复用只读渲染）。
