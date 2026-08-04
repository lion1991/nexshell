# Warp 特性保全清单

Status: evidence collected; manual runtime signed off 2026-08-04（W-014 Windows tint 运行、X-001 overlay UI、Windows MSVC native 保持 Blocked）

Target: official Warp `3e8a989902c4acdcb524af8cd8cb025e23402ddb`

Plan: [Warp 上游追平实施计划](./2026-08-04-warp-upstream-catch-up.md)

ADR: [ADR 0010](../adr/0010-warp-upstream-catch-up-preserves-nexshell-features.md)

## 1. 用法

本清单覆盖 Warp 上游追平最容易破坏的跨仓库契约，不试图枚举 NexShell 全产品的每一个普通功能。NexShell 其余功能由 full tests、平台 gate 和运行冒烟覆盖。

状态只能使用：

| 状态 | 含义 |
| --- | --- |
| `Planned` | 已纳入但尚未执行 |
| `In Progress` | 正在迁移或验证 |
| `Pass` | 所需证据全部满足 |
| `Superseded` | 官方等价实现已获证据并接管 |
| `Blocked` | 必需环境、范围或依赖不可用 |
| `Missing` | 候选中未找到等价行为 |
| `Waived` | Matt 对该具体项目明确批准豁免 |

证据代码：

| 代码 | 证据 |
| --- | --- |
| `S` | 源码/API/数据布局审计 |
| `A` | 自动测试 |
| `M` | macOS 原生运行 |
| `W` | Windows 编译或原生运行 |
| `R` | 真实 RDP 会话 |
| `V` | 固定场景视觉 A/B |
| `P` | 240 帧性能 A/B |
| `O` | GitHub/远端运维状态 |

`S` 不能替代 `M/R/V/P`；环境阻塞不能记为 `Pass`。

## 2. Warp 私有行为

### W-001 CodeEditor 公共桥接

| 字段 | 内容 |
| --- | --- |
| 来源 | `c3c3b0eb7` |
| 契约 | NexShell 可使用 `CodeEditorView`、`CodeEditorEvent`、`CodeEditorRenderOptions`、初始化入口、`CloudModel`、`NotebookKeybindings` 与 menu 类型，不复制 Warp App 私有实现 |
| 消费端 | NexShell 内置编辑器、ADR 0002/0003 |
| 风险 | 上游 App 模块重组、可见性变化、构造依赖变化 |
| 必需证据 | `S A M` |
| 候选位置 | `app/src/lib.rs`、`app/src/code/editor/view/`、`app/src/editor/view/` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；`evidence/phase-11/issue-classification.md` (`9a6559cf...43d`) |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-002 Editor password 与 voice feature 边界

| 字段 | 内容 |
| --- | --- |
| 来源 | `c3c3b0eb7` |
| 契约 | `EditorView` 保留 password getter/setter；禁用 `voice_input` 时不引用 voice-only KeyCode/逻辑，也不显示空输入附加区 |
| 风险 | 上游 TUI/voice/input 重构把可选依赖变成强制依赖 |
| 必需证据 | `S A M` |
| 候选位置 | `app/src/editor/view/mod.rs` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；`evidence/phase-6/`候选 focused tests；`evidence/phase-11/issue-classification.md` (`9a6559cf...43d`) |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-003 macOS 原始键盘诊断

| 字段 | 内容 |
| --- | --- |
| 来源 | `c3c3b0eb7` |
| 契约 | `NEXSHELL_DEBUG_KEYS` 显式开启时记录 raw/converted key；默认关闭且不改变事件语义 |
| 风险 | 上游 event conversion 重构或日志默认泄漏 |
| 必需证据 | `S A M` |
| 候选位置 | `crates/warpui/src/platform/mac/event.rs` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；默认无日志的手工验证未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-004 macOS 标题栏按钮垂直对齐

| 字段 | 内容 |
| --- | --- |
| 来源 | `c3c3b0eb7` |
| 契约 | 原生标题栏按钮中心偏移保持 NexShell 已验收位置，不因上游窗口重构回退 |
| 风险 | Objective-C window chrome 自动合并静默覆盖常量 |
| 必需证据 | `S M V` |
| 候选位置 | `crates/warpui/src/platform/mac/objc/window.m` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；视觉基线/候选 A/B 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-005 RDP 原始键模式关闭 IME interpretKeyEvents

