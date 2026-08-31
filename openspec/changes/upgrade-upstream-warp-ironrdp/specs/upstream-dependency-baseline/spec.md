## Purpose

规定 NexShell 对 IronRDP 与 Warp 两个上游依赖的基线记录方式、自有补丁台账与兼容校验规则，保证任意 checkout 都能重现并验证所依赖的确切上游版本。

## ADDED Requirements

### Requirement: IronRDP 基线单点钉定
`Cargo.toml` `[patch.crates-io]` 中所有 `ironrdp-*` crate SHALL 钉到同一 fork 仓库的同一 rev；该 rev 及其对应的上游合并基 SHALL 记录在 `docs/adr/0011-*.md` 中。

#### Scenario: 构建可重现
- **WHEN** 在干净环境执行 `cargo build --locked`
- **THEN** 解析出的全部 ironrdp crate 来源为同一 git rev，构建成功

### Requirement: 自有补丁台账
每个保留在 fork 中、上游无等价实现的补丁 SHALL 在 ADR 台账中登记（提交、用途、上游对应 issue/PR 或"未提交"、退役条件）；被上游等价实现取代的补丁 SHALL 登记退役证据（上游 PR 号与行为验证）。

#### Scenario: 补丁退役
- **WHEN** 上游合入了某保留补丁的等价实现
- **THEN** 台账标注退役并引用上游 PR，fork 分支移除该补丁

### Requirement: Warp 兼容基线校验
`warp-compatibility.toml` SHALL 记录已验证的官方目标基线与集成镜像提交；`scripts/verify-warp-compatibility.sh --strict` SHALL 在发布/打包前通过，开发模式允许后继分支并报告未提交修改。

#### Scenario: 严格模式
- **WHEN** 发布脚本以 `--strict` 校验
- **THEN** 要求 `../warp` 精确处于集成提交、工作树干净、远端正确、目标官方基线为祖先，任一不满足即失败

### Requirement: 稳定分支只接受验收后的候选
IronRDP 新基线与 Warp 新集成镜像 SHALL 先在隔离分支/worktree 完成，全部验收场景通过并留证后才合入 NexShell `main`；`main` 在此之前 MUST 保持可构建、可运行。

#### Scenario: 验收未通过
- **WHEN** 任一验收场景失败
- **THEN** 候选不合入，`main`、`egfx-fixes`、`private/master` 均不变
