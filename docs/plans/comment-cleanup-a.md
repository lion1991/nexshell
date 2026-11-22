# 注释清理附表：A 类（叙述型噪音）逐条

盘点 commit `f4dd308a33ef7a3aa5f1d5eabe6c9f7d79c4e358`。主清单见 [`comment-cleanup.md`](comment-cleanup.md)。A 类指只复述代码在做什么的注释，删除即可，不需要逐条拍板；有异议的条目改到主清单里处理。「状态」栏：空着表示待删；「工具依赖」表示删了会让构建或 lint 报错，已恢复，不再删。

## repo-ci-scripts（61）

| 文件 | 行号 | 所在符号 | 注释原文 | 状态 |
|---|---|---|---|---|
| `.forgejo/workflows/windows-release.yml` | 63 | `(顶层)` | # Prefer shallow fetch of the pinned sha; fall back to a full fetch |  |
| `.forgejo/workflows/windows-release.yml` | 118 | `(顶层)` | # Query existing release: 200 reuse, 404 create, anything else fails |  |
| `.gitignore` | 1 | `(顶层)` | # Rust 构建产物 |  |
| `.gitignore` | 5 | `(顶层)` | # 系统 / 编辑器 |  |
| `examples/rdp_probe.rs` | 513 | `main` | // 落盘最终 framebuffer。 |  |
| `locales/en.yml` | 1 | `(顶层)` | # === App === |  |
| `locales/en.yml` | 7 | `(顶层)` | # === Tab types === |  |
| `locales/en.yml` | 80 | `(顶层)` | # === Host overview sidebar === |  |
| `locales/en.yml` | 116 | `(顶层)` | # === Container management === |  |
| `locales/en.yml` | 133 | `(顶层)` | # === Sidebar === |  |
| `locales/en.yml` | 136 | `(顶层)` | # === Menu: new tab === |  |
| `locales/en.yml` | 145 | `(顶层)` | # === Menu: gear === |  |
| `locales/en.yml` | 153 | `(顶层)` | # === Context menu === |  |
| `locales/en.yml` | 196 | `(顶层)` | # === Git Panel === |  |
| `locales/en.yml` | 247 | `(顶层)` | # === Settings === |  |
| `locales/en.yml` | 252 | `(顶层)` | # === Appearance page === |  |
| `locales/en.yml` | 280 | `(顶层)` | # === Cursor styles === |  |
| `locales/en.yml` | 285 | `(顶层)` | # === Keybindings page === |  |
| `locales/en.yml` | 306 | `(顶层)` | # === Dialogs === |  |
| `locales/en.yml` | 319 | `(顶层)` | # === Toast / Notice === |  |
| `locales/en.yml` | 352 | `(顶层)` | # === Host management === |  |
| `locales/en.yml` | 399 | `(顶层)` | # === Host edit form === |  |
| `locales/en.yml` | 454 | `(顶层)` | # === Group / Tag management === |  |
| `locales/en.yml` | 467 | `(顶层)` | # === Shell chrome === |  |
| `locales/en.yml` | 474 | `(顶层)` | # === Tab context menu === |  |
| `locales/en.yml` | 478 | `(顶层)` | # === Tab context menu === |  |
| `locales/en.yml` | 494 | `(顶层)` | # === Terminal recording === |  |
| `locales/en.yml` | 502 | `(顶层)` | # === SSH host key === |  |
| `locales/zh-CN.yml` | 1 | `(顶层)` | # === App === |  |
| `locales/zh-CN.yml` | 7 | `(顶层)` | # === Tab types === |  |
| `locales/zh-CN.yml` | 80 | `(顶层)` | # === Host overview sidebar === |  |
| `locales/zh-CN.yml` | 116 | `(顶层)` | # === Container management === |  |
| `locales/zh-CN.yml` | 133 | `(顶层)` | # === Sidebar === |  |
| `locales/zh-CN.yml` | 136 | `(顶层)` | # === Menu: new tab === |  |
| `locales/zh-CN.yml` | 145 | `(顶层)` | # === Menu: gear === |  |
| `locales/zh-CN.yml` | 153 | `(顶层)` | # === Context menu === |  |
| `locales/zh-CN.yml` | 196 | `(顶层)` | # === Git Panel === |  |
| `locales/zh-CN.yml` | 247 | `(顶层)` | # === Settings === |  |
| `locales/zh-CN.yml` | 252 | `(顶层)` | # === Appearance page === |  |
| `locales/zh-CN.yml` | 280 | `(顶层)` | # === Cursor styles === |  |
| `locales/zh-CN.yml` | 285 | `(顶层)` | # === Keybindings page === |  |
| `locales/zh-CN.yml` | 306 | `(顶层)` | # === Dialogs === |  |
| `locales/zh-CN.yml` | 319 | `(顶层)` | # === Toast / Notice === |  |
| `locales/zh-CN.yml` | 352 | `(顶层)` | # === Host management === |  |
| `locales/zh-CN.yml` | 399 | `(顶层)` | # === Host edit form === |  |
| `locales/zh-CN.yml` | 454 | `(顶层)` | # === Group / Tag management === |  |
| `locales/zh-CN.yml` | 467 | `(顶层)` | # === Shell chrome === |  |
| `locales/zh-CN.yml` | 474 | `(顶层)` | # === Tab context menu === |  |
| `locales/zh-CN.yml` | 478 | `(顶层)` | # === Tab context menu === |  |
| `locales/zh-CN.yml` | 494 | `(顶层)` | # === Terminal recording === |  |
| `locales/zh-CN.yml` | 502 | `(顶层)` | # === SSH host key === |  |
| `scripts/build-dmg.sh` | 4 | `(顶层)` | # ── 配置 ── |  |
| `scripts/build-dmg.sh` | 30 | `(顶层)` | # ── 解析参数 ── |  |
| `scripts/build-dmg.sh` | 42 | `(顶层)` | # ── 1. 编译 ── |  |
| `scripts/build-dmg.sh` | 55 | `(顶层)` | # ── 2. 构建 .app bundle ── |  |
| `scripts/build-dmg.sh` | 63 | `(顶层)` | # Info.plist |  |
| `scripts/build-dmg.sh` | 141 | `(顶层)` | # 如果有 .icns 图标就拷进去 |  |
| `scripts/build-dmg.sh` | 145 | `(顶层)` | # 追加 CFBundleIconFile |  |
| `scripts/build-dmg.sh` | 160 | `(顶层)` | # 唯一指纹，消除同名歧义 |  |
| `scripts/build-dmg.sh` | 172 | `(顶层)` | # ── 4. 打包 DMG ── |  |
| `scripts/cargo-run.sh` | 7 | `(顶层)` | # crate 根 = nexshell/ |  |

