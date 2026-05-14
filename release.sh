#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: ./release.sh VERSION [--publish] [--update-appcast]" >&2
  exit 1
fi

VERSION="$1"
shift

PUBLISH=0
UPDATE_APPCAST=0
for arg in "$@"; do
  case "$arg" in
    --publish) PUBLISH=1 ;;
    --update-appcast) UPDATE_APPCAST=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 1 ;;
  esac
done

ROOT="$(cd "$(dirname "$0")" && pwd)"
DMG="$ROOT/dist/SeWriter.dmg"
TAG="v$VERSION"
RELEASE_URL="https://github.com/sego443/SeWriter/releases/download/$TAG/SeWriter.dmg"

perl -0pi -e 's/^version = "[^"]+"/version = "'"$VERSION"'"/m' "$ROOT/Cargo.toml"
perl -0pi -e 's#releases/tag/v[0-9]+\.[0-9]+\.[0-9]+#releases/tag/'"$TAG"'#g' "$ROOT/README.md"

cargo check
cargo test
"$ROOT/build-mac.sh"

if [[ "$UPDATE_APPCAST" == "1" ]]; then
  : "${SPARKLE_SIGN_UPDATE:?set SPARKLE_SIGN_UPDATE to Sparkle bin/sign_update}"
  SIGN_OUTPUT="$("$SPARKLE_SIGN_UPDATE" "$DMG")"
  mkdir -p "$ROOT/dist"
  cat > "$ROOT/dist/appcast-item-$VERSION.xml" <<ITEM
    <item>
      <title>Version $VERSION</title>
      <sparkle:version>${VERSION##*.}</sparkle:version>
      <sparkle:shortVersionString>$VERSION</sparkle:shortVersionString>
      <enclosure
        url="$RELEASE_URL"
        $SIGN_OUTPUT
        type="application/octet-stream" />
    </item>
ITEM
  echo "Generated appcast item: dist/appcast-item-$VERSION.xml"
  echo "Review it, then insert it into docs/appcast.xml."
fi

if [[ "$PUBLISH" == "1" ]]; then
  if ! command -v gh >/dev/null 2>&1; then
    echo "error: gh is required for --publish but is not installed" >&2
    exit 1
  fi
  NOTES="$ROOT/docs/release-notes/$TAG.md"
  if [[ ! -f "$NOTES" ]]; then
    NOTES="$ROOT/docs/release-notes/v$VERSION.md"
  fi
  if [[ -f "$NOTES" ]]; then
    gh release create "$TAG" "$DMG" --title "SeWriter $TAG" --notes-file "$NOTES"
  else
    gh release create "$TAG" "$DMG" --title "SeWriter $TAG" --notes "SeWriter $TAG"
  fi
else
  echo "Dry run complete. Built $DMG"
  echo "Publish manually or rerun with --publish after reviewing changes."
fi
