# RDP 不发 SuppressOutput：后台标签只停本地重绘，不停远端画面

Status: accepted (2026-10-11)，撤销 ADR 0014 决策 14

## 背景

v0.6.9 按 ADR 0014 决策 14 实现了 SuppressOutput。RDP 标签不在前台、切到主机库或设置页、应用被遮挡或最小化时，都通知服务端停发画面。

协议（MS-RDPBCGR 2.2.11.3.1）只规定 allowDisplayUpdates = 0 时服务端停发显示更新。但 Windows 服务端的实现是把会话切到无 GUI 模式，依赖屏幕的程序会截到黑屏或直接失败，例如：

- 截图、找图类自动化：UiPath、AutoIt、按键精灵等；
- UI 自动化测试；
- 录屏。

mstsc 只在最小化时发，也有这个问题。微软给的解法是客户端注册表 `HKCU\Software\Microsoft\Terminal Server Client\RemoteDesktop_SuppressWhenMinimized=2`，把它关掉。参考：SmartBear TestComplete「Running Tests in Minimized Remote Desktop Windows」、UiPath Robot「Recording and Remote Control Troubleshooting」、Microsoft Q&A「How to prevent minimized RDP sessions from freezing」。

NexShell 的触发面比 mstsc 广，切个标签就会让远端程序出问题。

## 决策

- **任何情况都不发 SuppressOutput**，远端始终照常出画面。
  - 删除 `rdp_session/suppress.rs`、可见性通道、`SessionMemo.visible`，以及重激活和重连之后的状态重发。
  - `ConnectionResult.suppress_output_support` 是 IronRDP 上游字段，不涉及 fork。
- **后台开销只在本地压**：画面不在屏上（非活动标签、非终端页、应用不可见）时，新帧和远端光标变化不触发重绘。
  - 首帧照样重绘，因为标签要从「连接中」翻转。
  - 连接信息面板的 1 s 采样照跑，屏上那个面板开着时才重绘。
  - 切回时渲染器按帧代号补传：落后超过 32 条脏区记录，或纹理已逐出（10 帧未画），就整帧上传，不会画出旧帧。
- **应用可见性观察者保留**（`NSApplicationDidChangeOcclusionStateNotification`），只用于上面的重绘门控。
- **离开 RDP 标签时抬起远端按键和左键**，顺带修两处远端卡键、卡鼠标：
  - `release_rdp_held_input`（原 `release_rdp_modifiers`）同时抬起按着的左键。除切标签和失焦外，切页（`set_app_page` 离开终端页）和新建标签（`push_terminal_tab`）也调用它。
  - 左键在画面上按下、拖出画面（黑边、标题栏、窗口外）才松手：原来抬起事件因坐标在画面外被丢掉，远端停在拖拽态。现在左键按着时坐标夹到画面边缘，拖动和抬起照发。

## 影响

- 后台 RDP 标签仍占网络带宽和硬件解码。远端桌面静止时流量很小；播放视频等场景下，后台照样收流。

## 替代方案

- **只在最小化时发**，与 mstsc、Windows App 一致：最小化后自动化照样失败，这正是 mstsc 用户要关掉它的原因。否决。
- **加「后台暂停画面」主机开关，默认关**：目前没有需求，先不做；真要省流量时再加，也只做成手动开关。
