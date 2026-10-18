# SuppressOutput + MouseCursor DVC + Auto Reconnect + EGFX 渲染管线

各项参照 Windows App（macOS）的行为。和规范不一致的地方会注明，并写清 NexShell 按哪个做。2026-10-10 已对照 MS-RDPBCGR / MS-RDPEMSC 原文与本机 Windows App 二进制复核。实施状态见文末"实施记录"，架构决策记在 ADR 0014 决策 13–16。

## 0. 前置 fork 补丁（各一行）

- **General 能力集 extraFlags 声明 `AUTORECONNECT_SUPPORTED`（0x0008，MS-RDPBCGR 2.2.7.1.1）**。fork 写死为 `FASTPATH_OUTPUT_SUPPORTED | NO_BITMAP_COMPRESSION_HDR`（`ironrdp-connector/src/connection_activation.rs`），不声明时服务端不下发 Auto-Reconnect Cookie。真机 3 次会话的 Save Session Info 都只有 LogonInfoV1。
- **早期能力标志声明 `RNS_UD_CS_SUPPORT_HEARTBEAT_PDU`**（`ironrdp-connector/src/connection.rs` 的 `early_capability_flags`）。x224 已能解 Heartbeat PDU，但只打日志（`ironrdp-session/src/x224/mod.rs`），需要透出给 NexShell 做断链检测。Windows App 靠心跳判定连接掉线，然后触发重连。
- **已实现**（fork `f7da0a6e`）：心跳透出为 `ActiveStageOutput::Heartbeat { period, count1, count2 }`。真机：声明后服务端开始下发 cookie；心跳参数 period = 1 s、count1 = 8、count2 = 12。

## 1. SuppressOutput PDU

> **已撤销（2026-10-11，ADR 0020）**：Windows 服务端收到 suppress 后会话进入无 GUI 模式，远端截图、自动化程序失败。现在任何情况都不发；后台 RDP 标签只停本地重绘。下文留作记录。

窗口最小化时通知服务端停止发送图形更新，恢复时带上桌面矩形重新请求。NexShell 还要覆盖 RDP tab 不可见的情况。

### 要实现

- **前提**：服务端 General 能力集的 `suppressOutputSupport`（2.2.7.1.1，仅服务端设置）为真才发。IronRDP 上游在 `ConnectionResult.suppress_output_support` 透出（不是 fork 补丁），NexShell 连接时读取并打日志（真机为 true）。
- **发送**：`ActiveStage::encode_static` + `ShareDataPdu::SuppressOutput(SuppressOutputPdu { desktop_rect })`，走 TCP I/O 通道，fork 不用改。
- **触发**：
  - Windows App 只在最小化时发：受 `SuppressWhenMinimized` 开关控制，入口是 `CoreFSM::SuppressScreenUpdate`。遮挡、应用隐藏是否也触发，未核实。
  - NexShell 额外覆盖 RDP tab 非活动、切到其他页面。Windows App 一个窗口一个会话，没有这种场景。而 NexShell 的非活动 tab 现在仍逐帧整帧上传（`src/root_view/rdp_section.rs` 的 `FrameUpdated` 不看 tab 是否活动），这里收益更大。
  - 事件来源：warpui 没有最小化、遮挡、隐藏的回调。实现只挂一个观察者 `NSApplicationDidChangeOcclusionStateNotification`（`src/macos_window_util.rs`）：应用任一窗口有部分在屏上即可见，全部最小化、应用隐藏、被完全遮挡、锁屏都算不可见，一个通知覆盖了窗口最小化、遮挡和应用隐藏五个通知。代价是主窗口最小化而主机编辑窗口还开着时仍算可见。
  - tab 与页面切换不单独挂钩：RootView 的 render 每帧调 `sync_rdp_visibility`，按"终端页 + 活动 tab + 应用可见"算出每个会话的可见性。
  - 送进会话：`RdpSessionHandle::set_visible` 在句柄内去重，经可见性通道送到会话线程；会话侧状态没变不发。
- **状态重发**：服务端 Deactivate-Reactivate 之后、自动重连之后，按当前可见性重发一次。
- **恢复**：allowDisplayUpdates=1 带矩形的那条 PDU 本身就会触发整屏刷新，不用再发 Refresh Rect（fork 的 `request_full_redraw` 正是这么用的）。

