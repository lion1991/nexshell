#!/usr/bin/env bash
# 由 AppIcon.svg 生成带透明通道的 AppIcon.icns
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ASSETS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)/assets"
SVG="$ASSETS_DIR/AppIcon.svg"
ICNS="$ASSETS_DIR/AppIcon.icns"

RSVG=$(command -v rsvg-convert || true)
if [ -z "$RSVG" ]; then
    for candidate in /opt/homebrew/bin/rsvg-convert /usr/local/bin/rsvg-convert; do
        [ -x "$candidate" ] && RSVG="$candidate" && break
    done
fi
if [ -z "$RSVG" ]; then
    echo "✘ 缺少 rsvg-convert，请先 brew install librsvg" >&2
    exit 1
fi

TMP=$(mktemp -d)
ICONSET="$TMP/AppIcon.iconset"
mkdir -p "$ICONSET"

# macOS iconset 规定的 10 个尺寸（@1x / @2x）
render() {
    local size=$1 name=$2
    "$RSVG" --background-color=none -w "$size" -h "$size" \
        -o "$ICONSET/$name" "$SVG"
}

render 16    icon_16x16.png
render 32    icon_16x16@2x.png
render 32    icon_32x32.png
render 64    icon_32x32@2x.png
render 128   icon_128x128.png
render 256   icon_128x128@2x.png
render 256   icon_256x256.png
render 512   icon_256x256@2x.png
render 512   icon_512x512.png
render 1024  icon_512x512@2x.png

iconutil -c icns -o "$ICNS" "$ICONSET"
rm -rf "$TMP"

echo "✔ 已生成: $ICNS"
