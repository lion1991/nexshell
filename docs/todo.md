# 待办清单

代码注释里的 TODO / FIXME / XXX，以及带「暂时、临时、workaround」这类说法的注释，必须引用这里的编号，如 `TODO(T-001): 补超时重试`。提交前的捷径检查会核实编号确实在下表里（见 `shortcut-guard.md`）。

编号顺延，不复用。做完后删掉代码里的引用，并把编号划掉（`~~T-003~~`）留在表里占号；划掉的编号不再算有效引用。

| 编号 | 要做什么 | 登记日期 |
|---|---|---|
| T-001 | 分屏后关掉原 pane（tab 级 ssh handle 属于第一个连接），确认文件面板和远程编辑还能不能用；不能用再定 handle 跟哪个 pane。代码位置：`src/root_view/terminal_section/split.rs` 的 `split_active_pane`。 | 2026-11-22 |
| T-002 | 主机概览一次性为每台主机起连接线程，主机多时加并发闸门（信号量分批）。现在只靠 fd 软上限提到 10240 缓解 EMFILE，没有限流。代码位置：`src/host_overview_fleet.rs` 的 `HostOverviewFleet::start`。 | 2026-11-22 |
