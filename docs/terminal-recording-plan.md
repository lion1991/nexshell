# 终端录制日志（纯文本 transcript）

## Context

需求：本地 PTY / 远程 SSH / 串口三种终端，都能从「开始录制」到「停止录制」把期间终端显示的**全部输出**保存成一份**纯文本可读** log；文件首行前加「日志开始」banner、末行后加「日志结束」banner（含时间戳）。

已与用户确认的取舍：
- 格式 = **纯文本 transcript**（去 ANSI 颜色/控制转义，保留实际显示文字），非 raw 字节。
- 入口 = **终端 tab 右键菜单**「开始录制 / 停止录制」（录制中显示「停止录制」）。
- 保存 = **停止时弹系统保存对话框**选位置/文件名。

关键约束：完整性是硬要求 —— 不能因终端 grid 的 scrollback 上限（2000 行）丢历史。

Warp 参考：Warp 有 `PtyRecorder`（`warp/app/src/terminal/recorder.rs`），但它录 **raw 字节**且无流式纯文本 transcript（`BlockGrid::contents_to_string()` 是 grid 快照、受行数上限约束）。所以**生命周期 API（start/stop/is_recording + Drop 兜底）对齐 PtyRecorder，纯文本流式累积为自研**。

## 核心设计

**单一挂钩点**：三种终端的输出都汇聚到 `TerminalRuntimeState::process_output()`（`src/terminal_runtime.rs:2461`）—— 本地经 `PtySink` trait，SSH/串口经 `remote_process_output()`（`:4047`）。在此处旁路 raw bytes 即全覆盖，无需动各事件循环。

**为什么旁路 raw bytes 而非读 grid**：grid 受 scrollback 上限截断、且把字节解释成 cells 拿不回原始换行流；raw bytes 是完整性的唯一来源。

**累积策略**：录制专用的轻量 line-based ANSI stripper 逐字节转纯文本 → 累积到内存 `Vec<u8>`（去 ANSI 后体积小）→ 停止时拼 banner 一次性 `fs::write` 到用户选的路径。与现有「内存 bytes → save picker → fs::write」范式一致（`transfer.rs:171`、`file_panel_section/actions.rs:479`）。设软上限（如 128MB）到顶置 `truncated` 标记并在结束 banner 注明，避免无界增长。

## 新建模块：`src/terminal_recorder.rs`

纯算法 + 生命周期两部分（全部录制逻辑集中此文件，预计 250–400 行含单测，不污染已 5111 行的 terminal_runtime.rs）。在 `src/lib.rs:56` 区加 `pub mod terminal_recorder;`。

```rust
pub struct AnsiTranscriptStripper { state, line: Vec<u8>, out: Vec<u8>, truncated, cap }
//   new(cap) / push(&[u8]) / finish()->Vec<u8> / is_truncated()
pub struct TerminalRecorder { stripper, started_at: chrono::DateTime<Local> }
//   start() / push_bytes(&[u8]) / finalize()->Vec<u8>(拼 banner) / is_recording 由外层 Option 表达
```

