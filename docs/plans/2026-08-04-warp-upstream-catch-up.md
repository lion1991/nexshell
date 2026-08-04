# Warp 上游追平实施计划

Status: approved plan, not started

Decision record: [ADR 0010](../adr/0010-warp-upstream-catch-up-preserves-nexshell-features.md)

Protection ledger: [Warp 特性保全清单](./2026-08-04-warp-preservation-ledger.md)

## 1. 目标

把 NexShell 使用的 Warp 集成镜像从当前私有提交完整追平到固定官方基线，同时保全全部 NexShell 自有行为和跨仓库契约。

| 身份 | 固定值 |
| --- | --- |
| NexShell 更新前基线 | `266c939695a9b7f892f442b16d88cff0cda1c305` |
| Warp 更新前集成镜像 | `a6adffe9a240b414d6b70dcfbc74b06b06269d8a` |
| Warp 官方目标基线 | `3e8a989902c4acdcb524af8cd8cb025e23402ddb` |
| 当前预演分叉 | 私有 `25`，官方 `796` |
| 当前预演显式冲突 | `14` 个文件 |

目标基线是不可移动的。实施期间官方 `master` 出现的新提交不进入本轮；若要改变目标 SHA，必须重新审计提交范围、共同祖先、冲突、保护清单和黄金基线。

私有 25 提交构成：17 个特性提交（ledger W-001~W-020 来源）+ 7 个上游摘取修复（U-001~U-007）+ 1 个镜像运维提交 `e6400b899`（Dependabot 抑制，由 O-002 验证）。

## 2. 成功定义

全部条件同时满足才算完成：

- Warp 候选包含官方目标基线的完整历史，保留当前私有历史，没有 rebase、squash 或 force-push。
- 保护清单中所有必需项为 `Pass` 或经 Matt 明确批准的 `Waived`；不能存在未批准的 `Blocked`、`Missing` 或 `Unknown`。
- 17 个 NexShell 特性提交的行为已保留或由经验证的上游等价实现接管；运维提交 `e6400b899` 的 Dependabot 抑制在合并后仍有效。
- 7 个上游摘取修复已证明由目标历史接管，不存在重复套用或行为回退。
- Warp 官方 presubmit 相对目标官方基线没有新增失败。
- NexShell focused、full、macOS、Windows、真实 RDP、视觉和性能门禁全部满足。
- 当前暂存的文件面板自动刷新补丁在候选组合上完成叠加验证，但没有进入同步提交。
- NexShell 记录最终 `official_upstream` 与 `integration_mirror`，严格校验能够阻止错误 sibling Warp 进入打包或发布。
- 私有镜像的继承 workflow 仍禁用，Dependabot 普通更新仍受抑制，Dependency Graph 仍启用。

编译通过、Git 冲突清零或应用能启动都不是单独的完成条件。

## 3. 不在范围内

- 不修改当前 NexShell 工作树的暂存或未跟踪内容。
- 不修改、升级或重新钉住 `../IronRDP`。
- 不顺手修复与 Warp 追平无因果关系的既有安全或产品缺陷。
- 不增加 Linux 作为 NexShell 发布平台。
- 不为 Windows 实现 Liquid Glass；只保留现有 wgpu tint 降级。
- 不安装或替换系统组件，不部署应用，不发布 DMG。
- 不在缺少单独批准时推送候选分支、创建远端 tag 或更新稳定分支。

## 4. 不变量

- 所有实施工作在成对隔离的干净 worktree 中进行。
- 当前工作树不 stash、不 reset、不 checkout、不提交、不清空暂存区。
- 高风险冲突文件禁止整文件选择 `ours` 或 `theirs`。
- Git 自动合并成功不等于语义审计完成。
- 上游结构优先，保护行为重放到上游结构中；不为保留旧代码形状而拒绝合法上游重构。
- 删除或移动生产代码时，原有契约测试必须移动或由等价测试替代。
- 环境阻塞记为 `Blocked`，不转换成静态 `Pass`。
- 远端候选、Warp 稳定、NexShell 稳定是三次独立审批。

## 5. 隔离布局

建议使用以下本地布局和分支名：

