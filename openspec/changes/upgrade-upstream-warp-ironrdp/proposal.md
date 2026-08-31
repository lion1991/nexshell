## Why

NexShell 的两个引擎上游都已明显领先：IronRDP fork `egfx-fixes`（基于 Devolutions 2026-07-02 `069786c`）落后上游 500+ 提交，且 fork 里 20 个自有补丁过半已被上游等价实现（EGFX 客户端渲染、ClearCodec/NSCodec、Progressive tiles、V-Bar、早期 cap 标志、rdpsnd 阻塞），继续维护只会累积冲突；上游同时带来我们缺失的可靠性修复（Fast-Path 输入批量、xrdp 断开、FreeRDP 系服务器连接、会话中途 CapsAdvertise 恢复、自动重连 cookie、rdpdr 失步、SIMD 逆 DWT）。Warp 镜像基线 `3e8a989`（08-03）落后官方 `86cfeb9`（08-30）311 提交，其中 Kitty 键盘协议 Cmd/Option 修饰键编码直接影响终端 TUI 体验。

## What Changes

- **IronRDP 升级**：fork 新建分支基于上游最新 master，逐条用 patch-id/行为对比甄别自有补丁，只保留上游无等价的（预计：rdpsnd 通道协商、rdpdr 路径沙箱、AVC420 regionRects、egfx dump 调试工具）；NexShell `Cargo.toml` 的 `[patch.crates-io]` 改钉新 rev 并适配上游破坏性 API（session 去 connector 依赖、DVC typed accessor、tls 显式证书校验、bulk 解压归属、`Sequence::step` 时间戳等）。**BREAKING**（仅内部 API，用户可见行为不变）。
- **RDP 可靠性提升**（升级后自然获得，需纳入验收）：xrdp 正常断开不再报协议错、GNOME Remote Desktop/FreeRDP 系服务器可连、Win11 高负载花屏可恢复、输入事件按协议 255 上限批量保序、滚轮值夹紧、驱动器重定向 QueryInformation 不失步。
- **Warp 追平**：按 ADR 0010 流程把集成镜像追到官方 `86cfeb9`（含 `21f413b79` grid 代码迁入 `warp_terminal` 的结构性变更），更新 `warp-compatibility.toml`；保护清单全部通过后才晋级稳定分支。
- **维护基线记录**：`../IronRDP` upstream 远端改可用地址；docs/adr 新增 0011 记录 IronRDP 基线与保留补丁台账。
- 非目标：RD Gateway、RemoteApp、麦克风、RDP-UDP 等上游新能力不在本次范围。

## Capabilities

### New Capabilities
- `rdp-session-reliability`：RDP 会话在升级后必须保全的用户可见行为（EGFX 视频、音频、剪贴板、驱动器互拷、动态分辨率、断开/重连）以及新增的服务器兼容与恢复行为。
- `upstream-dependency-baseline`：IronRDP / Warp 两个上游依赖的基线记录、自有补丁台账与可验证的兼容校验规则。
- `terminal-kitty-modifier-keys`：Kitty 键盘协议激活时 Cmd/Option 组合编辑键的编码行为（随 Warp 追平获得）。

### Modified Capabilities
（无：`openspec/specs/` 尚为空。）

## Impact

- 代码：`src/rdp_session/**`（~7.3k 行，含 `egfx/`）、`examples/{rdp_probe,egfx_replay}.rs`、`Cargo.toml` `[patch.crates-io]`、`Cargo.lock`、`warp-compatibility.toml`、`patches/ironrdp-*.patch`。
- 仓库：`../IronRDP`（新分支，不动 `egfx-fixes`）、`../warp`（新集成候选分支，不动 `master`/`private/master`）。
- 风险面：RDP 是纯 Rust 协议栈，升级面广；Warp 追平触及终端 grid 结构性重构。两者都必须在隔离分支/worktree 内完成并通过验收门禁后才合入 `main`，现有 `main` 与用户脏工作树不受影响。
