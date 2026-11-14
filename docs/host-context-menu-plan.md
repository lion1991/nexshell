# 主机卡片/列表 右键菜单 — 技术方案（已定稿）

目标：给 `host_management_view` 的卡片与列表行加右键菜单。
架构：native-shell（Rust gpui），**不涉及已废弃的 nexshell/src React**。
原则：照搬项目内已跑通的 file_panel 右键菜单链路（warp::menu），不造轮子。
菜单**全部平铺、无二级子菜单**（Warp Submenu 被官方标 deprecated，弃用）。

## 已确认决策

- A 重命名 = **卡片内联输入框**（仿 file_panel `file_panel_input_editor`）。
- B 排序 = 右键直接进入**已有拖拽重排模式** `HostEnterReorderMode`；
  新建 = 单项直接开**新建主机窗口** `HostNewHost`。两者均无子菜单。
- C 作废（不做按字段排序；拖拽模式自身已 `update_host_sort_orders` 落库）。
- D 复制/剪切/粘贴、恢复删除 = **会话内内存态**，应用重启失效。
- E 改分组（2026-11-13 Matt 拍板）= 右键「移到分组…」+ 多选栏「分组」按钮，不必进编辑窗口。
  点「移到分组…」后在原菜单位置弹出**独立的分组列表菜单**（仍不用子菜单）：无分组 + 全部分组，
  目标同属一个分组时该项打勾；末项「管理分组…」打开已有的分组/标签管理窗口。
  右键目标在多选集合里时作用于整个选中集，否则只改这一台；从选中集发起的移完收起选择栏。
  **暂不做**：把卡片拖到左侧分组栏（跨区域拖放，且与拖拽排序模式冲突），等右键用一阵再议。

## 最终菜单（平铺，分隔区用 `MenuItem::Separator`）

| 菜单项 | action | 状态 |
|---|---|---|
| 连接 | `HostQuickConnect(id)` | ✅ 复用 |
| — | Separator | |
| 编辑 | 选中目标 + `HostEditSelected` | ✅ 复用 |
| 重命名 | `HostRenameInline(id)` → 卡片内联 editor | 🆕 |
| 移到分组… | `HostShowGroupPicker{Some(id),pos}` → 同位置弹分组列表菜单 | 🆕（决策 E）|
| — | Separator | |
| 复制 | `HostClipboardCopy(id)` | 🆕 状态 |
| 剪切 | `HostClipboardCut(id)` | 🆕 状态 |
| 粘贴 | `HostClipboardPaste`（clipboard 空时 `with_disabled`）| 🆕 |
| — | Separator | |
| 删除 | 选中目标 + `HostDeleteSelected` | ✅ 复用 |
| 恢复 | `HostRestoreDeleted`（无备份时 `with_disabled`）| 🆕 状态 |
| — | Separator | |
| 复制地址 | `CopyHostAddress(id)` | ✅ 复用 |
| — | Separator | |
| 新建 | `HostNewHost` | ✅ 复用 |
| 排序 | `HostEnterReorderMode` | ✅ 复用 |

右键单台时仿 file_panel：show 函数里若目标不在选择集 → 先 `select_single`，
再复用 `HostEditSelected/HostDeleteSelected`。

## 链路（照搬 file_panel 模板，6 处改动）

1. **ShellModel 字段**（仿 main.rs:822）
   `host_card_context_menu: ViewHandle<warp::menu::Menu<TerminalGridAction>>`
   `show_host_card_context_menu: Option<Vector2F>`
2. **构造+订阅关闭**（仿 main.rs:1232）
   `add_typed_action_view(|_| Menu::new().with_drop_shadow())` +
   `subscribe_to_view` 听 `Event::Close` → `show_...=None; notify`。
3. **右键接入**（host_card.rs 卡片 + 列表行 Hoverable）
   `.on_right_click(move |ctx,_,pos| ctx.dispatch_typed_action(
     TerminalGridAction::HostShowContextMenu{host_id, position:pos}))`
4. **新增 action**（terminal_grid_element.rs）
   `HostShowContextMenu{host_id,position}`、`HostRenameInline(id)`、
   `HostClipboardCopy(id)`、`HostClipboardCut(id)`、`HostClipboardPaste`、
   `HostRestoreDeleted`。
5. **action 处理 + show 函数**（仿 main.rs:6508 / 7912）
   选中目标 → 构建 items → `menu.set_items` → `show_...=Some(pos)` →
   关其他菜单 → `focus` → `notify`。
6. **overlay 渲染**（仿 main.rs:2336 + render fn 2496）
   `add_positioned_overlay_child(ChildView::new(&self.host_card_context_menu),
     OffsetPositioning::offset_from_parent(pos,
       terminal_context_menu_offset_bounds(), TopLeft, TopLeft))`。

## 新增状态（HostManagementState，会话内内存态）

```rust
host_clipboard: Option<(String /*host_id*/, ClipOp /*Copy|Cut*/)>,
deleted_host_backup: Option<HostRecord>,   // 删除前完整记录，供"恢复"
```
- 粘贴：取源 host 完整记录 → `create_host`（名字加"副本"、新 id）；
  Cut 粘贴后删源 + 清 clipboard。
- 恢复：`delete_selected_hosts` 删除前备份记录，恢复时 `create_host` 重建。

## 重命名（决策 A，内联）

仿 file_panel 内联输入：host_card 渲染处增加"该卡片是否处于重命名态"判断，
是则把 name 的 `Text` 换成内联编辑 editor；提交走 `update_host`（仅改 name）。
需新增：重命名目标 id 状态 + 内联 editor handle + 提交/取消处理。

## i18n（locales/zh-CN.yml + en.yml）

仿 `file_panel_ctx_*` 命名，新增：
`host_ctx_connect/edit/rename/copy/cut/paste/delete/restore/copy_address/new/sort`。

## 实施阶段

1. 链路打通 + 复用项（连接/编辑/删除/复制地址/新建/排序）— 验证右键浮层
2. 复制/剪切/粘贴 + 恢复（状态层）
3. 重命名内联 editor
4. 列表行同步右键 + i18n + `cargo build` / 现有单测验证

## 移到分组（决策 E）实现要点

- 分组列表是独立的 `host_group_picker_menu`，**不要改成在主机菜单里原地换内容**：
  - 鼠标点菜单项时，一次点击 push 的 `Select(Index)` / item action / `Close(true)` 由
    warpui 倒序派发（`warpui_core/src/core/app.rs` `actions.into_iter().rev()`）。原地换内容会被先到的 Close 关掉，
    随后到的过期 `Select(Index)` 还会落到新条目上（越界后按 ↓ 会 panic）。
  - 键盘 Enter 时 Menu 在自身 update 内同步派发 item action，再改同一个 Menu 会 panic（Circular view update）。
- 两种顺序下主机菜单的 Close 都可能晚于列表打开：它的订阅在列表显示时不清 `context_menu_target`，
  `refocus_root_after_menu_close` 只在焦点仍在该菜单时才收回，所以列表的焦点不受影响。
- 写库 `set_hosts_group_in_db`：单事务批量 `UPDATE hosts SET group_id`，不经 upsert 整行重写。