## views-misc（93）

| 文件 | 行号 | 所在符号 | 注释原文 | 状态 |
|---|---|---|---|---|
| `src/design_tokens/mod.rs` | 40 | `ChromeColors` | // ── chrome（旧 UiColors）：标题栏 / 标签 / tooltip / 下拉 chrome ── |  |
| `src/design_tokens/mod.rs` | 98 | `OverviewColors` | // ── overview（旧 HostOverviewColors）：主机概览面板 / 图表 ── |  |
| `src/design_tokens/mod.rs` | 150 | `HostColors` | // ── host（旧 HostUiColors）：主机管理列表页 ── |  |
| `src/design_tokens/scale.rs` | 10 | `(顶层)` | // --- 间距（4 步进）--- |  |
| `src/design_tokens/scale.rs` | 18 | `(顶层)` | // --- 圆角 --- |  |
| `src/host_management_view/constants.rs` | 6 | `(顶层)` | // --- 图标路径 --- |  |
| `src/host_management_view/constants.rs` | 36 | `(顶层)` | // --- 面板布局 --- |  |
| `src/host_management_view/constants.rs` | 38 | `(顶层)` | // 密钥页：左侧密钥列表固定宽，右侧详情 Expanded 填充。 |  |
| `src/host_management_view/constants.rs` | 41 | `(顶层)` | // --- 工具栏 --- |  |
| `src/host_management_view/constants.rs` | 46 | `(顶层)` | // --- 搜索栏（顶部工具栏，液态玻璃通栏） --- |  |
| `src/host_management_view/constants.rs` | 56 | `(顶层)` | // --- 卡片 --- |  |
| `src/host_management_view/constants.rs` | 64 | `(顶层)` | // --- 分组导航 --- |  |
| `src/host_management_view/constants.rs` | 67 | `(顶层)` | // --- 通用 UI --- |  |
| `src/host_management_view/container_view.rs` | 259 | `host_section_hidden` | // 未装 docker |  |
| `src/host_management_view/container_view.rs` | 263 | `host_section_hidden` | // 已就绪且无容器 |  |
| `src/host_management_view/group_nav.rs` | 116 | `render_group_nav` | // 功能区（可扩展：未来在此追加更多功能菜单入口） |  |
| `src/host_management_view/group_nav.rs` | 292 | `render_group_item` | // 计数选中态跟随 accent，其余次级色。 |  |
| `src/host_management_view/group_nav.rs` | 351 | `group_marker` | // 分组色标：所有主机用 accent 列表图标，普通分组用按 id 取色的 8×8 圆角色点。 |  |
| `src/host_management_view/group_nav.rs` | 375 | `render_section_title` | // 区块小标题，如"最近访问"（降级为 SMALL 字号、无分隔线）。 |  |
| `src/host_management_view/group_nav.rs` | 393 | `render_recent_item` | // 最近访问项单行：>_ 图标 + 主机名（可截断）+ 弹性空档 + 相对时间恒右，点击快连。 |  |
| `src/host_management_view/group_nav.rs` | 425 | `render_recent_item` | // 24×24 协议色底 + 居中 14 图标（padding=(24-14)/2）。 |  |
| `src/host_management_view/group_nav.rs` | 490 | `relative_time` | // Unix 秒 → 相对时间中文。 |  |
| `src/host_management_view/group_nav.rs` | 647 | `render_function_item` | // 选中时背景恒为 hover 色（不随鼠标变），否则常规 hover 切换。 |  |
| `src/host_management_view/host_card.rs` | 58 | `render_name_element` | // 内联重命名：该卡处于重命名态时，name 处渲染单行输入框替代静态文本 |  |
| `src/host_management_view/host_card.rs` | 122 | `render_host_card` | // hover 颜色 eased 过渡；selected 边框保持瞬时 accent。 |  |
| `src/host_management_view/host_card.rs` | 143 | `render_host_card` | // checkbox 仅 hover/选中时淡入；hover 抬升 = 双层阴影 alpha 渐入。 |  |
| `src/host_management_view/host_card.rs` | 186 | `render_host_card` | // 顶部行：左侧系统图标（协议色底）+ 右侧多选 checkbox（hover/选中时淡入） |  |
| `src/host_management_view/host_card.rs` | 259 | `render_host_card` | // hover 抬升：双层阴影随 hover 淡入（alpha 由过渡驱动）。 |  |
| `src/host_management_view/host_card.rs` | 339 | `protocol_colors` | // 协议标识色：badge 与系统图标底共用，SSH 蓝 / Serial 橙 / RDP 紫 / Telnet 青。 |  |
| `src/host_management_view/host_card.rs` | 409 | `render_tags_row` | // 无标签不渲染占位。 |  |
| `src/host_management_view/host_card.rs` | 565 | `render_host_list_row` | // 行 hover/选中背景 eased 过渡（选中并入目标色，一并平滑）。 |  |
| `src/host_management_view/host_card.rs` | 706 | `render_list_select_checkbox` | // 列表行的多选 checkbox：与卡片视图同语义 |  |
| `src/host_management_view/key_manager_view.rs` | 85 | `divider` | // 列表行间细分隔线（横跨卡片内宽）。 |  |
| `src/host_management_view/key_manager_view.rs` | 96 | `vertical_divider` | // 左右两栏之间的竖向分隔线。 |  |
| `src/host_management_view/key_manager_view.rs` | 148 | `render_key_list_panel` | // 左栏：整面板即拖入导入区；密钥行收在一个大圆角卡片里，行间分隔线。 |  |
| `src/host_management_view/key_manager_view.rs` | 297 | `host_count_chip` | // 中性深色小标签（主机数 / 类型），区别于主色 tag。 |  |
| `src/host_management_view/key_manager_view.rs` | 332 | `render_key_detail_panel` | // 右栏：未选中显示空态，选中显示详情（可滚动）。 |  |
| `src/host_management_view/key_manager_view.rs` | 414 | `render_detail_content` | // 顶部「编辑」按钮（靠右），与编辑态的 Cancel/Save 位置呼应 |  |
| `src/host_management_view/key_manager_view.rs` | 434 | `render_detail_content` | // 信息卡：名称 + 密码短语 |  |
| `src/host_management_view/key_manager_view.rs` | 455 | `render_detail_content` | // 复制公钥到服务器：生成 echo >> authorized_keys 命令，可选中复制去服务器执行 |  |
| `src/host_management_view/key_manager_view.rs` | 467 | `render_detail_content` | // 公钥 |  |
| `src/host_management_view/key_manager_view.rs` | 474 | `render_detail_content` | // 私钥 |  |
| `src/host_management_view/key_manager_view.rs` | 543–544 | `render_copy_to_server` | // 复制公钥到服务器卡：默认折叠成可点击入口；点击复制 echo >> authorized_keys 命令到<br>// 剪贴板并展开命令（绿色等宽、可再次选中复制）。 |  |
| `src/host_management_view/key_manager_view.rs` | 614 | `code_block` | // 代码块：等宽小字号，soft-wrap 按宽度断行，可选中复制。 |  |
| `src/host_management_view/key_manager_view.rs` | 631 | `render_delete_button` | // 删除入口（点击进入二次确认）。 |  |
| `src/host_management_view/key_manager_view.rs` | 682 | `render_delete_section` | // 删除区：未确认显示入口；确认态显示提示 + 「确认删除」(红) / 「取消」。 |  |
| `src/host_management_view/key_manager_view.rs` | 738 | `render_detail_editing` | // 编辑态：顶部 取消/保存，信息卡里名称/口令变可输入。 |  |
| `src/host_management_view/mod.rs` | 158 | `render_host_management_panel` | // 容器视图常驻搜索栏：落在滚动区外，不随列表滚动。 |  |
| `src/host_management_view/search_bar.rs` | 124 | `render_search_bar` | // 全选 + 选择操作只在选择态出现（移出常驻栏） |  |
| `src/host_management_view/search_bar.rs` | 426 | `render_view_mode_toggle` | // 首帧直接落位，不从 0 滑过去。 |  |
| `src/host_management_view/search_bar.rs` | 433 | `render_view_mode_toggle` | // thumb 底层，尺寸/圆角与按钮自画背景块同规格，margin_left 定位。 |  |
| `src/host_management_view/search_bar.rs` | 488 | `render_view_mode_button` | // active 底色交给 thumb，这里只在非 active 时画 hover 底。 |  |
| `src/host_management_view/search_bar.rs` | 582 | `toolbar_divider` | // 工具栏竖向细分隔线，分区用。 |  |
| `src/host_management_view/search_bar.rs` | 599 | `render_toolbar_action_button` | // 次级操作纯图标按钮（隐私/导入/导出/云同步），hover 出背景 + tooltip 显示名字。 |  |
| `src/host_management_view/selection_bar.rs` | 196 | `render_reorder_bar` | // Min：同选择条，胶囊收拢包住内容。 |  |
| `src/host_management_view/status_view.rs` | 68 | `render_status_view` | // 清理已删除主机的 sweep 过渡（key = "{host_id}:mem\|load0..2"）。 |  |
| `src/host_management_view/status_view.rs` | 134 | `render_status_card` | // Load 同心环数据（外→内 = 1/5/15 分钟）：load/核数 归一 + 档位色；无核数只画轨道。 |  |
| `src/host_management_view/status_view.rs` | 141 | `render_status_card` | // 弧走档位色动态变化：正常绿 / 中级橙 / 高负载红（每环按各自 ratio 定档）。 |  |
| `src/host_management_view/status_view.rs` | 183 | `render_status_card` | // 顶部：主机名 + 连接状态 + 终端快连图标 |  |
| `src/host_management_view/status_view.rs` | 212 | `render_status_card` | // 参考卡四列：Load 同心环 / Mem 环 / 网络叠排 / 磁盘叠排。 |  |
| `src/settings_view/appearance_page.rs` | 45 | `AppearancePageState` | // Text 区块 |  |
| `src/settings_view/appearance_page.rs` | 53 | `AppearancePageState` | // Language 区块 |  |
| `src/settings_view/appearance_page.rs` | 144 | `render_appearance_page` | // --- Window --- |  |
| `src/settings_view/appearance_page.rs` | 236 | `render_appearance_page` | // --- Cursor --- |  |
| `src/settings_view/appearance_page.rs` | 278 | `render_appearance_page` | // --- Language --- |  |
| `src/settings_view/appearance_page.rs` | 329 | `render_appearance_page` | // --- 编辑器（文件面板「编辑」用）--- |  |
| `src/settings_view/mod.rs` | 118 | `render_settings_view` | // --- Sidebar --- |  |
| `src/settings_view/mod.rs` | 166 | `render_settings_view` | // --- Content header (居中 "Settings" 标题) --- |  |
| `src/settings_view/mod.rs` | 183 | `render_settings_view` | // --- Content --- |  |
| `src/settings_view/mod.rs` | 233 | `render_settings_view` | // warp: theme_chooser 侧面板 |  |
| `src/settings_view/mod.rs` | 334 | `render_theme_chooser_panel` | // --- 面板容器 --- |  |
| `src/themes/theme.rs` | 361 | `WarpThemeConfig.new` | // preload with built-in themes |  |
| `src/themes/theme.rs` | 546 | `render_preview` | // The border above the input box. |  |
| `src/themes/theme.rs` | 556 | `render_preview` | // The fake cursor within the input box. |  |
| `src/ui_components/avatar.rs` | 62 | `Avatar.build` | // One third of the total avatar height/width should be padding. |  |
| `src/ui_components/avatar.rs` | 76 | `Avatar.build` | // One third of the total avatar height/width should be padding. |  |
| `src/util/bindings.rs` | 11 | `CustomAction` | // CustomActions are attached to menu items, and may be attached to Bindings. |  |
| `src/util/bindings.rs` | 392 | `(顶层)` | // ── 键绑定查询/修改 (端口自 warp util::bindings) ───────────────────────── |  |
| `src/util/extensions.rs` | 1 | `SliceExt` | // 切片扩展：二分查找插入位置（端口自 warp app/src/util/extensions.rs） |  |
| `src/util/grid.rs` | 28 | `grid_cell_dimensions` | // Get the font metrics that make up the total line height for the font. |  |
| `src/util/grid.rs` | 32 | `grid_cell_dimensions` | // Using a ratio, compared to the default line height, to appropriately scale the cell's Height. |  |
| `src/util/grid.rs` | 50 | `grid_cell_dimensions` | // Return a default advance width if we couldn't load the advance from the glyph. |  |
| `src/util/grid.rs` | 67–68 | `calculate_grid_baseline_position` | // Leading is additional spacing between lines.<br>// Descent is the general guideline for how far the font will go below the baseline (for single spacing). |  |
| `src/util/mod.rs` | 9 | `merge_ranges` | // 合并相邻/重叠区间（端口自 warp app/src/util/mod.rs） |  |
| `src/view_components/action_button.rs` | 488 | `ActionButton.set_keybinding` | // Remove any subscription from a previous keybinding. |  |
| `src/view_components/action_button.rs` | 700 | `ActionButton.render` | // Set once an element has been added to the row, to simplify spacing with conditional elements. |  |
| `src/view_components/action_button.rs` | 703 | `ActionButton.render` | // Helper closure to render the keybinding |  |
| `src/view_components/action_button.rs` | 712 | `ActionButton.render` | // Compact mode: render as plain text without boxes |  |
| `src/view_components/action_button.rs` | 735 | `ActionButton.render` | // Standard mode: render with individual key boxes |  |
| `src/view_components/action_button.rs` | 787 | `ActionButton.render` | // Render keybinding before label if configured |  |
| `src/view_components/action_button.rs` | 807–810 | `ActionButton.render` | // Determine the maximum width constraint for the button label:<br>// - If both max_label_width and width are set, use the smaller value to ensure the label fits<br>// - If only one constraint is set, use that value<br>// - If no constraints are set, don't set a width constraint |  |
| `src/view_components/action_button.rs` | 837 | `ActionButton.render` | // Render keybinding after label (default position) if not already rendered before |  |
| `src/view_components/action_button.rs` | 927 | `ActionButton.render` | // buttons that only have an icon should always be square |  |

