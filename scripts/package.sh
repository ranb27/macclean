#!/bin/sh
# package.sh — build macclean.app + macclean-macos-arm64.dmg.
# Output lands in target/dist/. Requires macOS; prettier dmg when
# create-dmg is installed (brew install create-dmg), hdiutil otherwise.
set -eu
cd "$(dirname "$0")/.."

APP=macclean
BUNDLE_ID="$APP.app"
DMG_NAME="$APP-macos-arm64.dmg"
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
DIST=target/dist
STAGE=$DIST/stage

cargo build --release

rm -rf "$DIST" "$BUNDLE_ID"
mkdir -p "$BUNDLE_ID/Contents/MacOS" "$BUNDLE_ID/Contents/Resources" "$STAGE"

cp "target/release/$APP" "$BUNDLE_ID/Contents/MacOS/$APP"
sed "s/__VERSION__/$VER/g" bundle/Info.plist > "$BUNDLE_ID/Contents/Info.plist"
cp assets/macclean.icns "$BUNDLE_ID/Contents/Resources/macclean.icns"

# Ad-hoc sign so the bundle has a coherent signature on arm64.
codesign --force --deep --sign - "$BUNDLE_ID" 2>/dev/null || true

cp -R "$BUNDLE_ID" "$STAGE/"
ln -s /Applications "$STAGE/Applications"

if command -v create-dmg >/dev/null 2>&1; then
    create-dmg \
        --volname "$APP" \
        --window-size 540 360 \
        --icon-size 96 \
        --icon "$BUNDLE_ID" 140 180 \
        --app-drop-link 400 180 \
        --hide-extension "$BUNDLE_ID" \
        "$DIST/$DMG_NAME" \
        "$STAGE" || {
            echo "create-dmg failed, falling back to diskutil" >&2
            diskutil image create from --volumeName "$APP" --format UDZO \
                "$STAGE" "$DIST/$DMG_NAME"
        }
else
    diskutil image create from --volumeName "$APP" --format UDZO \
        "$STAGE" "$DIST/$DMG_NAME"
fi

rm -rf "$BUNDLE_ID" "$STAGE"
echo "packaged $DIST/$DMG_NAME (v$VER)"
