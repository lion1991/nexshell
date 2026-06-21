# RootView 拆分为多文件 impl 块，按 UI 面板组织

Status: accepted (2026-05-27, revised 2026-05-28)

## 背景

`main.rs` 拆分前 16125 行；第一阶段抽出 6 个 helper 模块（`ui_settings`、`ui_colors`、
`font_enumeration`、`title_bar_chrome`、`git_panel_view_helpers`、`file_panel_view_helpers`，
共 −1684 行）后当前 14441 行。其中：

- `RootView` god object：120+ 字段，3 个 `impl RootView` 块（1497 / 5414 / 3303 行），
  加 `impl Entity/View/TypedActionView` 约 12000 行
- 顶层自由函数 41 个（终端 helper、字体加载、应用启动等），共约 1500 行
- `mod macos_window_util` 263 行（macOS 平台辅助）
- `mod tests` 865 行（48 个 test fn，混测 RootView 方法与顶层 helper）

项目 CLAUDE.md 已规定"单个 .rs 文件超过 ~800 行视为偏大，超过 1500 行必须拆分"——本决策落实
这条规则在 RootView 上的执行。

同类项目对比：Wezterm 单文件封顶 3.6k、Alacritty 3.3k、Zellij 最大业务文件 ~10k（叫 `screen.rs`，
按职责命名）。Warp 单文件可达 27k 行，但都按职责命名（`terminal/view.rs`），主入口很薄。
我们的问题不是行数本身，而是"入口 + god object + 一堆混杂 helper 全堆在 main.rs"。

## 决策

### 形态与组织

- **多个 `impl RootView` 块跨文件**。`RootView` struct + `impl Entity/View` + `RootView::new()`
  住 `root_view/mod.rs`；其他 impl 按面板分到 `root_view/<面板>_section.rs`。
- **按 UI 面板/区域切分**。每个面板文件同时含 `render_*` 与 `handle_*` action handler。
- **main.rs 终态**：只剩 `fn main`、`mod` 声明、i18n 初始化、菜单/keystroke/text-input 注册、
  `open_main_window` 等启动装配代码，目标 < 500 行。

### 命名规则（避免与现有模块冲突）

`src/` 下已有：
- 状态/worker 模块：`git_panel.rs`、`file_panel.rs`、`host_management.rs`
- 视图子模块目录：`host_management_view/`、`settings_view/`
- helper 模块（第一阶段抽出）：`git_panel_view_helpers.rs`、`file_panel_view_helpers.rs` 等

为避免命名冲突与读者困惑，`root_view/` 下面板文件统一加 `_section` 后缀：
- `root_view/git_panel_section.rs`
- `root_view/file_panel_section.rs`
- `root_view/host_library_section.rs` — 主机管理列表页（区别于下条）
- `root_view/host_monitor_section.rs` — 终端 tab 内嵌的主机监控（进程/网络/系统信息）。
  改名自原 `host_overview_section`，避免与状态层 `src/host_overview.rs` 同名混淆
- `root_view/settings_section.rs`
- `root_view/tab_bar_section.rs`
- `root_view/terminal_section.rs`
- `root_view/find_section.rs`
- `root_view/context_menus_section.rs`

共 **9 个面板**。后缀 `_section` 传达"这是 RootView 方法集合，不是独立类型/状态/子组件"。

**main.rs / root_view 的 mod 组织**：
- `main.rs` 只写 `mod root_view;`（不在 main.rs 直接声明 9 个子模块）。
- `root_view/mod.rs` 顶部声明 9 个 `mod <面板>_section;` 子模块，并写 `RootView` struct +
  `impl Entity/View/TypedActionView` + `RootView::new()`。
- 各 `_section.rs` 文件只写 `impl RootView { fn handle_xxx / fn render_xxx_panel ... }`。

**主机相关三层视图代码定位**（避免读者困惑改哪个文件）：

| 文件 | 职责 |
|---|---|
| `src/host_management.rs` | 数据 / 状态：`HostManagementState`、Group/Card 数据结构、持久化 |
| `src/host_management_view/` (5 子模块共 2454 行) | 独立子组件 Element/View impl：`host_card`、`search_bar`、`group_nav`、`selection_bar` |
| `root_view/host_library_section.rs` | RootView 上的 action handler + render 入口；把上面两层组装到 RootView 渲染链路 |

`host_library` 与 `host_monitor` 是两个不同面板：前者是"主机管理"页面（卡片列表、编辑/分组等），
后者是终端 tab 内嵌的"主机监控"区（进程/网络/系统信息，相关 action 包括
`ToggleHostNetworkDropdown` / `SelectHostNetwork` / `SortHostProcesses` / `KillRemoteProcess`
/ `OpenProcessList` / `OpenNetworkList` / `OpenSystemInfo`）。

> 联动提示：`OpenProcessList` / `OpenNetworkList` / `OpenSystemInfo` 按渲染入口归 host_monitor。
> 若未来 `src/host_overview.rs`（状态层）拆为 `process_list` / `network_list` / `system_info`
> 三个子模块（见"不在本次范围"中标注的 ⚠️ 待办），这 3 个 action 的 section 归宿可能需相应调整。

**面板覆盖的"跨面板感"action 明确归宿**：

- `terminal_section.rs` 含 14 个分屏 action：`SplitRight` / `SplitDown` / `SplitLeft` /
  `SplitUp` / `ClosePane` / `FocusPane` / `NavigatePane{Left,Right,Up,Down}` /
  `StartPaneResizing` / `PaneResizeMove` / `EndPaneResizing` / `ToggleMaximizePane`。
  分屏是终端区域的布局行为，统一归 terminal_section。
  ⚠️ **`terminal_section` 预计为最大 section**：单文件要同时承载分屏、终端 render、
  键盘事件、overlay dispatch 四类正交职责，远超其他 section（多数只 render + handler 两类），
  失控风险显著高于面板平均。**预拆阈值取 1000 行**——经验整数，比通用 1500 阈值显著低
  但又给单文件留 1000 行回旋空间；没有严格推导，意在让 step 7 不等 1500 兜底触发就主动拆。
  附录 A 填完后必须先做粗估，**若 > 1000 行，step 7 直接拆 `terminal_render_section` +
  `terminal_split_section`** 两个文件。拆分时 action 归宿：
  - **分屏 action（附录 A #85-#98 共 14 个：SplitRight..ToggleMaximizePane，含
    FocusPane / NavigatePane{Left,Right,Up,Down}）→ `terminal_split_section.rs`**
  - **其余终端 action（含 #1-#3 CopySelection/PasteClipboard/ClearVisibleScreen、
    #7-#9 font size、#40-#41 TerminalMouseDown/ShowTerminalContextMenu）+ render +
    键盘事件 + overlay dispatch → `terminal_render_section.rs`**（render 是主线，
    键盘/overlay 是 render 链路上的辅助）
  - **step 7 实施记录（2026-05-29）**：粗估并实测整段 902 行，**低于 1000 预拆线**，
    故不强制拆两个并列 section；但沿用 step 1/3/4/6 已落地的**目录约定**建
    `root_view/terminal_section/`，内部按上面 render-vs-split 概念切成三个子模块——
    `render.rs`（560，render 链 + 光标投影 + 键盘/overlay 判定）、`actions.rs`（77，
    #1-3/#7-9/#40 handler）、`split.rs`（252，#85-98 分屏）。即"两个并列 section"的
    字面方案被等价的目录子模块替代，反巨石目标一致、每文件 <800。`ShowTerminalContextMenu`
    菜单内容仍留 main.rs 待 step 9；`reset_active_terminal_view_state` /
    `set_terminal_font_size` 等共享 infra 留 main.rs，靠孙模块私有访问零 pub 改动。
    `render_sidebar_panel`（终端 tab 左侧主机监控侧栏 assembler）按与 `render_file_panel` /
    `render_git_panel` 对称的归宿放进 `host_monitor_section/overview.rs`（与其调用的 6 个
    `render_overview_*` 叶子同住），terminal_section 仅通过
    `render_active_tab_body_with_side_panels` 跨 section 调一次，不再反向依赖 host_monitor 的 6 个 render。
