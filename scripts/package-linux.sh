#!/bin/sh
# Builds falcon-app in release mode and packages it for x86-64 Linux into target/linux:
# Falcon-x86_64.AppImage (one file that runs on most distributions), falcon_amd64.deb
# (installs on Debian and Ubuntu, with a menu entry) and SHA256SUMS. Both hold the same tree.
#
# Run scripts/build-ffmpeg.sh first: without FFmpeg, video can't play on Linux,
# so this refuses to go on. Needs dpkg-deb. Binaries only run on systems whose glibc is at
# least as new as the build machine's, so releases are built on an older Ubuntu.
set -eu
cd "$(dirname "$0")/.."

# appimagetool, pinned: its release's asset, with the SHA-256 GitHub records for it.
TOOL_VERSION=1.9.1
TOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
TOOL_URL="https://github.com/AppImage/appimagetool/releases/download/$TOOL_VERSION/appimagetool-x86_64.AppImage"

FFMPEG=${FFMPEG_PREFIX:-target/ffmpeg}
[ -f "$FFMPEG/lib/libavformat.so.61" ] || { echo "No FFmpeg in $FFMPEG: run scripts/build-ffmpeg.sh first" >&2; exit 1; }
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)

cargo build --release -p falcon-app

OUT=target/linux
TREE=$OUT/tree
rm -rf "$OUT"
mkdir -p "$TREE/usr/bin" "$TREE/usr/lib/falcon" "$TREE/usr/share/applications" "$TREE/usr/share/doc/falcon" \
  "$TREE/usr/share/icons/hicolor/256x256/apps"

install -m 755 target/release/falcon-app "$TREE/usr/bin/falcon-app"
# Falcon opens these at run time from lib/falcon beside bin, and they find each other through $ORIGIN.
for lib in libdav1d.so.7 libavutil.so.59 libswresample.so.5 libavfilter.so.10 libavcodec.so.61 libavformat.so.61 libswscale.so.8; do
  install -m 644 "$FFMPEG/lib/$lib" "$TREE/usr/lib/falcon/$lib"
done
install -m 644 crates/falcon-app/assets/icon-256.png "$TREE/usr/share/icons/hicolor/256x256/apps/falcon.png"
cat > "$TREE/usr/share/applications/falcon.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Falcon
Comment=Collect, tag, search and browse images and other files
Exec=falcon-app
Icon=falcon
Terminal=false
Categories=Graphics;Viewer;
StartupWMClass=falcon
DESKTOP
# The LGPL asks for its text and the source's whereabouts to ship along, and dav1d's licence for its text.
DOC=$TREE/usr/share/doc/falcon
cp LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md "$DOC/"
cp "$FFMPEG/COPYING.LGPLv2.1" "$FFMPEG/COPYING.dav1d" "$FFMPEG/BUILD-INFO.txt" "$DOC/"

# The .deb.
DEB=$OUT/deb
mkdir -p "$DEB/DEBIAN"
cp -R "$TREE/usr" "$DEB/usr"
cat > "$DEB/DEBIAN/control" <<CONTROL
Package: falcon
Version: $VERSION
Architecture: amd64
Maintainer: Ahmed Khan <AhmedKhan-GH@users.noreply.github.com>
Depends: libc6, libasound2t64 | libasound2
Recommends: xdg-utils
Section: graphics
Priority: optional
Homepage: https://ahmedkhan-gh.github.io/falcon/
Description: Open-source asset manager for Eagle-format libraries
 Collect, tag, search and browse images, fonts, audio and video in one library.
CONTROL
dpkg-deb --root-owner-group --build "$DEB" "$OUT/falcon_amd64.deb"

# The AppImage. appimagetool is itself an AppImage: extracting it to run means the build needs no FUSE.
TOOLS=target/appimage-tools
TOOL=$TOOLS/appimagetool-$TOOL_VERSION
if [ ! -x "$TOOL" ]; then
  mkdir -p "$TOOLS"
  curl -fsSL -o "$TOOL.part" "$TOOL_URL"
  echo "$TOOL_SHA256  $TOOL.part" | sha256sum -c -
  chmod +x "$TOOL.part"
  mv "$TOOL.part" "$TOOL"
fi
APPDIR=$OUT/Falcon.AppDir
mkdir -p "$APPDIR"
cp -R "$TREE/usr" "$APPDIR/usr"
cp "$TREE/usr/share/applications/falcon.desktop" "$APPDIR/falcon.desktop"
cp crates/falcon-app/assets/icon-256.png "$APPDIR/falcon.png"
cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
here=$(dirname "$(readlink -f "$0")")
exec "$here/usr/bin/falcon-app" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$TOOL" --no-appstream "$APPDIR" "$OUT/Falcon-x86_64.AppImage"

rm -rf "$TREE" "$DEB" "$APPDIR"
(cd "$OUT" && sha256sum Falcon-x86_64.AppImage falcon_amd64.deb > SHA256SUMS)
ls -l "$OUT"
