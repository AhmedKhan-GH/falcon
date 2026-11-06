#!/bin/sh
# Builds falcon-app in release mode and packages it for 64-bit Windows into target/windows:
# Falcon-x86_64-setup.exe (installs for the current user, no admin rights needed, see
# scripts/falcon.nsi), Falcon-x86_64-portable.zip (the same app to unzip and run, with the DLLs in
# a lib folder) and SHA256SUMS. The folder it installs stays in target/windows/Falcon.
#
# Run scripts/build-ffmpeg.sh first: without FFmpeg, video can't play on Windows, so this refuses
# to go on. With scripts/fetch-web-ifc.sh run too, IFC files get web-ifc's geometry. Run it in an
# MSYS2 UCRT64 shell that can also see cargo, for objdump. Visual Studio's C++ tools build the app;
# NSIS comes pinned, below. The installer isn't signed, so Windows warns people the first time.
set -eu
cd "$(dirname "$0")/.."

case "$(uname -s)" in
  MINGW* | UCRT*) ;;
  *) echo "Run this on Windows, in an MSYS2 UCRT64 shell" >&2; exit 1 ;;
esac
command -v objdump >/dev/null || { echo "Install objdump first (mingw-w64-ucrt-x86_64-binutils)" >&2; exit 1; }

# NSIS, pinned: the release's zip, with the SHA-256 SourceForge lists for it.
NSIS_VERSION=3.13
NSIS_SHA256=ba63dffc4410ee89193e1cb5a41989991bd77c61068da17e3156d136b7b0b3d8
NSIS_URL="https://downloads.sourceforge.net/project/nsis/NSIS%203/$NSIS_VERSION/nsis-$NSIS_VERSION.zip"

FFMPEG=${FFMPEG_PREFIX:-target/ffmpeg}
[ -f "$FFMPEG/lib/avformat-61.dll" ] || { echo "No FFmpeg in $FFMPEG: run scripts/build-ffmpeg.sh first" >&2; exit 1; }
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)

# The C runtime goes inside the programs, since a clean Windows may lack Visual C++'s DLLs.
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static"
export RUSTFLAGS
cargo build --release -p falcon-app

OUT=target/windows
APP=$OUT/Falcon
rm -rf "$OUT"
mkdir -p "$APP"

cp target/release/falcon-app.exe "$APP/"
# Falcon opens these at run time from its own folder, where they find each other.
for lib in libdav1d avutil-59 swresample-5 avfilter-10 avcodec-61 avformat-61 swscale-8; do
  cp "$FFMPEG/lib/$lib.dll" "$APP/"
done
# The LGPL asks for its text and the source's whereabouts to ship along, and dav1d's licence for its text.
cp LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md "$APP/"
cp "$FFMPEG/COPYING.LGPLv2.1" "$FFMPEG/COPYING.dav1d" "$FFMPEG/COPYING.winpthreads" "$FFMPEG/BUILD-INFO.txt" "$APP/"

# web-ifc is built into falcon-ifc-worker when scripts/fetch-web-ifc.sh has fetched it; the worker
# goes beside the app, and its licence and those of the libraries it builds against ship along.
WEB_IFC=${FALCON_WEB_IFC_DIR:-target/web-ifc}
WORKER=$(ls -t target/release/build/falcon-ifc-*/out/falcon-ifc-worker.exe 2>/dev/null | head -n 1 || true)
if [ -f "$WEB_IFC/SOURCES.txt" ] && [ -n "$WORKER" ]; then
  cp "$WORKER" "$APP/falcon-ifc-worker.exe"
  LIC=$APP/web-ifc
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
else
  echo "No web-ifc in $WEB_IFC (run scripts/fetch-web-ifc.sh): IFC shows only tessellated geometry"
fi

