# 内置代码编辑器：查看器就地可编辑 + 本地保存（反转 0002 只读定位）

Status: accepted (2026-05-30)

## 背景

ADR 0002 把代码查看器定为**只读**，「NexShell 本身没有任何编辑能力」是刻意范围，
要真正改文件一律「用外部程序打开」。实际使用中「改一行也要切到外部编辑器」摩擦明显。

技术地基现成：Warp `CodeEditorView` **本就是可编辑编辑器**（0002 只是经 `set_interaction_state`
设为只读 `Selectable`）。复核确认所需 API 齐备：

- 可编辑态 `InteractionState::Editable`；
- 内容变更事件 `CodeEditorEvent::ContentChanged`（可订阅）；
- 取全文 `CodeEditorView::text(ctx)`（内部 `model.content_string`），脏判定用它与已保存基线做文本对比
  （`EditOrigin::UserInitiated` 同时用于程序重载与用户粘贴/退格，无法区分，故不能靠 origin）。

## 决策

查看器**就地可编辑**，`Cmd+S` 保存本地文件，脏标记 + 未保存确认弹窗保护。复用既有引擎与设施，不造轮子。

- **可编辑**：`build_code_viewer_view` 用 `InteractionState::Editable` 替原 `Selectable`。
- **脏标记**：订阅 `CodeEditorEvent::ContentChanged`，回调对比当前 `view.text()` 与已保存基线
  `code_viewer_saved_content`（origin 无法区分重载与粘贴，故文本对比；改回原样自动消脏），
  写 tab 字段 `code_viewer_dirty`；标签渲染时 dirty 前缀 ● 圆点。
- **保存**：新增 action `CodeViewerSave` 绑 `Cmd+S`（active 为 CodeViewer 时生效）：
  `view.text(ctx)` → `std::fs::write(path)`；成功 → `dirty=false` + notice「已保存」，失败 → notice 错误。
- **未保存保护**：dirty 时所有「会丢失编辑」的路径都拦截，弹 `show_native_platform_modal`（`AlertDialogWithCallbacks`）确认：
  - 单标签关闭（`close_terminal_tab`）、reuse 换文件（`open_code_viewer_tab` 复用分支）：三按钮 保存/不保存/取消。
  - 批量关闭其他 / 关闭右侧（`close_other_terminal_tabs` / `close_terminal_tabs_right`）：含 dirty 时弹一次汇总确认（全部保存/全部丢弃/取消），回调按 anchor tab_id 重定位再关。
  - 关窗 / 退出 app（`on_should_close_window` / `on_should_terminate_app`）：有 dirty CodeViewer 即拦截确认，不静默退出。
- **范围**：沿用 0002——仅**本地文本文件**（远程只下载；二进制 / 超大回退「用外部程序打开」）。
- **外部「编辑」入口保留**：`EditorChoice` / 右键「编辑」仍走外部编辑器。定位区分：内置=快速改存，外部=重度编辑。

## Considered Options

- **默认只读 + 显式进编辑 / 查看·编辑双入口**：多一层模式切换；`CodeEditorView` 本就可编辑，就地可编辑最省，
  符合「加入编辑能力」的直接诉求。
- **远程 SFTP 写回**：超出 0001/0002「远程只下载」范围，需临时文件 / 上传 / 冲突处理，工作量大；首版聚焦本地。
- **自动保存（切走/关闭即写回）**：「误改即落盘」风险，与代码文件谨慎编辑的预期不符。
- **自建编辑器 / 自建确认弹窗**：复用 Warp `CodeEditorView` + 项目现成 `show_native_platform_modal`，零自建。

## Consequences

- 反转 0002 的「只读」刻意取舍：NexShell 现具备**受限的本地文本编辑**能力；0002 其余约束
  （远程只下载、二进制回退外部、查看标签复用 `reuse_view_tab`）仍适用。
- tab 状态新增 `code_viewer_dirty` + `code_viewer_saved_content`（脏判定基线）；单关 / 批量关闭 / reuse 换文件 / 关窗退出 路径均新增「未保存检查」分支。
- 新增 `Cmd+S` keystroke + `CodeViewerSave` action 注册（main.rs 装配）。
- 保存为**直接覆盖写**本地文件（无备份 / 版本）；用户须自行用 git 等兜底。
- 脏判定基线用 `view.text()` 归一化文本（非原始字节），避免混合行尾文件「打开即脏」；但保存按编辑器 primary 行尾（`text_with_line_ending`）重建——纯 LF / CRLF 文件保真，混合行尾文件保存会统一为 primary 行尾。这是 warp 编辑器固有行为。
- 脏标记依赖 `CodeEditorEvent::ContentChanged` 事件 + `view.text()`；vendored warp `lib.rs` 新增导出 `CodeEditorEvent`，升级时留意。
- 可编辑态须在启动调 `init_code_editor_view`（CodeEditorView 自己的方向键/退格/删除/选择/回车等 action 键绑定，单行 `warp::editor::init` 不含）；vendored warp `lib.rs` 为此 `pub use ...view::init`。漏调则仅 IME 字符输入可用、方向键/功能键全失效。
