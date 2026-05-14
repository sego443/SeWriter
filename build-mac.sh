#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────
# SeWriter macOS build script
# Produces: dist/SeWriter.dmg  (Universal Binary, unsigned)
# Usage: ./build-mac.sh
# ─────────────────────────────────────────────

APP_NAME="SeWriter"
BUNDLE_ID="com.sewriter.app"
CARGO="$HOME/.cargo/bin/cargo"

ROOT="$(cd "$(dirname "$0")" && pwd)"
DIST="$ROOT/dist"
APP="$DIST/$APP_NAME.app"
CONTENTS="$APP/Contents"
DMG="$DIST/$APP_NAME.dmg"
VERSION="${SEWRITER_VERSION:-$(awk -F\" '/^version =/ { print $2; exit }' "$ROOT/Cargo.toml")}"
BUNDLE_VERSION="${SEWRITER_BUNDLE_VERSION:-$(awk -v v="$VERSION" 'BEGIN { split(v, p, "."); print p[3] ? p[3] : v }')}"
APPCAST_URL="${SEWRITER_APPCAST_URL:-https://sego443.github.io/SeWriter/appcast.xml}"
SPARKLE_FRAMEWORK_PATH="${SPARKLE_FRAMEWORK_PATH:-}"
SPARKLE_PUBLIC_ED_KEY="${SPARKLE_PUBLIC_ED_KEY:-b9K4seR/8gKh4rk/mKM1j4ioM89zS1F2l1ebt/oPvSA=}"
ENABLE_SPARKLE=0

if [[ -n "$SPARKLE_FRAMEWORK_PATH" ]]; then
  if [[ -z "$SPARKLE_PUBLIC_ED_KEY" ]]; then
    echo "error: SPARKLE_PUBLIC_ED_KEY is empty" >&2
    exit 1
  fi
  if [[ ! -d "$SPARKLE_FRAMEWORK_PATH" ]]; then
    echo "error: SPARKLE_FRAMEWORK_PATH does not exist: $SPARKLE_FRAMEWORK_PATH" >&2
    exit 1
  fi
  ENABLE_SPARKLE=1
fi

# ── 0. Clean previous dist ──────────────────
rm -rf "$DIST"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources" "$CONTENTS/Frameworks"

echo "▸ Building arm64..."
if [[ "$ENABLE_SPARKLE" == "1" ]]; then
  SEWRITER_SPARKLE_FRAMEWORK="$SPARKLE_FRAMEWORK_PATH" "$CARGO" build --release --target aarch64-apple-darwin 2>&1 | tail -1
else
  "$CARGO" build --release --target aarch64-apple-darwin 2>&1 | tail -1
fi

echo "▸ Building x86_64..."
if [[ "$ENABLE_SPARKLE" == "1" ]]; then
  SEWRITER_SPARKLE_FRAMEWORK="$SPARKLE_FRAMEWORK_PATH" "$CARGO" build --release --target x86_64-apple-darwin 2>&1 | tail -1
else
  "$CARGO" build --release --target x86_64-apple-darwin 2>&1 | tail -1
fi

echo "▸ Merging Universal Binary..."
lipo -create \
  "$ROOT/target/aarch64-apple-darwin/release/sewriter" \
  "$ROOT/target/x86_64-apple-darwin/release/sewriter" \
  -output "$CONTENTS/MacOS/sewriter"
chmod +x "$CONTENTS/MacOS/sewriter"
lipo -info "$CONTENTS/MacOS/sewriter"

if [[ "$ENABLE_SPARKLE" == "1" ]]; then
  echo "▸ Bundling Sparkle..."
  cp -R "$SPARKLE_FRAMEWORK_PATH" "$CONTENTS/Frameworks/"
  install_name_tool -add_rpath "@executable_path/../Frameworks" "$CONTENTS/MacOS/sewriter" 2>/dev/null || true
fi

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
SPARKLE_PLIST_KEYS=""
if [[ "$ENABLE_SPARKLE" == "1" ]]; then
  SPARKLE_PLIST_KEYS=$(cat <<PLIST_KEYS
    <key>SUFeedURL</key>               <string>$APPCAST_URL</string>
    <key>SUPublicEDKey</key>           <string>$SPARKLE_PUBLIC_ED_KEY</string>
    <key>SUEnableAutomaticChecks</key> <true/>
PLIST_KEYS
)
fi
cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>             <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>      <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>       <string>$BUNDLE_ID</string>
    <key>CFBundleVersion</key>          <string>$BUNDLE_VERSION</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleExecutable</key>       <string>sewriter</string>
    <key>CFBundlePackageType</key>      <string>APPL</string>
    <key>CFBundleIconFile</key>         <string>AppIcon</string>
    <key>LSMinimumSystemVersion</key>   <string>12.0</string>
    <key>NSHighResolutionCapable</key>  <true/>
    <key>LSUIElement</key>              <true/>
$SPARKLE_PLIST_KEYS
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
