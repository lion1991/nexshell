# RDP 弱网优化：可靠 UDP 旁路（连接类型自动探测已否决）

Status: proposed (2026-09-28，2026-10-10 增补决策 11–16)；Win11 真机验证 UDP 迁移与弱网表现后转 accepted

## 背景

实际场景里 RDP 会被包在 UDP 隧道中，隧道丢包约 5%。此前 NexShell 只走 TCP：

- 连接类型写死 LAN，服务端按局域网画质编码，不管真实链路。
- 不声明多传输能力，服务端不会发起 UDP。

TCP 单流在 5% 随机丢包下的理论吞吐上限（Mathis 模型，MSS 1380，未计重传超时，实际更低）：

| RTT | 上限 |
|---|---|
| 30 ms | 约 2 Mbps |
| 80 ms | 约 0.75 Mbps |
| 150 ms | 约 0.4 Mbps |

另有队头阻塞：大块画面更新由几十个包组成，大多数更新会碰上丢包，要多等至少一个 RTT。

上游 IronRDP 2026-09（ADR 0013 基线）已实现可靠 UDP：RDPEUDP2 + TLS + RDPEMT 隧道，DRDYNVC Soft-Sync 把动态通道迁入隧道。参考客户端 `ironrdp-client` 在 `udp` feature 下接好。

## 决策

1. **连接类型保持 LAN，Autodetect 仅供试验**（`NEXSHELL_RDP_AUTODETECT=1`）。服务端估的带宽取决于它做不做连接时探测，与客户端报哪种连接类型无关：客户端声明了 `SUPPORT_NET_CHAR_AUTODETECT`，服务端开着探测就会测。真机（Win11，服务端计数器 `RemoteFX Network\Current TCP/UDP Bandwidth`）：

   | 服务端连接时探测（RDP-Tcp `SelectNetworkDetect`） | 报 LAN | 报 Autodetect |
   |---|---|---|
   | 开（0，Windows 默认） | 实测 102400 kbps | 实测 102400 kbps |
   | 关（1，事件 101 "Reason Code 2"） | 恒为 10000 kbps（LAN 类型的下限 10 Mbps） | 默认特征 RTT 400 ms、512 kbps；RemoteFX 自适应图形"按最小网络带宽优化"（事件 166），帧率掉到 6 以下 |

   报 LAN 在两种配置下都能用，Autodetect 在关了连接时探测的服务端上不可用，所以默认 LAN。关了连接时探测后，服务端靠会话中的持续探测修正估值：Windows App 应答会话期带宽测量，同配置下测到 95–103 Mbps；IronRDP 此前只在连接时应答带宽测量，会话期只应答 RTT，估值停在初值（决策 11 补上）。10 Mbps 的估值看起来压住了服务端的发送量：同样拖窗口，估值 10 Mbps 时流量平均 9.6 Mbps、最高 18 Mbps；100 Mbps 时平均 13.7、最高 37（两次操作不完全相同）。服务端建议保持默认（`SelectNetworkDetect` 为 0 或不配置）。
2. **声明 `TRANSPORT_TYPE_UDP_FECR | SOFT_SYNC_TCP_TO_UDP`**。服务端请求可靠 UDP 时建隧道，EGFX 图形、Display Control 等动态通道经 Soft-Sync 迁入后双向走 UDP。静态通道（剪贴板、rdpsnd 音频、rdpdr）与快速路径键鼠输入仍走 TCP。DRDYNVC 随 EGFX 注册，关 EGFX 时不声明 UDP，否则隧道建成后无通道可迁、启用隧道会报错断开。
3. **只做可靠 UDP**。有损 UDP（FEC + DTLS）上游未实现，服务端请求时回 E_ABORT。EGFX 依赖参考帧与帧确认，mstsc 也走可靠通道；NexShell 音频走 rdpsnd 静态通道，不受影响。
4. **与参考客户端的差异**（`src/rdp_session/udp.rs`）：
   - 直接调 `connect_udp` 而非 `MultitransportBootstrap::connect`，握手 3s、隧道 5s、TLS 10s（上游 10s / 10s / 130s）。连接期请求是内联等待的，UDP 被挡的主机每次连接都要白等这段时间。
   - 接收窗口 2^8 = 256 包，约 290 KB（上游默认 64 包，RTT 150ms 时下行被卡在约 4 Mbps）。
   - MTU 取协议下限 1132（上游 1232）。握手包要补零到 MTU，1232 字节载荷的 IP 包 1260 字节，过不了 MTU 1160 的隧道，分片在途中被丢。
   - 只提议协议版本 2（上游 3）。版本 3（RDPEUDP2）MTU 固定 1232，与 1132 冲突，Windows 11 收到后不应答；版本 2 按协商 MTU 走 MS-RDPEUDP 可靠传输。
   - UDP 的 TLS 与 TCP 同策略：平台根严格校验 + 同一 `host:port` 的 TOFU 指纹回调。