```text
/private/tmp/nexshell-warp-sync-3e8a9899/    # 易失根：全部可从 SHA 重建
  official-warp/       # 官方目标基线，只跑对照 presubmit
  baseline/
    warp/              # a6adffe9
    nexshell/          # 266c939
  candidate/
    warp/              # integration/nexshell-upstream-3e8a9899
    nexshell/          # integration/warp-3e8a9899
  staged-overlay/
    warp/              # 指向候选 Warp
    nexshell/          # 候选 NexShell + 当前暂存补丁
  targets/             # 每组验证独立 Cargo target

~/nexshell-warp-sync-3e8a9899/               # 持久根：不放 /private/tmp
  evidence/            # 日志、截图、帧统计、hash；不放凭据
```

证据必须放持久根：macOS 重启会清空 `/private/tmp`，periodic 也会清理旧文件，而黄金基线证据是晋级门禁的对照物且含人工采集成本，丢失即重采。worktree 与 target 目录丢失可从 SHA 重建，留在易失根。

分支名：

```text
Warp:     integration/nexshell-upstream-3e8a9899
NexShell: integration/warp-3e8a9899
```

隔离 worktree 共享 Git 对象和引用，但不共享 index 或工作目录。创建前记录两个当前工作树的 `git status --short --branch`、暂存路径和补丁 hash；创建后再次确认原工作树状态逐字不变。

## 6. Phase 0：远端和工具链预检

### 6.1 远端身份

只读核对：

```sh
git -C ../warp remote -v
git -C ../warp ls-remote https://github.com/warpdotdev/warp.git refs/heads/master
git -C ../warp ls-remote git@github.com:lion1991/warp-nexshell.git refs/heads/master
git ls-remote git@github.com:lion1991/nexshell.git refs/heads/main
```

期望：

- 官方目标提交仍可达；官方 `master` 已前进时 `ls-remote` 验不了任意对象，以 6.2 的 fetch 实际取回为准。不要求官方实时 HEAD 等于目标提交。
- 私有 Warp `master` 仍为 `a6adffe9`。
- NexShell `main` 仍为 `266c939`。
- 任一稳定远端移动时停止，不从新 HEAD 猜测新的起点。

### 6.2 抓取与图谱断言

目标对象当前不在 `../warp` 本地对象库（2026-08-04 预演在临时裸仓库完成，该仓库不作为实施依据），必须先显式抓取，跳过则后续命令直接 fatal：

```sh
git -C ../warp fetch origin master
git -C ../warp cat-file -e 3e8a989902c4acdcb524af8cd8cb025e23402ddb
```

随后验证：

```sh
git merge-base a6adffe9a240b414d6b70dcfbc74b06b06269d8a 3e8a989902c4acdcb524af8cd8cb025e23402ddb
git rev-list --left-right --count a6adffe9a240b414d6b70dcfbc74b06b06269d8a...3e8a989902c4acdcb524af8cd8cb025e23402ddb
git merge-tree --write-tree --name-only a6adffe9a240b414d6b70dcfbc74b06b06269d8a 3e8a989902c4acdcb524af8cd8cb025e23402ddb
```

预期分叉为 `25 796`，显式冲突为 14 个。不同结果必须先解释远端、对象或基线漂移，不能直接继续。

### 6.3 工具链

记录但不自动安装：

```sh
rustc --version --verbose
cargo --version
rustup target list --installed
xcodebuild -version
xcodebuild -showComponent MetalToolchain
cargo nextest --version
```

Warp 当前两侧都钉 Rust `1.92.0`。官方目标把 workspace crates 移到 Rust 2024 edition，并把 `wgpu` 从 `29.0.1` 升到 `30.0.0`；工具链一致不代表 edition/API 无需适配。

## 7. Phase 1：更新前黄金基线

任何合并前完成本阶段。原始证据保存在 `evidence/baseline/`，摘要回填保护清单。

### 7.1 源码与自动测试基线

Warp 基线至少运行：

```sh
CARGO_TARGET_DIR=/private/tmp/nexshell-warp-sync-3e8a9899/targets/baseline-warp \
  cargo test -p warpui_core --lib --no-fail-fast
CARGO_TARGET_DIR=/private/tmp/nexshell-warp-sync-3e8a9899/targets/baseline-warp \
  cargo test -p warpui --lib --no-fail-fast
CARGO_TARGET_DIR=/private/tmp/nexshell-warp-sync-3e8a9899/targets/baseline-warp \
  cargo test -p vim --no-fail-fast
CARGO_TARGET_DIR=/private/tmp/nexshell-warp-sync-3e8a9899/targets/baseline-warp \
  cargo check -p warpui -p warpui_core --all-targets
```

