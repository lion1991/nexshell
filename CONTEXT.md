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
一个标签的内容类型。除终端外，已有 ProcessList/NetworkList/SystemInfo/GitDiff 等**非终端**整页种类；**内置编辑器**、**RDP 标签**是其中两种。

**RDP 标签 (RDP Tab)**：
承载 Windows 远程桌面画面的**整页**标签。不参与 split、无侧栏；关闭标签即断开连接；连接中断显示页内「已断开 + 重连按钮」，不自动重连。引擎选型见 ADR 0007。
_Avoid_: 远程桌面窗口（不是独立窗口）、RDP 终端（不是终端）

**显示质量 (Display Quality)**：
RDP 主机的选项：**标准**（逻辑像素协商，省带宽，默认）/ **高清 HiDPI**（物理像素协商，LAN/同城用）。连接时定一次；之后窗口变化走等比缩放。
_Avoid_: 分辨率设置（用户不直接填数字）

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

### Warp 上游同步

**官方上游 (Official Upstream)**：
Warp 官方持续演进的代码谱系，是 NexShell 复用引擎能力的外部来源。
_Avoid_: 私有 Warp、NexShell Warp

**集成镜像 (Integration Mirror)**：
供 NexShell 消费的完整 Warp 代码谱系，同时承载官方上游能力与 NexShell 保护补丁。
_Avoid_: 普通 fork、依赖副本

**保护补丁 (Preservation Patch)**：
支撑 NexShell 自有特性、每次上游追平都必须保全的 Warp 侧行为差异。
_Avoid_: 临时修改、本地 hack

**上游追平 (Upstream Catch-up)**：
集成镜像完整包含指定官方基线及其历史，同时继续满足全部保护补丁的状态。
_Avoid_: 版本升级、挑选修复、尽量合并

**目标基线 (Target Baseline)**：
一次上游追平明确冻结的官方上游提交；该次追平的差异、冲突和验证都以它为唯一参照。
_Avoid_: 最新 master、实施时最新版

**特性保全 (Feature Preservation)**：
上游追平后，NexShell 自有能力的用户行为与跨仓库契约保持等价；任一能力缺失都表示追平失败。
_Avoid_: 编译通过、冲突已解、基本可用

**保护清单 (Preservation Ledger)**：
全部保护补丁及其保全结论的权威清单；未列入清单不表示可以删除。
_Avoid_: 提交列表、冲突文件列表

**补丁退役 (Patch Retirement)**：
官方上游已提供经验证的等价能力，并经明确批准后，保护补丁不再需要保留的状态。
_Avoid_: 自动合并成功、上游看起来已经修复

**镜像运维策略 (Mirror Operations Policy)**：
集成镜像特有、与应用运行能力分离的仓库维护约束，包括继承自动化的启停策略。
_Avoid_: NexShell 特性、保护补丁

**集成候选 (Integration Candidate)**：
已包含目标官方基线、但尚未通过全部特性保全验证的集成镜像状态。
_Avoid_: 已完成更新、可发布版本

**验证门禁 (Verification Gate)**：
集成候选进入集成镜像稳定分支前必须满足的证据集合；环境阻塞不等于通过。
_Avoid_: 编译门禁、冒烟测试

**兼容基线 (Compatibility Baseline)**：
NexShell 已验证兼容的官方目标基线与集成镜像提交身份，用于识别两个仓库是否处于同一受支持组合。
_Avoid_: `../warp` 当前内容、Cargo 路径依赖

## Relationships

- **RDP** 是主机库 `protocol` 的第三个取值（与 SSH / Serial 并列），不是新的主机实体；凭据复用主机的用户名/密码，Windows 域写在用户名里（`DOMAIN\user`）。
- **RDP 标签**的键盘按物理直映（⌘→Win、⌥→Alt、⌃→Ctrl），NexShell 自身快捷键本地优先；中文输入交给**远端** Windows 输入法。
- 服务器证书 v1 无条件接受，与 SSH host key 现状同姿态；「统一主机信任层（TOFU 钉扎）」是两协议一起做的后续项。
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