5. **缩放改走 `prepare_resize` 并按通道分流**。`encode_resize` 固定走 TCP，Soft-Sync 后 Display Control 已在 UDP 上。
6. **开关与观测**：UDP 默认开启，`NEXSHELL_RDP_DISABLE_UDP=1` 退回纯 TCP；`NEXSHELL_RDP_AUTODETECT=1` 改报自动探测；`NEXSHELL_RDP_NET_TRACE=1` 打印服务端测得的网络特征。有通道迁到 UDP 后，统计面板"传输"显示 `TCP + UDP`，UDP 收包计入接收码率。
7. **依赖**：新增 `ironrdp-rdpeudp` / `ironrdp-rdpeudp-tokio` / `ironrdp-rdpemt`，与其余 ironrdp crate 同走 fork 路径 patch。fork 在 ADR 0013 基线 `65cb3a93` 上追加决策 8（`7240b9a6`）、决策 9（`03e51ba6`）、决策 10（`973d4e78`、`40800b26`）、决策 11（`8182c7fc`、`586e6fcb`）、决策 12（`99495453`）、决策 13（`f7da0a6e`）、决策 15（`287f337d`）；当前 rev 见 ADR 0013 基线。
8. **fork 补丁：Soft-Sync 按服务端清单路由**（`ironrdp-dvc` client）。Windows 的 Soft-Sync 请求会列出客户端拒绝过或尚未打开的通道 ID（实测 `[2, 6, 7, 8, 9, 10, 11, 12]`，客户端只开了 7 号 Graphics）。上游见到未知 ID 就跳过整条隧道，而服务端发出请求时已经改走隧道（MS-RDPEDYC 3.2.5.3.1），结果画面停在"正在连接"。补丁：
   - 列出的 ID 全部按服务端路由记下，应答里带上该隧道。
   - 隧道上除 Data 外也接受 Create / Close，应答原路回隧道。实测 Windows 在 Soft-Sync 之后经隧道新建 Video::Control / Video::Data / Geometry（清单内 ID），以及清单外的 `AUDIO_PLAYBACK_DVC`（ID 16）。
   - 通道走哪条传输，跟随它的 Create 从哪条传输到达：隧道上的 Create 把该 ID 路由到隧道，TCP 上的 Create 把该 ID 改回 TCP（服务端会复用 ID）。
9. **fork 补丁：UDP socket 收发错误不再致命**（`ironrdp-rdpeudp-tokio` driver）。上游驱动遇到任何 socket 错误都会退出，隧道随之关闭，会话断开。补丁：
   - 发送失败按丢包处理：数据靠 RTO 重传，ACK 由下一个替代。macOS 在接口队列（如 VPN utun）满时会返回 ENOBUFS。
   - 接收时遇到 ICMP 回报的错误（拒绝、重置、主机或网络不可达）跳过。链路真断了仍由 65s 空闲超时关闭。
   - 驱动出错退出时打 warn 日志；`shutdown` 优先返回驱动自己的错误，断开提示里能看到根因，而不只是 TLS 层的"transport error"。
10. **fork 补丁：丢包下 ACK 完整上报**（`ironrdp-rdpeudp` connection / recv_window）。服务端同一数据报重传几次仍收不到 ACK 就判连接丢失（MS-RDPEUDP 3.1.6.1）；Soft-Sync 之后图形通道在隧道里，服务端随即拆掉整个连接，客户端看到的是 TCP 被重置。抓包确认：**Windows 只认 ACK 向量里列出的包，不按累计语义处理**。它自己当接收端时，向量也从对端最近一次 AoA 报起。上游接收端有以下缺陷：
    - **根因：发 ACK 即释放接收窗口**（`40800b26`）。每发一个 ACK 就把已连续收到的部分移出窗口，之后的向量只从第一个洞报起。某个包唯一的那次 ACK 在隧道里丢了，这个包就再也不会被报告，服务端重传到上限后断开。补丁：MS-RDPEUDP（v1/v2 帧格式）发 ACK 不再释放，向量下沿只随服务端 AoA 推进（RDPEUDP2 不变）；接收上限改从第一个洞起算一个窗口，等待 AoA 的已收历史不占窗口，最多保留一个窗口的历史，服务端不发 AoA 时也不会无限增长。
    - ACK 向量最多 32 个元素，从最高序号往下填，满了就丢最旧的段，洞多时向量够不到最低的洞（`973d4e78`）。补丁：按接收窗口预留向量空间（每槽一个元素，上限 256），不再截断；万一截断，保留最旧的段并调低 `snSourceAck`。代价是上行每包载荷从 1076 降到 852 字节，上行只有帧确认等小包，可忽略。
    - 重复包和窗口外的包不回 ACK（`973d4e78`）。补丁：每个 Source Packet 都回 ACK（3.1.1.4）。
    - ACK-of-ACKs 的累计值漏算重传队列里的包，可能越过尚未确认的上行数据（`973d4e78`）。补丁：取发送窗口与重传队列中的最小序号。
