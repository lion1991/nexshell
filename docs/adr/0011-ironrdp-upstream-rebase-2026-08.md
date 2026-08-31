# IronRDP 依赖追平上游并收缩 fork 补丁

Status: proposed (2026-08-31)；openspec change `upgrade-upstream-warp-ironrdp`

## 背景

fork `lion1991/IronRDP@egfx-fixes`（`f120928`，合并基 Devolutions `069786c` 2026-07-02）落后上游 500+ 提交。fork 的 21 个自有补丁过半已被上游等价实现，且上游带来我们缺失的可靠性修复（Fast-Path 输入批量 #1630、xrdp 裸 Disconnect Ultimatum #1692、FreeRDP 系服务器 Bandwidth Measure #1465/#1559、会话中途 CapsAdvertise 恢复 #1833、自动重连 cookie #1501、rdpdr QueryInformation #1810、SIMD 逆 DWT #1383 等）。

## 决策

- 新分支 `nexshell-2026-08` 从 `upstream/master 872845c`（2026-08-31）出发，只 cherry-pick/改写上游无等价的补丁；`egfx-fixes` 与旧 rev 原样保留作回退。
- NexShell `[patch.crates-io]` 全部 `ironrdp-*` 钉同一 rev；MSRV 随上游升到 1.94，仓库新增 `rust-toolchain.toml` 钉 `1.94.0`（Warp 仍 1.92，path 依赖随本 crate 工具链编译，已验证无影响）。
- 上游 #1443/#1461 在库内解码 Progressive 并内置合成器，与 NexShell 自有 EGFX 管线（`rdp_session/egfx/`，VideoToolbox 硬解 + 自有合成）重复。fork 增加 `GraphicsPipelineClient::with_builtin_compositing(false)`，NexShell 关闭库内解码/合成，行为与旧 fork 一致。
- Progressive 上屏改用上游 `DecodedTile`（像素 + REGION 裁剪矩形），帧内 tile 像素在 NexShell 侧缓存以保留原有"跨 PDU 累积重绘"语义（ADR 0008 第⑤步）。
- 剪贴板回环仍用 NexShell 自有 hash 抑制（`clipboard.rs`），不切上游 #1739 检测器：现有实现已验证、零改动零风险。

## 补丁台账

保留（改写后进入 `nexshell-2026-08`）：

| 旧 fork | 内容 | 处理 |
|---|---|---|
| f120928 | rdpdr-native 路径沙箱防 `..` 穿越 | 原样 cherry-pick |
| 2ab5c75 | ClearCodec V-bar 缓存游标与服务器 lockstep | 按上游 `resolve_vbar` 新签名改写 |
| 243bb84① / patches/band-order | Progressive UPGRADE 固定 extrapolate band 布局 | 改写 `decode/encode_upgrade_pass` |
| a0014f7 残余 | REGION flag 读 `Region.uses_reduce_extrapolate()`；`dwt_extrapolate::t` 饱和 clamp | 两处小改 |
| 3f168fe | egfx PDU/解码失败非致命 + `on_pipeline_error` 钩子 | 近原样 |
| c938eba + e311c53 | AVC420 只拷 regionRects、exclusive 语义（消绿边） | 重写 `decode_avc420` |
| d7cd990② | ClearCodec glyph hit 允许 dest 面积小于缓存 | 一行 |
| 6908d0c③ | rdpsnd 通道选项 `INITIALIZED\|ENCRYPT_RDP` | 改为 `Rdpsnd::channel_options()` 覆盖，不动 svc |
| （新） | `with_builtin_compositing(false)` | 见决策 |

退役（上游已等价，引用为证据）：32b3c99→#1237；9ba5322→#1175；2a12f37→#1698；fcfaee4→#1694；d7cd990①→`progressive_quant_for`；**05a442a→#1499（缩放域已换，叠加会错，必须退役）**；73ba18b→`handle_reset_graphics` 保留 ClearCodec 缓存；144ace2→#1728；4665094→#1803；6908d0c①②→`client_formats`/`play_wave`；bbd5249 CONTEXT 可省略→#1443；a0014f7 其余子点→#1696/#1499；243bb84②→#1443；8b4a6e7/42ada8d/4225a9b 重复或无目标文件。

延后（调试设施，非用户功能）：ab32498、9355e53、patches/raw-gfx-pdu-dump。NexShell 自有 `NEXSHELL_RDP_EGFX_WIRE_DUMP` + `examples/egfx_replay.rs` 覆盖回放需求；需要时再按新结构移植。

条件保留（需抓包确认后定）：bbd5249① TileState 按 surface 键控——若 Windows 跨 codecContextId 发 UPGRADE 则需要。

## 基线

- IronRDP：fork rev `21d61edb7ae7c3d670ace75c37720a847f5deffa`（分支 `nexshell-2026-08`），上游合并基 `872845c`。
- 验收证据：见 openspec change `upgrade-upstream-warp-ironrdp` tasks 第 4 节。

## 回退

`git revert` 合入提交即回到 `f120928`；`egfx-fixes` 分支不删。