## root-view（27）

| 文件 | 行号 | 所在符号 | 注释原文 | 状态 |
|---|---|---|---|---|
| `src/root_view/code_viewer_section.rs` | 153 | `RootView.place_code_viewer_content` | // 当前 tab 有未保存改动则先弹确认；否则直接换内容（ADR 0003）。 |  |
| `src/root_view/code_viewer_section.rs` | 286 | `RootView.start_code_viewer_save` | // 本地：同步覆盖写（ADR 0003 行为不变）。 |  |
| `src/root_view/code_viewer_section.rs` | 305 | `RootView.start_code_viewer_save` | // 远程：乐观异步，先冲突检测（force=false）。 |  |
| `src/root_view/code_viewer_section.rs` | 480 | `RootView.confirm_overwrite_remote` | // tab 已被换文件则放弃覆盖（review F）。 |  |
| `src/root_view/code_viewer_section.rs` | 598 | `RootView.confirm_discard_code_viewer_reuse` | // 先存当前内容（本地同步 / 远程异步），成功后换进新文件。 |  |
| `src/root_view/code_viewer_section.rs` | 806 | `RootView.code_viewer_reconnect_banner` | // 源终端找不到（已关）或未连通都视为断开。 |  |
| `src/root_view/context_menus_section.rs` | 312 | `RootView.show_file_panel_context_menu` | // 右键 entry：若该项不在选择集合内，则单独选中；已选中则保留现有多选集合 |  |
| `src/root_view/file_panel_section/actions.rs` | 28 | `RootView.handle_toggle_file_panel` | // 关门滑出途中再按 = 取消关门滑回。 |  |
| `src/root_view/file_panel_section/actions.rs` | 57 | `RootView.handle_toggle_file_panel` | // 首帧按实际宽度起搏滑入 |  |
| `src/root_view/file_panel_section/body.rs` | 421 | `RootView.render_file_panel_body` | // 断线 → 可点击重连；其它错误保持纯文本。 |  |
| `src/root_view/file_panel_section/body.rs` | 464 | `RootView.render_file_panel_body` | // warp: 列表外面套 ClippedScrollable 提供纵向滚动条 |  |
| `src/root_view/file_panel_section/body.rs` | 737 | `RootView.render_file_panel_entry` | // 远程文件双击 → 内置编辑器（ADR 0005）；二进制/超大由 handler 提示下载。 |  |
| `src/root_view/find_section.rs` | 123 | `RootView` | // 查找栏 overlay：active 时返回 top-right 浮层，否则 None。 |  |
| `src/root_view/find_section.rs` | 160 | `RootView.render_find_bar` | // 去框化输入区：搜索图标做左锚，输入直接融在玻璃条上。 |  |
| `src/root_view/find_section.rs` | 301 | `RootView.render_find_bar` | // 计数与导航按钮之间的发丝分隔，收整右侧簇（两侧等距留白）。 |  |
| `src/root_view/git_panel_section/mod.rs` | 42 | `RootView` | // ===== A 类：从 handle_action match arm body 抽出的 handler ===== |  |
| `src/root_view/git_panel_section/mod.rs` | 55 | `RootView.handle_toggle_git_panel` | // 关门滑出途中再按 = 取消关门滑回（跟随的 file 面板同步回位）。 |  |
| `src/root_view/git_panel_section/mod.rs` | 82 | `RootView.handle_toggle_git_panel` | // 首帧按实际宽度起搏滑入。 |  |
| `src/root_view/git_panel_section/mod.rs` | 189 | `RootView` | // ===== C 类：从 main.rs 第 3 块 impl RootView 整体搬入的 git_panel fn ===== |  |
| `src/root_view/git_panel_section/mod.rs` | 363 | `RootView` | // ===== render fn：从 main.rs 第 2 块 impl RootView 整体搬入 ===== |  |
| `src/root_view/mod.rs` | 115 | `(顶层)` | // crate root 保留的伴生类型（helper/section 也经 crate:: 引用）。 |  |
| `src/root_view/mod.rs` | 120 | `(顶层)` | // crate root 保留的布局/资源常量。 |  |
| `src/root_view/mod.rs` | 875 | `RootView.new` | // 启动首屏即填充最近访问 |  |
| `src/root_view/mod.rs` | 1833 | `RootView.on_focus` | // 重获焦点：RDP tab 恢复符号热键接管。 |  |
| `src/root_view/rdp_section.rs` | 70 | `RootView.connect_rdp_host` | // RDPSND 音频重定向默认开启。 |  |
| `src/root_view/settings_section.rs` | 264 | `RootView.handle_set_font_family` | // warp: appearance_page.rs:1928 — 更新 FilterableDropdown 选中项 |  |
| `src/root_view/settings_section.rs` | 283 | `RootView.handle_set_font_weight` | // warp: 更新 Dropdown 选中项 |  |