11. **fork 补丁：应答会话期带宽测量**（`ironrdp-session` x224 / `ironrdp-rdpeudp-tokio` autodetect）。连接后服务端用 Bandwidth Measure Start / Stop 持续测带宽（MS-RDPBCGR 2.2.14.1.2 / 2.2.14.1.4，requestType 0x0014 / 0x0429；有损 UDP 为 0x0114 / 0x0629），客户端回 Bandwidth Measure Results（2.2.14.2.2，responseType 0x000B）：timeDelta 为收到 Start 到收到 Stop 的毫秒数，byteCount 为其间服务端→客户端的全部字节（规范：连接后用正常 PDU 替代 Payload 消息）。规范允许两条承载路径，都实现：
    - TCP 主连接的 Auto-Detect Request PDU：上游只打 debug 日志后丢弃。补丁在 x224 处理器开窗，`ActiveStage::process` 入口按整帧累加（含 fastpath），Stop 时在 message channel 回应答。
    - RDPEMT 隧道子头（MS-RDPEMT 2.2.1.1.1）：上游驱动丢弃 `sub_headers`。补丁解析隧道 Data PDU 子头里的请求，按整个隧道 PDU 计数，应答放进空载荷 Data PDU 的子头回发；写泵通道改传 `TunnelData`。隧道内 RTT 请求一并应答。子头的 SubHeaderLength / SubHeaderType 就是探测结构自己的 headerLength / headerTypeId，SubHeaderData 只剩 sequenceNumber 之后的字段，所以按完整子头解析和回包（`586e6fcb`；初版把 SubHeaderData 当完整结构，真机上隧道请求全部解码失败）。
    - 没有 Start 就来的 Stop 不回：报 0 字节会被当成断链。Start 所在 PDU 不计入，Stop 所在 PDU 计入。
    - Windows 两条路径都用，隧道为主（见"验证"）。
    - 收益分情况：服务端开着连接时探测（默认）时初值已是实测值，会话期测量只会随链路下修，弱网下可能少卡也可能降画质；关了连接时探测的服务端（估值卡在 10 Mbps）收益最确定。真机对比看 `RemoteFX Network\Current TCP/UDP Bandwidth` 是否随时间变化。
12. **fork 补丁：QoE 帧确认**（`ironrdp-egfx` client）。EndFrame 后在标准 FrameAcknowledge 之外追加 RDPGFX_QOE_FRAME_ACKNOWLEDGE_PDU（MS-RDPEGFX 2.2.2.21）：timestamp 为处理 StartFrame 的单调毫秒（首值定原点，u32 回绕），timeDiffSE 为 StartFrame 到 EndFrame，timeDiffEDR 为 EndFrame 到 `on_frame_complete` 返回（NexShell 的 publish 在其中，真正上屏时刻拿不到），超过 65535 ms 填 0。只在确认的能力集是 V10 及以上时发送，V8 / V8.1 不发。规范明说它只作信息与调试用途，不计入弱网优化的预期收益。
13. **自动重连**（fork `f7da0a6e`，`src/rdp_session/reconnect.rs`；细节见 `docs/plans/2026-10-10-suppress-output-mouse-cursor.md` §3）。参照 Windows App。
    - fork 部分：
      - General extraFlags 声明 `AUTORECONNECT_SUPPORTED`，早期能力标志声明心跳；
      - 心跳透出为 `ActiveStageOutput::Heartbeat`；
      - `ClientConnector::with_network_characteristics_sync`：重连时用 NETCHAR_SYNC 回应第一条连接期探测请求（MS-RDPBCGR 2.2.14.2.3）。
    - 只对网络类断开重连：
      - 服务端发来的 Terminate 一律不重连；
      - 读写 I/O 错误里 InvalidData 算致命，其余算网络类；
      - 认证、协商、证书错误算致命。
      - 分类在 NexShell 侧完成，fork 不用透出 ErrorInfo 码。
    - 节奏：第一次立即，之后每 3 s 一次，最多 20 次；连接阶段总时限 60 s。连续 3 次走到 NLA 却没进入活动期就停，防止锁账户。
    - 掉线检测：
      - 心跳 period × (count1 + count2) 秒内 TCP 上没有任何数据；Mac 唤醒后缩短为一个周期加 2 s。
      - UDP 隧道先报错时等 TCP 1 s，看服务端会不会发来 Terminate，免得把被挤下线当成断网重连回去。
    - cookie 只存内存、取最新一个，被拒后退回凭据登录。UI 保留旧画面，叠加"正在重连 n/20"。
    - 修掉服务端 Terminate 后真实原因被 "session ended" 覆盖的问题。
