# nexshell 开发约定

唯一的 Rust crate（GPUI/warpui 原生终端 app），纯 cargo 构建，无 npm 层。终端功能优先复用 Warp 上游实现（源码在 `../warp/crates/*`，即 Cargo.toml 的 path 依赖），别自己造轮子，方便跟上游同步（策略见 ADR 0010）。架构决策与理由见 `docs/adr/`，RootView 拆分见 ADR 0001；目录现状直接看 `src/root_view/`，本文不复述。

## 边界（RootView）
- `root_view/<面板>_section.rs` 或同名目录只写 `impl RootView`；无 `&self` 的纯函数进 `src/*_helpers.rs`。文件顶部留一行此约定注释。
- section 之间禁止互相 `use`；共享只走 `&mut self` 访问 RootView 字段，或抽成 helper 自由函数。不建 misc/shared/common。
- 右键菜单内容集中在 `context_menus_section.rs`，其他 section 只调 `self.show_xxx_context_menu(...)`。
- `mod.rs` 只放 struct、trait impl、`new()` 与 action/render 派发，面板逻辑一律进 section。
- action handler 命名 `handle_<action>`，render 命名 `render_xxx_panel/section`，按主要触发场景归属。可见性取调用者最近范围（private → `pub(super)` → `pub(in crate::root_view)` → `pub(crate)`）。靠 rust-analyzer 跳转，不维护"谁调用"汇总注释。

## 行数阈值
单文件超 ~800 行偏大，超 1500 必须拆；`*_helpers.rs` 守 800，超了按子主题再拆，别和 section 合并；`root_view/mod.rs` 豁免上限，但受上面"只放派发"约束。section 超线时开 ADR 再拆，别硬塞。

## 易混点
`host_management.rs` 是数据/状态，`host_management_view/` 是独立子组件，`root_view/host_library_section/` 是 RootView 上的组装入口。`host_library`（主机管理列表页）≠ `host_monitor`（终端 tab 内嵌的进程/网络/系统监控）。

## rustfmt
手动格式化必带 `--config skip_children=true`（`rustfmt --edition 2021 --config skip_children=true <files>`；stable 1.8.0 起不认 `--skip-children` 长选项）。否则传 `mod.rs`/`lib.rs` 会顺着 `mod` 声明递归格式化整棵子模块树，污染 diff/blame。
