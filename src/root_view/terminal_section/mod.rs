// terminal_section — RootView 的终端区：终端 render、终端 action、分屏。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含子模块声明；各子模块只含 impl RootView，无自由函数。
// ADR 预拆阈值 1000 行：本 section 同时承载 render / 终端 action / 分屏三类职责，按 step 1/3/4/6 的目录约定
// 拆成子模块（render-vs-split 概念切分），避免单文件巨石：
//   render  终端 body / 分屏 body / 光标投影 + 键盘/overlay 输入判定（主机监控侧栏 assembler 归 host_monitor_section）
//   actions #1-3 复制/粘贴/清屏 + #7-9 字号 + #40 TerminalMouseDown
//   split   #85-98 分屏：split/close/navigate pane、focus、resize、maximize
// 注：终端右键菜单内容（show_terminal_context_menu / terminal_context_menu_items）已于 step 9 归 context_menus_section。

mod actions;
mod render;
mod split;