14. **SuppressOutput**（`src/rdp_session/suppress.rs`）。
    - 服务端声明 `suppressOutputSupport` 时：画面不可见发 allowDisplayUpdates = 0；恢复时发 1，并带含端点的桌面矩形，这条 PDU 本身会触发整屏刷新。
    - 不可见的情况：不是活动 tab、不在终端页，或应用不可见。应用可见性取自 `NSApplicationDidChangeOcclusionStateNotification`，全部窗口最小化、应用隐藏、被完全遮挡、锁屏都算不可见。
    - 重激活、重连后按当前可见性重发。
    - 后台 RDP tab 因此不再收帧，也不再整帧上传。
15. **fork 补丁：AVC420 只转换更新区域**（`ironrdp-egfx` decode，`287f337d`）。
    - `H264Decoder` 新增 `decode_regions`，默认实现是整帧解码再裁剪，行为不变。
    - NexShell 的 VideoToolbox 解码器覆盖它：借用 NV12 平面，只把 regionRects 转成 RGBA。
    - 收益有限：HiDPI 整帧 vImage 转换约 0.4 ms（ADR 0015），全屏视频时没有收益。
16. **光标改走 MouseCursor DVC**（MS-RDPEMSC，`src/rdp_session/mouse_cursor.rs`）。
    - 服务端创建通道后发 CapsAdvertise（v1，字节同 Windows App）；确认后服务端不再发 FastPath 指针，光标形状随隧道走 UDP，避开 TCP 队头阻塞。
    - 载荷沿用 `ironrdp-pdu` 的 `PointerAttribute` / `LargePointerAttribute` 解码（布局与 MS-RDPBCGR 一致），指针缓存按通道自建；位置更新照旧忽略。
    - 与 Windows App 不同：CapsConfirm 不合法时不关通道，只记日志。ironrdp-dvc 的处理器没法主动关通道，且服务端确认后已改走这条通道发光标。
    - 错误不上抛（DVC 层会当致命错误）：坏包只记日志，解码失败退回系统箭头。
    - 真机（Win11）：服务端创建该通道并确认能力，Soft-Sync 把它迁进隧道；此后光标变化全部经 DVC 下发，FastPath 指针停发。

## 影响与风险

- 画面下行的调速与重传由 Windows 服务端的 UDP 发送端负责，客户端的 NewReno 只管上行小流量。微软文档称 RDP Shortpath 的 UDP 传输基于 URCP 速率控制，普通 RDS 服务端是否同一实现未证实。
- 丢包若随发送速率上升（限速 / QoS），换 UDP 也救不了；应先在隧道里按不同速率测丢包。
- UDP 3389 被挡时，连接多等最多约 3s 后回落 TCP。
- Soft-Sync 之后隧道断开会让会话断开（与上游一致），由决策 13 自动重连。
- `rdp_session/mod.rs` 两次超过 1500 行：先把单测移到 `rdp_session/tests.rs`；决策 13–16 又把帧输出处理拆到 `outputs.rs`，重连、SuppressOutput、MouseCursor 各自独立成文件，主文件约 1440 行。

## 真机排查记录（Windows 11，UDP 3390，经 MTU 1160 的 UDP 隧道）

