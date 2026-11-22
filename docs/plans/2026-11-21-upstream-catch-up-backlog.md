# 上游追平 / 升级待办（2026-11-21 盘点）

三个上游（IronRDP、russh、Warp）在 2026-11-21 的状态，以及各自值得追的内容。只做了盘点，代码没改。每一项都是一次单独的追平，做之前由 Matt 拍板。

## 待 Matt 拍板

1. IronRDP 要不要开新一轮追平。前置条件是 ADR 0013 仍为 proposed、在等 Win11 实机回归：先把 0013 转为 accepted 再开，还是在新基线上合并回归？
2. russh 升级要不要排进计划。
3. Warp 的两个防崩溃修复要不要现在单独 cherry-pick，还是等下次整体同步。

## IronRDP：最值得追

- 现状：fork `lion1991/IronRDP` rev `538af770`，基线是上游 `9b151c4c`（2026-09-22），带 24 个补丁。上游之后又进了 335 个提交（截至 2026-11-20）。crates.io 版本：session 0.13、connector 0.12、cliprdr 0.9、egfx 0.5；我们锁在 session 0.11。
- 335 个提交里大多是服务端、FFI、web、activex、USB、打印机、摄像头、RemoteApp 等我们用不到的。对客户端有用的：

| 类别 | 上游 PR | 内容 |
|---|---|---|
| 剪贴板（ADR 0016 相关） | #2374 | 正确解码 CF_DIB bitfields（Windows 复制图片） |
| | #2053 | 格式数据应答按请求顺序配对 |
| 画面正确性 | #2117、#2133 | Progressive 每个 surface 各存 tile 状态 |
| | #2263 | RemoteFX 量化、DWT 取整、指针 alpha |
| | #2265 | 指针形状更新时同步位置、隐藏系统指针 |
| | #2378 | ack 暂停状态跨 graphics reset 保留 |
| | #2365 | 每个 surface 独立 H.264 解码器（我们走 VideoToolbox，需核实是否适用） |
| 安全加固（恶意服务器） | #2176、#2132、#2083、#2130、#2225 | Planar 先校验尺寸再分配、RLGR3 越界、静态通道重组按声明长度设上限、PDU 填充剩余长度检查、拒绝 512 位以下的 RSA 模数 |
| 体验 | #2451 | ShutdownRequest 之后的 EOF 按正常断开处理 |
| 性能 | #2150、#2297 | RLGR 位读取器重写、RGBA32 按行拷贝 |

- 有望退役的 fork 补丁：下列补丁按标题看上游已有类似修复，共约 9 个。重放时逐个核对，不要按标题直接删。

| fork 补丁 | 对应上游 PR |
|---|---|
| Soft-Sync 隧道 | #2007 / #2008 |
| 会话期带宽应答 | #2012 / #2128 |
| SRL | #2134 |
| DWT 取 REGION flags | #2085 |
| ClearCodec V-bar | #2433 |
| 驱动器不支持的 IRP | #2174 |
| AVC420 regionRects | #2052 |
| EGFX 失败不断会话 | #1789 |
| 驱动器路径沙箱 | #2221 |

- 代价：
  - 上游有多个破坏性改动，NexShell 代码要跟着改：#2182 / #2401 displaycontrol、#2012 session、#2115 / #2368 connector。
  - 改完要在 Win11 实机上回归。
  - 做法照 ADR 0011 / 0013：新开分支重放补丁，旧分支保留作回退。
  - 顺带评估 `docs/todo.md` 的 T-005：Progressive 改用库的 begin_frame / end_frame，去掉 NexShell 侧的二次解析。

## russh：落后多，升级成本大

- 现状：锁在 0.46.0，钉在 fork `lion1991/russh` 的 `nexshell-0.46` 分支（ADR 0017）。上游已到 0.65.x。
- 收益：
  - 拿到上游 2026-05 的安全修复。
  - 上游 #514 已经接受 `SSH-1.99-`，升级后可以撤掉 ADR 0017 的 fork 和 patch 项。
- 代价：
  - russh-keys 已并入 russh，密钥类型换成 `ssh-key`。连接、认证、known_hosts、主机密钥变更（ADR 0021）、SFTP 都要改。
  - 要按新 API 重写 ssh-rsa 追加和 known_hosts 归一化。
  - 要用 192.168.254.252 那台 SSH-1.99 交换机重新验证。
- 拖得越久，跨的版本越多。

## Warp：暂不追

- 现状：`../warp` 的 master 从上游 `86cfeb900`（2026-08-30）分出，上游之后进了 454 个提交。碰到我们所依赖 crate 的只有 30 个，大多是 AI 代理、云端功能顺带改的几行。
- 和我们相关的：
  - **防崩溃（建议跟进）**：
    - #15772：`FormattedTextElement::smart_select` 在 RTL 字形顺序反转时 panic。
    - #15705：字形无边界框时 `em_width` panic。这和我们的私有补丁 `5bf047181` 修的是同一处，下次同步时改用上游版本，删掉我们的补丁。
  - **小改进**：
    - #16105：vim 新增 `il` / `al` 文本对象（代码编辑器用得上）。
    - #15831：编辑器文本排版绕过一次性的 layout cache。
    - #16055：macOS 剪贴板识别 TIFF 图片。
    - #16789：warpui_core 的 spawn 只编译一份，编译量约降 5%。
  - **不适用**：#15763 宽字符提升崩溃修在 Warp 自己的网格（flat_storage）里，NexShell 终端网格不用它。
- 同步时按 ADR 0010 和特性保全清单（`2026-08-04-warp-preservation-ledger.md`）执行。推送顺序照 AGENTS.md：先推 warp。
