## Purpose

定义终端在 Kitty 键盘协议激活时对 Cmd/Option 修饰键组合的编辑类按键的编码行为，使 Claude Code 等 TUI 能区分这些组合键。

## ADDED Requirements

### Requirement: Kitty 协议下编码 Cmd/Option 修饰的编辑键
当前台程序启用 Kitty 键盘协议（含 disambiguate 标志）时，系统 SHALL 按协议把带 Cmd/Option 修饰的编辑键（方向键、Home/End、Backspace、Delete、Enter、Tab 等）编码为携带修饰位的 CSI u / CSI ~ 序列；协议未激活时 SHALL 保持现有 legacy 序列不变。

#### Scenario: Option+左方向
- **WHEN** Kitty 协议激活且用户按 Option+←
- **THEN** 发送带 Alt 修饰位的 CSI 序列（`CSI 1;3D`），程序可识别为单词跳转

#### Scenario: 协议未激活
- **WHEN** 前台为普通 shell（未启用 Kitty 协议）且用户按 Option+←
- **THEN** 发送与升级前相同的序列，行为无变化
