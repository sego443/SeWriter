#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-2.9.1}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="$ROOT/.cache/sparkle"
ARCHIVE="$CACHE/Sparkle-$VERSION.tar.xz"
URL="https://github.com/sparkle-project/Sparkle/releases/download/$VERSION/Sparkle-$VERSION.tar.xz"

mkdir -p "$CACHE"

if [[ ! -f "$ARCHIVE" ]]; then
  echo "Downloading Sparkle $VERSION..."
  curl -L "$URL" -o "$ARCHIVE"
fi

echo "Extracting Sparkle $VERSION..."
rm -rf "$CACHE/Sparkle-$VERSION"
mkdir -p "$CACHE/Sparkle-$VERSION"
tar -xf "$ARCHIVE" -C "$CACHE/Sparkle-$VERSION" --strip-components=1

echo ""
echo "Sparkle extracted to:"
echo "  $CACHE/Sparkle-$VERSION"
echo ""
echo "Framework candidate:"
if [[ -d "$CACHE/Sparkle-$VERSION/Sparkle.framework" ]]; then
  echo "$CACHE/Sparkle-$VERSION/Sparkle.framework"
else
  find "$CACHE/Sparkle-$VERSION" -name Sparkle.framework -type d | head -1
fi
