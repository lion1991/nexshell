# 左侧主机列表 + 概览右移：实施计划

决策与取舍见 ADR 0023。每步单独提交，可以单独回滚；每步做完停下来，交给 Matt 用一段时间。

## 第 1 步：概览右移（纯搬家）— 已完成

- `sidebar_helpers.rs`：`sidebar_content(page)` 只管左栏；`overview_panel_source(page, kind, ssh_source)` 给出右侧概览显示哪个标签。
- RootView：新增 `overview_panel_open`（持久化）、`overview_button_slot`、`overview_panel_slide / _pending / _closing`。关门时开关立即置 false、停采集，滑出收敛后由 `finalize_panel_closes` 落闸；`settle_panel_slides` 在切标签时把在途动画落定。
- 渲染：`render_active_tab_body_with_side_panels` 按「终端｜文件｜git 或概览」排布；`render_overview_panel` 改成左边框，滑入沿用 `panel_slide_in_from_right`。
- 删掉 `sidebar_push.rs`，终端页暂时没有左栏，左上角的侧栏按钮在终端页上收起。

## 第 2 步：左侧主机列表（分组折叠 + 单击连接 / 跳转）— 已完成

- 新增 `host_management_view/host_list_nav.rs`（渲染）、`host_list_helpers.rs`（分组、「已打开」映射、单击目标，都是带测试的纯函数）、`root_view/host_list_section.rs`。
- 终端页的 `sidebar_content` 返回 `HostList`（RDP 标签除外）；从 git 历史取回 `SidebarPush` 做推入推出。开关和主页分组导航共用 `sidebar_open`；列表不显示时（主页、设置、RDP）推入状态直接落到开关值，回来时不播动画。
- 算「已打开」时按标签种类过滤：只有 SSH / 串口 / Telnet / 直连 / RDP 会话算；进程 / 网络 / 系统详情页不算；Git Diff / 内置编辑器标签的 `host_id` 存的是来源标签 id，不是主机 id。圆点颜色跟标签栏离线红点同一判断（`is_disconnected`），同一主机有任一会话连着就是绿色。
- 分组按主页顺序，「所有主机」伪分组不列，空分组不列，没分组或分组已删的主机归到最后的「未分组」；全部主机都没分组时不画标题、直接平铺。折叠状态存在 `host_list_collapsed`。
- 单击已开的主机：跳到它的第一个会话；当前就在它的某个会话上时，轮到下一个会话（只有一个时不动）。

## 第 3 步：搜索框、右键菜单、行尾「+」、悬停提示 — 已完成

- 匹配逻辑抽成 `host_management::host_matches_query`（名称 / 地址 / 描述 / 标签），主页 `filtered_hosts` 和左栏共用；左栏过滤是 `host_list_helpers::filter_host_list_sections`。
- 搜索框用独立的 `host_list_search_editor`，和主页搜索词互不影响。搜索时分组全部展开（这时点分组标题不改折叠状态），没有结果时显示「无匹配的主机」。
- 键盘：搜索框聚焦时，终端停止接收键盘输入（计入 `overlay_editor_focused`）。回车打开第一个结果，和单击效果相同；Esc 清空搜索词。回车和 Esc 都会把焦点还给终端。
- 焦点不留在看不见的搜索框里：收起左栏或离开终端页时，`release_host_list_search_focus` 把焦点收回终端。单击主机行后焦点也回到终端。
- 右键菜单复用 `host_card_context_menu`，终端页上换成精简项：新开连接、编辑、复制地址（`host_list_context_menu_items`）。改名、剪贴板、删除等依赖主页，不放进来。右键时这一行保持高亮。
- 行尾「+」：悬停才出现，放在圆点左边，圆点位置不随悬停跳动。点它总是再开一个会话（`HostQuickConnect`）。
- 悬停提示：延迟 500 ms，浮在行右侧，显示主机名和地址。隐私模式下地址打码（`mask_endpoint`，顺带修了非 ASCII 主机名按字节截断会 panic 的问题）。右键菜单打开时不显示提示。

## 每步验证

- `cargo check --target x86_64-pc-windows-gnu`。
- 实测：开关面板、切标签时终端只 resize 一次。