## rdp-1（6）

| 文件 | 行号 | 所在符号 | 注释原文 | 状态 |
|---|---|---|---|---|
| `src/rdp_session/clipboard/dib.rs` | 27 | `bitmap_to_dib` | // BI_RGB |  |
| `src/rdp_session/egfx/surfaces.rs` | 154 | `Compositor` | // ---- 生命周期 ---- |  |
| `src/rdp_session/egfx/surfaces.rs` | 228 | `Compositor` | // ---- 写入 ---- |  |
| `src/rdp_session/egfx/surfaces.rs` | 386 | `Compositor` | // ---- surface / cache 操作 ---- |  |
| `src/rdp_session/egfx/surfaces.rs` | 704–706 | `color_to_rgba` | // ============================================================================<br>// 像素块自由函数（可单测）<br>// ============================================================================ |  |
| `src/rdp_session/egfx/surfaces.rs` | 727 | `blit` | // 目标可见范围。 |  |

## rdp-2（18）

| 文件 | 行号 | 所在符号 | 注释原文 | 状态 |
|---|---|---|---|---|
| `src/rdp_session/framebuffer.rs` | 127 | `RdpFramebuffer.compose_surface` | // clip ∩ surface-output-rect ∩ framebuffer。 |  |
| `src/rdp_session/mod.rs` | 692 | `connect_and_run` | // 阶段一：明文协商到 TLS 升级点。 |  |
| `src/rdp_session/mod.rs` | 761 | `connect_and_run` | // 阶段三：CredSSP(NLA) + 能力协商，收尾得到 ConnectionResult。 |  |
| `src/rdp_session/mod.rs` | 879 | `connect_and_run` | // 每会话一次的管线诊断日志（surface-command 含 marker / 位图回退）。 |  |
| `src/rdp_session/mod.rs` | 973 | `connect_and_run` | // Display Control 通道未就绪，忽略。 |  |
| `src/rdp_session/mod.rs` | 1033 | `connect_and_run` | // 本轮捕获的重激活 / 多传输请求（见 drain_outputs）。 |  |
| `src/rdp_session/mod.rs` | 1121 | `connect_and_run` | // 收到 DeactivateAll：跳出 drain，下方走重激活。 |  |
| `src/rdp_session/mouse_cursor.rs` | 310 | `tests.pointer_body` | // pad |  |
| `src/rdp_session/stats.rs` | 53 | `RdpStats.set_marker_mode` | // 未升到 EGFX 时标记为 RemoteFX 帧边界（1）。 |  |
| `src/rdp_view/hotkey_guard.rs` | 81 | `hotkey_transition` | // 状态未变：不重复 Push/Pop |  |
| `src/rdp_view/hotkey_guard.rs` | 96 | `HotkeyGuardSlot.set_engaged` | // Drop → Pop |  |
| `src/rdp_view/keymap.rs` | 19 | `scancode_for_key` | // 单字符（字母/数字/符号/空格）。 |  |
| `src/rdp_view/page_element.rs` | 134 | `RdpPageElement.send_mouse_move` | // 坐标未变，已处理但不重发 |  |
| `src/rdp_view/page_element.rs` | 425 | `RdpPageElement.layout` | // 占满内容区；画面通过 paint 内 letterbox 居中。 |  |
| `src/rdp_view/page_element.rs` | 437 | `RdpPageElement.paint` | // 背景铺满（含 letterbox 黑边）。 |  |
| `src/rdp_view/page_element.rs` | 447 | `RdpPageElement.paint` | // letterbox 几何（相对内容区）→ 叠加绝对 origin 后回写。 |  |
| `src/rdp_view/page_element.rs` | 455 | `RdpPageElement.paint` | // GPU 把整张桌面采样到 letterbox 矩形。 |  |
| `src/rdp_view/page_element.rs` | 489 | `RdpPageElement.dispatch_event` | // 修饰键：携带物理 KeyCode + 按下/抬起，维持远端修饰键状态。 |  |

