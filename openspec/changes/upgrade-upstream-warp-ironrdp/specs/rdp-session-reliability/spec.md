## Purpose

定义 NexShell RDP 会话在依赖升级前后必须保持的用户可见行为，以及升级后新增的服务器兼容性与故障恢复行为，作为升级验收的行为契约。

## ADDED Requirements

### Requirement: 既有 RDP 功能升级后零回归
升级 IronRDP 后，系统 SHALL 在同一组真实服务器（Windows 10/11 mstsc 服务端、xrdp）上保持下列功能与升级前等价：EGFX 视频（AVC420 硬解、ClearCodec、Progressive/RemoteFX）、音频输出、双向文本剪贴板、驱动器重定向文件互拷、窗口尺寸变化时动态重设远端分辨率、指针形状与位置。

#### Scenario: 视频画面一致
- **WHEN** 用 `examples/egfx_replay.rs` 回放升级前录制的 EGFX 码流样本
- **THEN** 解码输出帧与升级前基线帧逐像素一致，或差异仅限上游修复明确改善的区域并留证

#### Scenario: 真实会话冒烟
- **WHEN** 连接 Windows 11 与 xrdp 各一台，执行"播放视频 30s、复制粘贴文本、拖拽拷贝文件、拉伸窗口、全屏切换"
- **THEN** 每一项与升级前行为一致，无花屏、无静音、无失步、无崩溃

### Requirement: 会话正常结束不报协议错误
当服务器以裸 MCS Disconnect Provider Ultimatum 结束会话（如 xrdp 注销）时，系统 SHALL 以"正常断开"结束会话并显示对应提示，MUST NOT 显示协议错误。

#### Scenario: xrdp 注销
- **WHEN** 在 xrdp 会话内执行注销
- **THEN** RDP 标签页显示"连接已断开"类提示，日志无 decode/protocol error

### Requirement: FreeRDP 系服务器可完成连接
对连接期发起 Bandwidth Measure 自动检测的服务器（GNOME Remote Desktop 等），系统 SHALL 应答并完成授权、进入桌面。

#### Scenario: 连接 GNOME Remote Desktop
- **WHEN** 对 GNOME Remote Desktop 发起连接
- **THEN** 30s 内进入桌面，不卡在授权前

### Requirement: 解码器失步可恢复
当 EGFX 会话中途出现解码失步（服务器重发 CapsAdvertise/ResetGraphics 序列）时，系统 SHALL 重建表面并恢复正常画面，MUST NOT 停留在花屏或黑屏。

#### Scenario: 高负载下花屏恢复
- **WHEN** Windows 11 会话在高负载下触发解码器恢复序列
- **THEN** 5s 内画面恢复正常且后续帧正确

### Requirement: 输入事件按协议限制批量发送
系统 SHALL 把待发的 Fast-Path 输入事件按 255 个/帧上限分批并保持原始顺序；滚轮旋转值 SHALL 夹紧到线协议 9 位范围。

#### Scenario: 快速连续输入
- **WHEN** 在 1 帧内产生超过 255 个输入事件（按键连发+鼠标移动）
- **THEN** 远端按原顺序接收全部事件，无丢失或乱序

#### Scenario: 大幅滚轮
- **WHEN** 触控板产生超出 ±255 单位的滚轮增量
- **THEN** 远端接收方向正确的夹紧值，不出现反向滚动

### Requirement: 驱动器重定向与服务器保持同步
对发送非空 QueryBuffer 的 QueryInformation 请求，系统 SHALL 完整消费请求体，后续 rdpdr 请求 MUST 继续正确解析。

#### Scenario: 远端资源管理器浏览重定向盘
- **WHEN** 在远端打开重定向的本地目录并逐级浏览、复制文件
- **THEN** 目录列表完整、复制成功，日志无 rdpdr decode 错误
