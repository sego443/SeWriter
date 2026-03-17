#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────
# SeWriter macOS build script
# Produces: dist/SeWriter.dmg  (Universal Binary, unsigned)
# Usage: ./build-mac.sh
# ─────────────────────────────────────────────

APP_NAME="SeWriter"
BUNDLE_ID="com.sewriter.app"
VERSION="0.1.0"
CARGO="$HOME/.cargo/bin/cargo"

ROOT="$(cd "$(dirname "$0")" && pwd)"
DIST="$ROOT/dist"
APP="$DIST/$APP_NAME.app"
CONTENTS="$APP/Contents"
DMG="$DIST/$APP_NAME.dmg"

# ── 0. Clean previous dist ──────────────────
rm -rf "$DIST"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"

echo "▸ Building arm64..."
"$CARGO" build --release --target aarch64-apple-darwin 2>&1 | tail -1

echo "▸ Building x86_64..."
"$CARGO" build --release --target x86_64-apple-darwin 2>&1 | tail -1

echo "▸ Merging Universal Binary..."
lipo -create \
  "$ROOT/target/aarch64-apple-darwin/release/sewriter" \
  "$ROOT/target/x86_64-apple-darwin/release/sewriter" \
  -output "$CONTENTS/MacOS/sewriter"
chmod +x "$CONTENTS/MacOS/sewriter"
lipo -info "$CONTENTS/MacOS/sewriter"

# ── 1. Icon: png → icns ──────────────────────
echo "▸ Creating icon..."
ICONSET="$DIST/AppIcon.iconset"
mkdir -p "$ICONSET"
SRC="$ROOT/assets/icon.png"
sips -z 16   16   "$SRC" --out "$ICONSET/icon_16x16.png"      > /dev/null
sips -z 32   32   "$SRC" --out "$ICONSET/icon_16x16@2x.png"   > /dev/null
sips -z 32   32   "$SRC" --out "$ICONSET/icon_32x32.png"      > /dev/null
sips -z 64   64   "$SRC" --out "$ICONSET/icon_32x32@2x.png"   > /dev/null
sips -z 128  128  "$SRC" --out "$ICONSET/icon_128x128.png"    > /dev/null
sips -z 256  256  "$SRC" --out "$ICONSET/icon_128x128@2x.png" > /dev/null
sips -z 256  256  "$SRC" --out "$ICONSET/icon_256x256.png"    > /dev/null
sips -z 512  512  "$SRC" --out "$ICONSET/icon_256x256@2x.png" > /dev/null
sips -z 512  512  "$SRC" --out "$ICONSET/icon_512x512.png"    > /dev/null
cp "$SRC"                      "$ICONSET/icon_512x512@2x.png"
iconutil -c icns "$ICONSET" -o "$CONTENTS/Resources/AppIcon.icns"
rm -rf "$ICONSET"

# ── 2. Info.plist ────────────────────────────
echo "▸ Writing Info.plist..."
cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>             <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>      <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>       <string>$BUNDLE_ID</string>
    <key>CFBundleVersion</key>          <string>$VERSION</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleExecutable</key>       <string>sewriter</string>
    <key>CFBundlePackageType</key>      <string>APPL</string>
    <key>CFBundleIconFile</key>         <string>AppIcon</string>
    <key>LSMinimumSystemVersion</key>   <string>12.0</string>
    <key>NSHighResolutionCapable</key>  <true/>
    <key>LSUIElement</key>              <true/>
</dict>
</plist>
PLIST

# ── 3. Pack DMG ──────────────────────────────
echo "▸ Creating DMG..."
TMP_DMG="$DIST/tmp.dmg"
MOUNT="$DIST/mnt"
mkdir -p "$MOUNT"

hdiutil create -volname "$APP_NAME" -size 60m -fs HFS+ "$TMP_DMG" > /dev/null
hdiutil attach "$TMP_DMG" -nobrowse -mountpoint "$MOUNT" > /dev/null

cp -R "$APP" "$MOUNT/"
ln -s /Applications "$MOUNT/Applications"

hdiutil detach "$MOUNT" > /dev/null
hdiutil convert "$TMP_DMG" -format UDZO -o "$DMG" > /dev/null
rm "$TMP_DMG"
rmdir "$DIST/mnt" 2>/dev/null || true

# ── Done ─────────────────────────────────────
SIZE=$(du -sh "$DMG" | cut -f1)
echo ""
echo "✓ Done: dist/SeWriter.dmg  ($SIZE)"
echo ""
echo "Note for users who see 'unverified developer':"
echo "  xattr -d com.apple.quarantine /Applications/SeWriter.app"
