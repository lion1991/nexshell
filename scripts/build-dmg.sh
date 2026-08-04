#!/usr/bin/env bash
set -euo pipefail

# ── 配置 ──
APP_NAME="NexShell"
BUNDLE_ID="com.matt.nexshell"
BIN_NAME="nexshell"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
"$SCRIPT_DIR/verify-warp-compatibility.sh" --strict
# 版本号从 Cargo.toml 读，避免和 crate 版本漂移
VERSION="$(grep -m1 '^version' "$PROJECT_DIR/Cargo.toml" | sed -E 's/.*"(.*)".*/\1/')"
DMG_NAME="${APP_NAME}-${VERSION}.dmg"

# 打包脚本经常从 `cargo run` 启动的 NexShell 终端里执行。那种 shell 会
# 继承外层 Cargo 的构建变量，直接再跑 cargo 会污染 ring/rustls 的
# fingerprint，下一次普通 release build 就会把 Warp 依赖链带脏。
source "$SCRIPT_DIR/cargo-env.sh"

# target 可能被 ~/.cargo/config.toml 的 target-dir 重定向到同步盘外，不写死，问 cargo 要真实路径。
TARGET_ROOT="$(nexshell_run_cargo_clean metadata --no-deps --format-version 1 \
    --manifest-path "$PROJECT_DIR/Cargo.toml" | jq -r '.target_directory')"
[ -n "$TARGET_ROOT" ] && [ "$TARGET_ROOT" != "null" ] || TARGET_ROOT="$PROJECT_DIR/target"
BUILD_DIR="$TARGET_ROOT/release"
BUNDLE_DIR="$TARGET_ROOT/bundle"
APP_BUNDLE="$BUNDLE_DIR/$APP_NAME.app"
DMG_DIR="$TARGET_ROOT/dmg"

# ── 解析参数 ──
SKIP_BUILD=false
for arg in "$@"; do
    case $arg in
        --skip-build) SKIP_BUILD=true ;;
        --debug)
            BUILD_DIR="$TARGET_ROOT/debug"
            DMG_NAME="${APP_NAME}-${VERSION}-debug.dmg"
            ;;
    esac
done

# ── 1. 编译 ──
if [ "$SKIP_BUILD" = false ]; then
    echo "▸ 编译 release …"
    nexshell_run_cargo_clean build --release --manifest-path "$PROJECT_DIR/Cargo.toml"
else
    echo "▸ 跳过编译"
fi

if [ ! -f "$BUILD_DIR/$BIN_NAME" ]; then
    echo "✘ 找不到二进制: $BUILD_DIR/$BIN_NAME"
    exit 1
fi

# ── 2. 构建 .app bundle ──
echo "▸ 构建 $APP_NAME.app …"
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

cp "$BUILD_DIR/$BIN_NAME" "$APP_BUNDLE/Contents/MacOS/$APP_NAME"

# Info.plist
cat > "$APP_BUNDLE/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>${BUNDLE_ID}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleExecutable</key>
    <string>${APP_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
    <key>NSLocalNetworkUsageDescription</key>
    <string>NexShell 需要访问局域网，以便连接你的 SSH / RDP / Serial 主机。</string>
    <key>NSAppleEventsUsageDescription</key>
    <string>NexShell 中运行的程序可能使用 AppleScript 来控制其他应用。</string>
</dict>
</plist>
PLIST

# 如果有 .icns 图标就拷进去
ICON_PATH="$PROJECT_DIR/assets/AppIcon.icns"
if [ -f "$ICON_PATH" ]; then
    cp "$ICON_PATH" "$APP_BUNDLE/Contents/Resources/AppIcon.icns"
    # 追加 CFBundleIconFile
    /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string AppIcon" "$APP_BUNDLE/Contents/Info.plist"
    echo "  ✓ 使用自定义图标"
else
    echo "  ⚠ 未找到 assets/AppIcon.icns，使用系统默认图标"
fi

# ── 3. 签名 ──
# 有开发者证书就正式签（DR 锚 Team ID，TCC 授权重建不掉）；否则回退 ad-hoc。可用 NEXSHELL_SIGN_IDENTITY 覆盖（名字或 SHA-1）。
# 按 SHA-1 指纹签而非名字：login keychain 常有同名重复证书（如多张同名 Apple Development），按名字 codesign 会 ambiguous 报错中断。
SIGN_IDENTITY="${NEXSHELL_SIGN_IDENTITY:-}"
SIGN_LABEL="$SIGN_IDENTITY"
if [ -z "$SIGN_IDENTITY" ]; then
    IDENTITY_LINE="$(security find-identity -v -p codesigning 2>/dev/null | grep -E '"(Developer ID Application|Apple Development): ' | head -1 || true)"
    if [ -n "$IDENTITY_LINE" ]; then
        SIGN_IDENTITY="$(echo "$IDENTITY_LINE" | awk '{print $2}')"            # 唯一指纹，消除同名歧义
        SIGN_LABEL="$(echo "$IDENTITY_LINE" | sed -E 's/.*"(.*)".*/\1/') ($SIGN_IDENTITY)"
    fi
fi
if [ -n "$SIGN_IDENTITY" ]; then
    echo "▸ 正式签名: ${SIGN_LABEL}"
    codesign --force --deep --sign "$SIGN_IDENTITY" "$APP_BUNDLE"
else
    echo "▸ 无开发者证书，回退 Ad-hoc 签名 …"
    codesign --force --deep --sign - "$APP_BUNDLE"
fi

# ── 4. 打包 DMG ──
echo "▸ 打包 DMG …"
rm -rf "$DMG_DIR"
mkdir -p "$DMG_DIR"

STAGING="$DMG_DIR/staging"
rm -rf "$STAGING"
mkdir -p "$STAGING"
cp -R "$APP_BUNDLE" "$STAGING/"
ln -s /Applications "$STAGING/Applications"

hdiutil create \
    -volname "$APP_NAME" \
    -srcfolder "$STAGING" \
    -ov \
    -format UDZO \
    "$DMG_DIR/$DMG_NAME"

rm -rf "$STAGING"

DMG_PATH="$DMG_DIR/$DMG_NAME"
DMG_SIZE=$(du -h "$DMG_PATH" | cut -f1)
echo ""
echo "✔ 完成 ($DMG_SIZE)，双击 DMG → 拖拽 $APP_NAME 到 Applications 即可"
# 末行单独输出绝对路径：便于复制 / 终端点击 / tail -1 脚本取用
echo ""
echo "$DMG_PATH"