NexShell 基线至少运行：

```sh
CARGO_TARGET_DIR=/private/tmp/nexshell-warp-sync-3e8a9899/targets/baseline-nexshell \
  cargo test --bin nexshell --no-fail-fast
CARGO_TARGET_DIR=/private/tmp/nexshell-warp-sync-3e8a9899/targets/baseline-nexshell \
  cargo test --lib --no-fail-fast
CARGO_TARGET_DIR=/private/tmp/nexshell-warp-sync-3e8a9899/targets/baseline-nexshell \
  cargo check --all-targets
```

所有退出码、测试数、跳过项和 warning 都写入基线摘要。已有失败必须能稳定复现才可标记 `Known Baseline Failure`。

macOS 上 NexShell `cargo test --lib` 现存 2 个 `file_panel` worker symlink 测试失败（`/var` 与 `/private/var` 环境差异，非回归），直接预登记为 `Known Baseline Failure`；候选不得新增或加重。

### 7.2 Windows 已知基线失败

在更新前组合上运行现有交叉编译 gate：

```sh
CARGO_TARGET_DIR=/private/tmp/nexshell-warp-sync-3e8a9899/targets/baseline-windows \
  cargo check --target x86_64-pc-windows-gnu --all-targets
```

预期会在 Warp winit Cursor 穷举处失败。保存完整 E0004、文件和行号；候选必须消除该失败。不要把审查报告中的其他 Windows 安全发现混进本轮修复。

### 7.3 视觉基线

固定以下变量：

- 窗口尺寸使用 2 到 3 个预定义档位，记录像素尺寸和缩放比例。
- Dark/Light 各跑 menu 与 find，并分别使用高对比和低对比终端主题，组成八格矩阵。
- 单独捕获 goto line、commit detail、菜单 + 子菜单、find 常驻 + 终端 idle、find + 滚动、find + 持续输出。
- 捕获 Frosted、Liquid、Reduce Transparency/Off 和 zero-optical。
- 保存应用 SHA、Warp SHA、主题、字体、字号、屏幕 scale、截图 hash。

zero-optical 后续候选最大通道差不得超过 `2/255`；Liquid 使用相同场景并列人工签收。

### 7.4 性能基线

使用 `WARPUI_FRAME_STATS=1`，每场景至少 3 轮，每轮收集完整 240 帧窗口：

- `gpu.execute`
- `cpu.layout`
- `cpu.paint`
- `cpu.build_scene`

记录三轮各自 `p50/p95/max` 和三轮 `p95` 中位数。候选允许的噪声上限为 `max(基线 p95 × 10%, 0.5ms)`；`max` 只作尖峰诊断。

### 7.5 macOS 与真实 RDP 基线

运行并留证：

- 普通终端输入、组合键、IME 预编辑和候选框位置。
- RDP 原始键模式下 Cmd/Option/Ctrl/Shift 映射和本地快捷键优先级。
- 默认、隐藏、系统和位图指针；位图热点、重复指针缓存、标签切换和重连恢复。
- 合成拖拽与真实拖拽区分，RDP 页面不接收合成事件。
- `NEXSHELL_RDP_PTR_DUMP` 的位图输出不含凭据或会话内容。
- TerminalDecorations 的 undercurl/dotted/dashed 视觉和 signpost 脚本。
- `WARPUI_FRAME_STATS=1` 能同时输出 CPU 与 GPU 指标。

真实 RDP 地址、用户名、密码、证书或网络拓扑不写入仓库或证据摘要。

## 8. Phase 2：创建候选并执行原始合并

候选 Warp 从私有 `master` 精确 SHA 创建：

```sh
git switch -c integration/nexshell-upstream-3e8a9899 \
  a6adffe9a240b414d6b70dcfbc74b06b06269d8a
git merge --no-ff --no-commit \
  3e8a989902c4acdcb524af8cd8cb025e23402ddb
```

合并停在未提交状态后立即保存：

- `git status --short`
- `git diff --name-only --diff-filter=U`
- `git diff --cc`
- `git ls-files -u`
- 合并前 `merge-tree` 输出

不得先解决几个冲突再生成清单，否则会丢失原始证据。

## 9. Phase 3：语义解决显式冲突

按行为域而不是按文件顺序解决。

### 9.1 官方 App 与编辑器

冲突文件：

- `app/src/code/editor/view/vim_handler.rs`
- `app/src/editor/view/mod.rs`
- `app/src/lib.rs`
- `crates/vim/src/vim_tests.rs`

