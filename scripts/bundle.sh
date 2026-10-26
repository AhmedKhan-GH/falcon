#!/bin/sh
# Builds falcon-app in release mode and wraps it in target/release/Falcon.app.
set -e
cd "$(dirname "$0")/.."
cargo build --release -p falcon-app
APP=target/release/Falcon.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/falcon-app "$APP/Contents/MacOS/Falcon"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Falcon</string>
  <key>CFBundleDisplayName</key><string>Falcon</string>
  <key>CFBundleIdentifier</key><string>dev.falcon.app</string>
  <key>CFBundleExecutable</key><string>Falcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
</dict>
</plist>
PLIST
echo "Built $APP"