- `tab_bar_section.rs` 含 Chrome actions：`ToggleSidebar` / `WindowMinimize` /
  `WindowToggleMaximize` / `WindowClose`（参见 `terminal_grid_element.rs:713` 注释）。
  这几个概念上是全局窗口操作，但触发入口在 tab bar / title bar 区域，按"触发点"语义归属。

### 拆分前置盘点（step 1 前必做）

启动 step 1 前必须先完成三份清单，**直接以表格形式追加到本 ADR 末尾"附录 A/B/C"**
（不开临时文档，评审/修订集中在同一文件）：

1. **附录 A — action 全量分类清单**：列出 `TerminalGridAction` 所有 variant（约 177 个），
   每个标注归属面板。防止某个 variant 在 step 1-8 都没归宿、step 9 才发现遗漏。
2. **附录 B — 顶层自由 fn 归宿清单**：列出 main.rs 41 个顶层 fn，每个标注目标位置
   （helper 模块 / 留 main.rs 启动代码 / 新建模块）。
3. **附录 C — test fn 归宿清单**：列出 main.rs `mod tests` 48 个 test fn，每个标注
   归宿面板/模块，特别识别跨面板集成测试（标 `main_integration`）。详细规则见"测试搬迁
   策略"段。

三份清单确认后再开 step 1。附录骨架见文末。

### 面板模块边界约束

- **面板模块之间禁止互相 `use`**。共享逻辑只走两条路径：
  1. 通过 `&mut self` 访问 RootView 字段
  2. 抽到 `*_view_helpers.rs` 自由函数（与第一阶段 helper 模块约定一致）
- **多面板调用的 `&self` 方法按语义归宿，不开 `common.rs`**。例如
  `active_tab_supports_host_overview()` 被 git_panel / host_monitor / tab_bar 多处调用，
  但语义属于"tab 能力查询"，归 `tab_bar_section.rs`，其他面板通过 `self.xxx()` 跨文件调用。
- 这两条约束防止循环 use、防止某个面板悄悄变成"事实上的公共模块"。
- **`*_section.rs` 内禁止定义新的无 `&self` 自由函数**。新增 helper 一律加到对应
  `*_view_helpers.rs`，section 文件保持纯 `impl RootView`。每个 `*_section.rs` 文件顶部
  写入这条约定的简短注释。
- **跨面板调用的 `&self` 方法用 `pub(super)` 标记**（mod.rs 同级即 `root_view` 模块）。
  调用方靠 rust-analyzer 跳转，**不维护汇总注释**（注释易腐烂）。
  - 注意 `pub(super)` 在 Rust 里仅表示"父模块可见"，无法直接区分"跨 section 调用"与
    "mod.rs 内部辅助"。本 ADR 约定：**mod.rs 内部辅助方法（如 `new()` 的私有 helper、
    `impl View::render()` 的私有 helper）一律保持 private 默认**，只有真正被某个
    `*_section.rs` 调用的方法才升级到 `pub(super)`。
  - 这样 `pub(super)` 在 root_view 模块内事实上等价于"允许跨 section 调用"标志，
    code review 时按此读即可（接受偶尔"加了 pub(super) 但暂时只剩 mod.rs 内部用"的
    短暂误报——rust-analyzer 的 unused 提示会引出降级）。
- **过渡期可见性（step 1-10 期间）**：上述 `pub(super)` 约定假设 `impl TypedActionView`
  与 `impl View` 已搬到 `root_view/mod.rs`。但这两个 impl 在 step 11 才整体搬走，
  step 1-10 期间它们仍在 main.rs。从 main.rs 看，`root_view/*_section.rs` 是**孙模块**，
  `pub(super)` 不可见。所以：
  - **被 `impl TypedActionView::handle_action` match arm 调用的 handler**（`handle_xxx`）：
    step 1-10 期间用 **`pub(crate)`**；step 11 mod.rs 收尾后统一降级为 `pub(super)`。
  - **被 `impl View::render` 调用的 render 方法**（`render_xxx_panel`）：同上规则。
  - **仅供同 section 内部调用的方法**：保持 private 默认，不动。
  - **跨 section 但不被 main.rs 直接调用的方法**：直接用 `pub(super)`，不受影响。
  - step 11 收尾时做一次全局 `pub(crate) fn handle_/render_` → `pub(super) fn` 替换，
    并跑 cargo check 验证无误。

### handle_action / render() 主体的拆分与迁移

**3 个 `impl RootView` 块的隐含语义分组**（搬运起点指引，实测分析）：

| 块 | 行范围 | fn 数 | 主题 | 搬运取向 |
|---|---|---|---|---|
| 第 1 块 | 712-2208 | 51 | **infra/构造期**：`new` / editor 创建 / worker 启动 / dispatch 路由 / `attach_*` / `start_*_worker` | 多数留 `mod.rs`（构造+infra），少量按面板归到 section |
| 第 2 块 | 2863-8276 | 95 | **render 集中区**：清一色 `render_*` | 按面板归到对应 section 的 `render_xxx_panel` |
| 第 3 块 | 9572-12874 | 103 | **action 集中区**：action handler / `open_*_tab` / host 操作 / `show_*_context_menu` | 按面板归到对应 section 的 `handle_xxx` |

搬运时优先从对应块取方法，能减少跨块查找成本。三块在拆分结束后整块从 main.rs 删除。

- **拆分中间状态留 main.rs**。每步把面板相关的 match arm body / render inline 块抽成
  `self.handle_xxx(...)` / `self.render_xxx_panel(...)` 调用，body 搬到对应面板文件。
- **收尾 step 11 把 `impl TypedActionView` + `impl View` 整块搬到 `root_view/mod.rs`**。
  此时 `handle_action` 全是单行 arm、`render()` 只剩主 stack/flex 组装 + 几个
  `self.render_xxx_panel(...)` 调用——这是**最终形态**，搬过去后不再修改。
- **原 3 个 `impl RootView` 块**：方法搬空后整块从 main.rs 删除。最终 main.rs 不含任何
  `impl RootView` 块。

