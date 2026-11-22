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

## 第 3 步：搜索框、右键菜单、行尾「+」、悬停提示

- 从 `host_management.rs` 的 `filtered_hosts` 里抽出共用的匹配逻辑。
- 右键菜单用精简版：主页卡片菜单里的内联改名等功能依赖主页，不能照搬。
- 搜索框聚焦时停掉终端的键盘输入，按 Esc 把焦点还给终端。

## 每步验证

- `cargo check --target x86_64-pc-windows-gnu`。
- 实测：开关面板、切标签时终端只 resize 一次。
