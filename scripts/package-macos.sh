#!/bin/sh
# Builds Falcon.app in release mode and packages it for Apple Silicon Macs into target/macos:
# Falcon-arm64.dmg (its window holds the app and a link to Applications to drag it to) and
# SHA256SUMS.
#
# Run scripts/build-ffmpeg.sh first: without FFmpeg, formats macOS can't play won't play, so
# this refuses to go on. With scripts/fetch-web-ifc.sh run too, IFC files get web-ifc's
# geometry. Everything is built for macOS 12, the oldest Falcon supports, whatever macOS builds
# it. Needs only the Xcode command-line tools; codesign and hdiutil come with macOS.
#
# The app is signed ad hoc, which Apple Silicon needs to run it, but not with a Developer ID
# and not notarised, so macOS asks people to allow it the first time they open it.
set -eu
cd "$(dirname "$0")/.."

[ "$(uname -s)-$(uname -m)" = Darwin-arm64 ] || { echo "Run this on an Apple Silicon Mac" >&2; exit 1; }
MIN=12.0
MACOSX_DEPLOYMENT_TARGET=$MIN
export MACOSX_DEPLOYMENT_TARGET
FFMPEG=${FFMPEG_PREFIX:-target/ffmpeg}
[ -f "$FFMPEG/lib/libavformat.61.dylib" ] || { echo "No FFmpeg in $FFMPEG: run scripts/build-ffmpeg.sh first" >&2; exit 1; }

scripts/bundle.sh
APP=target/release/Falcon.app
FW=$APP/Contents/Frameworks
for lib in libdav1d.7 libavutil.59 libswresample.5 libavfilter.10 libavcodec.61 libavformat.61 libswscale.8; do
  [ -f "$FW/$lib.dylib" ] || { echo "$lib.dylib isn't in the app" >&2; exit 1; }
done
# Falcon's own licences ship along, next to FFmpeg's in Resources/FFmpeg.
cp LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md "$APP/Contents/Resources/"
# Sign everything ad hoc again, nested code first, so the whole bundle is sealed as it ships.
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

# Each binary must run on macOS $MIN and load only the system's libraries and the app's own:
# no other library, and no folder to look in, on the build machine. The IFC worker may need 13.3,
# as web-ifc does; IFC files fall back to falcon's own reader where it can't start.
for file in "$APP"/Contents/MacOS/* "$FW"/*.dylib; do
  want=$MIN
  [ "${file##*/}" = falcon-ifc-worker ] && want=13.3
  minos=$(otool -l "$file" | awk '/LC_BUILD_VERSION/ { found = 1 } found && $1 == "minos" { print $2; exit }')
  awk -v have="$minos" -v want="$want" 'BEGIN {
    if (split(have, h, ".") < 2) h[2] = 0; split(want, w, ".")
    exit !(have != "" && (h[1] < w[1] || (h[1] == w[1] && h[2] <= w[2])))
  }' || { echo "$file needs macOS ${minos:-?}, newer than $want" >&2; exit 1; }
  self=$(otool -D "$file" | sed -n 2p)
  for dep in $(otool -L "$file" | awk 'NR > 1 { print $1 }'); do
    [ "$dep" = "$self" ] && continue
    case "$dep" in
      /usr/lib/* | /System/Library/*) ;;
      @rpath/*) [ -f "$FW/${dep#@rpath/}" ] || { echo "$file links $dep, which isn't in the app" >&2; exit 1; } ;;
      *) echo "$file links $dep, outside the app and the system" >&2; exit 1 ;;
    esac
  done
  for rpath in $(otool -l "$file" | awk '$2 == "LC_RPATH" { found = 1 } found && $1 == "path" { print $2; found = 0 }'); do
    case "$rpath" in
      @executable_path/../Frameworks | @loader_path) ;;
      *) echo "$file looks for libraries in $rpath" >&2; exit 1 ;;
    esac
  done
done

OUT=target/macos
rm -rf "$OUT"
mkdir -p "$OUT/dmg"
ditto "$APP" "$OUT/dmg/Falcon.app"
ln -s /Applications "$OUT/dmg/Applications"
# hdiutil now and then finds the new image busy on build machines, so it gets three tries.
tries=1
until hdiutil create -volname Falcon -srcfolder "$OUT/dmg" -fs HFS+ -format UDZO -ov "$OUT/Falcon-arm64.dmg"; do
  [ "$tries" -lt 3 ] || exit 1
  tries=$((tries + 1))
  sleep 10
done
hdiutil verify "$OUT/Falcon-arm64.dmg"

rm -rf "$OUT/dmg"
(cd "$OUT" && shasum -a 256 Falcon-arm64.dmg > SHA256SUMS)
ls -l "$OUT"