## core-1（32）

| 文件 | 行号 | 所在符号 | 注释原文 | 状态 |
|---|---|---|---|---|
| `src/container_overview.rs` | 434 | `PsJson` | // ---- 解析 ---- |  |
| `src/container_overview.rs` | 610 | `parse_size` | // 数值前缀（含小数）与单位后缀分界。 |  |
| `src/cursor_smear.rs` | 72 | `CursorSmear.update` | // 首次出现：直接落格不补间。 |  |
| `src/file_drop_target.rs` | 106 | `FileDropTarget.dispatch_event` | // 默认 child-first：child 没消费再判定 bounds。 |  |
| `src/file_panel.rs` | 674 | `SftpWorkerHandle.drop` | // drop 时让所有未完成传输也尽快退出 |  |
| `src/file_panel.rs` | 1818 | `run_upload_dir` | // 远端先按深度顺序建好子目录 |  |
| `src/file_panel.rs` | 1907 | `run_download` | // 进度先转一遍：sftp_ops::TransferEvent → SftpEvent::DownloadProgress |  |
| `src/file_panel.rs` | 2128 | `run_list` | // 目录优先、名字升序；隐藏 . 和 .. |  |
| `src/git_ops.rs` | 447 | `(顶层)` | // ── 状态采集 ──────────────────────────────────────────────────────────────── |  |
| `src/git_ops.rs` | 631 | `CommitFileChange` | // ── 最近 commit ───────────────────────────────────────────────────────────── |  |
| `src/git_ops.rs` | 678 | `(顶层)` | // ── Diff preview ─────────────────────────────────────────────────────────── |  |
| `src/git_ops.rs` | 1109 | `(顶层)` | // ── 修改操作 ──────────────────────────────────────────────────────────────── |  |
| `src/git_panel.rs` | 702 | `build_repo_watcher` | // 任何一个事件不是噪声 → 触发 Refresh |  |
| `src/git_panel.rs` | 1257 | `tests.worker_full_loop_against_real_repo` | // 收 events 直到拿到 Snapshot |  |
| `src/git_panel.rs` | 1283 | `tests.worker_full_loop_against_real_repo` | // stage b.txt |  |
| `src/git_panel.rs` | 1809 | `tests.watcher_triggers_auto_refresh_on_workspace_change` | // 1) 收第一帧 Snapshot（干净状态） |  |
| `src/group_tag_manage_window.rs` | 25 | `GroupTagManageEvent` | // ── Model ── |  |
| `src/group_tag_manage_window.rs` | 43 | `GroupTagManageAction` | // ── Action ── |  |
| `src/group_tag_manage_window.rs` | 54 | `ManageStates` | // ── View ── |  |
| `src/group_tag_manage_window.rs` | 227 | `GroupTagManageView.render` | // scrollable content |  |
| `src/group_tag_manage_window.rs` | 230 | `GroupTagManageView.render` | // groups section |  |
| `src/group_tag_manage_window.rs` | 261 | `GroupTagManageView.render` | // tags section |  |
| `src/group_tag_manage_window.rs` | 364 | `render_header` | // ── Render helpers ── |  |
| `src/host_edit_window.rs` | 36 | `HostEditDraft` | // ── 数据模型 ── |  |
| `src/host_edit_window.rs` | 155 | `EditField` | // ── 焦点字段 ── |  |
| `src/host_edit_window.rs` | 189 | `HostEditEvent` | // ── Model (跨窗口通信) ── |  |
| `src/host_edit_window.rs` | 209 | `HostEditAction` | // ── Action ── |  |
| `src/host_edit_window.rs` | 243 | `FieldStates` | // ── View ── |  |
| `src/host_edit_window.rs` | 1184 | `(顶层)` | // ── 渲染函数 ── |  |
| `src/host_edit_window.rs` | 2541 | `render_rdp_quality_toggle` | // RDP 显示质量二选一：标准 / 高清 HiDPI。 |  |
| `src/host_management.rs` | 501 | `HostManagementState` | // 密钥页：当前选中查看详情的密钥 id |  |
| `src/host_management.rs` | 679 | `HostManagementState` | // 单选：若当前唯一选中即此项则取消，否则清空并只选中目标 |  |

