# 会话期带宽探测应答 + QoE 帧确认

## 1. 会话期带宽探测应答

当前 IronRDP 只在连接时应答带宽探测，会话期对 BW Start/Stop 只打 debug 日志然后丢弃。服务端因此停在初始带宽估值。

### 要实现

在 `ActiveStage` 和 UDP 隧道两条路径上，响应会话期带宽探测 PDU。fork 的 `ironrdp-pdu::rdp::autodetect` 已有完整编解码（requestType 枚举），不需要重写解析。

### 协议流程（MS-RDPBCGR §2.2.14）

1. 服务端发 **Bandwidth Measure Start**（§2.2.14.1.2）
2. 客户端记录收到时刻 `t_start`，清零字节计数器
3. 服务端持续发数据（连接期用 Bandwidth Measure Payload 0x0002，会话期用正常 PDU 替代）
4. 服务端发 **Bandwidth Measure Stop**（§2.2.14.1.4）
5. 客户端记录收到时刻 `t_stop`，构造 **Bandwidth Measure Results**（§2.2.14.2.2）应答

requestType / responseType 取值：

| 消息 | 连接期 | 会话期 TCP 主连接 / 可靠 UDP 隧道 | 有损 UDP 隧道 |
|---|---|---|---|
| Start | 0x1014 | 0x0014 | 0x0114 |
| Stop | 0x002B（带 payload） | 0x0429 | 0x0629 |
| Results（responseType） | 0x0003 | 0x000B | 0x000B |

0x0840 / 0x0880 / 0x08C0 是服务端→客户端的 Network Characteristics Result，客户端只接收不构造（已经由 `ActiveStageOutput::AutoDetect` 透出）。

### 应答 PDU 字段

| 字段 | 说明 |
|---|---|
| sequenceNumber | 最近一条 Stop 的 seqNum 回送 |
| responseType | 0x000B |
| timeDelta | `t_stop - t_start`，毫秒 |
| byteCount | `t_start` 到 `t_stop` 期间服务端→客户端的全部字节数 |

### 实现位置

两条路径，Windows 服务端可能走任一条：

**a. TCP 主连接：**
- fork `ironrdp-session/src/x224/mod.rs:516-519` 对 BW Start/Stop 打 `"not yet implemented"` 然后丢弃
- 字节计数需在 `ActiveStage::process` 入口统计 `frame.len()`（含 fastpath），x224 处理器自己看不到全部流量

**b. RDPEMT 隧道（UDP）：**
- `ironrdp-rdpeudp-tokio/src/tunnel.rs:140` 丢弃 sub_headers
- `transport.rs:756` 发包时子头恒为空
- 字节计数对 `read_tunnel_pdu` 返回的整个隧道 PDU 计数
- 应答需走 `send_data_with_sub_headers`（higher_layer_data 可为空），当前 write_pump 通道只传裸数据，需要改成带子头
- 子头的 SubHeaderLength / SubHeaderType 就是探测结构自己的 headerLength / headerTypeId，SubHeaderData 只剩 sequenceNumber 之后的字段：解析与回包都按完整子头来

### 先确认 Windows 走哪条路径

- TCP：`RUST_LOG=ironrdp_session=debug`，看是否出现 request_type 0x0014 的 `"Auto-detect request not yet implemented"`
- 隧道：临时在 `tunnel_data_loop` 打印非空 `sub_headers`（TLS 加密，抓包看不到）
- 两条都实现，但先知道哪条生效，验证时才知道看哪个计数器（`RemoteFX Network\Current TCP Bandwidth` / `Current UDP Bandwidth`）

### 收益说明

- 服务端开着 SelectNetworkDetect（Windows 默认）时，连接时探测初值已是 102400 kbps，会话期测量只会下修
- 弱网下下修带宽可能减少重传卡顿，也可能降画质，需 typeperf RemoteFX Network 前后对比
- 关了 SelectNetworkDetect 的服务端（估值卡在 10 Mbps）收益最确定

## 2. QoE 帧确认