### Context menus 与各面板的协作约定（让 step 1-8 不依赖 step 9）

`show_*_context_menu` / `*_context_menu_items` 集中在 main.rs:10294-10782 一块连续区域，
统一在 step 9 抽到 `root_view/context_menus_section.rs`。step 1-8 各面板：

- **只允许调用 `self.show_xxx_context_menu(ctx, ...)`**，菜单的具体 `MenuItemFields` 构造
  / dispatch 实现留在原位（main.rs）等 step 9 整块搬。这样 step 1-8 不依赖 step 9 完成
  即可独立推进。
- 面板 step 不"顺手"把同一面板的菜单实现一起搬走——避免 step 9 来时菜单代码已经被分散到
  多个 section、又得二次集中。
- step 9 完成后，菜单的具体内容（用什么 i18n key、disable 条件）仍按面板归属各自放在
  `context_menus_section.rs` 内的 `git_panel_*_items()` / `file_panel_*_items()` 等 fn 中。

### 方法命名

- action handler 统一用 `handle_<action_name>(...)` 前缀（如
  `handle_git_panel_stage_paths`），便于在面板文件里一眼识别"这是 action 处理"。
- render 维持现有 `render_xxx_panel/section` 风格。
- 归属规则：按主要触发场景，不按 method 名前缀机械分。横切的全局基础设施
  （如 `ui_colors()` getter、`cached_warp_theme` 更新）留 mod.rs。
- 不设 `misc.rs` / `shared.rs` / `common.rs` 避免新垃圾桶（详见"拒绝的备选"）。

### 顶层自由函数与启动代码归宿

main.rs 的 41 个顶层自由 fn 按场景归宿：

- **终端视图 helper**（`cursor_blink_visible` / `update_cursor_blink` /
  `terminal_window_title` / `terminal_tab_original_label` / `terminal_disconnected_notice_text`
  / `split_pane_header_*` / `terminal_tab_kind_uses_side_panel_layout` /
  `terminal_palette_ansi_color` 等）→ 新建 `src/terminal_view_helpers.rs`
- **字体加载**（`load_nexshell_monospace_font` / `load_nexshell_ui_font`）→ 加到
  现有 `src/font_enumeration.rs`
- **应用启动**（`register_terminal_key_bindings` / `register_menu_global_actions` /
  `register_warp_text_input_stack` / `register_warp_appearance` / `nexshell_menu_bar` /
  `open_main_window` / `configure_warp_text_input_custom_action_key_bindings` /
  `warp_text_input_custom_tag_to_keystroke` / `dispatch_to_root_view`）→ **留 main.rs**，
  这就是启动代码本身
- **杂项小函数**（`optional_text` / `shorten_path_for_badge` / `truncate_path_display` /
  `find_match_label` / `terminal_clear_key_binding` / 各种 overlay dispatch mode 等）→
  按使用场景归到对应 helper 模块或 `terminal_view_helpers.rs`

### macos_window_util 独立模块

`main.rs` 内 263 行的 `mod macos_window_util`（macOS 平台辅助）**独立成 `src/macos_window_util.rs`**，
不放 `root_view/`（它与 RootView 完全无关）。在 macos 拆分相关 step 一并处理（或独立小 step）。

### 测试搬迁策略

- 每个面板 step 把对应 RootView 方法的测试**一起搬**到面板文件底部 `#[cfg(test)] mod tests`。
- 顶层自由 fn 的测试跟着 fn 搬到对应 helper 模块。
- main.rs 顶部的 `mod tests` 终态保留少量真正"跨模块 / i18n / 启动"测试，预计 < 200 行。
- **附录 C — test fn 归宿清单**（step 0 必做，与附录 A/B 并列；见"拆分前置盘点"段）：
  48 个 test fn 中有跨面板集成测试（如
  `git_diff_tab_uses_side_panel_layout_so_git_panel_stays_visible`，同时涉及 tab /
  git_panel / 终端布局）。step 0 阶段统一标归宿，避免搬运时反复扯皮。

### Struct 字段分组注释（step 11 必做项）

搬 `struct RootView` 到 `mod.rs` 时按面板/职责加分组注释，方便定位字段：

```rust
struct RootView {
    // === 路由 ===
    window_id: WindowId,
    app_page: AppPage,

    // === 主机库（host_library_section） ===
    host_state: HostManagementState,
    host_view_states: RefCell<HostManagementViewStates>,
    ...

    // === Git 面板（git_panel_section） ===
    git_panel_width: f32,
    git_history_height: f32,
    ...
}
```

### handle_action 分组注释（step 11 必做项）

收尾后 `handle_action` 在 mod.rs 内将含 170+ 个单行 arm，按面板分组加注释让调度图可读：

```rust
fn handle_action(&mut self, action: TerminalGridAction, ctx: &mut ViewContext<Self>) {
    match action {
        // === Git 面板 ===
        TerminalGridAction::GitPanelStagePaths { paths } => self.handle_git_panel_stage_paths(paths, ctx),
        TerminalGridAction::GitPanelCommit => self.handle_git_panel_commit(ctx),
        ...

        // === 文件面板 ===
        TerminalGridAction::FilePanelDownload { name, is_dir } => self.handle_file_panel_download(name, is_dir, ctx),
        ...
    }
}
```

### 不在本次范围内

- **`lib.rs` (4546 行)**：96% 是 `mod tests` (4350 行)，生产代码仅 196 行。
  god 测试文件不是 god 生产文件，独立任务处理（拆测试到 `tests/` 子目录）。
- **`terminal_grid_element.rs` (4603 行)**：本质是 `impl Element` 渲染元件，由 RootView
  实例化嵌入。它的内部拆分是独立任务。
- **现有 `src/git_panel.rs` / `file_panel.rs` / `host_management.rs`** 等状态模块：
  本决策只移动 RootView 的方法，不动这些 lib 模块的内部结构。
- **`src/host_overview.rs` (1414 行)**：已接近 CLAUDE.md 1500 阈值的"主机监控状态"模块
  （注意区别于本 ADR 的 `root_view/host_monitor_section.rs` 视图方法集合，section 改名后
  二者不再同名）。本次只标注 ⚠️，下次独立 ADR 处理（候选拆分：`process_list` /
  `network_list` / `system_info` 三个子模块）。

### 第一阶段 helper 模块的阈值守护

`git_panel_view_helpers.rs` 已 777 行，逼近 800 阈值。后续 step 把面板 section 抽出后
需复查行数，**避免误把 helper 与 section 合并**——helper 是无 `&self` 的纯函数，section 是
`impl RootView`，混在一起反而难读。正确方向是 **再拆 helper**（按子主题拆 `git_commit_decoration.rs`
/ `git_history_layout.rs` 等），不是合并。

## 拒绝的备选

- **Trait extension**（`trait GitPanelView for RootView`）：强制接口边界但 Rust trait 跨文件
  共享 self 状态时阅读多一层间接；调用方还要 `use GitPanelView;` 才能看到方法。
