# Telnet 连接：自写 NVT 编解码 + std TcpStream，复用串口式终端管线

Status: accepted (2026-10-02)

## 背景

交换机、路由器、老 Linux 设备常只开 Telnet（或 SSH 太老，见 ADR 0017）。主机库已有 SSH / RDP / Serial 三种协议，其中 Serial 是「字节流直灌终端 grid」：独立线程读写、`remote_process_output` 喂解析器、`remote_mark_disconnected` 置断开。Telnet 本质也是字节流，只多一层 IAC 协商。

## 决策

- **主机库第四种 `protocol`：`Telnet`**（库里存 `telnet`）。复用现有列：`host`/`port`（默认 23）/`username`/`password`/`keep_alive_*`/`tcp_connect_timeout`/`term_encoding`，不加表字段。旧版程序读到 `telnet` 会当成 SSH（读取时未知值回落 SSH），可接受。
- **协议编解码自写，不引 crate**（`src/telnet.rs`，纯函数，单测覆盖）。只协商 BINARY / ECHO / SGA / TTYPE / NAWS，其余 WONT/DONT：
  - 解析是跨 read 边界的状态机（IAC 命令、SB…SE 子协商可被拆包）。
  - 每个选项按「我方 / 对方」记状态，只在状态变化时回应；对自己发出请求的回应不再确认（RFC 1143 的简化版），避免与部分 telnetd 互相 DO/WILL 死循环。
  - 连上即主动 WILL NAWS/TTYPE/SGA、DO SGA/ECHO（同 PuTTY 主动模式）；TTYPE 回 `xterm-256color`（与 SSH PTY 一致）；NAWS 同意后立刻发、窗口变化再发。
  - 发送：0xFF 转义为 0xFF 0xFF；非 BINARY 下裸 CR 发成 CR NUL（RFC 854）。接收：非 BINARY 下 CR NUL 丢 NUL。
- **传输**：`std::net::TcpStream` + 独立线程（`terminal_runtime/telnet.rs` 子模块，主文件已超行数阈值）。读线程把收到的字节转进请求通道，主循环独占 codec 状态，无轮询延迟。关闭时 `shutdown(Both)` 让读线程退出。
- **编码**：与 SSH 一样经 `RemoteTerminalEncoding` 按主机的 `term_encoding` 转码（GBK 网络设备是主要场景；Serial 没做这层）。
- **自动登录**：用户名/密码可选。填了就在登录提示出现时各答一次（`login:` / `username:` / `user name:` / `用户名:` 与 `password:` / `密码:`，大小写不敏感、只匹配输出末尾）；答完密码、或 16 KiB 输出内没见到提示即停用，之后会话里再出现 `login:` 不会误发。登录失败的重试交给用户手动输入。
- **保活**：开启时按间隔发 IAC NOP（服务端按规范忽略），复用 SSH 的 keep-alive 开关与间隔；「最大失败次数」「认证超时」对 Telnet 无意义，表单不显示。
- **标签**：新增 `TerminalSessionKind::Telnet`，行为同 Remote/Serial 的终端标签：可录制、可分屏（新 pane 再开一条 Telnet）、断开显示提示与重连；没有 SFTP / exec 通道，文件面板提示「不支持文件浏览」（串口标签同样处理），无主机监控。同一主机可开多个标签，不做串口那种独占去重。

## 验证

- 单测：编解码（拆包、转义、CR 处理、协商不回环、NAWS）与自动登录；本地假服务端集成测试覆盖协商、自动登录、GBK 输出、输入、窗口变化与服务端断开。
- 实机：公网 telehack.com 收发正常；交换机 192.168.254.252（Telnet 23）连接、登录、回车与显示正常。

## 已知限制

- 明文传输，表单与文档不另做警告（用户选 Telnet 即知情）。
- 服务端不回显（从不发 WILL ECHO）时本地看不到输入；v1 不做本地行编辑/本地回显，网络设备与常见 telnetd 都会回显。
- 不支持 LINEMODE、ENVIRON、加密/认证选项（RFC 2941/2946）。

## Considered Options

- **`libtelnet-rs` / `nectar` 等 crate**：要么只做解析不管选项状态，要么绑 tokio codec 框架，接进现有同步线程模型反而更绕；协商子集很小，自写 + 单测更可控。否。
- **PTY 里跑系统 `telnet` 命令（DirectPty）**：macOS 自 10.13 起不自带 telnet，且拿不到编码转换与自动登录。否。
- **轮询式单线程（同串口，20ms 读超时）**：实现最简但每次按键最多多 20ms 延迟；读线程转发方案几乎一样简单。否。
