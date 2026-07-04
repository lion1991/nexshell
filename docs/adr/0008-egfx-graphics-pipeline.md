# EGFX 图形管线：ironrdp-egfx + VideoToolbox 硬解，connector 临时 fork patch

Status: accepted (2026-07-03)

## 背景

RDP 7.1 级图形（ADR 0007 已知短板）撞到天花板：现代 Windows 对 7.1 型客户端默认只给 GDI 位图管线（实测局域网 40 Mbps / 10 fps），RemoteFX 需要逐台服务端开三条组策略并重启，不可接受。微软 Windows App 的流畅度（同环境 150 Mbps+）来自 EGFX 图形管线（MS-RDPEGFX，RDP 8+/10：H.264/AVC 视频编码 + 严格帧边界），服务端见到 EGFX 能力即主动走视频管线，无需任何服务端配置。

调研结论（2026-07）：上游 `ironrdp-egfx` 0.2 已发布，是完整的 EGFX DVC 处理器（能力协商 V8~V10.7、帧流控、`H264Decoder` trait 留给调用方接解码器），与树内 ironrdp 依赖版本完全兼容；但尚无任何官方客户端集成先例，surface 像素合成层缺失，且 connector 未暴露 `SUPPORT_DYN_VC_GFX_PROTOCOL` 早期能力标志（上游 PR #1237 未合并）——没有这个标志服务端不会开 EGFX 通道。

## 决策

1. **接入 `ironrdp-egfx` 0.2 + `ironrdp-dvc` 0.7**（发布版，不切 git master），自己实现客户端第一次真实集成：surface 像素缓冲与合成、离屏 cache、EndFrame 对齐现有 publish_frame 发布语义。
2. **H.264 解码直接用 VideoToolbox 硬解**（实现 `H264Decoder` trait：Annex-B→AVCC + `VTDecompressionSession`），不走库内置 openh264 软解。理由：系统框架零专利负担（openh264 源码自编译不在 Cisco 授权范围）；Apple Silicon 硬解近零 CPU；objc2 生态已随 GPUI 在树（补 `objc2-video-toolbox` 系仅同生态加包）。
3. **connector 以 GitHub fork + `[patch.crates-io]` 钉 rev 打补丁**，仅加 `SUPPORT_DYN_VC_GFX_PROTOCOL` 开关（等价上游 PR #1237，38 行）。退出条件：#1237 合并发版后删 patch 回归发布版。
4. **保留 RDP 7.1 管线为自动回退**：服务端不支持 EGFX 时走现有 RemoteFX/位图路径，零回归。
5. **AVC444 暂缓**（上游无解码路径，色度重组需全自研，收益为视频模式文字锐度）；UDP 传输（MS-RDPEUDP）不做。

## 接受的代价

- fork patch 是临时维护负担（38 行、单点 OR 操作，风险低，有明确退出条件）。
- 作为 ironrdp-egfx 首个真实客户端集成方，surface 合成/缓存语义没有参考实现可抄，需按 MS-RDPEGFX 自行验证。
- VideoToolbox 为 macOS 专属；若未来跨平台需按平台换解码后端（`H264Decoder` trait 已是抽象边界）。

## Considered Options

- **openh264 软解起步、VT 收尾（两步走）**：调试变量更少，但多一次切换成本且引入专利灰区，Matt 拍板直接 VT。
- **等上游补齐（#1237/#1175/#1199 合并 + 官方客户端集成）**：无 ETA，阻塞整个方向。否。
- **connector 源码 vendor 进仓库**：60+ 文件入库、升级手动同步，比 fork 重。否。

## 第②步真机测试与修复（2026-07-03）

真机（Win，默认混合管线，无 AVC）暴露：ClearCodec 首块即 `suite exceeds region pixel count`
并级联 glyph/vbar cache miss；Progressive 首帧 `quant index 255 / table length 0` 后刷屏
`missing CONTEXT block`；仅 Uncompressed 小块上屏。诊断：

- **ClearCodec**（已修）：根因是 RLEX 子码解码器 bug + 解码器 per-surface 作用域错误。fork
  cherry-pick 上游 **PR #1175**（rev 9ba53227）：`rlex.rs` 解析期拒 `stop_index >= palette_count`、
  `clearcodec/mod.rs` 越界改容错 `break`+`palette.get`+像素边界检查、解码器改**连接级单例**
  （`GraphicsPipelineClient` 字段，仅 ResetGraphics 重建）。nexshell 删手动接线（`write_clearcodec`
  + per-surface `clear_decoders`），改由库 `on_bitmap_updated`（RGBA）落盘。矩形本就 exclusive
  （`right-left`），非 bug。
