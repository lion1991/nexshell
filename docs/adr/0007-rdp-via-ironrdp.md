# RDP 远程桌面：用 IronRDP（纯 Rust），不走 FreeRDP FFI

Status: accepted (2026-07-02)

## 背景

NexShell 要支持 RDP 远程桌面（连 Windows 主机，作为主机库的一种连接方式）。业界两条路：

- **FreeRDP（C 库）+ FFI**：协议实现最满血（RemoteFX Progressive、H.264/EGFX、全套虚拟通道）。竞品 HaloShell 即静态链 FreeRDP 3.27 + 自写 shim，但它是 Tauri/webview 架构，没有原生渲染层，只能 IPC 推帧给 canvas——处境与我们不同。
- **IronRDP（Devolutions 维护的纯 Rust 实现）**：sans-IO 核心（`ironrdp-connector`/`ironrdp-session` 状态机，任意 runtime 可驱动），NLA/CredSSP 走自家 `sspi` crate（NTLM/Kerberos 都有，现代 Windows 默认强制 NLA 没问题），TLS 用 rustls。官方 viewer 即 winit+softbuffer 原生渲染，证明「解码→RGBA framebuffer→自绘」是官方支持路径。

NexShell 现状：纯 Rust 工具链（russh + rustls 0.23 已在树），零 C 依赖；warpui 有原生 RGBA 渲染（`CustomImageFormat::Rgba` + `Scene::draw_image`）；SSH 已确立「专用 OS 线程 + current-thread tokio + async_channel 推回 UI」模式。

## 决策

用 **IronRDP**。理由：

- 保住纯 cargo 构建：`cargo add` 即可，不引入 FreeRDP+OpenSSL 静态编译链，build-dmg.sh 零改动。
- 单一 TLS 栈：复用已在树的 rustls 0.23，不出现 rustls/OpenSSL 双栈。
- 无 unsafe FFI 边界：FreeRDP shim 崩溃会带走整个 app。
- sans-IO 正好嵌进现有 per-thread current-thread tokio 模式，与 SSH 对称。
- 符合「不造轮子」：IronRDP 是维护中的官方实现，非自研协议栈。

## 接受的代价

- **图形编解码短板**：IronRDP 目前完整支持 raw/RLE/RDP6.0/RemoteFX，**H.264/EGFX 图形管线未完成**（上游 Devolutions/IronRDP#1158 推进中）。日常远程操作足够；高帧率视频/游戏画面流畅度不如 FreeRDP。上游补齐后升级 crate 即得，无需改架构。

## Considered Options

- **FreeRDP FFI（HaloShell 路线）**：编解码满血，但拖进 C 工具链 + OpenSSL 双 TLS 栈 + unsafe shim 维护，破坏纯 Rust 构建。否。
- **自研 RDP 协议栈**：违反「不造轮子」，MS-RDPBCGR 体量不现实。否。
