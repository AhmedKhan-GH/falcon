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
echo "Built $APP"
