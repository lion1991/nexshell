# 容器管理：docker CLI over SSH exec，复刻 host_overview 采集模式

Status: accepted (2026-07-07)

## 背景

主机库要加容器管理页（参考 ServerCat Containers）：按主机分组展示各 SSH 主机上的 Docker 容器卡片（名称、运行状态、CPU 环、MEM、磁盘 R/W、网络 ↑/↓），并支持 start/stop/restart/logs 操作。

现状：`host_overview.rs` 已确立「独立 OS 线程 + current-thread tokio + 一条 exec 采集脚本 + 差分快照 + async_channel 推回 UI」模式，`host_overview_fleet.rs` 提供多主机批量编排；`SshSession::exec_command` 是现成的单命令执行 helper，`spawn_remote_exec` 是现成的一次性远端操作 helper。仓库内零容器相关代码。

## 决策

- **采集走远端 docker CLI，不碰 Docker API socket**：一条 shell 脚本 `command -v docker` 探测 + `docker ps -a --format '{{json .}}'` + `docker stats --no-stream --format '{{json .}}'`，一次 exec 拿全量。不引 bollard/HTTP-over-socket——那需要 socket 转发或 TCP 暴露，破坏「SSH 一条通道走天下」。
- **架构复刻 host_overview 双文件模式**：`container_overview.rs`（采集/解析/快照）+ `container_fleet.rs`（多主机编排，5s 轮询）。进 Containers 页启动 fleet、切走即停，与 Status 页互斥，同一主机不叠加监控连接。
- **NetIO/BlockIO 显示累计量不做差分速率**：`docker stats` 原生输出即累计，参考 ServerCat 同样展示累计；省掉两帧差分状态机。
- **操作层复用 `spawn_remote_exec`**：start/stop/restart 一次性连接跑命令后触发刷新；logs 直接开终端 tab 跑 `docker logs -f --tail 200`，复用现有终端，不自建日志查看器。
- **权限失败明示不自愈**：用户不在 docker 组时卡片区显示错误提示，不自动回退 sudo（避免隐蔽行为与密码交互）。
- **v1 只做 docker**：podman 的 stats 输出格式有差异，v2 单独适配。

## UI 归属

- `HostViewMode` 加 `Containers`，`group_nav.rs` 功能区加入口，`host_management_view/mod.rs` body 分派加一臂。
- 新建 `host_management_view/container_view.rs`：主机名 section 标题 + 双列容器卡片。复用 `RingGauge`（CPU 环+健康点）、状态卡沉淀的大数字排版、`FloatTransition` sweep 动画。
- i18n 前缀 `host_container_*`。

## Considered Options

- **bollard（Docker API client）+ SSH socket 转发**：类型化 API 但要为每主机维护 socket 转发通道，连接管理复杂度不成比例。否。
- **采集并入 HOST_OVERVIEW_COLLECT_COMMAND**：省一条连接，但 `docker stats --no-stream` 自带 ~2s 采样等待会拖慢整个 host overview 周期，且状态页/终端侧栏并不需要容器数据。否。
- **速率差分（↑/↓、R/W 显示 per-sec）**：语义更接近状态卡，但参考产品即累计、实现翻倍，v1 不做，留候选。