要求：

- 保留上游 Rust 2024、Vim `Indent/Dedent`、Replace mode 和 TUI Vim 演进。
- 保留 NexShell 需要的 `CodeEditorView`、事件、render options、初始化入口、菜单和单例导出。
- 保留 password setter/getter 与 `voice_input` feature gate，不把 Warp AI/TUI 产品依赖强制带进 NexShell。
- 更新 NexShell 的 `VimOperator` 穷举匹配，明确 `Indent/Dedent` 在内置编辑器与文本输入中的语义。
- 上游 `5gg` 修复由目标历史接管，保留或迁移对应回归测试。

### 9.2 macOS 平台、IME、输入与光标

冲突文件：

- `crates/warpui/src/platform/headless/event_loop.rs`
- `crates/warpui/src/platform/mac/app.rs`
- `crates/warpui/src/platform/mac/delegate.rs`
- `crates/warpui/src/platform/mac/event.rs`
- `crates/warpui/src/platform/mac/fonts.rs`
- `crates/warpui/src/platform/mac/window.rs`

要求：

- 采用上游最新窗口、URL 打开返回值、认证/麦克风、窗口转移和字体结构。
- 重放 `set_ime_disabled`、`invalidateImeCoordinatesAsync`、合成拖拽标记和 RDP 原始键盘旁路。
- 保留 `Cursor::Hidden/CustomImage`、全局位图注册、热点和 NSCursor 缓存生命周期。
- 保留 `NEXSHELL_DEBUG_KEYS`，默认关闭且不改变正常输入行为。
- 保留系统注销/关机不阻塞退出的上游行为，由目标历史接管后去除重复实现。
- 保留 CGFont identity 修复并验证目标历史中的上游版本等价。

### 9.3 WarpUI Core

冲突文件：

- `crates/warpui_core/src/core/app.rs`
- `crates/warpui_core/src/core/window.rs`
- `crates/warpui_core/src/elements/formatted_text_element_tests.rs`
- `crates/warpui_core/src/presenter.rs`

要求：

- 将 RDP 原始键盘分派适配到上游最新 action/window dispatch 模型。
- 将 frame stats 打点适配上游 presenter，不恢复旧结构。
- 将已被上游删除的 formatted-text 测试迁移到新模块，保留语义选词边界契约。
- 接受上游 `EntityId` hasher、窗口转移回调和 presenter 重构，同时验证本地扩展没有重新引入旧行为。

## 10. Phase 4：审计自动合并热点

至少检查以下双方共同修改但可能自动合并的路径：

- `Cargo.toml`、`Cargo.lock` 和受影响 crate manifests
- `crates/warpui/build.rs`
- `crates/warpui/src/platform/mac/objc/app.h`
- `crates/warpui/src/platform/mac/objc/window.m`
- `crates/warpui/src/platform/mac/objc/host_view.m`
- `crates/warpui/src/platform/mac/rendering/metal/renderer.rs`
- `crates/warpui/src/platform/mac/rendering/metal/glass/`
- `crates/warpui/src/platform/mac/rendering/metal/shaders/shader_types.h`
- `crates/warpui/src/platform/mac/rendering/metal/shaders/shaders.metal`
- `crates/warpui/src/rendering/wgpu/renderer/rect.rs`
- `crates/warpui/src/windowing/winit/`
- `crates/warpui_core/src/event.rs`
- `crates/warpui_core/src/platform/mod.rs`
- `crates/warpui_core/src/scene.rs` 与 `scene_tests.rs`
- `crates/warpui_core/src/text/` 与 `text_layout.rs`

逐项确认：

- Rust/C/Metal uniform layout 完全一致，bindgen allowlist 没有漏字段。
- Metal offscreen main target、present pass、scratch 生命周期和 resize/recreate 顺序仍正确。
- wgpu 30 tint fallback 编译且不误启用 Liquid optical。
- Quad、Ring、TerminalDecorations、双层阴影、镂空和 Rect AA 都仍进入 Scene 与 Metal draw 顺序。
- zero-optical 保留 Frosted bit-compatible 路径。
- Glass optical-active 上限、terminal dirty 和迟滞状态没有被上游 frame/runtime 改写破坏。
- Windows 对新 Cursor variants 有安全降级且匹配穷举。

每个保护项在 ledger 中记录候选位置和证据，不只记录“文件已检查”。