### PDU 格式（MS-RDPBCGR 2.2.11.3.1）

**TS_SUPPRESS_OUTPUT_PDU（Share Data Header + payload）：**

| 偏移 | 大小 | 字段 | 说明 |
|---|---|---|---|
| 0 | 18 | Share Data Header | pduType2 = 35 (0x23) |
| 18 | 1 | allowDisplayUpdates | 0 = suppress, 1 = allow |
| 19 | 3 | pad | 填 0 |
| 22 | 8 | desktopRect (可选) | 仅 allowDisplayUpdates=1 时存在 |

**desktopRect（TS_RECTANGLE16）：**

| 偏移 | 大小 | 字段 |
|---|---|---|
| 0 | 2 | left (= 0) |
| 2 | 2 | top (= 0) |
| 4 | 2 | right |
| 6 | 2 | bottom |

- 规范：TS_RECTANGLE16 是包含式矩形（2.2.11.1），right = 宽 − 1，bottom = 高 − 1。fork 的 `request_full_redraw` 也这么填。
- Windows App 实发 right = 宽、bottom = 高（`COR::ORSendSuppressOutputPDU`），Windows 服务端两种都接受。NexShell 按规范填。

### 关键参数

- allowDisplayUpdates：不可见时发 0，恢复时发 1
- desktopRect：仅 allow=1 时附带，取当前桌面尺寸（动态缩放后的值）
- PDU 总长（不含 MCS / X.224 头）：suppress 时 22 字节，allow 时 30 字节

## 2. MouseCursor DVC（MS-RDPEMSC）

光标位图和位置更新改走这条 DVC。Soft-Sync 之后它随其他动态通道走 UDP 隧道，避开 TCP 的队头阻塞：真机在 7.8% 丢包下，TCP 上的 RTT 有 200–900 ms，隧道约 68 ms。

按规范，它会替代 TS_POINTER_PDU 和全部 FastPath 指针更新，位置更新也包括在内。NexShell 的光标位置在本地画，新路径里的位置更新照旧忽略，收益在形状更新上。

### 要实现

- **注册**：DVC 由服务端发 Create Request 创建，客户端不能主动打开。在 `build_dvc_client`（`src/rdp_session/egfx/mod.rs`）里注册通道名，`DvcProcessor::start()` 时发 CapsAdvertise。Windows App 也是在 `OnChannelOpened` 里发。
- **先探测**：只注册通道、打一行日志，确认这台 Windows 会不会创建它、是否经 Soft-Sync 迁到隧道，再做完整实现。Windows App 用 `EnableMouseCursorDVC` 开关控制这条通道。哪些服务端版本支持未核实，规范 v1.0 发布于 2023-09。
  - 探测结果（Win11）：服务端创建了该通道（channel 8），Soft-Sync 清单 `[2, 6, 7, 8, 9, 10, 11, 12]` 也把它迁进了隧道。
- **已实现**（`src/rdp_session/mouse_cursor.rs`，默认开启，`NEXSHELL_RDP_MOUSE_CURSOR_DVC=0` 关闭）：`start()` 发 CapsAdvertise，处理 CapsConfirm 与六种 updateType。真机（Win11，2026-10-10）：服务端确认能力，之后 87 次光标变化全部经 DVC 下发（含 41×39 的大光标），FastPath 指针为 0，无解码错误。
- **CapsConfirm 不合法**：Windows App 关闭通道、回落到 FastPath 指针。NexShell 只记日志、照常处理更新：ironrdp-dvc 的处理器没法主动关通道，且服务端确认后已改走这条通道发光标。
- **错误处理**：处理器返回错误会被 DVC 层当成致命错误，所以坏包只记日志；指针解码失败同 FastPath，丢掉该缓存槽、退回系统箭头。
- **解码复用**：
  - `ironrdp-pdu` 的 basic_output/pointer：ColorPointerAttribute / PointerAttribute / CachedPointerAttribute / LargePointerAttribute / Point16。
  - `ironrdp-graphics` 的 `DecodedPointer::decode_*`。
  - 光标缓存（缓存索引 → 位图）要自己建：FastPath 的缓存在 `ironrdp-session` 内部，没法共用。
- **出口**：处理器持有 `event_tx`，发 `PointerChanged`，和 FastPath 指针走同一条路径。DRDYNVC 目前只在开 EGFX 时注册，这条通道同样依赖 EGFX。

