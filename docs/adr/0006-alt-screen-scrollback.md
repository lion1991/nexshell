# 备用屏 scrollback：给 alt grid 撑历史让 tmux/vim 历史可滚（不走 tmux control mode）

Status: accepted (2026-06-19)

> 实现修订：原定 fork alacritty，落地时发现 `Term::grid_mut()` + `Grid::update_history()` 已是公开 API，运行时补历史即可——**最终未 fork**。详见「决策」与「Consequences」。

## 背景

本地终端跑 tmux / vim / less 等**备用屏 (alternate screen)** 程序时，滚出屏幕的历史无法用滚轮回看。实测默认 tmux 3.5a 启动序列：`\e[?1049h`（切备用屏）、mouse `?1000/1002/1003/1006` 全部 reset、`?1007`（alternate-scroll）未设——即 tmux 自己**不提供任何滚轮机制**，历史只在它自己的 pane buffer 里；而备用屏在 emulator 侧又没有 scrollback。

用户要求参照 iTerm2（实测体验好）。调研发现 iTerm2 的"滚 tmux 历史"**不靠 tmux 集成**，而是两个模拟器层默认开的通用特性：

- `KEY_SCROLLBACK_IN_ALTERNATE_SCREEN`（@YES）：把备用屏"从滚动区顶部滚出"的行存进 scrollback。
- `KEY_ALLOW_ALTERNATE_MOUSE_SCROLL`（@YES）：程序未请求鼠标时，滚轮滚本地 scrollback。
- 防污染：仅当**滚动区顶部 = 第 0 行**时才追加历史（状态栏在底部、其 CUP 重绘不进历史）。

我方后端 `alacritty_terminal 0.25`：alt（inactive）grid 历史容量**写死 0**（`term/mod.rs:416`），备用屏滚出的行直接丢。但其 `scroll_up` 本就有"`region.start==0` 才进 history、否则丢弃"的闸——与 iTerm2 防污染逻辑**一致**。实测 tmux 用滚动区（顶部=第 0 行）+ `SU`/`\n` 滚动渲染追加输出，正好命中该闸。

## 决策

实现"备用屏 scrollback"，参照 iTerm2，**不做 tmux control mode、不 fork alacritty**：

- **用 alacritty 公开 API**：`Term::grid_mut()` + `Grid::update_history()`（皆 pub）。在 `process_output` 检测到"进入备用屏"（`alt_after && !alt_before`）时，对当前（alt）grid 调 `update_history(0)`（清上轮残留）再 `update_history(scrollback)`（撑开本轮）。alacritty 默认只给主屏历史、alt grid 出厂写死 0，这里运行时补上。
- **复用 alacritty 既有 `region.start==0` 闸**做防污染（= iTerm2 核心防污染），不另写。
- **关掉默认的 alternate-scroll**：alacritty 的 `TermMode` 默认**开** ALTERNATE_SCROLL（DECSET 1007，`term/mod.rs:116`），与 iTerm2（默认关）相反——会把备用屏滚轮转发成 ↑/↓ 给应用、而非滚本地 scrollback（tmux mouse off 收到方向键只会动 shell 历史，滚轮"失效"）。`TerminalGridCore::new` 注入一次 `?1007l` 关掉，使默认=本地滚动；应用显式 `?1007h` 再开（iTerm2 同款）。
- **滚轮其余零改动**：关掉 1007 后，备用屏滚轮本就穿透到 `rt.scroll()`（`terminal_grid_element.rs:3146`）→ 滚本地历史；GUI 滚动条按 history 渲染、拖动可用。
- **常开、不加设置**（YAGNI）；alt 历史容量 = 主屏 `scrolling_history`。
- 实现位置：`terminal_runtime.rs` `TerminalGridCore`（新增 `scrollback` 字段、`new` 注入 `?1007l`、`process_output` 的 alt-enter 分支）。

## Considered Options

- **tmux control mode（`tmux -CC`）**：iTerm2 风格的深集成（window→tab、pane→split + 完整历史）。要实现控制协议解析 / pane 映射 / resize 协商，工作量与维护极大，且**用户目标（回看历史）不需要**。否。旧 spec `2026-06-18-tmux-control-mode-integration-design.md` 已就此设计，现搁置为"未来可选高级集成"。
- **fork/patch alacritty_terminal**（让 alt grid 出厂带历史）：原计划如此，但 `grid_mut()` / `update_history()` 已是 pub，运行时即可补历史——**无需 fork**，省掉第二个 fork 的维护。否。
- **在我方层自截备用屏滚出行**：行在 alacritty `scroll_up` 内部就被丢，外层拿不到；要么镜像整 grid 比对（脆弱、内存翻倍）。可行性极低。否。
- **复刻 iTerm2 "清屏(Ctrl-L)存历史"特例**（`saveScrollBufferWhenClearing` + CUP(1,1)+ED 识别）：alacritty 无此行为；属可选增强，本次不做。
- **做成设置项**（iTerm2 有，默认开）：YAGNI，先常开，有人要关再补。

## Consequences

- **不引入新 fork**：纯用 alacritty 公开 API，无 `[patch.crates-io]` 新增、无 rebase 维护负担。
- 备用屏程序（tmux / vim / less 等）**统一**获得滚轮回看历史——通用特性，非仅 tmux。
- 进备用屏时给 alt grid 撑 ≈ 主屏 `scrolling_history` 行历史；每次进入先 `update_history(0)` 清上轮残留，本会话从空历史开始。
- 防污染依赖"滚动区顶部 = 第 0 行"：极少数应用若用非 0 顶部滚动区做主内容滚动，历史可能不全（可接受，与 iTerm2 同限）。
- 已知小限：若"进备用屏 + 大量滚动输出"挤在**同一个 process_output chunk**，撑历史发生在该 chunk advance 之后，本 chunk 内滚出的行不入历史。实测 tmux 先发 `?1049h` 再逐帧输出、首帧只填屏不滚出——不受影响。
- 单测：`terminal_runtime.rs::tests::{alt_screen_retains_scrollback, primary_screen_scrollback_baseline, alternate_scroll_off_by_default_enabled_by_app}`；`lib.rs` 的 `terminal_grid_snapshot_reports_alt_screen_for_alt_scroll` 同步改为默认关。
- 真机已验证：tmux 跑 `seq 1 100`，滚动条出现、滚轮可上下回看历史、拖动可用。