- **状态外移成子 model**（`GitPanelView` 独立 struct）：最彻底，但等于重写，
  与"防止破坏功能"诉求冲突。
- **按职责层切**（render.rs / actions.rs / state.rs 三大文件）：看一个面板要跨三个文件跳，
  与"改某面板就看一个文件"的诉求反。
- **分流函数**（`handle_action` → `dispatch_git/dispatch_file_panel`）：`TerminalGridAction`
  是平面 enum，没有天然命名空间分组，需人工判断每个 variant 归哪个面板，错了静默走漏。
- **`root_view/<面板>.rs` 无后缀命名**：与 `src/git_panel.rs` 等同名冲突，读者困惑。
- **`root_view/common.rs` / `shared_state.rs` 集中横切方法**（即使加"3+ 面板调用"准入标准）：
  会打散方法的语义归宿（如 `active_tab_*` 本该归 tab_bar），仍是变相 misc。
  替代方案：按语义归宿到主要面板，其他面板通过 `self.xxx()` 跨文件调用。

## 后果

- 改某面板只看一个文件；新人能从 `root_view/mod.rs` 的 `handle_action` match 一眼看清
  action 调度图后跳转到面板文件。
- 多文件 impl 同类型是稍非主流的 Rust 模式（多数教程都是单文件 impl），与 Warp 内部
  view.rs 27k 行的"单文件大"路线不同。Rust 工具链（rust-analyzer / cargo / IDE 跳转）
  对此完整支持，serde、wezterm 等大型 crate 都使用。
- `RootView::new()` 仍是 350 行巨型构造器，本次不拆。若后续优化可单独抽 `RootViewBuilder`,
  在新 ADR 记录。
- **`root_view/mod.rs` 豁免 CLAUDE.md 1500 阈值**：mod.rs 起步预估 ~2466 行
  （struct + 字段分组注释 ~200 / `new()` ~350 / `impl Entity` 2 / `impl View::render()` 534 /
  `impl TypedActionView` 含分组注释 ~1350 / 9 个 `mod *_section;` 声明 ~30）。该量级
  在产业内是 root view god struct 的成熟形态：wezterm `termwindow/mod.rs` 3629 行、
  Zed `editor.rs` 12137 行、Zed `workspace.rs` 15984 行，均健康在用。CLAUDE.md 1500 阈值
  是普通业务模块约定，不适用于此类容器。**mod.rs 不预设硬阈值，只要 9 个 section 拆出后
  mod.rs 不再持续增长即视为达标**。如未来确需二次拆分（如增加新 View trait 实现使 render
  膨胀），届时独立 ADR 处理。
- **section 文件保留 1500 阈值**：`root_view/*_section.rs` 任一超 1500 行触发独立 ADR
  二次拆分（候选：terminal_section → `terminal_render_section` + `terminal_split_section`；
  git_panel_section → `git_status_section` + `git_history_section` 等）。附录 A/B/C 填完后
  可对各 section 行数做粗估，提前发现高危 section。