# Each binary may load only the app's own files and what Windows itself has, nothing the build
# machine happens to have in System32 either: no compiler's runtime and no graphics SDK.
SYSTEM32="$(cygpath -u "$SYSTEMROOT")/System32"
for file in "$APP"/*.exe "$APP"/*.dll; do
  for dep in $(objdump -p "$file" | sed -n 's/^[[:space:]]*DLL Name: //p'); do
    lower=$(echo "$dep" | tr '[:upper:]' '[:lower:]')
    case "$lower" in
      vcruntime* | msvcp* | vcomp* | concrt* | ucrtbased* | libgcc* | libstdc++* | libwinpthread* | zlib* | libz* | vulkan*)
        echo "$file links $dep, which a clean Windows lacks" >&2
        exit 1
        ;;
      api-ms-win-* | ext-ms-win-*) ;;
      *)
        [ -f "$APP/$dep" ] || [ -f "$SYSTEM32/$dep" ] || { echo "$file links $dep, outside the app and the system" >&2; exit 1; }
        ;;
    esac
  done
done

# NSIS isn't installed anywhere: its zip runs from where it's unpacked.
TOOLS=target/nsis-tools
NSIS=$TOOLS/nsis-$NSIS_VERSION
if [ ! -x "$NSIS/makensis.exe" ]; then
  rm -rf "$TOOLS"
  mkdir -p "$TOOLS"
  curl -fsSL -o "$TOOLS/nsis.zip" "$NSIS_URL"
  echo "$NSIS_SHA256  $TOOLS/nsis.zip" | sha256sum -c -
  # Windows's own tar reads zip files, which MSYS2's doesn't.
  (cd "$TOOLS" && "$SYSTEM32/tar.exe" -xf nsis.zip)
  rm "$TOOLS/nsis.zip"
fi

# The installer lists every file by name, so the uninstaller removes those and nothing else.
INSTALL=$OUT/install-files.nsh
UNINSTALL=$OUT/uninstall-files.nsh
: >"$INSTALL"
: >"$UNINSTALL"
for dir in $(cd "$APP" && find . -type d | sort); do
  sub=$(echo "${dir#.}" | sed 's|/|\\|g')
  echo "SetOutPath \"\$INSTDIR$sub\"" >>"$INSTALL"
  for file in $(cd "$APP/$dir" && find . -maxdepth 1 -type f | sort); do
    name=${file#./}
    echo "File \"$(cygpath -wa "$APP/$dir/$name")\"" >>"$INSTALL"
    echo "Delete \"\$INSTDIR$sub\\$name\"" >>"$UNINSTALL"
  done
done
for dir in $(cd "$APP" && find . -mindepth 1 -type d | sort -r); do
  echo "RMDir \"\$INSTDIR$(echo "${dir#.}" | sed 's|/|\\|g')\"" >>"$UNINSTALL"
done

# With a dash, not NSIS's usual slash, which the shell would take for a path to convert.
"$NSIS/makensis.exe" -V2 -WX -INPUTCHARSET UTF8 \
  "-DVERSION=$VERSION" \
  "-DOUTFILE=$(cygpath -wa "$OUT/Falcon-x86_64-setup.exe")" \
  "-DICON=$(cygpath -wa crates/falcon-app/assets/Falcon.ico)" \
  "-DFILES_INSTALL=$(cygpath -wa "$INSTALL")" \
  "-DFILES_UNINSTALL=$(cygpath -wa "$UNINSTALL")" \
  scripts/falcon.nsi

rm "$INSTALL" "$UNINSTALL"

# The portable zip: one Falcon folder with the programs and licences at the top and the DLLs in
# lib, where Falcon also looks for FFmpeg. Windows's own tar writes zip files; MSYS2's doesn't.
PORTABLE=$OUT/portable
mkdir -p "$PORTABLE"
cp -R "$APP" "$PORTABLE/Falcon"
mkdir "$PORTABLE/Falcon/lib"
mv "$PORTABLE/Falcon"/*.dll "$PORTABLE/Falcon/lib/"
(cd "$PORTABLE" && "$SYSTEM32/tar.exe" -a -cf ../Falcon-x86_64-portable.zip Falcon)
rm -rf "$PORTABLE"

(cd "$OUT" && sha256sum Falcon-x86_64-setup.exe Falcon-x86_64-portable.zip > SHA256SUMS)
ls -l "$OUT"