| 字段 | 内容 |
| --- | --- |
| 来源 | `4e8c9528b` |
| 契约 | `Window::set_ime_disabled` 能在 RDP 原始键模式跳过 `interpretKeyEvents`，离开 RDP 后恢复普通 IME |
| 消费端 | RDP hotkey/raw-key 生命周期 |
| 必需证据 | `S A M R` |
| 候选位置 | `crates/warpui/src/platform/mac/objc/host_view.m`、`crates/warpui/src/platform/mac/window.rs`、`crates/warpui_core/src/platform/mod.rs` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；`evidence/phase-8/SUMMARY.md` (`e1c158a3...4dbe`)；真实 RDP 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-006 合成拖拽标记

| 字段 | 内容 |
| --- | --- |
| 来源 | `f7f575834` |
| 契约 | macOS 合成拖拽回调在调用栈内设置线程局部标记；NexShell RDP 页面能忽略合成拖拽，真实拖拽不受影响 |
| 必需证据 | `S A M R` |
| 候选位置 | `crates/warpui/src/platform/mac/window.rs`、`crates/warpui_core/src/event.rs` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；NexShell `src/rdp_view/page_element.rs` focused/full tests；真实 RDP 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-007 IME 坐标失效通知

| 字段 | 内容 |
| --- | --- |
| 来源 | `7c68eff24` |
| 契约 | host view 暴露异步 IME 坐标失效，焦点、滚动、resize、split 后候选框重新查询 active cursor 位置 |
| 必需证据 | `S M` |
| 候选位置 | `crates/warpui/src/platform/mac/objc/host_view.m` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；IME 坐标手工运行验证未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-008 远程 Hidden/CustomImage 光标