- 拆分按面板增量推进，**前置盘点 → 11 个 step**：
  - step 0: action 全量分类 + 顶层 fn 归宿清单（前置盘点，已 commit 不算 step）
  - step 1: `git_panel_section` （首拆验证方案）
  - step 2: `file_panel_section`
  - step 3: `host_library_section`
  - step 4: `host_monitor_section`（新增）
  - step 5: `settings_section`
  - step 6: `tab_bar_section`
  - step 7: `terminal_section`
  - step 8: `find_section`
    - **实施记录（2026-05-29）**：单文件 322 行（符合 ADR 字面命名，无偏差不建目录）。
      搬出 create_find_editor（pub(crate)，new() 调用）、handle_find_editor_event（私有，
      仅 create_find_editor 订阅闭包调用）、close_find_bar / handle_open_find_bar /
      handle_find_step（pub(crate)，handle_action 分发）、render_find_bar（pub(crate)，
      从 View::render 内联 `if find_state.active` overlay 块抽出，返回 `Option<Box<dyn Element>>`，
      caller 用 `if let Some` 挂到 terminal_stack）。find_match_label 自由 fn 仍留 main.rs 待 step 10（已于 step 10 迁 terminal_view_helpers）。
  - step 9: `context_menus_section`
    - **实施记录（2026-05-29）**：单文件 639 行（符合字面命名，无目录）。搬出 18 方法 + 3 测试：
      6 个 render_*_context_menu（pub(crate)，View::render 调用）、8 个 show_*/toggle_*（pub(crate)，
      handle_action 分发）、4 个 *_items（私有，仅本文件 show_* 调用）；3 个测试（附录 C 归此）体未改，
      super:: 解析到本模块 import 绑定。Appearance::as_ref 走 SingletonEntity path-syntax，需 `as _` 导入。
      main.rs 同删 7 个变 unused 的 import（GitDiffKind/GitDiffSelection、apply_file_panel_*/
      FilePanelSelectMode、apply_git_panel_selection/GitPanelSelectMode、local|remote_file_panel_context_menu_items、
      git_panel_context_menu_items/git_panel_context_paths、horizontal_tab_context_menu_items/HorizontalTab*/
      TAB_COLOR_OPTIONS），5030→4412 行。warning 净零（23=23），测试 223+107 通过。
      terminal_context_menu_offset_bounds / *_position_is_window_bounded 按附录归 terminal_view_helpers（step 10）。
  - step 10: 顶层自由 fn 重定位 + `macos_window_util` 独立
    - **实施记录（2026-05-29）**：附录 B 的 41 个顶层 fn 按归宿落位——新建
      `terminal_view_helpers.rs`（602 行，26 fn：23 pub(crate) + 3 私有 normalized_serial_port /
      shorten_path_for_badge / truncate_path_display；17 测试）、新建 `host_monitor_view_helpers.rs`
      （38 行，format_usage_metric / overview_usage_bar_width / format_uptime + 1 测试）、
      `font_enumeration.rs` 追加 load_nexshell_monospace_font / load_nexshell_ui_font
      （+ `use warpui::fonts`）+ 2 测试；内联 `mod macos_window_util` 独立成 `src/macos_window_util.rs`
      （65 行，cfg macos，引用路径 `crate::macos_window_util` / `macos_window_util::` 不变）。
      main.rs 仅留 10 个启动装配 fn（register_* / dispatch_to_root_view / nexshell_menu_bar /
      open_main_window / main），4411→3379。顶部加 3 个 mod 声明 + `use terminal_view_helpers`(12) /
      font load_nexshell_*(2)，删随 fn 迁出而 unused 的 3 组 import（warpui 10 element + ColorU、
      monospace/ui_font_families、format_bytes_short/DiskMetric/UsageMetric）；7 个 section 改
      `use crate::*_view_helpers::` 路径（find / tab_render / terminal render / tab_bar actions /
      host_monitor system+overview / host_library edit_window）。
    - **附录 C 测试归位**：顺带补搬 step1/第一阶段遗漏的 16 个 helper 测试（git_panel_view_helpers 15 +
      title_bar_chrome 1），main.rs `mod tests` 达终态——仅剩 3 个 main_integration（git_diff 布局 /
      zh-CN 措辞 / keystroke fallback）。GIT_COMMIT_EDITOR_MIN/MAX_HEIGHT 系 const 留 main.rs，
      git_panel 测试改用 `crate::` 引；title_bar 测试因模块顶部已 `use super::{常量}` 故零改。
      warning 零新增（step10 去重 20 ⊂ HEAD 23，并消除 3 条 mod tests 历史 unused：PathBuf /
      GitStatusSnapshot+GitPanelState / DiskMetric）；测试 223+107 通过。
    - ⚠️ git_panel_view_helpers.rs 含测试后达 1017 行（生产 777 + 测试 ~240），超 800 偏大线（<1500）；
      按"第一阶段 helper 模块的阈值守护"段，其再拆（git_commit_decoration / git_history_layout 等）
      为独立任务，step 10 不处理。
  - step 11: 收尾——`impl Entity/View/TypedActionView` 整体搬入 `mod.rs`、`struct RootView`
    搬入并加分组注释、`new()` 搬入、原 3 个 `impl RootView` 块从 main.rs 删除
    - **实施记录（2026-05-29）**：`struct RootView` + 3 个 `impl RootView` 块 + `impl Entity/View/
      TypedActionView` 全部迁入 `root_view/mod.rs`（main.rs 3379→853，mod.rs →2635，符合"豁免 1500、
      不再增长"约定）。main.rs 加 `pub(crate) use root_view::RootView;` 重导出，保持全库 `crate::RootView`
      路径零改动。可见性：`struct RootView` 升 `pub(crate)`（重导出需）、`new()` 升 `pub(super)`（main.rs
      open_main_window 调用）、字段 `active_tab_index` 升 `pub(crate)`（main.rs close_tab 全局 action 读取）；
      其余字段/infra 方法保持私有，section（root_view 后代）靠后代私有访问零改动。
    - **可见性 blanket 的偏差修正**：ADR 原文写 `pub(crate) fn handle_/render_ → pub(super)`，但多数
      section 已演化成**嵌套目录子模块**（`terminal_section/render.rs` 等是 root_view 的孙模块），
      被 **mod.rs 派发**的 handle_/render_ 若用 `pub(super)` 只到达其父 section、够不到 mod.rs。故这 180 个
      统一改 **`pub(in crate::root_view)`**（精确覆盖 root_view 整棵树、不外泄 crate）。
      注意可见性实为三层、并非全改 pub(super)：① mod.rs 派发的 handle_/render_ → `pub(in crate::root_view)`；
      ② section 内同级方法互调（非 handle_/render_，约 18 处）→ 维持 `pub(super)` 未动；③ 仅本文件内用 → private。
      另有约 43 个非 handle_/render_ 的 RootView 方法（editor 构造 / split_active_pane / run_git_* / connect_host 等）
      仍是 step 1-9 设的 `pub(crate)`：本 step 按 ADR 字面只动 handle_/render_，未顺带收紧（无外部调用方，
      可作后续一致性清理统一降 `pub(in crate::root_view)`）。
    - **伴生类型留 crate root**：`TabModel`/`TerminalSessionKind`/`TerminalSessionTab`/`CursorBlinkState`/
      `TabMoveDirection`/`AppPage`/`FilePanelInputIntent`/`HostPasswordIntent`/`EmbeddedAssets`/全部布局常量
      仍在 main.rs（被 helper/section 全库 `crate::X` 引用）；mod.rs 经 `use crate::{…}` 引入。
      `terminal_view_helpers` 一处测试 `crate::ThemeChoice`（旧捷径靠 main.rs import）改规范路径
      `crate::terminal_grid_element::ThemeChoice`。
    - **分组注释（必做）**：struct 加 15 处 `// === 面板 ===` 分隔、handle_action 加 14 处（两处旧 `---` 升 `===`）；
      字段/arm **均未重排**（实际非严格按面板排布，交错处如实标注），仅插注释，保证忠实搬迁。
    - warning 净零（19 ⊂ baseline 20，并顺带消除 1 条 HEAD 死 import internal_colors）；
      cargo build 链接通过，测试 lib 223 + bin 107 全过。
    - ⚠️ main.rs 853 行（>800 偏大、<1500）：残留 = 启动装配 fn + 上述伴生数据类型（`TerminalSessionTab`
      约 90 字段最重）。把这些会话数据类型抽到独立模块（如 `src/terminal_session.rs`）以压回 <800 为**独立后续任务**，
      step 11 不处理（与 step 10 标注 git_panel_view_helpers 再拆同属"独立任务"惯例）。
  - **拆分全部完成**：step 0–11 落地，main.rs 不再含 god object，RootView 按面板分布于 `root_view/`。
- 每 step 独立 commit + `cargo check/test`；每 2-3 个面板跑一次 `cargo run` 走核心 UI 交互。
  出错立即 `git revert HEAD`。

---

## 附录 A — TerminalGridAction 全量分类清单（step 0 产物）

来源：`src/terminal_grid_element.rs` `pub enum TerminalGridAction`（实测 177 个 variant，
按 enum 声明顺序列出）。

汇总：terminal 22 / find 3 / tab_bar 26 / host_monitor 10 / host_library 37 / file_panel 26
/ git_panel 28 / settings 24 / dead code 1 = **177**。

> step 0 归属全部确认（不留 `?`）。`CloseAllDropdowns` 经 `grep -rn CloseAllDropdowns src/`
> 确认全库无 dispatch 站，标 dead code，由独立 issue 清理，不进入任何 section。

