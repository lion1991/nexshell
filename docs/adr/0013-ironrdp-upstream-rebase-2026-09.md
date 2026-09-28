# IronRDP 依赖追平上游 2026-09

Status: proposed (2026-09-24)；Win11 真机回归通过后转 accepted

## 背景

ADR 0011 基线（上游 `872845c` + 12 个 fork 补丁，rev `d9ee675`）之后，上游又进 82 个提交。约 35 个是 CI pr-automation，其余多为 server / activex / web / USB / 摄像头 / UDP 传输等本项目不用的功能。客户端相关的修复：

| 上游 | 内容 | 对 NexShell |
|---|---|---|
| #1703 `fb0c0413` | Progressive YCbCr→RGB 改 i64 | 走的正是库内 Progressive 解码；越界系数 debug panic、release 错像素 |
| #1962 `9b151c4c` | RLE 解码宽高上限 8192 | 安全：恶意服务器几字节即可触发约 12GB 分配 |
| #1861 `2971839d` | I/O 通道拼接的 Share Control PDU 逐个解码 | Win7 等老服务器 |
| #1541 `9b04c90c` | Share Data PDU totalLength 少报不再拒绝 | VirtualBox VRDP |
| #1860 `0d493e5a` | 无 bitmap codec 时不声明 surface 能力 | Win7 退回基础位图更新 |
| #1966 `d2bb7376` | 补 ServerShutdown / ServerReboot 错误码 | 断开原因更准确 |
| #1788 `e0727394` | AVC420 regionRects 改为 `ExclusiveRectangle`（破坏性） | 见台账 |

## 决策

- 新分支 `nexshell-2026-09` 从 `upstream/master 9b151c4c`（2026-09-22）出发，按 ADR 0011 同法重放补丁；`nexshell-2026-08` 保留作回退。
- NexShell 源码零改动：`ActiveStage` 公开 API 只有新增（DVC tunnel、multitransport 编码等），用到的签名未变；crate 版本号均未变。
- 不引入新功能：reliable UDP / multitransport（#1869/#1858/#1836）等留待单独评估。
- #1923（AVC420 按 full-range BT.709 转色）只改库内 openh264 路径；NexShell 走 VideoToolbox + 自有转色（`decoder_vt.rs`，当前 BT.601），是否跟进另行真机核实，不混入本次。

## 补丁台账变化

12 个补丁中 10 个原样重放，2 个改写：

| 旧 fork | 新 fork | 处理 |
|---|---|---|
| bcf7df26 AVC420 按 regionRect 拷贝、exclusive 语义 | `43236749` | 上游 #1788 已把类型改成 `ExclusiveRectangle`，"把 Inclusive 类型当 exclusive 解读"部分退役；只保留逐 regionRect 拷贝，`extract_region_rgba` 改收 `ExclusiveRectangle` |
| 21d61edb `with_builtin_compositing` | `3d19a658` | 与上游 #1874 ResetGraphics 校验冲突。关闭内置合成时：不 reset compositor、不置 `pending_output_reset`（ActiveStage 不因此重置 session image）、不按 compositor 上限拒绝 reset（输出缓冲归 handler）；补单测 |

## 基线

- IronRDP：fork rev `65cb3a93cf26d96682de4f2b9560efa42d9a8d28`（分支 `nexshell-2026-09`），上游合并基 `9b151c4c`。
- IronRDP 验证：`ironrdp-egfx` 单测 62 通过；`ironrdp-testsuite-core` 1684 通过（含上游 #1848 Haven 真机 Progressive fixtures）；改动 crate clippy 无新告警。
- NexShell 验证：`cargo check --all-targets`（aarch64 / x86_64 macOS）、lib 测试 480、bin 测试 195 通过。Windows（x86_64-pc-windows-gnu）交叉检查中 ironrdp 全部依赖编译通过，仅 `src/osc7.rs` 的 `libc::gethostname` 报错——main 上 `f47bfb5` 引入的既有问题，与本次无关，不混入。
- 回放证据：Win11 EGFX dump（1205 条记录 / 1069 帧），`egfx_replay` 新旧逐帧 hash 完全一致、0 解码失败。该 dump 以 Progressive 为主，AVC420 路径的改写仅由单测覆盖（语义不变，只换类型）。
- 真机回归：待做（EGFX 画面、剪贴板、共享盘拷文件、音频、重连）。

## 回退

`git revert` 合入提交即回到 `d9ee675`；`nexshell-2026-08` 分支不删。