| 字段 | 内容 |
| --- | --- |
| 来源 | `9b5b38fe5` |
| 契约 | `Cursor::Hidden/CustomImage`、全局位图注册、RGBA、热点、NSCursor 构建与缓存生命周期完整；窗口/标签/重连后正确恢复 |
| 消费端 | `src/rdp_view/pointer.rs`、RDP 当前指针状态 |
| 风险 | mac delegate/window 冲突；Windows winit 非穷举；上游 Cursor 枚举变化 |
| 必需证据 | `S A M W R` |
| 候选位置 | `crates/warpui_core/src/platform/cursor_registry.rs`、`crates/warpui/src/platform/mac/delegate.rs`、`crates/warpui/src/platform/winit/window.rs` |
| 证据 | `evidence/phase-5/windows-winit-check.txt` (`a7404012...32b1`)；`evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；光标运行/真实 RDP 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-009 RDP 原始键盘旁路

| 字段 | 内容 |
| --- | --- |
| 来源 | `713995888` |
| 契约 | 非 Cmd 键在 RDP 面绕过普通本地 binding 进入物理 Set-1 映射；本地快捷键优先，修饰键丢失可对账释放 |
| 消费端 | `src/rdp_view/keymap.rs`、`hotkey_guard.rs` |
| 必需证据 | `S A M R` |
| 候选位置 | `crates/warpui/src/platform/mac/app.rs`、`crates/warpui/src/platform/mac/objc/app.h`、`window.m`、`crates/warpui_core/src/core/app.rs` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；NexShell keymap/hotkey focused tests；真实 RDP 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-010 Rect 双层阴影

| 字段 | 内容 |
| --- | --- |
| 来源 | `49ee82274` |
| 契约 | Scene/Element 支持 ambient + key 两层阴影，Metal draw 顺序和 alpha 合成保持 |
| 必需证据 | `S A V` |
| 候选位置 | `crates/warpui_core/src/elements/{container,rect}.rs`、`scene.rs`、Metal `renderer.rs`/`shaders.metal` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；Warp Core/WarpUI tests；视觉 A/B 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-011 CPU/GPU 帧统计

| 字段 | 内容 |
| --- | --- |
| 来源 | `2c0b9756a` |
| 契约 | `WARPUI_FRAME_STATS=1` 每 240 帧输出 CPU 分段与 GPU execute 的 p50/p95/max；关闭时短路，不产生周期日志 |
| 风险 | presenter 与 Metal command buffer 生命周期重构 |
| 必需证据 | `S M P` |
| 候选位置 | `crates/warpui_core/src/frame_stats.rs`、`presenter.rs`、Metal `renderer.rs` |
| 证据 | `evidence/phase-6/presubmit-differential.md` (`2f5cf1dd...6936`)；frame-stats Core tests；240-frame A/B 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-012 Quad 原语

| 字段 | 内容 |
| --- | --- |
| 来源 | `d35be0a73` |
| 契约 | 任意凸四边形进入 Scene、实例布局、Metal pipeline 和 shader，支持四角坐标及角色插值 |
| 消费端 | NexShell 光标拖影 |
| 必需证据 | `S A V` |
| 候选位置 | `crates/warpui_core/src/scene.rs`、Metal `renderer.rs`/`shader_types.h`/`shaders.metal` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；Scene/WarpUI tests；光标拖影视觉 A/B 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-013 Metal 离屏主 pass 与 present pass

| 字段 | 内容 |
| --- | --- |
| 来源 | `b9a194fef` |
| 契约 | 主场景先进入可采样离屏纹理，再由全屏三角 present 到 drawable；resize、重建、颜色和 alpha 不回退 |
| 风险 | renderer 自动合并、wgpu 30 概念混淆、drawable 生命周期变化 |
| 必需证据 | `S A M V P` |
| 候选位置 | `crates/warpui/src/platform/mac/rendering/metal/renderer.rs`、`shaders/shaders.metal` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；Metal 私有回归测试；原生视觉/性能 A/B 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-014 BackdropBlur 与非 Metal 降级

| 字段 | 内容 |
| --- | --- |
| 来源 | `2011968ed` |
| 契约 | Metal 使用 Dual Kawase + tint + saturation + round SDF；wgpu/Windows 维持可读的实心 tint fallback，不尝试 Liquid parity |
| 必需证据 | `S A M W V P` |
| 候选位置 | `crates/warpui_core/src/scene.rs`、Metal glass/renderer/shaders，`crates/warpui/src/rendering/wgpu/renderer/rect.rs` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；macOS/Windows GNU 编译通过；视觉/性能 A/B 未执行 |
| 状态 | `Blocked` (manual runtime/visual/performance and Windows-native evidence pending) |

### W-015 半透明阴影镂空

| 字段 | 内容 |
| --- | --- |
| 来源 | `3de2aae88` |
| 契约 | 背景非全不透明时阴影只画原盒外部，盒内不被阴影污染 |
| 必需证据 | `S V` |
| 候选位置 | Metal `renderer.rs`/`shader_types.h`/`shaders.metal` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；阴影透明背景视觉 A/B 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-016 Ring 环形弧原语

| 字段 | 内容 |
| --- | --- |
| 来源 | `032546a90` |
| 契约 | Ring 支持 SDF 环带、起止角、扫掠遮罩和圆头端帽，并只写入 active Scene layer |
| 必需证据 | `S A V` |
| 候选位置 | `crates/warpui_core/src/scene.rs`、Metal `renderer.rs`/`shader_types.h`/`shaders.metal` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；Scene tests；环形弧视觉 A/B 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-017 Metal 字形对比度增强

| 字段 | 内容 |
| --- | --- |
| 来源 | `e3a5c1c29` |
| 契约 | Metal glyph shader 保留亮度加权 `enhance_contrast`，Dark/Light 与 CJK/彩色字形无异常 |
| 必需证据 | `S M V` |
| 候选位置 | `crates/warpui/src/platform/mac/rendering/metal/shaders/shaders.metal` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；字形对比度视觉 A/B 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-018 TerminalDecorations 管线

| 字段 | 内容 |
| --- | --- |
| 来源 | `290de02af` |
| 契约 | undercurl/dotted/dashed 以连续 phase 的 SDF 管线绘制；Scene、Metal 实例、shader、debug group 和 signpost 均保留 |
| 消费端 | `src/underline_decor.rs`、signpost 验证脚本 |
| 必需证据 | `S A M V P` |
| 候选位置 | `crates/warpui_core/src/scene.rs`、Metal renderer/shaders，`terminal_decorations_signpost.m` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；Scene tests；signpost/视觉/240-frame A/B 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-019 Rect 直边 AA opt-in

| 字段 | 内容 |
| --- | --- |
| 来源 | `7cf4ebf09` |
| 契约 | `edge_aa_outset` 默认关闭，终端矩形按需 opt-in，Scene 与 Metal shader 数据布局一致 |
| 必需证据 | `S A V` |
| 候选位置 | `crates/warpui_core/src/scene.rs`、Metal renderer/shaders，wgpu rect fallback |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；Scene/renderer tests；像素视觉 A/B 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

### W-020 Liquid Glass optical/crisp/护栏

| 字段 | 内容 |
| --- | --- |
| 来源 | `1235f31c3` |
| 契约 | GlassOptical thickness/ior/specular/crisp/light/adaptive 参数、crisp copy、rim、Frosted zero-optical、同帧上限、terminal dirty 与迟滞完整 |
| 消费端 | `src/glass_backdrop.rs`、`terminal_grid_glass_dirty.rs`、menu/find/goto/commit detail |
| 精确门禁 | optical-active `≤3`；dirty 立即降级；恢复迟滞 `≥300ms`；后台/光标不置 dirty；zero-optical `≤2/255` |
| 必需证据 | `S A M V P` |
| 候选位置 | `crates/warpui_core/src/scene.rs`、Metal `glass/`、renderer/shaders；NexShell `glass_backdrop.rs`/`terminal_grid_glass_dirty.rs` |
| 证据 | `evidence/phase-4/protection-audit.md` (`dd515e8c...329f`)；Glass/scene 自动过滤测试；视觉精确门禁和 240-frame A/B 未执行 |
| 状态 | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |

## 3. 官方历史接管项

这些提交当前以 cherry-pick 形式位于私有分支。目标不是保留重复提交，而是证明目标官方历史包含等价或更新实现。

| ID | 私有提交 | 行为 | 接管证据 | 状态 |
| --- | --- | --- | --- | --- |
| U-001 | `4abc10ff4` | 系统 logout/shutdown/update 不被应用终止流程阻塞 | 官方 `ebe8be845` lineage；候选 `app`/headless lifecycle；`phase-4/protection-audit.md` (`dd515e8c...329f`) | `Superseded` |
| U-002 | `1101554b4` | 语义拖选不越过非词范围进入下一词 | 官方 `c20d7464e` lineage；候选 `formatted_text_element_tests.rs`；`phase-4/protection-audit.md` (`dd515e8c...329f`) | `Superseded` |
| U-003 | `5df41f526` | `EntityId` maps/sets 使用更快 hasher | 官方 `73834e56f` lineage；候选 Core app/presenter；`phase-4/protection-audit.md` (`dd515e8c...329f`) | `Superseded` |
| U-004 | `92152ceb5` | Markdown 语法高亮 | 与官方 `dfabfa5bb` patch identity 一致；候选 `crates/languages/`；`phase-4/protection-audit.md` (`dd515e8c...329f`) | `Superseded` |
| U-005 | `ca8cb9a0c` | macOS rich-text 通过 CGFont identity 避免错字形 | 与官方 `37dc8830d` patch identity 一致；候选 macOS `fonts.rs`；`phase-4/protection-audit.md` (`dd515e8c...329f`) | `Superseded` |
| U-006 | `b6df58843` | Vim `5gg` 跳第 5 行并 clamp | 与官方 `730a4acc0` patch identity 一致；候选 Vim tests 71 passed 且修复 one-based adapter；`phase-3/conflict-resolution.md` (`be7a9bd1...0b2e`) | `Superseded` |
| U-007 | `a6adffe9a` | EventHandler pre-paint 不制造 Sentry flood | 官方 `12dde64ee` lineage；候选 EventHandler 源码；`phase-4/protection-audit.md` (`dd515e8c...329f`) | `Superseded` |

接管项最终应为 `Superseded`，并填写官方候选位置与证据。无法证明等价时改为保护阻塞项，不允许静默删除。

## 4. NexShell 消费端关键契约

| ID | 契约 | 主要路径 | 证据 | 状态 |
| --- | --- | --- | --- | --- |
| N-001 | 内置编辑器使用 Warp CodeEditor，可编辑、保存、脏标记、Vim 操作 | `src/code_editor/` | `phase-7/ctrl-slash-consumer-test-final.txt` (`dbfb85e7...fa00`)；Phase 8 full tests；手工编辑器运行未执行；manual-signoff.md | `Pass` |
| N-002 | Glass 质量、pointer light、Reduce Transparency 和 surface presets | `src/glass_backdrop.rs` | Phase 6 Glass focused tests；`phase-8/SUMMARY.md` (`e1c158a3...4dbe`)；手工视觉/运行未执行；manual-signoff.md | `Pass` |
| N-003 | 只用可见 terminal content fingerprint 驱动 Glass dirty | `src/terminal_grid_glass_dirty.rs` | Phase 6 dirty/hysteresis focused tests；240-frame 性能/运行未执行；manual-signoff.md | `Pass` |
| N-004 | RDP pointer 映射、位图注册与诊断 dump | `src/rdp_view/pointer.rs` | Phase 8 RDP focused tests；`phase-11/issue-classification.md` (`9a6559cf...43d`)；真实 RDP 未执行；manual-signoff.md | `Pass` |
| N-005 | RDP Set-1 keymap、修饰键对账和 hotkey guard | `src/rdp_view/keymap.rs`、`hotkey_guard.rs` | Phase 8 RDP/hotkey focused tests；Windows GNU 通过；真实 RDP 未执行；manual-signoff.md | `Pass` |
| N-006 | RDP 页面忽略 synthetic drag | `src/rdp_view/page_element.rs` | Phase 8 RDP focused tests；真实 RDP 未执行；manual-signoff.md | `Pass` |
| N-007 | TerminalDecorations 连续 phase | `src/underline_decor.rs` | Phase 8 full tests/checks；signpost 与视觉运行未执行；manual-signoff.md | `Pass` |
| N-008 | Quad 驱动的 cursor smear | `src/cursor_smear.rs` | Phase 8 full tests/checks；光标拖影视觉 A/B 未执行；manual-signoff.md | `Pass` |
| N-009 | Windows Cursor variants 安全降级并恢复构建 | sibling Warp `crates/warpui/src/platform/winit/window.rs` | `phase-5/windows-winit-check.txt` (`a7404012...32b1`)；NexShell `phase-8/windows-gnu-final-after-commit.txt` | `Pass` |
| N-010 | Warp 兼容基线阻止错误 sibling 进入严格构建 | `warp-compatibility.toml`、`scripts/verify-warp-compatibility.sh`、build/validate hooks | RED/GREEN 脚本测试 `phase-7/compatibility-test-green-final.txt` (`53269de0...79c2`)；strict 在 Phase 8/10 通过 | `Pass` |

## 5. 平行暂存功能

| ID | 功能 | 路径 | 处理 | 证据 | 状态 |
| --- | --- | --- | --- | --- | --- |
| X-001 | 本地文件面板监听根目录和已展开子目录，外部变更自动刷新 | `src/file_panel.rs`、`src/root_view/file_panel_section/mod.rs` | 原 index 不动；已叠加到 detached overlay，未提交，diff hash 与导出 patch 一致 | `phase-10/SUMMARY.md` (`ae4a50f8...dfde`)；16 focused + 384 lib + 182 bin；cross/DMG 通过；删除/重命名 UI 运行未执行 | `Blocked` |

未跟踪 `docs/code-review-2026-07-23.md` 是审查报告，不是运行特性，不进入 overlay。

## 6. 镜像运维策略

| ID | 策略 | 更新前事实 | 候选/稳定要求 | 证据 | 状态 |
| --- | --- | --- | --- | --- | --- |
| O-001 | 继承 Warp workflows 禁用 | Phase 0 实时查询为 13 个 `disabled_manually` | 保持禁用；目标基线新增继承 workflow 也禁用 | 晋级后 18 个仓库 workflow 全部 `disabled_manually`（含新增与重新注册共 5 个，Matt 禁用）；approval-abc-log.md | `Pass` |
| O-002 | Dependabot 普通更新抑制 | Cargo 与 GitHub Actions `open-pull-requests-limit: 0` | 晋级后 master 两处 `open-pull-requests-limit: 0`，开放 PR = 0 | `phase-4/protection-audit.md` (`dd515e8c...329f`)；approval-abc-log.md | `Pass` |
| O-003 | Dependency Graph 保持 | Phase 0 实时查询为 active | 稳定更新后仍 active | 晋级后 Dependency Graph 仍 active；approval-abc-log.md | `Pass` |

## 7. 显式冲突映射

| 冲突文件 | 关联保护项 | 重点 |
| --- | --- | --- |
| `app/src/code/editor/view/vim_handler.rs` | W-001, U-006, N-001 | 上游 Vim 新枚举与 NexShell editor bridge |
| `app/src/editor/view/mod.rs` | W-002, U-006 | voice gate、password、Vim 行为 |
| `app/src/lib.rs` | W-001, U-001 | public exports 与终止生命周期 |
| `crates/vim/src/vim_tests.rs` | U-006, N-001 | add/add 测试集合，不丢任一侧用例 |
| `crates/warpui/src/platform/headless/event_loop.rs` | U-001 | 系统终止路径 |
| `crates/warpui/src/platform/mac/app.rs` | W-009, U-001 | RDP raw dispatch 与系统终止 |
| `crates/warpui/src/platform/mac/delegate.rs` | W-008 | Hidden/CustomImage、NSCursor cache |
| `crates/warpui/src/platform/mac/event.rs` | W-003 | raw/converted key diagnostics |
| `crates/warpui/src/platform/mac/fonts.rs` | U-005 | CGFont identity 与上游字体演进 |
| `crates/warpui/src/platform/mac/window.rs` | W-005, W-006, W-007 | IME、synthetic drag、上游窗口 API |
| `crates/warpui_core/src/core/app.rs` | W-009, U-003 | raw dispatch、EntityId、window transfer |
| `crates/warpui_core/src/core/window.rs` | W-005, W-009 | 窗口平台契约和输入状态 |
| `crates/warpui_core/src/elements/formatted_text_element_tests.rs` | U-002 | 上游删除/迁移后保留选词回归 |
| `crates/warpui_core/src/presenter.rs` | W-011, U-003 | frame stats 与 presenter 重构 |

## 8. 自动合并高风险映射

| 行为域 | 关联保护项 | 必查路径 |
| --- | --- | --- |
| Metal pass 与 Glass | W-013, W-014, W-020 | renderer、glass、shader_types、shaders |
| Scene 原语 | W-010, W-012, W-014, W-016, W-018, W-019, W-020 | scene、elements、scene tests |
| Objective-C bridge | W-004, W-005, W-007, W-009 | app.h、window.m、host_view.m |
| wgpu/Windows | W-014, W-019, N-009 | wgpu rect、winit cursor/window |
| 文本与编辑器 | U-002, U-004, U-005, U-006 | text、layout、languages、editor、vim |
| Cargo/toolchain | 全部 | workspace manifests、Cargo.lock、Rust 2024、wgpu 30 |

## 9. 黄金基线与候选结果

本地候选执行结果：

| Gate | Baseline | Candidate | Delta | Evidence | Status |
| --- | --- | --- | --- | --- | --- |
| Warp focused tests | Core 291/0/7 ignored；WarpUI 47/0/1；Vim 57/0 | Core 314/0/7；WarpUI 47/0/1；Vim 71/0 | 通过数不降，无新失败 | `baseline/SUMMARY.md` (`c9f6e15f...6571`)；`phase-6/` focused logs | `Pass` |
| Warp full presubmit | 官方目标退出 101 | 候选退出 101 | 同一 `command-signatures-v2` Yarn/Node 首阻塞，无候选特有诊断 | `phase-6/presubmit-differential.md` (`2f5cf1dd...6936`) | `Blocked` (Upstream Baseline Failure) |
| NexShell bin tests | 182 passed | 182 passed | 0 | `baseline/SUMMARY.md`；`phase-8/nexshell-bin-final.txt` | `Pass` |
| NexShell lib tests | 378 passed / 2 failed / 1 ignored | 378 passed / 2 failed / 1 ignored | 精确同两个 `/var` alias 失败，无新增/加重 | `baseline/nexshell-lib.txt`；`phase-8/nexshell-lib-final.txt` | `Pass` (Known Baseline Failure unchanged) |
| macOS all-targets | Pass | Pass | 0 新增编译失败 | `baseline/nexshell-all-targets.txt`；`phase-8/nexshell-all-targets-final.txt` | `Pass` |
| macOS Intel check | 未要求黄金基线 | Pass | 候选 x86_64 Apple 完整编译 | `phase-8/macos-intel-final.txt` | `Pass` |
| Windows GNU cross check | Cursor E0004，Hidden/CustomImage 非穷举 | Pass | Preservation Blocker 已修复 | `baseline/nexshell-windows-gnu.txt`；`phase-5/windows-winit-check.txt`；`phase-8/windows-gnu-final-after-commit.txt` | `Pass` |
| Windows MSVC native | 无 Windows host | 无 Windows host | 未执行 `.bat`/MSVC/smoke | `phase-11/issue-classification.md` (`9a6559cf...43d`) | `Blocked` (no Windows host) |
| Liquid visual matrix | 未采集 | 未采集 | 任务范围不执行手工 A/B | `phase-11/issue-classification.md` | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |
| zero-optical | 未采集 | 未采集 | `≤2/255` 未做手工像素对比 | `phase-11/issue-classification.md` | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |
| frame p95 | 未采集 | 未采集 | 240 帧三轮 A/B 未执行 | `phase-11/issue-classification.md` | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |
| IME/input | 未采集 | 未采集 | 源码/自动化通过，手工运行未执行 | `phase-4/protection-audit.md`；`phase-11/issue-classification.md` | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |
| RDP cursor/drag/reconnect | 未采集 | 未采集 | 源码/自动化通过，真实 RDP 未执行 | `phase-8/` focused logs；`phase-11/issue-classification.md` | `Pass`（2026-08-04 Matt 运行时签收：manual-signoff.md） |
| staged overlay | 原 patch SHA-256 `ffddbf62...b98` | 原样应用；16 focused，384 lib，182 bin，cross/DMG 通过 | 自动兼容；外部删除/重命名 UI 未手工验证 | `phase-10/SUMMARY.md` (`ae4a50f8...dfde`) | `Blocked` (manual runtime evidence pending) |
| strict compatibility | 不存在 | RED/GREEN 测试与候选/overlay strict 全通过 | 阻止错 sibling、dirty、remote mismatch、source archive | `phase-7/compatibility-test-green-final.txt` (`53269de0...79c2`)；Phase 8/10 strict logs | `Pass` |
| mirror operations | Phase 0 实时核对 | 审批点 A/B/C 后复核：workflow 全禁、Dependabot 抑制、Graph active | 无意外 run（仅 notify 空跑 failure 无副作用） | approval-abc-log.md | `Pass` |

## 10. 晋级签收

以下项目全部填写后才可请求稳定晋级：

- 最终 Warp SHA：`a82e44d2e20ab441a96d4fe8fccd7377cfb76eeb`
- 最终 NexShell 代码 SHA：`16a62c18e731e7ed0e7924987f1754a1dc0420e6`；最终候选 HEAD 是包含本 ledger 的文档提交，由于 commit 不能自包含其 SHA，精确值记于仓库外 `evidence/phase-12/final-report.md`
- Warp merge commit：`ad64d61ab2ec583d96157a9d84fa437283a1bea1`
- Warp merge commit parents：`a6adffe9a240b414d6b70dcfbc74b06b06269d8a` + `3e8a989902c4acdcb524af8cd8cb025e23402ddb`
- `official_upstream` ancestor check：Pass，`3e8a989902c4acdcb524af8cd8cb025e23402ddb` 是最终 Warp HEAD 的祖先
- `integration_mirror` exact check：Pass，manifest 精确值为 `a82e44d2e20ab441a96d4fe8fccd7377cfb76eeb`
- 未解决 `Blocked`：W-014（Windows tint 原生运行）、X-001 与 staged overlay gate（overlay UI 运行，属平行任务）、Windows MSVC native（无主机）、Warp full presubmit（Upstream Baseline Failure）；其余 W/N/O 项已凭 2026-08-04 Matt 运行时签收与审批点 C 复核转 `Pass`（manual-signoff.md、approval-abc-log.md）
- `Missing`：0；`Unknown`：0
- Matt 明确批准的 `Waived`：无
- 候选分支远端 SHA：审批点 A ✅ Warp `integration/nexshell-upstream-3e8a9899` = `a82e44d2e…`；NexShell `integration/warp-3e8a9899` = `c8daa2a7d…`（ls-remote 精确一致）
- 旧组合 tag SHA：审批点 B ✅ `nexshell-before-upstream-3e8a9899`^{} = `a6adffe9…`；`before-warp-3e8a9899`^{} = `266c939…`
- Warp `private/master` 晋级审批：审批点 C ✅ 2026-08-04 快进至 `a82e44d2e…`，镜像运维复核见 approval-abc-log.md
- NexShell `main` 晋级审批：待审批点 D