| # | Variant | 归属 | 备注 |
|---|---|---|---|
| 1 | `CopySelection` | terminal | |
| 2 | `PasteClipboard` | terminal | |
| 3 | `ClearVisibleScreen` | terminal | |
| 4 | `OpenFindBar` | find | |
| 5 | `CloseFindBar` | find | |
| 6 | `FindStep(i32)` | find | |
| 7 | `IncreaseFontSize` | terminal | 终端字体缩放 |
| 8 | `DecreaseFontSize` | terminal | |
| 9 | `ResetFontSize` | terminal | |
| 10 | `ToggleSidebar` | tab_bar | Chrome |
| 11 | `WindowMinimize` | tab_bar | Chrome |
| 12 | `WindowToggleMaximize` | tab_bar | Chrome |
| 13 | `WindowClose` | tab_bar | Chrome |
| 14 | `ToggleHostNetworkDropdown` | host_monitor | |
| 15 | `SelectHostNetwork(String)` | host_monitor | |
| 16 | `SortHostProcesses(...)` | host_monitor | |
| 17 | `SortHostNetwork(...)` | host_monitor | |
| 18 | `CopyHostAddress(String)` | host_monitor | |
| 19 | `NewTab` | tab_bar | |
| 20 | `ToggleNewSessionMenu` | tab_bar | |
| 21 | `SelectTab(usize)` | tab_bar | |
| 22 | `MoveTabLeft(usize)` | tab_bar | |
| 23 | `MoveTabRight(usize)` | tab_bar | |
| 24 | `RenameTab(usize)` | tab_bar | |
| 25 | `ResetTabName(usize)` | tab_bar | |
| 26 | `CloseTab(usize)` | tab_bar | |
| 27 | `CloseOtherTabs(usize)` | tab_bar | |
| 28 | `CloseTabsRight(usize)` | tab_bar | |
| 29 | `ReconnectTab(usize)` | tab_bar | |
| 30 | `DuplicateTab(usize)` | tab_bar | |
| 31 | `ToggleTabColor { color, tab_index }` | tab_bar | |
| 32 | `ActivatePrevTab` | tab_bar | |
| 33 | `ActivateNextTab` | tab_bar | |
| 34 | `ToggleTabRightClickMenu { tab_index, anchor }` | tab_bar | 菜单内容归 context_menus（step 9） |
| 35 | `TabHoverWidthStart { width }` | tab_bar | |
| 36 | `TabHoverWidthEnd` | tab_bar | |
| 37 | `StartTabDrag` | tab_bar | |
| 38 | `DragTab { tab_index, tab_position }` | tab_bar | |
| 39 | `DropTab` | tab_bar | |
| 40 | `TerminalMouseDown` | terminal | |
| 41 | `ShowTerminalContextMenu { position, has_selection }` | terminal | 触发点；菜单内容归 context_menus |
| 42 | `HostShowContextMenu { host_id, position }` | host_library | |
| 43 | `HostClipboardCopy(String)` | host_library | |
| 44 | `HostClipboardCut(String)` | host_library | |
| 45 | `HostClipboardPaste` | host_library | |
| 46 | `HostRestoreDeleted` | host_library | |
| 47 | `HostRenameInline(String)` | host_library | |
| 48 | `HostEditOne(String)` | host_library | |
| 49 | `HostDeleteOne(String)` | host_library | |
| 50 | `HostQuickConnect(String)` | host_library | |
| 51 | `HostToggleSelect(String)` | host_library | |
| 52 | `HostSelectSingle(String)` | host_library | |
| 53 | `HostToggleSelectAll` | host_library | |
| 54 | `HostSelectGroup(String)` | host_library | |
| 55 | `HostToggleTag(String)` | host_library | |
| 56 | `HostCycleProtocol` | host_library | |
| 57 | `HostToggleProtocolDropdown` | host_library | |
| 58 | `HostSetProtocolFilter(...)` | host_library | |
| 59 | `HostSetViewMode(...)` | host_library | |
| 60 | `HostTogglePrivacy` | host_library | |
| 61 | `HostRefresh` | host_library | |
| 62 | `HostNewHost` | host_library | |
| 63 | `HostDeleteSelected` | host_library | |
| 64 | `HostEditSelected` | host_library | |
| 65 | `HostConnectSelected` | host_library | |
| 66 | `HostEnterReorderMode` | host_library | |
| 67 | `HostExitReorderMode` | host_library | |
| 68 | `HostStartCardDrag` | host_library | |
| 69 | `HostDragCard { host_id, card_position }` | host_library | |
| 70 | `HostDropCard` | host_library | |
| 71 | `HostClearSelection` | host_library | |
| 72 | `HostImport` | host_library | |
| 73 | `HostExport` | host_library | |
| 74 | `HostPasswordConfirm` | host_library | |
| 75 | `HostPasswordCancel` | host_library | |
| 76 | `HostCloudSync` | host_library | |
| 77 | `HostManageGroupsTags` | host_library | |
| 78 | `HostBackToTerminal` | host_library | 触发在 host library 内"返回"按钮 |
| 79 | `ShowHostManagement` | tab_bar | 触发在 title bar 主机库按钮；与 Chrome action 同区域，归 tab_bar |
| 80 | `OpenProcessList` | host_monitor | |
| 81 | `OpenNetworkList` | host_monitor | |
| 82 | `OpenSystemInfo` | host_monitor | |
| 83 | `ProcessListShowContextMenu { ... }` | host_monitor | 菜单内容归 context_menus |
| 84 | `KillRemoteProcess { pid, label }` | host_monitor | |
| 85 | `SplitRight` | terminal | 分屏 |
| 86 | `SplitDown` | terminal | |
| 87 | `SplitLeft` | terminal | |
| 88 | `SplitUp` | terminal | |
| 89 | `ClosePane` | terminal | |
| 90 | `FocusPane(NexPaneId)` | terminal | |
| 91 | `NavigatePaneLeft` | terminal | |
| 92 | `NavigatePaneRight` | terminal | |
| 93 | `NavigatePaneUp` | terminal | |
| 94 | `NavigatePaneDown` | terminal | |
| 95 | `StartPaneResizing(DraggedBorder)` | terminal | |
| 96 | `PaneResizeMove(Vector2F)` | terminal | |
| 97 | `EndPaneResizing` | terminal | |
| 98 | `ToggleMaximizePane` | terminal | |
| 99 | `ToggleFilePanel` | file_panel | |
| 100 | `FilePanelRefresh` | file_panel | |
| 101 | `FilePanelGoUp` | file_panel | |
| 102 | `FilePanelEnterDir(String)` | file_panel | |
| 103 | `FilePanelSelect { name, mode }` | file_panel | |
| 104 | `FilePanelTreeItemClicked { path, is_dir, mode }` | file_panel | |
| 105 | `FilePanelDropFiles(Vec<String>)` | file_panel | |
| 106 | `FilePanelShowContextMenu { name, is_dir, position }` | file_panel | 菜单内容归 context_menus |
| 107 | `FilePanelDownload { name, is_dir }` | file_panel | |
| 108 | `FilePanelOpenUploadDialog` | file_panel | |
| 109 | `FilePanelCancelTransfer(u64)` | file_panel | |
| 110 | `FilePanelDelete { name, is_dir }` | file_panel | |
| 111 | `FilePanelStartRename { name }` | file_panel | |
| 112 | `FilePanelStartNewDir` | file_panel | |
| 113 | `FilePanelStartNewFile` | file_panel | |
| 114 | `FilePanelStartNewFileIn { parent }` | file_panel | |
| 115 | `FilePanelCdToDirectory { path }` | file_panel | |
| 116 | `FilePanelOpenDirectoryInNewTab { path }` | file_panel | |
| 117 | `FilePanelRevealInFileManager { path }` | file_panel | |
| 118 | `FilePanelCopyPath { name }` | file_panel | |
| 119 | `FilePanelCopyRelativePath { path }` | file_panel | |
| 120 | `FilePanelInputConfirm` | file_panel | |
| 121 | `FilePanelInputCancel` | file_panel | |
| 122 | `FilePanelResizeStart(f32)` | file_panel | |
| 123 | `FilePanelResizeMove(f32)` | file_panel | |
| 124 | `FilePanelResizeEnd` | file_panel | |
| 125 | `ToggleGitPanel` | git_panel | |
| 126 | `GitPanelRefresh` | git_panel | |
| 127 | `GitPanelSelectDiff { path, kind }` | git_panel | |
| 128 | `GitPanelSelectEntry { path, kind, mode }` | git_panel | |
| 129 | `GitPanelStage(String)` | git_panel | |
| 130 | `GitPanelStageAll(Vec<String>)` | git_panel | |
| 131 | `GitPanelUnstage(String)` | git_panel | |
| 132 | `GitPanelStagePaths { tab_id, paths }` | git_panel | |
| 133 | `GitPanelUnstagePaths { tab_id, paths }` | git_panel | |
| 134 | `GitPanelAddToGitignore { tab_id, paths }` | git_panel | |
| 135 | `GitPanelShowContextMenu { ... }` | git_panel | 菜单内容归 context_menus |
| 136 | `GitPanelDiscardWorktreeChanges { tab_id, path }` | git_panel | |
| 137 | `GitPanelResizeStart(f32)` | git_panel | |
| 138 | `GitPanelResizeMove(f32)` | git_panel | |
| 139 | `GitPanelResizeEnd` | git_panel | |
| 140 | `GitHistoryResizeStart(f32)` | git_panel | |
| 141 | `GitHistoryResizeMove(f32)` | git_panel | |
| 142 | `GitHistoryResizeEnd` | git_panel | |
| 143 | `GitHistoryScrolled { tab_id, scroll_start, delta_y }` | git_panel | |
| 144 | `GitCommitRowHover { tab_id, sha, hovered }` | git_panel | |
| 145 | `GitCommitDetailHover { tab_id, sha, hovered }` | git_panel | |
| 146 | `GitCommitSelect { tab_id, sha }` | git_panel | |
| 147 | `GitCommitHoverSweep` | git_panel | |
| 148 | `GitCommitCopySha(String)` | git_panel | |
| 149 | `GitCommitEditorFocus` | git_panel | |
| 150 | `GitCommitConfirm` | git_panel | |
| 151 | `GitCommitDiscard` | git_panel | |
| 152 | `GitPushConfirm` | git_panel | |
| 153 | `ToggleSettingsMenu` | settings | 与 SettingsMenuWhatsNew/Documentation/Feedback/ViewLogs 同组，按菜单内容归 settings |
| 154 | `SettingsMenuWhatsNew` | settings | |
| 155 | `SettingsMenuDocumentation` | settings | |
| 156 | `SettingsMenuFeedback` | settings | |
| 157 | `SettingsMenuViewLogs` | settings | |
| 158 | `ShowSettings` | settings | |
| 159 | `ShowSettingsKeybindings` | settings | |
| 160 | `CloseSettingsTab` | settings | |
| 161 | `SettingsSelectPage(NexSettingsSection)` | settings | |
| 162 | `SetTheme(ThemeChoice)` | settings | |
| 163 | `SetTerminalFontSize(f32)` | settings | |
| 164 | `SetCursorBlink(bool)` | settings | |
| 165 | `SetOpacity(u8)` | settings | |
| 166 | `SetCursorStyle(CursorStyleChoice)` | settings | |
| 167 | `SetFontFamily(String)` | settings | |
| 168 | `SetFontWeight(warpui::fonts::Weight)` | settings | |
| 169 | `SetLineHeight(f32)` | settings | |
| 170 | `ResetLineHeight` | settings | |
| 171 | `ToggleFontDropdown` | settings | |
| 172 | `ToggleFontWeightDropdown` | settings | |
| 173 | `ToggleViewAllFonts` | settings | |
| 174 | `CloseAllDropdowns` | **dead code** | handler 仅 `ctx.notify()`，全库 grep 无 dispatch 站；独立 issue 清理，不参与本次拆分 |
| 175 | `ShowThemeChooser` | settings | |
| 176 | `CloseThemeChooser` | settings | |
| 177 | `SetLanguage(LanguageChoice)` | settings | |