## core-2（18）

| 文件 | 行号 | 所在符号 | 注释原文 | 状态 |
|---|---|---|---|---|
| `src/lib.rs` | 4601 | `tests.terminal_grid_snapshot_with_marked_text_overrides_cells_at_cursor_with_underline` | // Put cursor at column 0 of row 0 (default after construction). |  |
| `src/lib.rs` | 4712 | `tests.terminal_runtime_bell_pulse_advances_when_grid_emits_bell_event` | // Build a grid, feed a BEL byte, drain events, observe pulse increment. |  |
| `src/lib.rs` | 4724 | `tests.terminal_grid_core_scrollback_lets_user_view_history_then_jump_back_to_bottom` | // Generate enough output to push prior lines into scrollback. |  |
| `src/macos_window_util.rs` | 20 | `(顶层)` | // NSApplicationOcclusionStateVisible |  |
| `src/main.rs` | 91 | `(顶层)` | // main.rs 残留只剩启动装配与伴生类型，下列为这部分仍需的少量符号。 |  |
| `src/menu.rs` | 423 | `MenuItemLabel.render` | // Create column layout for stacked text |  |
| `src/menu.rs` | 428 | `MenuItemLabel.render` | // Add primary text |  |
| `src/menu.rs` | 436 | `MenuItemLabel.render` | // Add secondary text with spacing |  |
| `src/menu.rs` | 1425 | `MenuItemFields.render` | // Render tooltip if present and hovered |  |
| `src/menu.rs` | 1599 | `MenuItem.selectable` | // Whether it's selectable depends only on whether the item is disabled or not. |  |
| `src/menu.rs` | 1607 | `MenuItem.selectable` | // Separator is simply a non-selectable option in the menu list. |  |
| `src/pane_tree.rs` | 118–120 | `PaneData` | // ---------------------------------------------------------------------------<br>// PaneData<br>// --------------------------------------------------------------------------- |  |
| `src/pane_tree.rs` | 241–243 | `PaneNode` | // ---------------------------------------------------------------------------<br>// PaneNode<br>// --------------------------------------------------------------------------- |  |
| `src/pane_tree.rs` | 396–398 | `PaneBranch` | // ---------------------------------------------------------------------------<br>// PaneBranch<br>// --------------------------------------------------------------------------- |  |
| `src/pane_tree.rs` | 673–675 | `create_divider_placeholder` | // ---------------------------------------------------------------------------<br>// Divider rendering (adapted from warp/app/src/pane_group/tree.rs)<br>// --------------------------------------------------------------------------- |  |
| `src/pane_tree.rs` | 733–735 | `PaneData` | // ---------------------------------------------------------------------------<br>// Debug impls<br>// --------------------------------------------------------------------------- |  |
| `src/sftp_ops.rs` | 129 | `ensure_remote_dir` | // 父目录递归 |  |
| `src/ssh_key_store.rs` | 73 | `list_ssh_keys_with_usage` | // 列表 + 各密钥被多少主机引用（一钥多机价值的可视化）。 |  |

