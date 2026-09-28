# RDP 弱网优化：可靠 UDP 旁路（连接类型自动探测已否决）

Status: proposed (2026-09-28)；Win11 真机验证 UDP 迁移与弱网表现后转 accepted

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

1. **连接类型保持 LAN，Autodetect 仅供试验**（`NEXSHELL_RDP_AUTODETECT=1`）。原计划跟 mstsc 一样默认 Autodetect，真机否决：服务端若关了网络探测（RDP-Tcp `SelectNetworkDetect = 1`，事件 101 "Reason Code 2"），会回给客户端默认特征（RTT 400 ms、512 kbps），RemoteFX 自适应图形随即"按最小网络带宽优化"（事件 166），发布帧率掉到 6 以下；改回 LAN 后同链路 30 fps 以上。另外 IronRDP 会话期只应答 RTT，不做带宽测量，服务端拿不到持续的带宽数据。
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
7. **依赖**：新增 `ironrdp-rdpeudp` / `ironrdp-rdpeudp-tokio` / `ironrdp-rdpemt`，与其余 ironrdp crate 同走 fork 路径 patch。fork 在 ADR 0013 基线 `65cb3a93` 上追加决策 8（`7240b9a6`）、决策 9（`03e51ba6`）两个补丁；当前 rev 见 ADR 0013 基线。
8. **fork 补丁：Soft-Sync 按服务端清单路由**（`ironrdp-dvc` client）。Windows 的 Soft-Sync 请求会列出客户端拒绝过或尚未打开的通道 ID（实测 `[2, 6, 7, 8, 9, 10, 11, 12]`，客户端只开了 7 号 Graphics）。上游见到未知 ID 就跳过整条隧道，而服务端发出请求时已经改走隧道（MS-RDPEDYC 3.2.5.3.1），结果画面停在"正在连接"。补丁：
   - 列出的 ID 全部按服务端路由记下，应答里带上该隧道。
   - 隧道上除 Data 外也接受 Create / Close，应答原路回隧道。实测 Windows 在 Soft-Sync 之后经隧道新建 Video::Control / Video::Data / Geometry（清单内 ID），以及清单外的 `AUDIO_PLAYBACK_DVC`（ID 16）。
   - 通道走哪条传输，跟随它的 Create 从哪条传输到达：隧道上的 Create 把该 ID 路由到隧道，TCP 上的 Create 把该 ID 改回 TCP（服务端会复用 ID）。
9. **fork 补丁：UDP socket 收发错误不再致命**（`ironrdp-rdpeudp-tokio` driver）。上游驱动遇到任何 socket 错误都会退出，隧道随之关闭，会话断开。补丁：
   - 发送失败按丢包处理：数据靠 RTO 重传，ACK 由下一个替代。macOS 在接口队列（如 VPN utun）满时会返回 ENOBUFS。
   - 接收时遇到 ICMP 回报的错误（拒绝、重置、主机或网络不可达）跳过。链路真断了仍由 65s 空闲超时关闭。
   - 驱动出错退出时打 warn 日志；`shutdown` 优先返回驱动自己的错误，断开提示里能看到根因，而不只是 TLS 层的"transport error"。

## 影响与风险

- 画面下行的调速与重传由 Windows 服务端的 UDP 发送端负责，客户端的 NewReno 只管上行小流量。微软文档称 RDP Shortpath 的 UDP 传输基于 URCP 速率控制，普通 RDS 服务端是否同一实现未证实。
- 丢包若随发送速率上升（限速 / QoS），换 UDP 也救不了；应先在隧道里按不同速率测丢包。
- UDP 3389 被挡时，连接多等最多约 3s 后回落 TCP。
- Soft-Sync 之后隧道断开会让会话断开（与上游一致），靠用户重连。
- `rdp_session/mod.rs` 已超 1500 行，本次把单测移到 `rdp_session/tests.rs`，主文件回到约 1390 行。

## 真机排查记录（Windows 11，UDP 3390，经 MTU 1160 的 UDP 隧道）

| 现象 | 证据 | 原因与处理 |
|---|---|---|
| 握手超时 | pktmon 未见 SYN | 1260 字节 IP 包超过隧道 MTU → MTU 改 1132 |
| 握手超时 | SYN 到达协议栈、无丢弃；事件 135 "No UDP Packets Received" | 版本 3 固定 MTU 1232 → 只提议版本 2 |
| 隧道建成但画面不出 | 事件 132 显示 Graphics 在隧道 1；客户端日志 Soft-Sync 清单含未打开的 ID；UDP 收到约 5 KB 后停止 | 客户端未切换 → 决策 8 的 fork 补丁 |
| Autodetect 下帧率 < 6 | 事件 101（探测被服务器配置关闭）、166（自适应图形按最小带宽优化）；服务端回 RTT 400 ms / 512 kbps | 连接类型改回 LAN，30 fps 以上 |
| 图形复位后断开 | 隧道包 `1c 10 "AUDIO_PLAYBACK_DVC"`：清单外 ID 16 的 Create | 路由改为跟随 Create 到达的传输 |
| 运行 1–6 分钟后 "reliable UDP tunnel closed" | 抓包：客户端先停止发包，服务端指数退避重传约 21s 后放弃（事件 226 UdpEventErrorOnSend）；客户端报 `TLS error, caused by: RDPEUDP2 transport error`，即驱动出错退出，只可能来自 socket 收发 | 补丁后日志：`send` 返回 ENOBUFS（os error 55），7 次集中在同一毫秒，都是 12 字节的纯 ACK；按丢包处理后连续运行 10 分钟以上不断开 → 决策 9 |

Windows 侧诊断：`Microsoft-Windows-RemoteDesktopServices-RdpCoreTS/Operational` 事件 130/131/132/135，客户端设 `RUST_LOG=ironrdp_dvc=debug` 看 Soft-Sync 清单与建通道顺序，`ironrdp_rdpeudp_tokio=debug` 看驱动退出原因与被丢弃的收发错误。

## 验证

- 单测：多传输标志、两个环境变量开关、请求判定（同协议只试一次、无 Soft-Sync 拒绝、有损拒绝）、连接器配置（标志随 UDP 开关且要求 EGFX）。`cargo test --lib` 与 `--bin` 全部通过。fork：`ironrdp-dvc` 7、`ironrdp-session` 44、`ironrdp-rdpeudp-tokio` 55 项通过。
- 真机已验证（Win11，经 MTU 1160 的 UDP 隧道）：隧道建立，面板显示 `TCP + UDP`，图形数据基本走 UDP；LAN 连接类型下 30 fps 以上；打决策 9 补丁后连续运行 10 分钟以上不断开；音频（rdpsnd 静态通道，服务端先在隧道和 TCP 上试 `AUDIO_PLAYBACK_DVC`，被拒后回落）正常。
- 真机待做：
  - 缩放、剪贴板、共享盘在 UDP 下逐项回归。
  - UDP 被挡：连接回落 TCP，额外等待不超过约 3s。
  - 隧道内 5% 丢包：与 `NEXSHELL_RDP_DISABLE_UDP=1` 对比流畅度与接收码率。
  - 连接异常时分别设两个开关复现，区分是 UDP 还是自动探测引起。

## 回退

运行期设 `NEXSHELL_RDP_DISABLE_UDP=1`；或 revert 合入提交回到纯 TCP。