**stripper 状态机**（`StripState = Normal|Esc|Csi|Osc|OscEsc`，状态跨多次 push 保留 → 天然处理跨 chunk 的半截 escape，无需额外残留 buf）：
- 可打印字节（含 UTF-8 ≥0x20）→ 追加当前行；`\n` → flush 行；`\r` → 清空当前行（回行首覆盖语义）；`\b` → pop 末字符；`\t` → 补空格到 8 列对齐；其余 C0 → 丢弃。
- `ESC` → 进 Esc，按下一字节分流：`[`→Csi、`]`→Osc、其余单字符 escape 吃掉回 Normal。
- Csi：吞参数/中间字节，遇 final `0x40..=0x7e` 结束。Osc：吞到 `BEL` 或 `ESC \`(ST)。
- finalize 时 flush 残留末行（无 `\n` 结尾也不丢）。
- banner：`===== {日志开始 i18n} {start %Y-%m-%d %H:%M:%S} =====\n` + transcript（保证 `\n` 收尾）+ `===== {日志结束} {now} =====\n`。

**局限（写入 PR 描述，不当 bug）**：vim/top 等全屏 TUI 靠光标定位原地重绘，纯文本流式会把每帧字符线性堆叠成「重绘垃圾」。还原需完整 grid 模拟（=放弃历史完整性），故不处理。适用滚动型会话（命令+输出/编译日志/SSH 交互）。

## 集成改动（按 file:line）

**`src/terminal_runtime.rs`**（仅薄集成，零业务逻辑）：
- `TerminalRuntimeState`（`:2191`）加字段 `recorder: Option<TerminalRecorder>`。
- **两处**初始化补 `recorder: None`：`new()`（`:2264` 区）和 `failed()`（`:2912`，易漏）。
- `process_output()`（`:2461`）函数顶部加：`if let Some(r) = self.recorder.as_mut() { r.push_bytes(bytes); }`。
- `impl LocalTerminalRuntime` 加三个薄转发（锁 `self.state` 是 FairMutex，`.lock()` 直返 guard）：`start_recording()` / `stop_recording() -> Option<Vec<u8>>` / `is_recording() -> bool`。

**`src/terminal_grid_element.rs:737`**：`TerminalGridAction` 加变体 `ToggleTabRecording(usize)`（挨着 `ReconnectTab`）。

**`src/root_view/mod.rs:2032`**：`handle_action` 加派发 `ToggleTabRecording(index) => self.handle_toggle_tab_recording(*index, ctx)`。

**`src/warp_tab_context_menu.rs`**：`HorizontalTabContextMenuActions`（`:67`）加 `toggle_recording: Option<A>` + `is_recording: bool`；加自由函数 `recording_menu_items`（仿 `reconnect_menu_items` `:215`），按 `is_recording` 选 i18n key。
- ⚠️ 构造点共 5 处需补字段：`context_menus_section.rs:71` + `lib.rs:817/881/936/1028`（测试，否则不编译，补 `toggle_recording: None, is_recording: false`）。

**`src/root_view/context_menus_section.rs:71`**（菜单构造块）：传 `toggle_recording`（content tab 即 GitDiff/CodeViewer 用 `!is_content_tab` 守卫排除）+ `is_recording`。
- ⚠️ `is_recording` 必须与 handler 取**同一个 runtime**：`tab.pane_terminals.get(&tab.focused_pane_id)` 回退 `tab.terminal`（规范解析模式见 `mod.rs:2439-2443`；tab 创建时主终端已插入 pane_terminals，`mod.rs:2264-2265`），否则分屏时菜单文案与实际录制状态不一致。短暂 `lock().ok()` 读取后立即释放。

**`src/root_view/tab_bar_section/actions.rs`**（紧邻 `handle_reconnect_tab` `:131`，只写 `impl RootView`）：
```
pub(in crate::root_view) fn handle_toggle_tab_recording(&mut self, index, ctx)
```
逻辑：取活动 pane runtime（`tab.pane_terminals.get(&tab.focused_pane_id)` 回退 `tab.terminal`，与菜单状态读取同一解析）→ `lock().ok()`；
- 录制中 → `stop_recording()` 取 bytes → drop 锁 → `open_save_file_picker`（default filename = tab label + 时间戳 `.log`）→ callback `fs::write` + `host_state.notice` toast（成功/失败/取消范式见 `transfer.rs:173-193`）。
- 否则 → `start_recording()` + 「已开始录制」notice。
- 结尾 `ctx.notify()` + 关菜单（仿 `handle_toggle_tab_color` `:151`）。

**`locales/en.yml` + `locales/zh-CN.yml`**（375 行 tab 菜单 key 区）：`tab_ctx_record_start/stop`、`toast_recording_started/saved/save_failed`、banner 文案 `log_banner_start/end`（含中文「日志开始/日志结束」）。

## 模块合规

录制算法/逻辑全在新文件 `terminal_recorder.rs`（<800 行）。terminal_runtime.rs 仅 1 字段 + 2 处 `None` + 3 行 push + 3 个薄方法（~15 行）；terminal_grid_element.rs 仅 1 个 enum 变体；section 文件只写 `impl RootView`、不互相 use。无新第三方依赖（chrono 已有 `Cargo.toml:40`，save picker 用 warpui 自带）。

## 验证

**单测**（`terminal_recorder.rs` 内 `#[cfg(test)]`）：
- `hello\r\nworld\n`→`hello\nworld\n`；`loading...\rdone\n`→`done\n`；`\x1b[31mred\x1b[0m\n`→`red\n`。
- 跨 chunk：`\x1b[3` + `1mX\n`→`X\n`；OSC `\x1b]0;ti` + `tle\x07A\n`→`A\n`。
- 退格 `abc\x08\x08X\n`→`aX\n`；UTF-8 `中文\n` 不拆坏；末行无换行 `tail` finalize 不丢。
- banner 首/末行正则匹配 `===== 日志开始 \d{4}-...=====`。

**手动 e2e**（`cargo run`）：
1. 本地 tab：开始 → `ls`/`echo`/`cat` → 停止 → 保存 → 检查首尾 banner + 去 ANSI 纯文本完整。
2. **完整性 > scrollback**：录制中 `seq 1 5000`（远超 2000 行），停止确认 1..5000 全在（路线 1 关键验证点）。
3. SSH tab：录 `uname -a`/`ls`，确认远程路径（`:4047`）命中。
4. 串口 tab（有设备/虚拟串口）：同上。
5. 菜单文案：录制中右键显示「停止录制」，停止后变回「开始录制」。
6. 边界：取消保存对话框不写文件不崩溃；录制中关 tab，recorder 随 state 析构无泄漏。
