## Context

- IronRDP：NexShell 通过 `[patch.crates-io]` 把 19 个 `ironrdp-*` crate 统一钉到 `lion1991/IronRDP@f120928`（分支 `egfx-fixes`，合并基 upstream `069786c` 2026-07-02）。fork 有 21 个自有提交，集中在 `ironrdp-graphics`（progressive/clearcodec）、`ironrdp-egfx/client.rs`、`rdpsnd`、`rdpdr-native`。NexShell 侧 ~80 处 ironrdp API 引用，集中在 `src/rdp_session/`。
- 上游 git master（2026-08-31）相对 `069786c` 含多次 `!` 破坏性变更，且 crates.io 7-10 发布的 0.10/0.11 系列已早于大部分我们需要的修复，故必须走 git rev 而非 crates.io 版本。
- Warp：集成镜像 `8d3fb124f`（含官方 `3e8a989`），追平流程由 ADR 0010 与 `docs/plans/2026-08-04-warp-upstream-catch-up.md` 固定，本设计不重复。

## Goals / Non-Goals

- Goals：把 fork 补丁量降到最低；NexShell RDP 行为零回归并获得上游可靠性修复；Warp 追到 `86cfeb9`；全部工作在隔离分支完成。
- Non-Goals：不采用 RD Gateway/RemoteApp/麦克风/RDP-UDP；不修改 EGFX 硬解（VideoToolbox）架构；不在本次处理 Windows 平台 RDP 验证（沿用现状）。

## Decisions

1. **整体 rebase 到上游 master 而非 cherry-pick 修复到旧 rev**。理由：所需修复散布在 100+ 提交且依赖中间的 API 重构，逐个回移成本高于一次性适配；且 fork 过半补丁上游已等价，rebase 后补丁面缩小。替代方案（继续在 `069786c` 上 cherry-pick）只作为回退路线。
2. **新分支 `nexshell-2026-08` 从 `upstream/master` 出发，逐条 `git cherry-pick` 自有补丁**，每条先用 `git patch-id` + 读上游对应代码判断是否已等价；等价则跳过并在 ADR 台账留证。不 rebase `egfx-fixes`，旧分支与旧 rev 原样保留以便随时回退。
3. **`patches/*.patch` 两个文件**（raw gfx dump、upgrade band order）在新 rev 上重新评估：若上游已含则删除，否则并入 fork 分支提交，不再以外置 patch 形式存在。
4. **NexShell 适配走独立 worktree + 分支 `integration/ironrdp-2026-08`**，不动当前脏工作树；`Cargo.toml` 只改 `[patch.crates-io]` rev 与受影响的版本号。
5. **验收以"回放 + 真机"双轨**：`examples/egfx_replay.rs` 对既有码流样本做像素对比（自动化、可重复）；真机冒烟覆盖 spec 的场景。回放基线用升级前 `main` 构建先行采集。
6. **Warp 追平在 IronRDP 合入后单独进行**，复用 ADR 0010 的成对隔离 worktree 流程，目标基线改为 `86cfeb9`；`21f413b79` 的 grid 迁移按"采用上游结构、重放保护行为"处理。两阶段不交叉，避免同时改动两个引擎无法定位回归。

## Risks / Trade-offs

- [上游 API 破坏性变更多，适配量超预期] → 先在 worktree 里 `cargo check` 定位全部编译错误再动手，按模块分批提交；不可解时回退路线为旧 rev + 单点 cherry-pick。
- [上游"等价"实现行为与我们补丁细节不同（如 Progressive band 顺序）] → 回放像素对比作为硬门禁；差异必须逐一解释。
- [rdpsnd/rdpdr-native 上游重构后我们的补丁失效] → 这两处补丁小（各 1 文件），失效即改写为对新结构的等价修复。
- [`../IronRDP` upstream https fetch 被重置] → 切 ssh 远端；仍不行则用 `gh api` 下载 tarball 建本地 ref。
- [Warp `21f413b79` 大规模移动导致保护补丁冲突] → 严格走 ADR 0010 的行为域重放，不整文件取 ours/theirs；该阶段独立验收。
- [验证环境缺 GNOME Remote Desktop / Win11 高负载场景] → 无法验证的场景在台账标注"未验证、依上游测试"，不阻断合入但记录为已知缺口。

## Migration Plan

1. 修 upstream 远端并 fetch；建 fork 分支与 NexShell worktree。
2. 采集升级前回放基线（EGFX 样本帧）。
3. fork 分支 cherry-pick 甄别 → 推到 `lion1991/IronRDP`。
4. NexShell 适配 → `cargo check/clippy/test` → 回放对比 → 真机冒烟。
5. 写 ADR 0011，合入 `main`。
6. 回滚：`git revert` 合入提交即回到旧 rev（`egfx-fixes` 与 `f120928` 保留不删）。
7. Warp 阶段按 ADR 0010 单独执行与晋级。

## Open Questions

- 上游 `#1461` 让 `ActiveStage` 直接合成 EGFX 输出；我们 `rdp_session/egfx/` 自带合成与 VideoToolbox 硬解。适配时是"关掉上游合成只用其 DVC 层"还是"改用上游合成、硬解结果回灌"，可在适配阶段按编译与性能结果决定，不影响 spec。