## 11. Phase 5：形成 Warp 合并提交

第一个 Warp 提交必须是双亲 merge commit，包含：

- 官方完整历史
- 显式冲突解决
- 让 Warp 候选自身类型完整、结构一致所必需的保护适配

提交前检查：

```sh
git diff --check
git diff --cached --name-only
git status --short
git log -1 --pretty=raw
```

合并后测试发现的缺口不修改或 amend 该 merge commit；按以下行为域追加独立提交：

- macOS IME/input/cursor/RDP dispatch
- Metal/Scene/Liquid Glass
- Windows/winit fallback
- 上游 API 或测试迁移
- 镜像运维策略

## 12. Phase 6：Warp 验证

### 12.1 focused gates

至少运行：

```sh
cargo test -p warpui_core --lib --no-fail-fast
cargo test -p warpui --lib --no-fail-fast
cargo test -p vim --no-fail-fast
cargo check -p warpui -p warpui_core --all-targets
cargo check -p warp --all-targets
./script/format --check
```

补充保护项 focused filters：

- `glass_optical`
- `backdrop_blur`
- `terminal_content_dirty_for_glass`
- `draw_ring`
- `terminal_decoration`
- `rect_edge_aa`
- `formatted_text` / `word_boundaries`
- `vim` 的 `5gg`、`Indent/Dedent`、Replace mode
- `transfer_view`
- `Ctrl+/` escape sequence

### 12.2 官方完整差分 presubmit

在 `official-warp` 与 `candidate/warp` 使用相同工具链和不同 target 目录运行：

```sh
./script/presubmit
```

保存每个步骤的退出码和日志。判定规则：

- 官方基线通过而候选失败：`Update Regression`，阻断。
- 两侧同一测试以同一原因失败：可记录 `Upstream Baseline Failure`。
- 两侧都因环境失败但未进入代码：记录 `Blocked`，不能声称 presubmit 通过。
- 候选测试减少、被删除或跳过数增加：必须解释，默认阻断。

## 13. Phase 7：NexShell 消费端适配

候选 NexShell 从 `266c939` 创建，不包含当前暂存补丁。

预期适配点：

- `VimOperator::Indent/Dedent` 的穷举和编辑语义。
- Warp 平台 `open_url` 返回值等 API 变化。
- Rust 2024 依赖 crate 带来的类型、match 或 lint 变化；NexShell 自身不因依赖升级被强制改 edition。
- wgpu 30 依赖锁文件变化，不启用 experimental renderer。
- Windows Cursor fallback 与 winit 编译。
- 上游 editor/char-cell/tab-width 演进对内置编辑器的影响。
- `Ctrl+/` 发送 `0x1f` 的 NexShell 终端回归测试。

新增根级 `warp-compatibility.toml`，最终形态至少包含：

```toml
schema_version = 1

[warp]
official_upstream = "3e8a989902c4acdcb524af8cd8cb025e23402ddb"
integration_mirror = "<通过门禁后的最终 Warp SHA>"
relative_path = "../warp"
remote = "git@github.com:lion1991/warp-nexshell.git"
```

新增 `scripts/verify-warp-compatibility.sh`：

- 默认开发模式验证两个基线都是 `../warp` HEAD 的祖先；后继提交允许，dirty 明确报告。
- `--strict` 要求 HEAD 精确等于 `integration_mirror`、Warp 工作树干净、远端身份正确、官方目标是祖先。
- 缺 Git 元数据时明确失败，不把 source archive 猜成兼容。
- `scripts/build-dmg.sh` 在构建前调用 `--strict`。
- 完整验证入口调用 `--strict`。
- 普通 `build.rs`、`cargo check/test/run` 不隐式读取 Git 状态。

## 14. Phase 8：NexShell 自动门禁

### 14.1 macOS Apple Silicon

```sh
rustfmt --edition 2021 --config skip_children=true <本轮修改的 Rust 文件>
cargo test --bin nexshell --no-fail-fast
cargo test --lib --no-fail-fast
cargo check --all-targets
bash scripts/tests/macos-bundle-metadata.sh
bash scripts/build-dmg.sh
```

格式化只针对本轮 NexShell 修改文件并带 `skip_children=true`；Warp 使用自己的 `./script/format`。

### 14.2 macOS Intel 编译

```sh
cargo check --target x86_64-apple-darwin --all-targets
```

