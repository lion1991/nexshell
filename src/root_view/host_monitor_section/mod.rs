// host_monitor_section — 终端 tab 内嵌的主机监控（概览/进程/网络/系统信息）。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含子模块声明；各子模块只含 impl RootView，无自由函数。
// 子模块：overview 侧栏 assembler（render_sidebar_panel）+ 通用渲染件 / system 系统+磁盘 / process 进程 / network 网络 / actions handler+开页。

mod actions;
mod network;
mod overview;
mod process;
mod system;
