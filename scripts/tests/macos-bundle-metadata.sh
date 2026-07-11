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