如果依赖链接只允许原生架构，先区分 compile、link 和环境错误；不能直接删除平台 gate。

### 14.3 Windows

Mac 交叉 gate：

```sh
cargo check --target x86_64-pc-windows-gnu --all-targets
```

Windows 原生 gate：

```bat
scripts\build-windows-exe.bat
cargo test --lib
```

原生冒烟覆盖启动、终端输入、文件面板、内置编辑器和 wgpu tint 降级。Liquid optical 不作为 Windows 目标。

## 15. Phase 9：运行态 A/B 门禁

### 15.1 Liquid Glass 与 Scene 原语

用 Phase 1 完全相同的窗口、主题、字体、尺寸和操作复测：

- 八格可读性矩阵
- menu、submenu、find、goto、commit detail
- Frosted、Liquid、Reduce Transparency/Off、zero-optical
- Quad 光标拖影
- Ring 与圆头端帽
- 双层阴影与透明背景镂空
- TerminalDecorations undercurl/dotted/dashed
- Rect 直边 AA
- Metal 字形对比度

精确断言：

- 同帧 optical-active glass 不超过 3。
- 终端内容 dirty 立即降级为 frosted。
- dirty 停止后至少 300ms 再恢复，不能逐帧闪烁。
- 光标闪烁、IME overlay、后台 tab 输出不触发降级。
- zero-optical 最大通道差不超过 `2/255`。

### 15.2 性能

重复三轮 240 帧采样。任一指标满足 `候选 p95 中位数 - 基线 p95 中位数 > max(基线 p95 中位数 × 10%, 0.5ms)` 时阻断，直到定位、修复或由 Matt 明确批准豁免。

### 15.3 IME 与输入

- 普通终端中文输入候选框跟随 active cursor。
- 焦点切换、滚动、resize 和 split 后候选框位置更新。
- RDP 原始键模式跳过 `interpretKeyEvents`，离开 RDP 后恢复。
- 本地快捷键优先，非 Cmd 键正确透传到远端。
- `NEXSHELL_DEBUG_KEYS` 只在显式启用时输出，不泄漏默认日志。

### 15.4 RDP 光标与拖拽

- Arrow、IBeam、Resize、Hidden、CustomImage 切换。
- 位图 alpha、热点、尺寸、重复 cache key 和缓存上限。
- RDP tab 切换、失焦、断线、重连和关闭后系统光标恢复。
- 合成拖拽不进入 RDP，真实拖拽仍产生远端输入。
- Windows fallback 不崩溃、不出现非穷举构建错误。

### 15.5 帧统计和装饰线

- `WARPUI_FRAME_STATS=1` 输出 CPU 与 GPU 指标，关闭时不产生统计日志。
- `scripts/verify-terminal-decorations-signpost.sh` 捕获 `TerminalDecorations` signpost。
- 装饰线长文本连续相位、不同缩放和滚动时无断裂或漂移。

## 16. Phase 10：平行暂存功能叠加验证

从原 NexShell 工作树导出仅限以下暂存路径的 patch 和 hash：

```text
src/file_panel.rs
src/root_view/file_panel_section/mod.rs
```

在 `staged-overlay/nexshell` 叠加，不提交。验证：

- 外部创建、删除、重命名触发根目录刷新。
- 已展开子目录自动刷新，不替换根 cwd。
- 手动 Refresh 同时更新已加载子树。
- PollWatcher 失败时仅降级为手动刷新，不终止 worker。
- worker shutdown 后 watcher 不遗留活动线程。
- 原补丁新增测试和 NexShell full gates 通过。

叠加出现的适配问题记录为同步兼容问题，但修复必须保持文件面板原任务的独立提交边界。原工作树 index 全程不变。

## 17. Phase 11：问题分类与停止条件

分类：

| 分类 | 处理 |
| --- | --- |
| `Update Regression` | 本轮必须修复 |
| `Preservation Blocker` | 本轮必须修复；包括 Windows Cursor gate |
| `Known Baseline Failure` | 留证、不得恶化，不顺手扩张 |
| `Upstream Baseline Failure` | 同环境差分确认后留证 |
| `Blocked` | 停止晋级，等待环境或范围决策 |
| `Waived` | 仅 Matt 对具体项目明确批准后使用 |

立即停止并报告的条件：

