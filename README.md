# NexShell Native Shell

NexShell 的**唯一架构**：纯 Rust + GPUI/warpui 原生终端 app，自带全套 UI（标题栏/标签/活动栏/终端/底部工具/主机库/监控）。旧的 Tauri + React 外壳已于 2026-06 废弃并删除（见仓库历史）。

模块组织与开发约定见 [`CLAUDE.md`](./CLAUDE.md)：RootView 面板拆分、行数阈值、命名与可见性、主机相关三层、rustfmt 约定等。

## 命令

纯 Rust crate，直接用 cargo（在 `nexshell/` 下）：

```sh
cargo run                                     # 启动 app（default feature = warpui-app）
cargo check                                   # 类型检查
cargo test                                    # 跑测试
cargo check --target x86_64-pc-windows-msvc   # Windows 交叉编译 gate
bash scripts/build-dmg.sh                     # 打包 .app + DMG（开发者证书签名）
```

### Metal Toolchain

GPUI 编译 Metal shader 需要 Xcode Metal Toolchain。多数 mac 上它默认激活，直接 `cargo run` 即可。若 cargo 报找不到 Metal Toolchain（装了但非默认），用 wrapper 自动探测注入：

```sh
bash scripts/cargo-run.sh         # = 注入 TOOLCHAINS 后 cargo run
bash scripts/cargo-run.sh check   # 透传任意 cargo 子命令
```

或手动 `export TOOLCHAINS=$(xcodebuild -showComponent MetalToolchain | awk '/Identifier/{print $NF}')`；未装则 `xcodebuild -downloadComponent MetalToolchain`。

## Warp 参考策略

Warp 源码是终端工程与 UI 打磨的首要参考——目标是把稳定的终端基础设施适配进 NexShell 的产品形态，而非嵌入整个 Warp app。每个 Rust 终端功能的实现都参考 Warp 对应代码，不自造轮子。warp 是 fork（master = 上游 + nexshell 私有补丁），升级靠 rebase 补丁 + cargo check 验证漂移。

## 从 Tauri 迁移的已知功能差异

数据无缝（与旧 Tauri 共用同一 `com.matt.nexshell/nexshell.db`），但旧版两项 host-library 邻近功能 native 端暂未实现，对应旧表成为库内孤儿数据（无害、不阻塞）：

- **快捷命令**（旧 `quick_commands` 表）：native 仅有 history ghost-text 补全，无用户自定义命令片段联想。
- **标签自定义颜色**（旧 `tag_styles` 表）：native 标签用统一主题色，无 per-tag 配色。

即"功能对等"指 SSH / 主机库 / 终端 / 系统集成四大块 0 blocker，不含上述两项。若后续补齐，复用同库即可读回旧数据。