### PDU 格式

**RDP_MOUSE_CURSOR_HEADER（2.2.2.1）：**

| 偏移 | 大小 | 字段 | 说明 |
|---|---|---|---|
| 0 | 1 | pduType | 0x01 CS_CAPS_ADVERTISE / 0x02 SC_CAPS_CONFIRM / 0x03 SC_MOUSEPTR_UPDATE |
| 1 | 1 | updateType | 只对 MOUSEPTR_UPDATE 有意义 |
| 2 | 2 | reserved | 0 |

**CapsAdvertise（客户端→服务端，16 字节）= header + 能力集（2.2.2.3.1）：**

| 偏移 | 大小 | 字段 | 值 |
|---|---|---|---|
| 0 | 4 | header | pduType = 0x01，其余 0 |
| 4 | 4 | signature | 0x53504143（"CAPS"） |
| 8 | 4 | version | 0x00000001 |
| 12 | 4 | size | 0x0000000C |

Windows App 实发字节：`01 00 00 00 43 41 50 53 01 00 00 00 0C 00 00 00`。

**updateType**：0x05 SYSTEM_NULL、0x06 SYSTEM_DEFAULT、0x08 POSITION、0x0A CACHED、0x0B POINTER、0x0C LARGE_POINTER。载荷已对照 MS-RDPEMSC v2.0（2024-04）核对：POSITION 为 TS_POINT16，CACHED 为 2 字节缓存索引，POINTER / LARGE_POINTER 与 MS-RDPBCGR 的 TS_POINTERATTRIBUTE / TS_FP_LARGEPOINTERATTRIBUTE 布局相同，结尾可有 1 字节 pad；SYSTEM_NULL / SYSTEM_DEFAULT 无载荷。

### 关键参数

- 通道名：`Microsoft::Windows::RDS::MouseCursor`
- DVC 发送 flags：0

## 3. Auto Reconnect

网络中断后自动重连回原会话，用户不用重新输入密码。

### 前提

- 第 0 节的 `AUTORECONNECT_SUPPORTED`。
- 服务端没有禁用自动重连。
- NLA 下重连照样要走 CredSSP 认证。NexShell 内存里有密码（`RdpTabState.config`），所以用户不用重新输入。cookie 的作用是精确接回原会话；即使没有 cookie，Windows 默认也会把同一用户接回他断开的会话。

### Cookie

- **来源**：Save Session Info PDU（INFOTYPE_LOGON_EXTENDED_INFO，LOGON_EX_AUTORECONNECTCOOKIE）里的 ARC_SC_PRIVATE_PACKET。fork 透出为 `ActiveStageOutput::AutoReconnectCookie`，NexShell 在 `rdp_session/outputs.rs` 记入会话内存。
- **保存**：服务端每小时以及每次连接都会换 cookie，始终用最新一个。只存内存，按 tab 保存。增强安全模式下，同一个 cookie 算出的 verifier 是固定的，拿到 cookie 就能接管会话（规范 5.5 也要求只存内存）。
- **重连时**：调用 `ClientConnector::with_auto_reconnect_cookie`，fork 会在 Client Info 的 extraInfo 里填 ARC_CS_PRIVATE_PACKET。`cbAutoReconnectCookie` 为 0x1C，并要求前面的 performanceFlags、clientSessionId 字段存在，fork 已处理。
- **Auto-Reconnect Status PDU**（pduType2 = 50）：表示 cookie 被拒，fork 透出为 `AutoReconnectFailed`。这时退回普通凭据登录，不再带 cookie。

### PDU 格式（MS-RDPBCGR 2.2.4.2 / 2.2.4.3，校验值见 5.5）

**Server Auto-Reconnect Packet（保存）：**

| 偏移 | 大小 | 字段 | 说明 |
|---|---|---|---|
| 0 | 4 | cbLen | 28 |
| 4 | 4 | version | 1 |
| 8 | 4 | logonId | 会话 ID |
| 12 | 16 | arcRandomBits | 服务端随机数 |

**Client Auto-Reconnect Packet（重连时发送）：**