- 稳定远端 SHA 与计划不符。
- 需要改写历史、force-push 或整文件选边才能继续。
- 需要修改 IronRDP 或其他未授权仓库。
- 保护项无法在上游新结构中定位或验证。
- 黄金基线无法采集，或候选缺少对应 A/B 证据。
- Windows、macOS 或真实 RDP 必测环境不可用。
- 性能超过阈值，视觉或输入行为存在无法解释的差异。
- 原 NexShell 工作树或暂存区发生意外变化。

## 18. Phase 12：提交和候选验收

Warp 提交层次：

1. 双亲上游 merge commit，含必要冲突适配。
2. macOS IME/input/cursor 保护修复。
3. Metal/Scene/Liquid Glass 保护修复。
4. Windows/winit 保护修复。
5. 其他由测试暴露的独立保护修复。
6. 镜像运维配置保全，仅在上游配置变化确实需要时存在。

NexShell 提交层次：

1. Warp API/Vim/Windows 消费端适配和回归测试。
2. 兼容基线与双模式校验。
3. 计划、ledger、README 或其他同步文档。

提交不要求机械凑满上述数量；没有实际变更的层次不创建空提交。禁止 squash、rebase 或 amend 已形成的同步历史。

最终本地验收：

```sh
git -C candidate/warp diff --check
git -C candidate/warp status --short --branch
git -C candidate/nexshell diff --check
git -C candidate/nexshell status --short --branch
scripts/verify-warp-compatibility.sh --strict
```

同时检查：

- Warp merge commit 有两个正确父提交。
- 官方目标提交是最终 Warp HEAD 的祖先。
- NexShell manifest 中 `integration_mirror` 精确等于最终 Warp HEAD。
- ledger 每项包含证据链接或日志 hash。
- 原工作树状态与 Phase 0 快照一致。

## 19. Phase 13：远端晋级，全部需要单独审批

### 19.1 推送候选分支

审批点 A：只推送两个候选分支，不改稳定分支。

推送后用 `git ls-remote` 核对远端 SHA，并检查远端 diff 与本地一致。

### 19.2 创建旧组合 tag

审批点 B：创建带注释 tag。

建议名称：

```text
Warp:     nexshell-before-upstream-3e8a9899
NexShell: before-warp-3e8a9899
```

tag 分别指向 `a6adffe9` 与 `266c939`，只用于定位和重建，不用于强制移动稳定分支。

### 19.3 Warp 稳定晋级

审批点 C：把已验证 Warp 候选快进到 `private/master`。

随后必须：

- `git ls-remote` 验证远端 HEAD。
- 枚举 GitHub workflows，保持继承工作流 `disabled_manually`。
- 对目标基线新增的 workflow 逐项禁用。
- 验证 Cargo 与 GitHub Actions 的 Dependabot `open-pull-requests-limit: 0`。
- 验证 Dependency Graph 仍 active。
- 确认没有意外 schedule、release、repo-sync 或 Dependabot PR run。

### 19.4 NexShell 稳定晋级

审批点 D：Warp 稳定和远端运维检查完成后，更新 NexShell `main`。

随后从稳定分支重新执行严格兼容检查和最终本地产物构建。批准 Warp 稳定不自动批准 NexShell 稳定。

## 20. 回滚

不 reset、不 force-push。

若候选阶段失败，保留候选分支和证据，不触碰稳定分支。

若稳定后发现回归：

1. 先停止 NexShell 新产物发布。
2. 按提交逆序 revert NexShell 适配提交。
3. 按提交逆序 revert Warp 后续保护修复。
4. 对 Warp merge commit 使用正确第一父方向的普通 revert。
5. 重新运行旧组合兼容检查和关键冒烟。
6. 使用旧组合 tag 重建证据，但不把稳定分支 reset 到 tag。

回滚顺序和实际 revert SHA 在晋级前根据最终提交图生成并保存，不能在事故时临时猜测。

## 21. 最终交付

- Warp merge 与保护修复提交清单。
- NexShell 适配、测试和兼容校验提交清单。
- 填完整的保护 ledger。
- 官方与候选 Warp presubmit 差分报告。
- NexShell macOS/Windows 自动门禁结果。
- 黄金基线与候选视觉/性能 A/B 摘要。
- IME、RDP 光标、原始键盘和拖拽运行证据。
- 暂存文件面板补丁叠加验证结果。
- 远端候选 SHA、稳定 SHA、tag 和 workflow 状态。
- 明确区分本地完成、候选推送、Warp 稳定、NexShell 稳定、DMG 构建、发布与部署状态。
