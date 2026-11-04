#!/bin/sh
# Builds FFmpeg and Falcon.app for Apple Silicon and packages them into target/macos/Falcon-arm64.dmg,
# whose window offers dragging the app to Applications. The app is only ad-hoc signed, not notarised,
# so macOS asks people to allow it the first time.
set -eu
cd "$(dirname "$0")/.."

# Without this, FFmpeg's libraries only load on macOS as new as the build machine's.
export MACOSX_DEPLOYMENT_TARGET=12.0

scripts/build-ffmpeg.sh
scripts/bundle.sh

APP=target/release/Falcon.app
codesign --verify --deep "$APP"
[ -f "$APP/Contents/Frameworks/libavformat.61.dylib" ] || { echo "FFmpeg didn't get into the app" >&2; exit 1; }
for file in "$APP/Contents/MacOS/Falcon" "$APP"/Contents/Frameworks/*.dylib; do
  minos=$(otool -l "$file" | awk '/LC_BUILD_VERSION/ { found = 1 } found && $1 == "minos" { print $2; exit }')
  case "$minos" in
    11.*|12.0|12) ;;
    *) echo "$file needs macOS $minos, newer than the 12.0 Falcon promises" >&2; exit 1 ;;
  esac
  if otool -L "$file" | grep -q "$PWD"; then
    echo "$file links a path on the build machine" >&2
    exit 1
  fi
done

OUT=target/macos
rm -rf "$OUT"
mkdir -p "$OUT/stage"
cp -R "$APP" "$OUT/stage/Falcon.app"
ln -s /Applications "$OUT/stage/Applications"
hdiutil create -volname Falcon -srcfolder "$OUT/stage" -ov -format UDZO "$OUT/Falcon-arm64.dmg"
rm -rf "$OUT/stage"
ls -l "$OUT"