| 偏移 | 大小 | 字段 | 说明 |
|---|---|---|---|
| 0 | 4 | cbLen | 28 |
| 4 | 4 | version | 1 |
| 8 | 4 | logonId | 原始会话 ID |
| 12 | 16 | securityVerifier | HMAC-MD5(key = arcRandomBits, data = ClientRandom) |

- ClientRandom **不是** TLS 握手里的 client random。TLS / NLA（增强安全）下没有 RDP client random，按 5.5 用 32 字节 0。fork 的 `ClientAutoReconnect::from_server_cookie` 已实现，NexShell 不用自己算。

### 重连策略（参照 Windows App `CTSAutoReconnectionHandler`）

- **只对网络类断开重连**：
  - 会重连：socket 类错误（DNS、连接、收发、超时、断开）、错误码 267 / 520、SSL 类（少数码除外）。
  - 不重连：服务端主动断开，如 "Another user connected"、注销、管理员断开。Windows App 对这类断开的日志是 "Not ARCing event, not a network error"。
- **节奏**：第一次立即重试，之后每 3 秒一次，次数上限由 `MaxAutoReconnectAttempts` 控制（Windows App 的默认值未核实）。
- **断链检测**：心跳（第 0 节），加上读错误、UDP 隧道关闭。
- **Mac 睡眠唤醒**：Windows App 的提示会区分"连接中断"和"Mac 进入睡眠"。NexShell 的 `AppCallbacks` 有 `on_cpu_will_sleep` / `on_cpu_awakened` 两个回调，目前没用上。
- **重连后回传网络特征**：用 Network Characteristics Sync（responseType 0x0018）把上次测得的带宽和 RTT 告诉服务端（Windows App 也这么做），服务端就不用从初值重新收敛。
  - 时机按规范 2.2.14.2.3：它用来回应**连接期**的 RTT Measure Request 或 Bandwidth Measure Start，让服务端跳过连接期探测，不在会话期发。fork 加了 `ClientConnector::with_network_characteristics_sync`，只回应第一条请求。
  - 取值：带宽取服务端最近一次 NetChar 结果里的 bandwidth，RTT 取 averageRTT（同 FreeRDP）。两者都测到过才发。

### NexShell 改动点

- **断开原因类型化**：现在只有字符串（`read frame failed` / `reliable UDP tunnel closed` / `terminated: …`）。
  - 实现在 NexShell 侧（`src/rdp_session/reconnect.rs`），fork 不用透出 ErrorInfo 码：服务端发来的 Terminate（含 ErrorInfo）一律不重连。
  - 读写 I/O 错误中 InvalidData（帧解析失败、TLS 记录损坏）算致命，其余（断开、重置、超时）算网络类。connector 的认证、协商错误算致命，错误链里带 I/O 错误的按同一规则分。
  - 证书变化算致命，单独走 `SessionError::CertificateChanged` → `RdpEvent::CertificateChanged`：UI 显示证书变更页（新旧 SHA-256 指纹），「信任新证书并连接」固定用户看到的那枚新指纹（`rdp_cert_store::pin`）后重连；编辑保存主机仍会清掉指纹。
- **重连循环**：放在 `run_rdp_event_loop`，它持有 SessionInputs、framebuffer 和 event_tx。`connect_and_run` 加一个 cookie 参数。UI 加"重连中"状态，用覆盖层显示重试次数，同时保留旧画面。
- **重建状态**：重建 UDP 隧道和 EGFX 状态，重发 SuppressOutput 状态（已随 ADR 0020 删除）。另外沿用断开时的桌面尺寸，并丢弃断线期间攒下的键鼠输入。
- **实现补充的保护**：
  - 次数上限取 20，同 .rdp 文件 `autoreconnect max retries` 的默认值。连接阶段（TCP → TLS → NLA → 激活）总时限 60 s，断网时读写会一直挂着，靠它进入下一次尝试。
  - 连续 3 次走到 NLA 却没进入活动期就停：服务端认证失败时可能直接断 TCP，看起来像网络错误，一直重试会锁账户。
  - UDP 隧道先报错时，等 TCP 1 s 看服务端会不会发来 Terminate：服务端断开时 UDP 可能先报错，不等一下会把被挤下线当成断网，重连回去又把对方挤掉。
  - 心跳超时为 period × (count1 + count2)，TCP 上任何数据都会重置计时（真机 1 × (8 + 12) = 20 s）。Mac 唤醒后缩短为一个周期加 2 s。