## 附录 B — main.rs 顶层自由函数归宿清单（step 0 产物）

来源：`src/main.rs` 顶层 41 个 `fn`（含 `fn main`；当前全是裸 `fn`，无 `pub fn`）。

汇总：terminal_view_helpers 26（新建） / host_monitor_view_helpers 3（新建） /
font_enumeration 2（追加） / 留 main.rs 10 = **41**。

> 备注：本表初始归属偏粗。**触发条件式二次拆**：当某个面板 section（step 1-8）累计在
> terminal_view_helpers.rs 沉淀的纯函数 ≥ 3 个时，即时新建对应 `<面板>_view_helpers.rs`
> 把这些函数挪过去（如 step 6 拆 tab_bar_section 时发现 `close_button_element` 等 3+ 个
> tab_bar 专用 helper 已堆在 terminal_view_helpers，就新建 `tab_bar_view_helpers.rs`）。
> 单一面板 helper 不足 3 个时维持现状不动，避免散碎模块。

> 不保留行号列：每次代码改动行号都会失效；函数名在 main.rs 顶层唯一，
> `grep -n "fn 函数名" src/main.rs` 即可定位（不锚定 `^`，兼容未来的 `pub fn`）。

| 函数名 | 目标位置 | 备注 |
|---|---|---|
| `format_usage_metric` | host_monitor_view_helpers.rs（新建） | CPU/Mem 度量格式化 |
| `overview_usage_bar_width` | host_monitor_view_helpers.rs | usage bar 宽度 |
| `terminal_keyboard_input_enabled` | terminal_view_helpers.rs（新建） | |
| `format_uptime` | host_monitor_view_helpers.rs | 主机启动时长 |
| `terminal_tab_original_label` | terminal_view_helpers.rs | |
| `terminal_disconnected_notice_text` | terminal_view_helpers.rs | |
| `inactive_terminal_runtime` | terminal_view_helpers.rs | |
| `normalized_serial_port` | terminal_view_helpers.rs | 串口辅助 |
| `serial_port_from_host_config` | terminal_view_helpers.rs | |
| `connected_serial_tab_port` | terminal_view_helpers.rs | |
| `occupied_serial_port_index` | terminal_view_helpers.rs | |
| `terminal_palette_ansi_color` | terminal_view_helpers.rs | |
| `load_nexshell_monospace_font` | font_enumeration.rs（追加） | |
| `load_nexshell_ui_font` | font_enumeration.rs（追加） | |
| `optional_text` | terminal_view_helpers.rs | 通用小函数 |
| `close_button_element` | terminal_view_helpers.rs | tab_bar 专用，触发二次拆条件时挪 tab_bar_view_helpers |
| `find_match_label` | terminal_view_helpers.rs | find 专用，触发二次拆条件时挪 find_view_helpers |
| `update_cursor_blink` | terminal_view_helpers.rs | |
| `cursor_blink_visible` | terminal_view_helpers.rs | |
| `terminal_window_title` | terminal_view_helpers.rs | |
| `terminal_context_menu_offset_bounds` | terminal_view_helpers.rs | |
| `root_overlay_event_dispatch_mode` | terminal_view_helpers.rs | overlay 通用 |
| `terminal_overlay_event_dispatch_mode` | terminal_view_helpers.rs | |
| `root_debug_key_log` | terminal_view_helpers.rs | debug |
| `shorten_path_for_badge` | terminal_view_helpers.rs | |
| `split_pane_header_badge_title` | terminal_view_helpers.rs | 分屏 header |
| `split_pane_header_badge_icon` | terminal_view_helpers.rs | |
| `terminal_tab_kind_uses_side_panel_layout` | terminal_view_helpers.rs | |
| `split_pane_header_background_color` | terminal_view_helpers.rs | |
| `truncate_path_display` | terminal_view_helpers.rs | |
| `terminal_clear_key_binding` | terminal_view_helpers.rs | |
| `register_terminal_key_bindings` | 留 main.rs | 启动装配 |
| `dispatch_to_root_view` | 留 main.rs | 启动装配 |
| `register_menu_global_actions` | 留 main.rs | 启动装配 |
| `register_warp_text_input_stack` | 留 main.rs | 启动装配 |
| `register_warp_appearance` | 留 main.rs | 启动装配 |
| `warp_text_input_custom_tag_to_keystroke` | 留 main.rs | 启动装配 |
| `configure_warp_text_input_custom_action_key_bindings` | 留 main.rs | 启动装配 |
| `nexshell_menu_bar` | 留 main.rs | 启动装配 |
| `open_main_window` | 留 main.rs | 启动装配 |
| `main` | 留 main.rs | 入口 |

