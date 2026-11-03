#!/bin/sh
# Builds falcon-app in release mode and wraps it in target/release/Falcon.app.
set -e
cd "$(dirname "$0")/.."
cargo build --release -p falcon-app
APP=target/release/Falcon.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/falcon-app "$APP/Contents/MacOS/Falcon"
cp crates/falcon-app/assets/Falcon.icns "$APP/Contents/Resources/Falcon.icns"
# The FFmpeg fallback player's libraries, when scripts/build-ffmpeg.sh has
# built them. Falcon opens them at run time, so the app works without them;
# only formats macOS can't play are then unavailable.
FFMPEG=${FFMPEG_PREFIX:-target/ffmpeg}
if [ -f "$FFMPEG/lib/libavformat.61.dylib" ]; then
  FW="$APP/Contents/Frameworks"
  mkdir -p "$FW"
  # libdav1d is the AV1 decoder libavcodec links (absent from builds
  # made before it was added).
  for lib in libdav1d.7 libavutil.59 libswresample.5 libavfilter.10 libavcodec.61 libavformat.61 libswscale.8; do
    [ "$lib" = libdav1d.7 ] && [ ! -f "$FFMPEG/lib/$lib.dylib" ] && continue
    cp -L "$FFMPEG/lib/$lib.dylib" "$FW/$lib.dylib"
    chmod u+w "$FW/$lib.dylib"
    install_name_tool -id "@rpath/$lib.dylib" "$FW/$lib.dylib"
    # Point links to the other FFmpeg libraries at the bundle's copies.
    for dep in $(otool -L "$FW/$lib.dylib" | awk 'NR > 2 { print $1 }' | grep -E 'lib(avutil|swresample|avfilter|avcodec|avformat|swscale|dav1d)\.' || true); do
      install_name_tool -change "$dep" "@rpath/$(basename "$dep")" "$FW/$lib.dylib"
    done
    otool -l "$FW/$lib.dylib" | grep -q 'path @loader_path ' || install_name_tool -add_rpath @loader_path "$FW/$lib.dylib"
    codesign --force --sign - "$FW/$lib.dylib" 2>/dev/null
  done
  install_name_tool -add_rpath @executable_path/../Frameworks "$APP/Contents/MacOS/Falcon"
  RESIGN=1
  # The LGPL asks for its text and the source's whereabouts to ship along,
  # and dav1d's BSD licence for its text. They go in Resources: code
  # signing allows only code in Frameworks.
  mkdir -p "$APP/Contents/Resources/FFmpeg"
  cp "$FFMPEG/COPYING.LGPLv2.1" "$APP/Contents/Resources/FFmpeg/COPYING.LGPLv2.1.txt"
  if [ -f "$FFMPEG/COPYING.dav1d" ]; then
    cp "$FFMPEG/COPYING.dav1d" "$APP/Contents/Resources/FFmpeg/COPYING.dav1d.txt"
  fi
  cp "$FFMPEG/BUILD-INFO.txt" "$APP/Contents/Resources/FFmpeg/BUILD-INFO.txt"
  echo "Bundled FFmpeg from $FFMPEG"
else
  echo "No FFmpeg in $FFMPEG (run scripts/build-ffmpeg.sh): formats macOS can't play won't play"