| 现象 | 证据 | 原因与处理 |
|---|---|---|
| 握手超时 | pktmon 未见 SYN | 1260 字节 IP 包超过隧道 MTU → MTU 改 1132 |
| 握手超时 | SYN 到达协议栈、无丢弃；事件 135 "No UDP Packets Received" | 版本 3 固定 MTU 1232 → 只提议版本 2 |
| 隧道建成但画面不出 | 事件 132 显示 Graphics 在隧道 1；客户端日志 Soft-Sync 清单含未打开的 ID；UDP 收到约 5 KB 后停止 | 客户端未切换 → 决策 8 的 fork 补丁 |
| Autodetect 下帧率 < 6 | 事件 101（探测被服务器配置关闭）、166（自适应图形按最小带宽优化）；服务端回 RTT 400 ms / 512 kbps | 连接类型改回 LAN，30 fps 以上 |
| 图形复位后断开 | 隧道包 `1c 10 "AUDIO_PLAYBACK_DVC"`：清单外 ID 16 的 Create | 路由改为跟随 Create 到达的传输 |
| 报 LAN 时服务端估带宽恒为 10 Mbps，Windows App 为 95–103 Mbps | `RemoteFX Network` 计数器；RDP-Tcp `SelectNetworkDetect = 1`（关连接时探测） | LAN 取类型下限，IronRDP 不应答会话期带宽测量，估值不更新 → 服务端改 0 后报 LAN 与 Autodetect 均测到 102400 kbps，Autodetect 下 62 fps、帧质量 100% → 决策 1 |
| 运行 1–6 分钟后 "reliable UDP tunnel closed" | 抓包：客户端先停止发包，服务端指数退避重传约 21s 后放弃（事件 226 UdpEventErrorOnSend）；客户端报 `TLS error, caused by: RDPEUDP2 transport error`，即驱动出错退出，只可能来自 socket 收发 | 补丁后日志：`send` 返回 ENOBUFS（os error 55），7 次集中在同一毫秒，都是 12 字节的纯 ACK；按丢包处理后连续运行 10 分钟以上不断开 → 决策 9 |
| 网络不稳（隧道丢包 5%、RTT 50–230 ms）时运行十几秒到几分钟后断开，报 `read frame failed: Connection reset by peer (os error 54)` 或 `reliable UDP tunnel closed` | 客户端 utun 抓包：客户端为某包单独发的 ACK 丢失，之后向量从更高的洞报起；服务端对该包重传 3–6 次（间隔翻倍，约 19s），随后 TCP RST。RST 序号若超前于客户端已收（中间 TCP 段丢了），macOS 按 RFC 5961 忽略它，界面卡住，直到 65s UDP 空闲超时才报 tunnel closed | 发 ACK 即释放接收窗口，Windows 不按累计语义确认 → 决策 10。补丁后同一网络连续运行 10 分钟：服务端重传 308 个包，每个至多 4 次即被确认，无 RST |

客户端抓包：用户在 `access_bpf` 组时可免 sudo，`tcpdump -i <utun> -s 320 -w rdp.pcap host <服务端>`。RDPEUDP 头部（`snSourceAck`、ACK 向量、AoA、`snSourceStart`）是明文，能直接看出服务端在重传哪个包、客户端的 ACK 有没有覆盖它。

Windows 侧诊断：`Microsoft-Windows-RemoteDesktopServices-RdpCoreTS/Operational` 事件 130/131/132/135，客户端设 `RUST_LOG=ironrdp_dvc=debug` 看 Soft-Sync 清单与建通道顺序，`ironrdp_rdpeudp_tokio=debug` 看驱动退出原因与被丢弃的收发错误。

服务端性能计数器用 `typeperf` 采样：`RemoteFX Graphics(*)` 看出帧、帧质量、编码耗时，以及按客户端 / 网络 / 服务端资源不足分类的跳帧；`RemoteFX Network(*)` 看带宽估值、RTT、TCP 与 UDP 各自的发送速率。`typeperf` 只跟踪采样开始时已有的会话实例，客户端重连后要重新采。

## 验证

