#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
BUILD_SCRIPT="$PROJECT_DIR/scripts/build-dmg.sh"
PLIST="$(mktemp)"
trap 'rm -f "$PLIST"' EXIT

awk '
    /cat > "\$APP_BUNDLE\/Contents\/Info.plist" << PLIST/ {
        capture = 1
        next
    }
    capture && /^PLIST$/ { exit }
    capture { print }
' "$BUILD_SCRIPT" > "$PLIST"

plutil -lint "$PLIST" >/dev/null

expected="NexShell 中运行的程序可能使用 AppleScript 来控制其他应用。"
actual="$(plutil -extract NSAppleEventsUsageDescription raw -o - "$PLIST" 2>/dev/null || true)"

if [[ "$actual" != "$expected" ]]; then
    printf 'expected NSAppleEventsUsageDescription=%q, got %q\n' "$expected" "$actual" >&2
    exit 1
fi

# 其他应用按「能打开 .command」筛终端，缺了这条 NexShell 不会出现在候选里。
if ! plutil -extract CFBundleDocumentTypes xml1 -o - "$PLIST" | grep -q '<string>com.apple.terminal.shell-script</string>'; then
    echo 'expected CFBundleDocumentTypes to claim com.apple.terminal.shell-script' >&2
    exit 1
fi
