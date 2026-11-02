# 0022 macOS 主窗口 X 改为隐藏 App，只有退出才结束程序

Status: accepted（2026-11-01，Matt 拍板）

## 决策

- macOS 上主窗口的 X 和菜单 Close Window 只隐藏整个 App（等同 Cmd+H），SSH、RDP、终端进程继续运行，不弹确认。
- 只有菜单退出、Cmd+Q、Dock 退出才结束程序。前台进程、未保存内容、录制中的确认从关窗时移到退出时。
- 点 Dock 或 Cmd+Tab 恢复原窗口，不新建窗口。
- 辅助窗口（主机编辑等）的 X 仍是真正关闭。Windows、Linux 不变：没有 Dock，隐藏后无处找回。

## 代码里看不出的约束

- 用 `[NSApp hide:]` 而不是逐窗口 `orderOut:`：App 隐藏后 AppKit 会自己处理恢复、辅助窗口和窗口顺序。`orderOut:` 要自己维护这些，还要改 Dock 重开回调，否则辅助窗口可见时点 Dock 找不回主窗口。
- `hide:` 遇到全屏窗口不生效，要先退全屏，转场结束后再隐藏。进入全屏动画中 `toggleFullScreen:` 会被 AppKit 忽略，在 `DidEnter` 回调里立刻 toggle 同样无效（均实测），所以 `DidEnter` 后延迟 0.1 秒再 toggle，`DidExit` 后再隐藏。
- warp `applicationShouldTerminate:` 靠 `applicationDidHide:` 接着退出，已隐藏时收不到这个通知，会等 5 秒兜底才退出。fork 补丁：已隐藏时直接返回 `NSTerminateNow`。追平上游时要保留。
