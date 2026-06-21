// tab_bar_section — RootView 的标题栏 / 标签栏：tab 渲染、标题栏 chrome、标签生命周期与 Chrome action。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含子模块声明；各子模块只含 impl RootView，无自由函数。
// 子模块：title_bar 标题栏 chrome render / tab_render 单个 tab render / actions handler + 标签生命周期。
// 注：标签右键菜单内容（toggle_tab_right_click_menu / tab_right_click_menu_items）已于 step 9 归 context_menus_section。

mod actions;
mod tab_render;
mod title_bar;