## core-3（29）

| 文件 | 行号 | 所在符号 | 注释原文 | 状态 |
|---|---|---|---|---|
| `src/terminal_grid_element.rs` | 841 | `TerminalGridAction` | // --- 主机管理面板 actions --- |  |
| `src/terminal_grid_element.rs` | 905 | `TerminalGridAction` | // --- 容器管理 actions --- |  |
| `src/terminal_grid_element.rs` | 943 | `TerminalGridAction` | // --- 分屏 actions --- |  |
| `src/terminal_grid_element.rs` | 958 | `TerminalGridAction` | // --- 文件面板 actions --- |  |
| `src/terminal_grid_element.rs` | 1032 | `TerminalGridAction` | // --- git 面板 actions --- |  |
| `src/terminal_grid_element.rs` | 1102 | `TerminalGridAction` | // --- 设置菜单 actions --- |  |
| `src/terminal_grid_element.rs` | 1108 | `TerminalGridAction` | // --- 设置页面 actions --- |  |
| `src/terminal_grid_element.rs` | 2804 | `TerminalGridElement.paint` | // Grid 内容偏移：左边距 + 上边距 + 平滑滚动亚行偏移。 |  |
| `src/terminal_grid_element.rs` | 2958 | `TerminalGridElement.paint` | // 结束 grid content clip |  |
| `src/terminal_grid_element.rs` | 3291 | `TerminalGridElement.dispatch_event` | // ── 鼠标选区 ──────────────────────────────────────────────── |  |
| `src/terminal_grid_element.rs` | 3446 | `TerminalGridElement.dispatch_event` | // ── 滚轮（像素级平滑滚动）───────────────────────────── |  |
| `src/terminal_grid_element.rs` | 3481 | `TerminalGridElement.dispatch_event` | // mouse-app 模式：逐行发 wheel report，清除亚行余量。 |  |
| `src/terminal_grid_element.rs` | 3512 | `TerminalGridElement.dispatch_event` | // alt-screen 模式：发 escape 序列，清除亚行余量。 |  |
| `src/terminal_grid_element.rs` | 3540 | `TerminalGridElement.dispatch_event` | // 预估 scroll 后 display_offset 是否撞顶/触底 |  |
| `src/terminal_runtime.rs` | 472 | `terminal_snapshot_with_input_editor` | // 注册 ghost text style（灰色前景） |  |
| `src/terminal_runtime.rs` | 516 | `terminal_snapshot_with_input_editor` | // ghost text：将 suggestion 中超出 buffer 的后缀以灰色投射 |  |
| `src/terminal_runtime.rs` | 534 | `terminal_snapshot_with_input_editor` | // 清除投射区域之后到行尾的残留内容 |  |
| `src/terminal_runtime.rs` | 2563 | `TerminalRuntimeState.scan_osc7` | // 处理所有完整 sequence，得到剩余 tail。 |  |
| `src/terminal_runtime.rs` | 2580 | `TerminalRuntimeState.scan_bootstrap_marker` | // 拼前一次尾部 + 本次输入，扫子串 |  |
| `src/terminal_runtime.rs` | 3250 | `LocalTerminalRuntime.spawn_pty` | // Option&lt;RawFd> |  |
| `src/terminal_runtime.rs` | 3730 | `LocalTerminalRuntime.set_find_query` | // Scroll the focused match into view so the user sees the first hit. |  |
| `src/terminal_runtime.rs` | 4318 | `run_remote_ssh_event_loop` | // 连接成功: 撤掉过渡层，清屏 + 清 scrollback + 恢复光标 |  |
| `src/terminal_runtime.rs` | 5021 | `TerminalPalette.from_theme` | // selection = accent with reduced opacity, blended on background |  |
| `src/terminal_runtime.rs` | 5754 | `tests.consume_osc7_split_across_chunks` | // 第二轮拼上 tail |  |
| `src/terminal_runtime.rs` | 5781 | `tests.scan_osc7_updates_local_cwd` | // 第二次 cwd 变更 |  |
| `src/time_format.rs` | 5 | `(顶层)` | // Some conversion ratios for time units. |  |
| `src/ui_anim.rs` | 120 | `(顶层)` | // ── 数值过渡（环弧 sweep 等）──────────────────────────────────── |  |
| `src/ui_anim.rs` | 211 | `(顶层)` | // ── 一维弹簧（位移/宽度动画）──────────────────────────────────── |  |
| `src/ui_anim.rs` | 334 | `(顶层)` | // ── 提示条时间线 ─────────────────────────────────────────────── |  |
