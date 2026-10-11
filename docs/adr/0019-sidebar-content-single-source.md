# 标题栏侧栏按钮与主机概览采集按「侧栏内容」统一判断

Status: accepted (2026-10-10)

## 背景

标题栏左侧的侧栏按钮在主机管理页开关分组导航，在终端页开关主机概览，但概览只支持已保存的 SSH 主机。原来只对串口 / Telnet 隐藏按钮，结果在本地终端、RDP、设置页、进程 / 网络 / 系统信息详情页上，按钮都点了没反应。详情页还会被判为「支持概览」，按钮亮着却不画侧栏。

另外，SSH 标签只要打开过概览，之后收起侧栏或切走，后台仍每 3 秒新开连接执行一次采集，直到断线。

## 决策

- **侧栏内容单一来源**：`sidebar_helpers::sidebar_content(page, active_kind, ssh_source_tab)` 纯函数求出 `SidebarContent::{None, HostLibraryNav, HostOverview { source_tab }}`。
  - 主机管理页为分组导航，设置页没有侧栏。
  - 终端页要同时满足：该标签布局带侧栏（详情页、RDP 不带），且来源终端的主机是已保存的 SSH 主机。
  - Git Diff / 代码查看器沿用 `source_terminal_tab_index()` 回溯到来源终端，显示那台机器的概览，与文件面板一致。
- **三处都按它判断**：标题栏按钮显隐（`render_sidebar_toggle`）、侧栏渲染（`should_render_host_overview_sidebar`）、概览采集（`sync_host_overview_monitor`）。侧栏里的交互（切网卡、打开详情页、结束进程）用 `overview_tab_index()` 取概览所在的标签。
- **没有侧栏就收起按钮**，不置灰：这些页面根本没有侧栏，置灰会让人以为只是暂时不可用。按钮槽宽用弹簧动画（`SpringAnim`，约 200 ms）收成 0，主页按钮和标签平滑左移补位，图标随槽宽缩放淡出；收展途中不响应点击。最初留同尺寸空位防 tab 行跳动，实测红绿灯与主页按钮间的空白不好看，改为动画收起。
- **侧栏本身推入推出**（2026-10-10 补）：开关侧栏、在有 / 无概览的标签之间切换时，侧栏与终端一起平移约 200 ms（`SidebarPush` + `sidebar_slide`），不再整块闪现。终端直接按最终宽度布局，PTY 只 resize 一次；收起途中画最后显示的那个标签的概览，那个标签已关就直接落定。详情页 / RDP 没有侧栏布局，进去时直接落定，回来不会凭空滑出上一个侧栏。
- **文件 / git 按钮同样收展**：git 按钮在非本地标签、文件按钮在孤儿查看标签上收起，用与侧栏按钮同一套 `SlotReveal` 槽宽动画，右侧按钮跟着滑，不再跳 24 px。
- **概览采集只跟着显示中的侧栏跑**：收起侧栏、切到别的标签或页面就停掉采集，保留最后一帧；再显示时 `HostOverviewResume` 接上网速历史，且已有数据时不发缺速率的首帧（先采一轮记下，1 秒后再采），CPU / 网速不会闪成空。详情页的采集就是页面本身，不受侧栏影响。
- `sidebar_open` 仍是全局持久化开关，不按标签存：切到没有侧栏的标签只是不显示，切回自动恢复。

## 影响

- 新增 `src/sidebar_helpers.rs`、`src/sidebar_push.rs` 与 RootView 的 `sidebar_toggle_slot` / `file_button_slot` / `git_button_slot` / `sidebar_slide`（`SlotReveal`，计入 `any_ui_anim_active`）；删除 `should_show_host_overview_sidebar` / `should_run_host_overview_monitor` 与 `active_tab_supports_host_overview`。
- `spawn_host_overview_monitor` 多一个 `HostOverviewResume` 参数；主机状态页的 fleet 传默认值，行为不变。
- 切回 SSH 标签或重新打开侧栏时要重新建采集连接，先显示上一帧（标「更新中」），连上后约 1 秒刷新。

## 未做

- 本地终端的「本机概览」（`sysinfo` 采集，产出与远程相同的快照）：做了以后 `sidebar_content` 加一个本机来源即可，按钮会在本地标签上重新出现。
- 后台的进程 / 网络 / 系统信息详情页仍持续采集。