fi
# web-ifc is built into falcon-ifc-worker when scripts/fetch-web-ifc.sh has fetched it;
# the worker goes beside the app's binary, and its licence and those of the libraries it
# builds against ship along.
WEB_IFC=${FALCON_WEB_IFC_DIR:-target/web-ifc}
WORKER=$(ls -t target/release/build/falcon-ifc-*/out/falcon-ifc-worker 2>/dev/null | head -n 1)
if [ -f "$WEB_IFC/SOURCES.txt" ] && [ -n "$WORKER" ]; then
  cp "$WORKER" "$APP/Contents/MacOS/falcon-ifc-worker"
  codesign --force --sign - "$APP/Contents/MacOS/falcon-ifc-worker" 2>/dev/null
  LIC="$APP/Contents/Resources/web-ifc"
  mkdir -p "$LIC"
  cp "$WEB_IFC/SOURCES.txt" "$LIC/SOURCES.txt"
  cp "$WEB_IFC/web-ifc/LICENSE.md" "$LIC/web-ifc-LICENSE.txt"
  cp "$WEB_IFC/glm/copying.txt" "$LIC/glm-LICENSE.txt"
  cp "$WEB_IFC/tinynurbs/LICENSE" "$LIC/tinynurbs-LICENSE.txt"
  cp "$WEB_IFC/fast_float/LICENSE-MIT" "$LIC/fast_float-LICENSE-MIT.txt"
  cp "$WEB_IFC/earcut/LICENSE" "$LIC/earcut-LICENSE.txt"
  cp "$WEB_IFC/cdt/LICENSE" "$LIC/CDT-LICENSE.txt"
  sed '/\*\//q' "$WEB_IFC/cdt/CDT/include/predicates.h" >"$LIC/CDT-predicates-LICENSE.txt"
  # CDT's portable_nth_element.hpp is Apache-2.0; the licence text is the one of this repository.
  cp LICENSE-APACHE "$LIC/LICENSE-CDT-portable_nth_element-Apache-2.0.txt"
  cp "$WEB_IFC/spdlog/LICENSE" "$LIC/spdlog-LICENSE.txt"
  cp "$WEB_IFC/stduuid/LICENSE" "$LIC/stduuid-LICENSE.txt"
  cp "$WEB_IFC/unordered_dense/LICENSE" "$LIC/unordered_dense-LICENSE.txt"
  echo "web-ifc worker bundled; licences in $LIC"
else
  echo "No web-ifc in $WEB_IFC (run scripts/fetch-web-ifc.sh): IFC shows only tessellated geometry"
fi
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
# The document types let Finder's Open With and Dock drops hand files, packages and libraries to falcon, which imports files and opens packages and libraries.
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Falcon</string>
  <key>CFBundleDisplayName</key><string>Falcon</string>
  <key>CFBundleIdentifier</key><string>dev.falcon.app</string>
  <key>CFBundleExecutable</key><string>Falcon</string>
  <key>CFBundleIconFile</key><string>Falcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key><string>Eagle link</string>
      <key>CFBundleURLSchemes</key><array><string>eagle</string></array>
      <key>CFBundleTypeRole</key><string>Viewer</string>
    </dict>
  </array>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>Eagle Package</string>
      <key>CFBundleTypeExtensions</key><array><string>eaglepack</string></array>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Alternate</string>
    </dict>
    <dict>
      <key>CFBundleTypeName</key><string>Eagle Library</string>
      <key>CFBundleTypeExtensions</key><array><string>library</string></array>
      <key>LSTypeIsPackage</key><false/>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Alternate</string>
    </dict>
    <dict>
      <key>CFBundleTypeName</key><string>Images, Videos and Design Files</string>
      <key>CFBundleTypeExtensions</key>
      <array>
        <string>jpg</string>
        <string>jpeg</string>
        <string>png</string>
        <string>mp4</string>
        <string>tif</string>
        <string>tiff</string>
        <string>psd</string>
        <string>ai</string>
        <string>pdf</string>
        <string>svg</string>
        <string>bmp</string>
        <string>icns</string>
        <string>eps</string>
        <string>xd</string>
        <string>webp</string>
        <string>sketch</string>
        <string>afdesign</string>
        <string>afphoto</string>
        <string>mov</string>
        <string>m4v</string>
        <string>webm</string>
        <string>key</string>
        <string>pptx</string>
        <string>ppt</string>
        <string>gif</string>
        <string>exr</string>
        <string>heic</string>
        <string>skt</string>
        <string>hdr</string>
        <string>c4d</string>
      </array>
      <key>CFBundleTypeMIMETypes</key>
      <array>
        <string>image/png</string>
        <string>image/jpeg</string>
        <string>image/gif</string>
      </array>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Alternate</string>
    </dict>
    <dict>
      <key>CFBundleTypeName</key><string>Folder</string>
      <key>LSItemContentTypes</key><array><string>public.folder</string></array>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>None</string>
    </dict>
  </array>
</dict>
</plist>
PLIST
# Changing the executable's rpath voided its ad-hoc signature; sign the
# whole bundle again (ad hoc, as the linker did).
if [ -n "${RESIGN:-}" ]; then
  codesign --force --sign - "$APP"
fi
echo "Built $APP"