- 单测：多传输标志、两个环境变量开关、请求判定（同协议只试一次、无 Soft-Sync 拒绝、有损拒绝）、连接器配置（标志随 UDP 开关且要求 EGFX）。`cargo test --lib` 与 `--bin` 全部通过。fork：`ironrdp-dvc` 7、`ironrdp-session` 46、`ironrdp-rdpeudp`（`--features std`）201、`ironrdp-rdpeudp-tokio` 60、`ironrdp-egfx` 66、`ironrdp-testsuite-core` 1686 项通过；决策 11 的单测覆盖 Start 前后计数边界、无 Start 的 Stop 不回、有损变体、RTT 应答、坏子头跳过、按 Windows 实发字节的子头编解码；决策 12 覆盖 V8 不发 / V10.7 追加、无 StartFrame 不发、65535 ms 截断；决策 13 覆盖错误分类、心跳超时与唤醒缩短、NETCHAR_SYNC 取值，fork 侧覆盖 NETCHAR_SYNC 只回应第一条请求（RTT 与 BW Start 各一）和心跳透出；决策 14 覆盖矩形含端点；决策 15 覆盖区域转换与整帧转换后裁剪逐字节一致（含奇数原点、越界裁剪）及 `decode_regions` 默认实现；决策 10 的单测覆盖已收包留在向量中直到 AoA（按抓包场景构造，旧逻辑下失败）、未释放历史不缩小窗口且有上限、多洞时向量够到最低的洞、最坏向量加满载荷不超 MTU、重复包回 ACK、AoA 不越过待重传包。
- 真机已验证（Win11，经 MTU 1160 的 UDP 隧道）：隧道建立，面板显示 `TCP + UDP`，图形数据基本走 UDP；LAN 连接类型下 30 fps 以上；打决策 9 补丁后连续运行 10 分钟以上不断开；打决策 10 补丁后在丢包 5% 的隧道里连续运行 10 分钟不断开；音频（rdpsnd 静态通道，服务端先在隧道和 TCP 上试 `AUDIO_PLAYBACK_DVC`，被拒后回落）正常；服务端开连接时探测后，报 LAN 与 Autodetect 的带宽估值都是实测值（决策 1）。
- 真机已验证决策 11（2026-10-10，Win11，base RTT 约 65 ms、ping 丢包 7.8%）：
  - Windows 两条路径都用，隧道为主。修复子头解析后的一次约 12 分钟会话里，应答隧道带宽测量 4134 次、TCP 主连接 7705 次（TCP 上的探测主要出现在播放视频期间）。
  - 服务端隧道带宽估值从连接时的 363 kbps 随测量更新，每分钟中位数 0.8–3.9 Mbps，峰值 5.6 Mbps。修复前隧道请求全部被丢弃，估值不会更新。
  - 服务端测得 TCP 上的 RTT 为 200–900 ms（最高 3.9 s），隧道约 68 ms。丢包下 TCP 队头阻塞，客户端日志里 TCP 请求成簇到达（簇间隔 p50 76 ms、p90 216 ms），与快速重传和超时重传的时间对得上。
  - 约一半样本的 Start / Stop 在同一毫秒内处理，timeDelta 取下限 1 ms，单样本偏高。服务端平滑后的估值未见异常，暂不处理。
- 真机已验证决策 13 / 14 / 16 的前提（2026-10-10，Win11）：
  - 服务端 `suppressOutputSupport` 为 true，切 tab 时 suppress / allow 正常收发；
  - 声明 `AUTORECONNECT_SUPPORTED` 后，服务端开始下发 cookie；
  - 心跳参数为 1 s / 8 / 12，即 20 s 判定掉线；
  - MouseCursor 通道被创建（channel 8），并列入 Soft-Sync 清单；决策 16 实现后，一次会话 87 次光标变化全部经 DVC 下发，FastPath 指针为 0。
- 以下真机验证决定不做（2026-10-10）：
  - 决策 13：断网 20 s 以上后重连并接回原会话；睡眠唤醒；在别处登录挤下线时不重连。
  - 决策 14：最小化、隐藏应用后画面能恢复。
  - 缩放、剪贴板、共享盘在 UDP 下逐项回归。
  - UDP 被挡：连接回落 TCP，额外等待不超过约 3s。
  - 隧道内 5% 丢包：与 `NEXSHELL_RDP_DISABLE_UDP=1` 对比流畅度与接收码率。
  - 连接异常时分别设两个开关复现，区分是 UDP 还是自动探测引起。
  - 决策 11：用 typeperf 在服务端对照 `RemoteFX Network` 的带宽估值，比较弱网隧道里的帧率与画质有无变化。日志用 `RUST_LOG=ironrdp_session::x224=debug,ironrdp_rdpeudp_tokio=debug` 加 `NEXSHELL_RDP_NET_TRACE=1`。
  - 决策 12：Wireshark 解密或服务端事件确认 QoE PDU 被接受，EGFX 通道不因它断开。

## 回退

运行期设 `NEXSHELL_RDP_DISABLE_UDP=1`；或 revert 合入提交回到纯 TCP。光标设 `NEXSHELL_RDP_MOUSE_CURSOR_DVC=0` 退回 FastPath 指针。
