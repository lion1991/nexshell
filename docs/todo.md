# 待办清单

代码注释里的 TODO / FIXME / XXX，以及带「暂时、临时、workaround」这类说法的注释，必须引用这里的编号，如 `TODO(T-001): 补超时重试`。提交前的捷径检查会核实编号确实在下表里（见 `shortcut-guard.md`）。

编号顺延，不复用。做完后删掉代码里的引用，并把编号划掉（`~~T-003~~`）留在表里占号；划掉的编号不再算有效引用。

| 编号 | 要做什么 | 登记日期 |
|---|---|---|
| T-001 | 分屏后关掉原 pane（tab 级 ssh handle 属于第一个连接），确认文件面板和远程编辑还能不能用；不能用再定 handle 跟哪个 pane。代码位置：`src/root_view/terminal_section/split.rs` 的 `split_active_pane`。 | 2026-11-22 |
| T-002 | 主机概览一次性为每台主机起连接线程，主机多时加并发闸门（信号量分批）。现在只靠 fd 软上限提到 10240 缓解 EMFILE，没有限流。代码位置：`src/host_overview_fleet.rs` 的 `HostOverviewFleet::start`。 | 2026-11-22 |
| T-003 | 把约 233 处 `UiColors` / `HostOverviewColors` 改成 design_tokens 的 `ChromeColors` / `OverviewColors`，删掉 `src/ui_colors.rs` 这个旧名门面。纯改名，挑没有并行分支的时候做。 | 2026-11-22 |
| T-004 | EGFX 的 MapSurfaceToScaledOutput 现在只取原点、忽略 target 缩放。先在真机开 `NEXSHELL_RDP_EGFX_TRACE`，看日志里有没有 `map_scaled`（服务端会不会发）；发了再做缩放渲染，一直没见过就关掉这条。代码位置：`src/rdp_session/egfx/mod.rs` 的 `on_map_surface_to_scaled_output`。 | 2026-11-22 |
| T-005 | 下一轮 IronRDP 追平时评估改用库的 `begin_frame` / `end_frame` 加 `DecodedTile.update_rectangles`，去掉 Progressive 的二次解析和 `tile_pixels` 缓存。验收用 ADR 0008 第⑤步的复测：键盘移窗 7 轮 + 鼠标拖动 7 轮零残留。代码位置：`src/rdp_session/egfx/surfaces.rs` 的 `progressive_paint_plan`。 | 2026-11-22 |
