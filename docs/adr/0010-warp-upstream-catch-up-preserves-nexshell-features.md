# Warp 上游追平必须保全 NexShell 特性

Status: accepted (2026-08-04)

NexShell 通过集成镜像复用 Warp 引擎，同时在 Warp 侧维护输入、IME、远程光标、Metal 原语和 Liquid Glass 等保护补丁。集成镜像采用完整、非快进的上游合并，不通过选择性摘取继续扩大结构漂移，也不通过 rebase、squash 或强制推送改写私有历史；本轮目标基线固定为官方提交 `3e8a989902c4acdcb524af8cd8cb025e23402ddb`。

特性保全以用户行为和跨仓库契约等价为准，不要求逐行保留原实现。全部 NexShell 原生私有能力默认进入保护清单；只有官方上游等价能力已取得源码、自动测试和运行态证据，并经明确批准，原保护补丁才可退役。环境阻塞不等于验证通过，保护清单未全部通过时不得更新 `private/master`。

上游合并、Warp 侧保护修复和 NexShell 消费端适配保持独立提交层次。第一个 Warp 提交保留双亲上游合并关系及必要冲突适配；测试发现的保护缺口按行为域追加修复；NexShell 的 API 适配和回归测试留在 NexShell 仓库。两个稳定分支只在完整验证门禁通过后更新。

NexShell 还必须显式记录已验证的官方目标基线和最终集成镜像提交，不能继续把 `../warp` 路径下碰巧存在的源码视为兼容契约。该兼容基线由仓库内校验工具验证，但不把 Warp 改为 NexShell 的 submodule。

兼容校验分为开发和严格两种模式。开发模式允许 Warp 位于已验证集成提交的后继分支，并报告未提交修改；严格模式要求精确集成提交、干净工作树、正确远端和目标官方基线祖先关系，完整验证、打包与发布必须使用严格模式。普通 Cargo 构建不通过 `build.rs` 隐式读取 Git 状态。

本轮平台门禁覆盖 macOS Apple Silicon 完整运行、macOS Intel 编译检查，以及 Windows x86_64 交叉编译和原生冒烟；Windows 保持现有 tint 降级，不扩展 Liquid Glass。Linux 不是本轮 NexShell 发布目标。私有 Cursor 扩展导致的现有 Windows 穷举匹配缺口必须随保护补丁适配一并修复，不能作为既有问题豁免。

Warp 官方完整 presubmit 采用同环境差分门禁：目标官方基线与集成候选运行相同的格式、workspace Clippy、workspace nextest 和文档测试，候选不得新增失败；官方基线已有失败必须逐项留证，不能作为新增回归的豁免。冲突触及的官方 App、WarpUI、Core 和 Vim 路径还需独立 focused 验证，Warp 门禁与 NexShell 门禁互不替代。

全量追平不改变镜像运维策略。官方 workflow 文件继续随上游保留，但继承的 CI、发布、同步、清理和定时工作流在私有仓库保持手动禁用；新出现的继承工作流也必须在默认分支更新后复查并禁用。Dependency Graph 保持启用，Dependabot 动态工作流通过 Cargo 与 GitHub Actions 配置中的 `open-pull-requests-limit: 0` 抑制普通版本更新。

同步必须在成对的干净隔离 worktree 中进行，不得 stash、提交、清空或吸收现有 NexShell 工作树的暂存修改。当前文件面板自动刷新修改作为平行功能保留，在同步候选稳定后叠加到专用验证工作树做兼容性验证，但不混入上游同步提交；未跟踪的静态审查报告不属于运行特性。

任何合并前必须先以 NexShell `266c939695a9b7f892f442b16d88cff0cda1c305` 和 Warp `a6adffe9a240b414d6b70dcfbc74b06b06269d8a` 采集更新前黄金基线。视觉、帧统计、IME、输入、合成拖拽、RDP 指针与重连都使用相同场景做候选 A/B；现有 Windows 构建失败记录为必须在本轮消除的已知基线失败，而不是候选可继承的豁免。

性能门禁对基线和候选各采集至少三轮完整 240 帧窗口，比较 `gpu.execute`、`cpu.layout`、`cpu.paint` 与 `cpu.build_scene` 的三轮 `p95` 中位数；候选增幅超过 `max(基线 p95 × 10%, 0.5ms)` 时阻断，`max` 仅作尖峰诊断。Glass 数量上限、dirty 降级、至少 300ms 迟滞和后台/光标 dirty 排除属于精确行为门禁；zero-optical 最大通道差保持 `2/255`，Liquid 视觉以固定场景并列截图签收。

冲突解决以行为域语义重放为准，不以 Git 冲突列表为边界。显式冲突和双方共同修改的自动合并文件都必须记录上游意图、私有行为、候选位置与验证证据；高风险文件禁止整文件选择 `ours` 或 `theirs`。优先采用上游结构并重放保护行为，删除或迁移的测试必须在新结构中保留等价覆盖；上游摘取修复只有通过 patch-id、源码和回归测试证明等价后才由上游接管。

稳定晋级使用候选分支和旧组合注释 tag，不直接把未验收提交推入稳定分支。Warp 候选先晋级 `private/master` 并复核远端自动化，NexShell 再以准确兼容基线晋级 `main`；候选推送、Warp 稳定和 NexShell 稳定分别审批。稳定历史不强推、不改写，回滚使用普通 revert，旧组合 tag 仅用于定位和重建。

实施写入仅限成对隔离的 Warp 与 NexShell worktree，以及经单独批准后的候选分支、基线 tag、稳定分支和 GitHub workflow 状态。当前 NexShell 脏工作树、IronRDP、系统组件、应用部署与发布不在本轮写入范围；若保全需要扩展到其他仓库或环境，候选必须停止并重新申请范围。

验证发现的问题按 `Update Regression`、`Preservation Blocker` 和 `Known Baseline Failure` 分类。本轮必须修复候选新增或加重的回归，以及阻止保护补丁适配新上游的问题；与追平无因果关系的既有缺陷只留证并要求候选不恶化，不顺手混入同步提交。当前 Windows Cursor 穷举缺口属于保护阻塞项。
