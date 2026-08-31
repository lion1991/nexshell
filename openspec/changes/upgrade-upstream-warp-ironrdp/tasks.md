## 1. 准备与隔离

- [x] 1.1 `../IronRDP` 的 `upstream` 远端切 ssh 并 fetch 到 2026-08-31 master
- [x] 1.2 IronRDP 新建分支 `nexshell-2026-08`（基于 `upstream/master`），不动 `egfx-fixes`
- [x] 1.3 NexShell 新建 worktree + 分支 `integration/ironrdp-2026-08`（基于 `main`），不动当前脏工作树
- [ ] 1.4 用升级前 `main` 构建，设 `NEXSHELL_RDP_EGFX_WIRE_DUMP=<path>` 连 Win11 录 ~30s 视频 dump（需 Matt 真机操作），再用 `egfx_replay --out-dir` 采基线帧

## 2. IronRDP fork 补丁甄别

- [x] 2.1 对 21 个自有提交逐条 patch-id/源码对比上游，形成"保留 / 已等价退役"台账
- [ ] 2.2 cherry-pick 保留补丁到 `nexshell-2026-08`，冲突按上游新结构改写
- [ ] 2.3 评估 `patches/*.patch` 两个外置补丁：并入分支或删除
- [ ] 2.4 fork 分支 `cargo check -p` 涉及 crate + 上游测试套件通过，推送到 `lion1991/IronRDP`

## 3. NexShell 适配

- [x] 3.1 `Cargo.toml` `[patch.crates-io]` 钉新 rev，版本号按上游 workspace 对齐
- [ ] 3.2 修复编译错误：connector/session 拆分、DVC typed accessor、tls 显式证书校验、bulk 解压、`Sequence::step` 时间戳
- [x] 3.3 决定 EGFX 合成路径（自有合成 vs 上游 `#1461`），保持 VideoToolbox 硬解
- [x] 3.4 剪贴板回环：评估用上游 `#1739` 替换 `clipboard.rs` 的 hash 方案
- [ ] 3.5 `cargo clippy --all-targets` / `cargo test` / `examples` 全部通过

## 4. RDP 验收

- [ ] 4.1 `egfx_replay` 回放对比基线帧，差异逐一解释
- [ ] 4.2 Windows 11 真机冒烟：视频 30s、剪贴板、文件互拷、拉伸/全屏、指针
- [ ] 4.3 xrdp 真机：注销后显示正常断开，无协议错误
- [ ] 4.4 输入压力：连发按键+鼠标移动无丢失乱序；大幅滚轮方向正确
- [ ] 4.5 无法验证的场景（GNOME RD、高负载恢复）在台账标注

## 5. 记录与合入

- [ ] 5.1 写 `docs/adr/0011-ironrdp-upstream-rebase-2026-08.md`（基线、补丁台账、验收证据）
- [ ] 5.2 合入 `main`（普通 merge，可 revert），保留 `egfx-fixes` 与旧 rev 作回退

## 6. Warp 追平（IronRDP 合入后独立执行）

- [ ] 6.1 按 ADR 0010 建成对隔离 worktree，目标基线 `86cfeb9`，采集黄金基线
- [ ] 6.2 上游合并 + `21f413b79` grid 迁移冲突按行为域重放保护补丁
- [ ] 6.3 验证 Kitty Cmd/Option 编辑键编码（spec `terminal-kitty-modifier-keys`）
- [ ] 6.4 保护清单、Warp presubmit 差分、NexShell 三平台编译门禁全部通过
- [ ] 6.5 更新 `warp-compatibility.toml`，按审批顺序晋级 `private/master` 与 `main`