- **Progressive**（未解，上游缺口）：库 `ProgressiveDecoder` 作用域/上下文管理本就正确
  （per-`codec_context_id` + 缺 CONTEXT 回退），nexshell 调用无误。真机失败是 `RFX_TILE_DIFFERENCE`
  差分 tile 未消费导致的 region 解析错位（上游 **#1240** 跟踪，#1199 仅测试无修复）。暂保留该路径
  + 失败日志频控（前 5 + 每 300）。根治靠 AVC 或上游补差分 tile。
- **AVC 未启用**（服务端行为）：无「请求全 AVC」正向标志；AVC 默认开（未置 `AVC_DISABLED`），
  故客户端非主因。最接近的杠杆是 `AVC_THIN_CLIENT`（0x40，V10_3+）——提示服务端整屏用 AVC、
  少混其它 codec，后续 AVC 工作可试。`SMALL_CACHE` 是我方 capabilities() 硬编的缓存尺寸提示，
  与 AVC/解码失败无关，去留无正确性影响。

## 第③步 黑/灰块修复（2026-07-03，真机全链路溯源）

症状：文字/混合区域成片黑块 + 均匀灰块 (128,128,128)，持久不恢复。用 rdp_probe（headless
+ 鼠标抖动模拟交互）+ 逐 PDU 坐标 trace（`NEXSHELL_RDP_EGFX_TRACE`）+ 像素级 last-writer
覆盖掩码（`NEXSHELL_RDP_EGFX_COVERAGE`）+ fork 侧 payload dump 离线复放，定位三个独立根因：

1. **fork：ResetGraphics 误清 ClearCodec 缓存**。库在 `handle_reset_graphics` 里重建
   `ClearCodecDecoder`，但服务端 glyph/V-bar 缓存是连接生命周期（MS-RDPEGFX 2.2.2.14 只销毁
   surface/映射），真机连接 ~3s 必有第二次 ResetGraphics，之后所有 cache-hit 全 miss
   （"V-bar cache miss on hit" 级联）。修复：解码器跨 reset 存活（fork 73ba18b）。
2. **fork：ClearCodec subcodec 层 NSCodec 是静默 no-op**。Windows 把标签栏/任务栏/缩略图带
   这类混合区域编成 NSCodec 子码流，解码"成功"但输出保持全零 → 黑块，且被服务端 64px tile
   cache（SurfaceToCache/CacheToSurface 打底翻贴模式）快照后全屏扩散放大。修复：按 FreeRDP
   nsc.c 实现 MS-RDPNSC 解码（4 平面 RLE + AYCoCg→BGRA + 色度欠采样，fork 144ace2，
   新 `ironrdp-graphics/src/nscodec.rs`）。真机 88 个大面积全黑 payload 复放后归零。
3. **nexshell：DeleteEncodingContext 误清 Progressive tile 状态**。FreeRDP 里 DEC 是 no-op；
   我们据此调 `progressive.delete_surface`，服务端按 codec context 频繁发 DEC，随后的 UPGRADE
   tile 在全零系数上升级 → 恰好输出中性灰 (128,128,128) 并被 cache 固化。修复：DEC 改 no-op，
   tile 状态只随 surface delete/reset 消亡。

诊断工具沉淀：probe `RDP_JIGGLE`（无交互会被服务端 ~6s 掐线，画面永不收敛，是早期误判来源）、
trace/coverage 两个 env 开关、fork `clear_black`/`prog_coverage` 复放 example、
`IRONRDP_EGFX_DUMP_CLEAR_CAP`。注意：macOS 本地网络权限会让本会话编译的二进制 TCP 直连局域网
报 EHOSTUNREACH（系统 nc/python 不受限），headless 调试需 loopback 中继。

## 第④步 偶发绿色横线（2026-07-03）

黑/灰块修完后残留偶发 1px 绿色短横线。根因：fork 把 AVC420 metaData `regionRects` 按
inclusive（right/bottom +1）拷贝，但 RDPGFX_RECT16 线上语义是 **exclusive**（FreeRDP
gdi/gfx.c 对 AVC meta rect 一律 right−left 算宽；上游把它解析成 `InclusiveRectangle`
纯属类型误标——同一线格式在 WireToSurface1 destRect 就是 `ExclusiveRectangle`）。+1 每
region 多拷一行/一列：多数时候多拷的是相邻有效内容（不可见），当 rect 满高（exclusive
bottom=1080）时多拷的是 16 对齐解码帧（1920×1088）padding 宏块的未初始化 YUV → 绿色。
修复：exclusive 语义直取 min 裁剪（fork e311c53），回归测试锁定 padding 行不得泄漏进输出。
真机 60s 复测零绿线伪影。