- **先修既有 bug**（已修）：服务端 Terminate 之后，`run_rdp_event_loop` 又补发一条 `Disconnected{"session ended"}`，覆盖掉真实原因。现在只由 `run_rdp_event_loop` 发一次 Disconnected。

## 4. EGFX 渲染管线优化

Windows App 的渲染管线：解码写入 CPU 纹理 → 按 dirty rect CopyRect 到前端缓冲 → Metal compute shader 缩放 → present。

### 4.1 按 dirty rect 更新纹理

Windows App 在 `OffscreenSurface::Flush` 里按 dirty rect 逐个 `CopyRect`，把解码纹理拷到前端缓冲。rect 先用 NEON SIMD 裁剪到 surface 边界，flush 后清空 region。

**现状**：

- 解码这一侧已经按脏区包围盒累积和合成。
- 瓶颈在 UI 侧。每次 `FrameUpdated` 都会：
  - 持锁把整帧拷进一个新 Vec；
  - `insert_raw_asset_bytes` 再拷一次成 RgbaImage；
  - Metal 端按 `Arc<StaticImage>` 指针缓存纹理，所以每帧都新建 MTLTexture，并整帧 `replaceRegion`（`src/root_view/rdp_section.rs`）。
- 每次拷贝 1080p 约 8 MB，HiDPI（3456×2160）约 30 MB。

**要实现**：

- macOS 上 warpui 用的是 Metal 渲染器，不走 wgpu 的 `write_texture`。需要在 warpui 加常驻纹理和按矩形 `replaceRegion` 的接口。这是跨仓改动，要遵守 ADR 0010 的保全要求。
- `FrameUpdated` 本来就带 dirty（目前被丢弃），改成只拷脏区。
- dirty region 可以从单个包围盒升级为矩形列表，rect 裁剪到 surface 边界，flush 后清空。
- 动手前先用 `NEXSHELL_RDP_EGFX_DIAG=1` 量出当前每帧的拷贝和合成耗时，改完再对比。
- **已实现**（ADR 0014 决策 17，Warp fork 保全清单 W-022）：
  - warpui_core 新增常驻图片：`LiveImageSource::try_read`（非阻塞读像素，并给出某帧代号以来的脏区，None 表示整帧）、`Scene::draw_live_image`、`LiveTextureCache`（按 source 的 Arc 身份保留纹理；source 释放或 10 帧未画就回收）。
  - Metal 两份纹理轮流写，避开上一帧可能仍在 GPU 上读的那份；写入前再按帧栅栏确认最后读它的帧已在 GPU 上执行完（drawable 池刚建时可能有两帧在途）。wgpu 用一份。headless 渲染器不画东西，不用改。
  - `RdpFramebuffer` 记最近 32 次发布的脏区，换分辨率帧代号不归零；`rdp_view/live_image.rs` 把它接成像素源。UI 收到 `FrameUpdated` 只触发重绘，关 tab 也不用再覆盖透明帧释放资产。
  - 脏区：会话每帧仍只发布一个包围盒，渲染器拿到的是自己帧代号之后各帧包围盒的列表，逐个上传。
  - 效果：UI 线程不再有整帧拷贝；GPU 只上传脏区，全屏视频时也只剩一次整帧上传（原来是两次 CPU 拷贝加一次新建纹理整帧上传）。
  - 测量：`WARPUI_FRAME_STATS=1` 看 `gpu.live_image_upload` 和 `cpu.encode_commit`。原 `NEXSHELL_RDP_EGFX_DIAG=1` 下的 `[rdp-ui]` 整帧拷贝统计随旧路径删除。
  - 真机短测（2026-10-10，约 16 s）：`gpu.live_image_upload` p50 0.08 ms / p95 0.82 ms / max 3.24 ms，`cpu.encode_commit` p50 约 0.2 ms，EGFX 无解码错误。

### 4.2 非阻塞纹理锁

Windows App 的 `drawRect:` 用 `MetalGraphicsHandle::TryLockTexture` 非阻塞地拿纹理锁，拿不到就跳过这一帧，避免解码线程和渲染线程互相阻塞。

**现状**：

- 绘制本身不加锁（`src/rdp_view/page_element.rs`）。
- 锁竞争发生在两处：UI 收到帧时持锁做整帧拷贝，解码侧 EndFrame 合成也要拿锁。两边用的都是阻塞的 parking_lot Mutex。

