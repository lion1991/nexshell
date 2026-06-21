#!/usr/bin/env bash
# 探测 Xcode Metal Toolchain 并注入 TOOLCHAINS，再透传给 cargo。
# 多数 mac 上 Metal toolchain 是默认激活的，直接 cargo run 即可，无需本脚本；
# 仅当 cargo 报"找不到 Metal Toolchain"（装了但非默认）时才需要它。
# 用法: scripts/cargo-run.sh [cargo 子命令与参数]   默认 run
set -euo pipefail
cd "$(dirname "$0")/.."  # crate 根 = nexshell/

if [ -z "${TOOLCHAINS:-}" ]; then
    TOOLCHAINS="$(xcodebuild -showComponent MetalToolchain 2>/dev/null \
        | awk -F': ' '/Toolchain Identifier/{print $2; exit}')"
fi
if [ -z "${TOOLCHAINS:-}" ]; then
    echo "✘ 未找到 Metal Toolchain。先装：xcodebuild -downloadComponent MetalToolchain" >&2
    exit 1
fi

[ $# -eq 0 ] && set -- run
exec env TOOLCHAINS="$TOOLCHAINS" cargo "$@"