规范明确标注为信息性和调试用途（MS-RDPEGFX §2.2.2.21："sent solely for informational and debugging purposes"）。FreeRDP 默认不发。不要把它当作弱网优化的预期收益。

### 要实现

在 EGFX EndFrame 处理时，标准 FrameAcknowledge 照发（§3.3.5.12 要求，fork 靠它回报队列深度给服务端做帧调速），额外追加一条 QoE 帧确认 PDU。仅在协商版本 ≥ V10 时发送（NexShell 声明 V10.7 但可能回落到 V8.1/V8，需在 `on_capabilities_confirmed` 后判断）。

### PDU 格式（MS-RDPEGFX §2.2.2.21）

**RDPGFX_QOE_FRAME_ACKNOWLEDGE_PDU：**

| 偏移 | 大小 | 字段 | 说明 |
|---|---|---|---|
| 0 | 8 | EGFX Header | cmdId = 0x0016, flags = 0, pduLength = 20 |
| 8 | 4 | frameId | 帧 ID |
| 12 | 4 | timestamp | 客户端开始处理 StartFrame 的本地毫秒（首个值定义原点） |
| 16 | 2 | timeDiffSE | StartFrame 到 EndFrame 的处理耗时 (ms) |
| 18 | 2 | timeDiffEDR | EndFrame 到渲染完成的耗时 (ms) |

### 关键参数

- frameId：对应 EndFrame PDU 的 frameId
- timestamp：单调时钟（`Instant`）自会话起的毫秒数，u32 wrapping，首值定义原点，服务端负责处理回绕
- timeDiffSE：从开始处理 StartFrame 到开始处理 EndFrame 的全部耗时（包含所有解码命令），不只是 VT 解码时间。`AVC420_TIMING` 是窗口累计值且被 diag 清零，不能直接用
- timeDiffEDR：从 EndFrame 到渲染完成的耗时。NexShell 在 handler 里只到 publish 给 UI 线程，拿不到真正上屏时刻，EDR 填到 publish 为止或照 FreeRDP 填 1
- 溢出保护：两个差值字段是 16 位毫秒，超过 65535 ms 时填 0（规范：超过 65 秒 SHOULD 填 0）

### 实现点

fork `ironrdp-egfx/src/client.rs` `handle_end_frame`。StartFrame 时记 `Instant`，EndFrame 时附加第二条 DvcMessage。约 30 行。

## 实施记录（2026-10-10）

- **fork 分支 `nexshell-2026-09`**：
  - `8182c7fc`：带宽测量两条路径。`ironrdp-session` 在 x224 处理，在 `ActiveStage::process` 计数；`ironrdp-rdpeudp-tokio` 新增 `autodetect.rs`，写泵通道改传 `TunnelData`。
  - `99495453`：QoE 帧确认。
  - `586e6fcb`：隧道子头按完整子头解析和回包。
  - 决策记入 ADR 0014 第 11、12 条。
- **NexShell**：`Cargo.toml` 钉到 `586e6fcb`，用 git 源 `cargo check` 通过。NexShell 侧代码不用改：服务端更新后的估值仍以 Network Characteristics Result 送达，`NEXSHELL_RDP_NET_TRACE=1` 能打出 TCP 上的估值，隧道上的估值看 `ironrdp_rdpeudp_tokio` 的 debug 日志。
- **真机**（Win11，base RTT 约 65 ms，ping 丢包 7.8%）：
  - **走哪条路径**：两条都用，隧道为主。初版隧道请求全部解码失败（RTT 请求只剩 4 字节），由此发现子头与探测头重叠。修复后，一次约 12 分钟的会话里应答隧道带宽测量 4134 次、TCP 7705 次。
  - **估值**：服务端隧道带宽估值从 363 kbps 随测量更新到 1–5 Mbps。TCP 上服务端测得的 RTT 为 200–900 ms，是丢包下队头阻塞造成的；隧道约 68 ms。
  - **样本精度**：约一半样本的 Start / Stop 在同一毫秒内处理，timeDelta 取下限 1 ms，单样本偏高，服务端平滑后无异常。
  - **待做**：用 typeperf 对照服务端估值；比较弱网下的帧率与画质。