**建议**：

- 4.1 把锁内拷贝缩到脏区以后，持锁时间自然变短，可能就不需要这一项了。
- 如果仍要用 try_lock，拿不到锁时必须留到下次补上，不能直接丢掉，否则画面会停在旧帧。

**已实现**（随 4.1）：
- 渲染器用 try_lock 读 framebuffer，拿不到就画旧纹理；纹理记着自己的帧代号，下次读到时补上落下的脏区。
- 写帧方解锁后才发 `FrameUpdated`，会再触发重绘；锁被只读方占着时没有后续事件，`RdpLiveImage` 读不到就经 redraw 通道请求一次重绘。
- UI 线程原有三处阻塞地锁 framebuffer 读桌面尺寸（页面渲染、动态分辨率轮询、连接信息面板），改为按 `Resized` 事件跟踪；首次连接激活时尺寸变了也发 `Resized`。

### 4.3 Compute shader 缩放：不需要

Metal 采样器已经在 GPU 上做线性缩放，画到 letterbox 矩形里。HiDPI 下 NexShell 按物理像素请求桌面，基本是 1:1。有锐化或色彩空间转换的需求时再考虑。

### 4.4 AVC420 只转换更新区域（Windows App 文档之外的补充）

VT 输出 NV12，现在每帧都用 vImage 把整帧转成 RGBA、写进一个新 Vec（`src/rdp_session/egfx/decoder_vt.rs`），然后再按区域拷贝，更新区域小的时候很浪费。AVC444 路径的做法是借用 NV12 平面、只转换区域矩形（`avc444_stream.rs`），AVC420 可以照搬。

**已实现**：
- AVC420 由库内解码，NexShell 拿不到 NV12。把解码器传成 None 也不行：库会因此过滤掉 AVC 能力集。
- 所以在 fork（`287f337d`）给 `H264Decoder` 加了 `decode_regions`，默认实现是整帧解码再裁剪，行为不变。
- NexShell 的 VT 解码器覆盖它：借用 NV12 平面，把每个区域向外对齐到偶数坐标（色度 2×2 下采样）后用 vImage 转换，再裁掉多转的边（`vimage::nv12_regions_to_rgba`）。单测确认结果与整帧转换后裁剪逐字节一致。

**收益有限**：ADR 0015 实测 2560×1472 整帧 vImage 转换约 0.4 ms。这一项省下的是这部分转换和一次整帧分配；全屏视频时区域就是整屏，没有收益。另外 macOS 默认声明 AVC444，只有混合模式下的 AVC420 帧或关闭 AVC444 时才走这条路径。

## 实施记录

原定顺序与当前状态（2026-10-10）：

| 步骤 | 状态 | 位置 |
|---|---|---|
| 0. fork 能力位 | 已做，真机已验证 | fork `f7da0a6e` |
| 1. SuppressOutput + "session ended" 覆盖 bug | 已做；真机见到 suppress / allow 收发。2026-10-11 撤销 SuppressOutput（ADR 0020） | `root_view/rdp_section.rs`、`macos_window_util.rs` |
| 2. Auto Reconnect | 已做，未做真机验证 | fork `f7da0a6e`；`rdp_session/reconnect.rs` |
| 3. EGFX 4.1 / 4.2 | 已做，真机短测通过（改分辨率、重连未测） | Warp `f1cf81c31`（`warpui_core/src/rendering/live_image.rs`、Metal / wgpu 渲染器）；`rdp_view/live_image.rs`、`rdp_session/framebuffer.rs` |
| 3. EGFX 4.4 | 已做 | fork `287f337d`；`egfx/decoder_vt.rs`、`egfx/vimage.rs` |
| 4. MouseCursor | 已做，真机已验证 | `rdp_session/mouse_cursor.rs` |

`rdp_session/mod.rs` 超过 1500 行，按约定把帧输出处理拆到 `rdp_session/outputs.rs`。

以下真机验证决定不做（2026-10-10）：
- 断网 20 s 以上后自动重连，并接回原会话；
- 睡眠唤醒后的提示语和重连；
- 在别处登录同一账号把 NexShell 挤下线时，不自动重连；
- 最小化、隐藏应用后画面能恢复。