## 附录 C — test fn 归宿清单（step 0 产物）

来源：`src/main.rs` 顶部 `mod tests`（实测 48 个 test fn）。

汇总：terminal_view_helpers 17 / git_panel_view_helpers 15 / git_panel_section 5 /
context_menus_section 3 / main_integration (main.rs) 3 / font_enumeration 2 /
host_monitor_view_helpers 1 / title_bar_chrome 1 / file_panel_section 1 = **48**。

> `main_integration` 表示跨面板集成测试或启动/i18n/keystroke 类测试，目标位置为
> `src/main.rs` 顶部 `mod tests`（这部分测试不随任何 section 迁出，预计 < 200 行）。
>
> 不保留行号列：每次代码改动行号都会失效；test fn 名在 `mod tests` 内唯一，
> `grep -n "fn 测试名" src/main.rs` 即可定位。

| Test fn | 归宿 | 备注 |
|---|---|---|
| `cursor_blink_toggles_at_interval_and_resets_when_disabled` | terminal_view_helpers | |
| `terminal_window_title_uses_runtime_title_or_default` | terminal_view_helpers | |
| `remote_terminal_tab_label_prefers_connection_label_over_runtime_title` | terminal_view_helpers | |
| `serial_terminal_tab_label_prefers_connection_label_over_runtime_title` | terminal_view_helpers | |
| `local_terminal_tab_label_still_prefers_runtime_title` | terminal_view_helpers | |
| `git_diff_tab_uses_side_panel_layout_so_git_panel_stays_visible` | main_integration (main.rs) | 跨 git+tab+终端布局 |
| `remote_terminal_disconnect_notice_uses_runtime_status` | terminal_view_helpers | |
| `serial_terminal_disconnect_notice_uses_runtime_status` | terminal_view_helpers | |
| `inactive_terminal_runtime_replaces_previous_runtime_after_all_tabs_close` | terminal_view_helpers | |
| `occupied_serial_port_index_matches_trimmed_ports_and_can_skip_current_tab` | terminal_view_helpers | |
| `remote_split_pane_badge_uses_connection_label_when_runtime_title_is_empty` | terminal_view_helpers | |
| `remote_split_pane_badge_uses_remote_icon` | terminal_view_helpers | |
| `split_pane_header_background_is_opaque` | terminal_view_helpers | |
| `host_overview_usage_bar_width_is_consistent_across_metrics` | host_monitor_view_helpers | |
| `git_commit_decoration_label_hides_remote_branch_name` | git_panel_view_helpers | |
| `git_commit_decoration_badge_classifies_local_and_remote_refs` | git_panel_view_helpers | |
| `git_commit_decoration_badges_show_local_and_remote_refs_on_same_commit` | git_panel_view_helpers | |
| `git_panel_entry_state_key_separates_staged_and_worktree_rows` | git_panel_view_helpers | |
| `git_panel_entry_tooltip_uses_full_status_and_path` | git_panel_view_helpers | |
| `git_panel_stage_all_paths_use_current_section_entries` | git_panel_section | 测 RootView 方法 |
| `git_panel_context_menu_uses_section_appropriate_batch_actions` | context_menus_section | |
| `local_file_panel_context_menu_matches_warp_project_explorer_order` | context_menus_section | |
| `remote_file_panel_context_menu_keeps_sftp_actions` | context_menus_section | |
| `local_file_panel_relative_path_uses_project_root` | file_panel_section | |
| `git_panel_footer_switches_to_push_only_when_clean_and_ahead` | git_panel_section | |
| `git_panel_footer_keeps_push_surface_for_clean_branch_without_upstream` | git_panel_section | |
| `git_panel_footer_keeps_commit_mode_while_changes_exist` | git_panel_section | |
| `git_panel_body_loading_keeps_existing_clean_snapshot_visible` | git_panel_section | |
| `git_commit_editor_uses_wrapping_autogrow_layout_bounds` | git_panel_view_helpers | |
| `git_push_busy_label_animates_without_localized_ellipsis` | git_panel_view_helpers | |
| `git_ssh_host_key_prompt_info_exposes_fingerprint_and_raw_prompt` | git_panel_view_helpers | |
| `terminal_keyboard_input_pauses_for_overlay_editors` | terminal_view_helpers | |
| `git_commit_row_visual_hover_tracks_actual_mouse_position` | git_panel_view_helpers | |
| `git_commit_copy_payload_prefers_full_sha` | git_panel_view_helpers | |
| `git_commit_detail_time_formats_relative_and_absolute_time` | git_panel_view_helpers | |
| `git_commit_hover_target_survives_detail_hover_and_stale_row_exit` | git_panel_view_helpers | |
| `git_commit_hover_target_waits_before_clearing_between_row_and_detail` | git_panel_view_helpers | |
| `git_commit_detail_target_does_not_pin_click_selection_after_hover_leaves` | git_panel_view_helpers | |
| `git_history_scroll_load_more_triggers_only_near_bottom_while_scrolling_down` | git_panel_view_helpers | |
| `title_bar_layout_keeps_windows_controls_on_right` | title_bar_chrome | 测的是 `super::title_bar_chrome_layout(...)`，跟 helper 模块走 |
| `terminal_context_menu_position_is_window_bounded` | terminal_view_helpers | |
| `root_overlay_stack_uses_waterfall_event_dispatch` | terminal_view_helpers | |
| `terminal_overlay_stack_uses_waterfall_event_dispatch` | terminal_view_helpers | |
| `zh_cn_pane_menu_labels_use_window_wording` | main_integration (main.rs) | i18n |
| `windows_font_candidates_include_cjk_families_first` | font_enumeration | |
| `available_windows_fonts_only_include_detected_families_with_cjk_first` | font_enumeration | |
| `terminal_clear_key_binding_matches_warp_platform_policy` | terminal_view_helpers | |
| `warp_text_input_custom_actions_have_keystroke_fallbacks` | main_integration (main.rs) | 启动/keystroke |

