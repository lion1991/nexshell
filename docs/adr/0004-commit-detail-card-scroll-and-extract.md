# 提交详情卡：内容滚动化 + 抽出独立模块

Status: accepted (2026-05-30)

## 背景

git 提交历史的「详情卡」（hover/选中某 commit 弹出的浮层）渲染在
`git_panel_view_helpers.rs` 的 `render_git_commit_detail_card`。它是锚定在 commit 行上的
positioned overlay（`history_section.rs` 的 `Stack` + `add_positioned_overlay_child`，
`PositionedElementOffsetBounds::WindowByPosition`，相对**窗口**裁剪）。

两类问题：

1. **文件列表显示不全 / 很乱**：文件列表用裸 `Flex::column()`，既无 `max_height` 也无滚动容器。
   commit 涉及文件多时卡片无限向下拉高、底部超出窗口被裁，且每行是 `path  +14 -10` 一长串文本，
   长路径溢出、无层次、增删数不着色。
2. **正文同类溢出**：提交正文（body）同样无高度上限，长 body（本仓库 ADR 提交很常见）也会把整卡撑过窗口。

同时 `git_panel_view_helpers.rs` 改动前已 ~1017 行，超 crate `CLAUDE.md` 的 helper 800 行阈值；
继续往里堆详情卡渲染加剧超限，违反「给已有大文件追加代码前先考虑顺手抽取到新模块」。

参照实现：Warp `app/src/code_review/git_dialog`（`render_file_changes_box` /
`render_file_list` / `split_file_path`）——文件列表套 `ClippedScrollable + max_height(130)`，
每行「文件名(主色) + 目录(灰) + 右侧 绿+/红-」。

## 决策

**A. 详情卡可变高区段一律滚动化**，复用 `ClippedScrollable + ConstrainedBox::with_max_height`，不造轮子：

- 文件列表：`scroll_capped(list, files_scroll_state, GIT_COMMIT_DETAIL_FILES_MAX_HEIGHT=220)`。
- 提交正文：`scroll_capped(body, body_scroll_state, GIT_COMMIT_DETAIL_BODY_MAX_HEIGHT=160)`。
- 头部/标题/统计/SHA·复制保持固定可见（含复制按钮的 footer 不随内容滚走）。
- 滚动状态按短 SHA 索引存 tab：新增 `git_panel_commit_detail_files_scroll_states` /
  `git_panel_commit_detail_body_scroll_states`（沿用既有 `git_panel_commit_detail_states` 等
  per-sha map 模式；同样暂无清理，与现状一致）。
- 文件行重构为 Warp `render_file_list` 同款：文件名（主色，`new_inline` 始终可见，溢出在 paint 期淡出裁剪）
  + 目录（灰，`split_file_path` 末尾保留斜杠）+ 右侧 `+N`(绿 `colors.download`) `-N`(红 `colors.upload`)，
  `Expanded(1.0)` 包名字段把统计推到右侧固定。改名显示 `新名  ← 旧路径`；二进制/无 numstat 不显示统计。
- overlay 内 `ClippedScrollable` 的滚轮事件由 `Stack` waterfall（topmost-first）正确拦截，
  不会误触发历史列表的 load-more（已核 warpui `stack`/`scrollable`/`clipped_scrollable` 源码）。

**B. 抽出独立模块** `git_commit_detail_helpers.rs`：把详情卡渲染整簇
（`render_git_commit_detail_card` + 文件行/统计 + `split_file_path`/`git_commit_file_change_display`
+ 详情卡专用的 `git_commit_stat_label`/`git_commit_copy_payload`/`format_git_commit_authored_at_for_detail*`
及其单测）从 `git_panel_view_helpers.rs` 移出。

- 入口 `render_git_commit_detail_card` 保持 `pub(crate)`，由 `history_section.rs` 调用；其余转 private。
- 提交行渲染（`render_git_panel_commit_row_content`）、装饰徽章、hover/scroll 状态机等**留在**
  `git_panel_view_helpers.rs`（与提交行共享，非详情卡专用）。
- 抽出后 `git_panel_view_helpers.rs` 降到 ~735 行（回落 800 内），新模块 ~448 行。

GIT_COMMIT_DETAIL_FILES_MAX_HEIGHT 取 220（Warp 用 130）：Warp 是独立 modal，本卡是窄(360)
window 锚定 overlay，220 ≈ 11 行更合手；非机械照搬像素。

## Considered Options

- **只 cap 文件列表、不动正文**：只解决一半；本仓库长 body 常见，整卡仍会被窗口裁。否。
- **整卡套单个外层滚动**：头部/复制按钮会一起滚走，且文件列表失去独立视口；分区段各自滚更可控。否。
- **正文只截断淡出不滚动**：长正文尾部不可见、信息丢失；与文件列表处理不对称。否。
- **本次不拆模块、记 TODO**：文件本已超阈值、继续堆加更糟；详情卡簇内聚、抽出成本可控，遂本次一并做。

## 影响

- 改动文件：`git_commit_detail_helpers.rs`(新)、`git_panel_view_helpers.rs`、`main.rs`（常量+tab 字段+mod）、
  `root_view/mod.rs`（字段初始化）、`history_section.rs`（取双滚动句柄+传参+import 迁移）。
- 行为：详情卡高度有界，文件列表/正文超出可滚；视觉对齐 Warp。其余 git 面板逻辑不变。
