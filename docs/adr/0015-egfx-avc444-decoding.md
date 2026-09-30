# EGFX AVC444 解码：声明 V10.7，用上服务端的 H.264

Status: proposed (2026-09-29)

Plan: [实施计划](../plans/2026-09-29-egfx-avc444.md)

## 背景

- Win11 对只报 V8.1 的客户端不给 H.264：服务端事件 162 在 15 次连接里全是 `version 0x80105 … AVC available: 0`（见 ADR 0008 "真机更正"）。画面全走 Progressive / ClearCodec 等 CPU 解码，VideoToolbox 闲置，服务端的 H.264 硬件编码也用不上。
- 报 V10.x 且不置 `AVC_DISABLED`，服务端就给 H.264；但这等于声明支持 AVC444，Win11 不配任何策略也会选 AVC444（IronRDP #1563）。库没有 AVC444 解码，这类 PDU 只会落到 `on_unhandled_pdu`，画面全黑。
- 同机对照（DWMFRAMEINTERVAL = 15）：Windows App 报 V10.7 拿到 H.264，3456×2168 下 60–66 fps，因客户端资源不足跳帧接近 0；NexShell 在 1080p 下同样 60–66 fps，但每秒跳帧 1–2 帧。
- 预期收益：
  - 运动画面流量更小，弱网下重传更少。
  - 撑得起 Retina 原生分辨率。
  - 服务端能用显卡编码。
  - 4:4:4 色度，文字不发虚。

## 协议要点

依据 MS-RDPEGFX 2.2.4.5，FreeRDP（`libfreerdp/codec/h264.c`、`libfreerdp/primitives/prim_YUV.c`）作对照实现。

- **PDU 与编码号**：AVC444 走 WireToSurface1，codecId 为 0x000E（v1）/ 0x000F（v2），`destRect` 与其它 wire1 编码相同。
- **载荷结构**：`RFX_AVC444_BITMAP_STREAM` 由三部分组成：
  - 4 字节 streamInfo：低 30 位是第一路长度，高 2 位是 LC。
  - 一路或两路 `RFX_AVC420_BITMAP_STREAM`，各自带区域矩形和量化元数据。
  - LC 取值：0 表示主辅两路都有；1 表示只有主流；2 表示只有辅流，此时辅流放在第一路的位置。
  - fork 已能解析：`Avc444BitmapStream`（`crates/ironrdp-egfx/src/pdu/avc.rs`）。
- **主流**：一帧 YUV420。Y 为全分辨率；U/V 每个 2×2 块只有一个值，放在 (2x, 2y) 位置，存的是 2×2 块滤波后的值。
- **辅流**：同样是一帧 YUV420，装的是主流缺的那 3/4 色度采样：
  - v1：辅流 Y 平面每 16 行一组，前 8 行装 U 的奇数行，后 8 行装 V 的奇数行；辅流 U/V 平面装偶数行上的奇数列。
  - v2：辅流 Y 平面左半装 U 的奇数列，右半装 V 的奇数列，覆盖所有行；辅流 U/V 平面按四分之一宽分块，装奇数行上的偶数列（列号 4k 和 4k+2）。
- **反滤波**：合成后，每个 2×2 块的 (2x, 2y) 位置取 `4·原值 − 另外三个`。只有和原值相差 ≥ 30 才采用新值，否则保留原值（FreeRDP `CONDITIONAL_CLIP`）。
- **颜色**：YUV BT.709 全范围。FreeRDP 的系数是 403 / −48 / −120 / 475（除以 256）。不是 YCoCg，YCoCg 属于 RDP6 平面位图编码。

## 决策

1. **在 NexShell 的 EGFX handler 里实现，不改 fork 的解码路径**：在 `on_unhandled_pdu` 里接住 0x000E / 0x000F，解析复用 fork 的 `Avc444BitmapStream`。
   - 理由：库的 `H264Decoder` 只交出 RGBA，AVC444 合成需要原始 YUV。在 fork 里改接口会扩大 fork 补丁，和 ADR 0011 收缩 fork 的方向相反。
2. **VideoToolbox 解码器增加"输出 YUV 平面"模式**。主辅两路用两个解码实例还是共用一个，由计划第 0 步抓真实数据后决定：
   - Windows App 的二进制字符串显示每个会话建两个解码器。
   - FreeRDP 两路共用一个解码上下文（`h264.c` 的 `log_decompress`）。
3. **按 surface 保存 YUV444 平面**：
   - LC=1 只更新亮度和主色度，LC=2 只补辅色度。
   - 只处理区域矩形内的像素，区域外不动。
   - ResetGraphics 或删除 surface 时清掉对应状态。
4. **颜色转换改为 BT.709 全范围**。AVC420 路径现在写死了 BT.601（`decoder_vt.rs` 的 `ycbcr_to_rgb`），一并修正。
5. **能力声明**：新增 V10.7（SMALL_CACHE），排在 V8.1、V8 之前，由 `NEXSHELL_RDP_EGFX_AVC444` 控制：
   - 实现和真机验证期间：设为 1 才声明。
   - 验证通过后：改为默认声明，设为 0 关闭。
6. **先用 CPU 实现**，1080p 下估计每帧 7–8ms。Retina 原生分辨率需要的 Metal 合成另立后续。
7. **ADR 0008 决策 5（AVC444 暂缓）由本 ADR 取代。**

## 影响与风险

- **容易出错的地方**：
  - v1/v2 排布。
  - LC 状态。
  - 坐标映射：IronRDP #2042 发现 Windows 解出的帧是整个 surface 大小，区域矩形是 surface 坐标，不是相对 `destRect`。

  应对：第 0 步抓真实数据做离线回放，加上拆分—合成往返单测。
- **服务端负担**：服务端用软件编码 H.264 时，负担比 Progressive 高。Windows App 那组实测编码平均 7ms、最高 23ms。要用显卡编码，得同时开 `AVCHardwareEncodePreferred` 和 `AVC444ModePreferred`。
- **服务端选 AVC420 的情况**（比如策略关了 AVC444）：走库现有的 AVC420 路径和 VideoToolbox 解码。
- **客户端负担**：合成和颜色转换都跑在会话线程上，1080p 以上会挤占帧预算，要靠后续的 Metal 合成来解决。

## 回退

不设 `NEXSHELL_RDP_EGFX_AVC444`（开发期），或默认开启后设为 0：只报 V8.1 + V8，行为和现在一样。

## 验证

- **单测**：
  - v1、v2 各自的拆分—合成往返：按规范构造 YUV444，拆成主辅两路，再合成还原；(2x, 2y) 位置按反滤波规则比对。
  - LC=1 / LC=2 的状态处理、区域裁剪、BT.709 转换系数。
- **回放**：第 0 步抓到的真实 AVC444 数据离线解成图，和屏幕截图对照。
- **真机**：
  - 事件 162 显示 `AVC available: 1`。
  - 诊断计数里 AVC444 不再计入 DROP。
  - 同分辨率、同样操作下，和现状对比帧率、客户端跳帧、流量。
  - 服务端开显卡编码后，确认出现事件 170。
