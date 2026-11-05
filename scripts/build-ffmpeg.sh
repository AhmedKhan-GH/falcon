#!/bin/sh
# Builds the LGPL FFmpeg that falcon's fallback video player loads at run time, decoding plus
# the atempo filter only, with the dav1d AV1 decoder (BSD-2-Clause) as a shared library
# libavcodec links.
#
# Downloads the release tarballs, checks them against pinned SHA-256s, configures FFmpeg
# without GPL or nonfree parts, without encoders, muxers, programs, avdevice or postproc,
# with atempo as libavfilter's only filter (its abuffer and abuffersink graph ends are always
# built), and installs shared libraries and headers into target/ffmpeg (never
# system-wide). dav1d is built with meson and ninja, which go into a Python venv under
# target/ffmpeg-tools with pkgconf (FFmpeg's configure finds dav1d through pkg-config).
# Nothing it produces is committed. scripts/bundle.sh copies the libraries into Falcon.app.
#
# On Windows, run it in an MSYS2 UCRT64 shell with mingw-w64 gcc, binutils, nasm and zlib, plus
# make and diffutils, and a Windows Python (not MSYS2's, whose venv can't take these wheels). The
# DLLs go in target/ffmpeg/lib like the other systems' libraries, with libgcc, winpthread and
# zlib linked in, and the script refuses DLLs that need anything a clean Windows lacks.
#
# Re-running is cheap: the tools, dav1d and FFmpeg are each rebuilt only when missing or
# when their pinned version (or FFmpeg's configure line, or the macOS they are built for)
# changes. FFMPEG_FORCE=1 rebuilds FFmpeg and dav1d regardless.
#
# Environment:
#   FFMPEG_PREFIX     install prefix (default: <repo>/target/ffmpeg)
#   FFMPEG_BUILD_DIR  empty scratch directory for the downloads and the builds
#                     (default: <repo>/target/ffmpeg-build, wiped before each build)
#   FFMPEG_FORCE      1 to rebuild even when the pinned versions are already installed
#   FFMPEG_TARBALL    absolute path of ffmpeg-<version>.tar.xz, for when ffmpeg.org can't be reached:
#                     a copy from anywhere, used only if it matches the pinned SHA-256
#   PYTHON            the Python that makes the tools venv (default: python3, on Windows python)
#   MACOSX_DEPLOYMENT_TARGET  on macOS, the oldest macOS the libraries load on (default: 12.0)
set -eu

VERSION=7.1.5
SHA256=de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f
URL="https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz"
# ffmpeg.org is sometimes unreachable for a while. MacPorts keeps the same file, and the SHA-256
# below is checked wherever it came from.
MIRROR_URL="https://distfiles.macports.org/ffmpeg/ffmpeg-$VERSION.tar.xz"

# dav1d's release tarball. The hash is the one VideoLAN publishes next to it
# (dav1d-1.5.4.tar.xz.sha256) and the one MacPorts' dav1d Portfile records for the same file.
DAV1D_VERSION=1.5.4
DAV1D_SHA256=686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5
DAV1D_URL="https://downloads.videolan.org/pub/videolan/dav1d/$DAV1D_VERSION/dav1d-$DAV1D_VERSION.tar.xz"
# The file names, tools and link settings each system needs.
case "$(uname -s)" in
  MINGW* | UCRT*) WINDOWS=1 ;;
  *) WINDOWS= ;;
esac
if [ "$(uname -s)" = Darwin ]; then
  DAV1D_LIB=libdav1d.7.dylib
  AVCODEC_LIB=libavcodec.61.dylib
  AVFILTER_LIB=libavfilter.10.dylib
  SHA256SUM="shasum -a 256"
  DAV1D_CC=clang
  # On arm64 dav1d's assembly is plain .S that clang assembles (no nasm needed).
  JOBS=$(sysctl -n hw.ncpu)
  # Built for the oldest macOS Falcon supports, not the one building it, or they won't load there.
  MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-12.0}
  export MACOSX_DEPLOYMENT_TARGET
  TARGET_STAMP=" macos $MACOSX_DEPLOYMENT_TARGET"
elif [ -n "$WINDOWS" ]; then
  [ "${MSYSTEM:-}" = UCRT64 ] || { echo "Run this in an MSYS2 UCRT64 shell" >&2; exit 1; }
  DAV1D_LIB=libdav1d.dll
  AVCODEC_LIB=avcodec-61.dll
  AVFILTER_LIB=avfilter-10.dll
  SHA256SUM=sha256sum
  DAV1D_CC=gcc
  command -v nasm >/dev/null || { echo "Install nasm first: without it video decodes far too slowly" >&2; exit 1; }
  JOBS=$(nproc)
  TARGET_STAMP=" windows"
else
  DAV1D_LIB=libdav1d.so.7
  AVCODEC_LIB=libavcodec.so.61
  AVFILTER_LIB=libavfilter.so.10
  SHA256SUM=sha256sum
  DAV1D_CC=${CC:-cc}
  # x86 assembly is what makes decoding fast there, and it needs nasm.
  command -v nasm >/dev/null || { echo "Install nasm first: without it video decodes far too slowly" >&2; exit 1; }
  JOBS=$(nproc)
  TARGET_STAMP=
fi

# Build tools for dav1d, as pinned wheels with their PyPI SHA-256s.
MESON_VERSION=1.12.1
MESON_SHA256=930bc7542cbd9f57009e182fd014eba48cf1a0180a7b9006d2d32d8f168d8b02
NINJA_VERSION=1.13.2
NINJA_SHA256=fd82e26c0706ad4ab88e5fdd26f3fab0a987a90f810160f6c322e752c6af298b
NINJA_SHA256_LINUX=65a24341b5ac09fcadcc37082660be40a94174e51a937fabf6e2cae26225fa2c
NINJA_SHA256_WINDOWS=1293f4078278b70d0ee4b6cc8f3a9e030656c9b2f59909970343c4fe76070118
PKGCONF_VERSION=3.0.7.post0
PKGCONF_SHA256=47cbf6889d84297b9a4e1741ae016daa4e198f3ec8bab7ce74554cc50b77cce4
PKGCONF_SHA256_LINUX=bd3ee4b5294a888d58462324cb83b7c0bc7150cc15b64b36aaf983e4b0ab90d5
PKGCONF_SHA256_WINDOWS=e7906bb5ec1d0d074e397466075a8545f1c50cb11710768644f222ba8acebc7f
# What pkgconf's Windows wheel depends on.
COLORAMA_VERSION=0.4.6
COLORAMA_SHA256=4f1d9991f5acc0ca119f9d443620b77f9d6b33703e51011c16baf57afb285fc6

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PREFIX=${FFMPEG_PREFIX:-$ROOT/target/ffmpeg}
BUILD=${FFMPEG_BUILD_DIR:-$ROOT/target/ffmpeg-build}
TOOLS=$ROOT/target/ffmpeg-tools
VENV=$TOOLS/venv
# A Windows Python's venv keeps its programs in Scripts.
if [ -n "$WINDOWS" ]; then VENV_BIN=$VENV/Scripts; else VENV_BIN=$VENV/bin; fi
# dav1d is installed here first, then its library is copied into $PREFIX/lib, so
# rebuilding FFmpeg (which empties $PREFIX) doesn't mean rebuilding dav1d.
DAV1D_PREFIX=$TOOLS/dav1d
FORCE=${FFMPEG_FORCE:-}

VIDEO_DECODERS=h264,hevc,mpeg1video,mpeg2video,mpeg4,msmpeg4v1,msmpeg4v2,msmpeg4v3,\
wmv1,wmv2,wmv3,vc1,h263,h263i,h263p,flv,vp6,vp6a,vp6f,vp8,vp9,theora,\
rv10,rv20,rv30,rv40,mjpeg,mjpegb,dvvideo,prores,dnxhd,cinepak,huffyuv,ffvhuff,ffv1,\
rawvideo,indeo3,indeo4,indeo5,svq1,svq3,msvideo1,msrle,qtrle,utvideo,magicyuv,png,libdav1d
AUDIO_DECODERS=aac,aac_latm,mp1,mp1float,mp2,mp2float,mp3,mp3float,ac3,eac3,dca,truehd,mlp,\
vorbis,opus,flac,alac,wmav1,wmav2,wmapro,wmalossless,cook,ra_144,ra_288,sipr,atrac3,\
nellymoser,amrnb,amrwb,ape,adpcm_ms,adpcm_ima_wav,adpcm_ima_qt,adpcm_swf,gsm_ms,\
pcm_s8,pcm_u8,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be,\
pcm_f32le,pcm_f32be,pcm_f64le,pcm_alaw,pcm_mulaw,pcm_bluray,pcm_dvd
DEMUXERS=matroska,av1,obu,avi,asf,flv,live_flv,ogg,mpegts,mpegps,mpegvideo,mov,rm,\
h264,hevc,m4v,vc1,ivf,nut,dv,mxf,yuv4mpegpipe,\
aac,mp3,flac,wav,w64,aiff,ac3,eac3,dts,truehd,amr,ape

# Pinned meson, ninja and pkgconf in a venv under target/ (nothing system-wide).
TOOLS_STAMP="meson $MESON_VERSION ninja $NINJA_VERSION pkgconf $PKGCONF_VERSION"
if [ "$(cat "$VENV/stamp" 2>/dev/null)" != "$TOOLS_STAMP" ]; then
  rm -rf "$VENV"
  mkdir -p "$TOOLS"
  if [ -n "$WINDOWS" ]; then "${PYTHON:-python}" -m venv "$VENV"; else "${PYTHON:-python3}" -m venv "$VENV"; fi
  cat > "$TOOLS/requirements.txt" <<REQ
meson==$MESON_VERSION --hash=sha256:$MESON_SHA256
ninja==$NINJA_VERSION --hash=sha256:$NINJA_SHA256 --hash=sha256:$NINJA_SHA256_LINUX --hash=sha256:$NINJA_SHA256_WINDOWS
pkgconf==$PKGCONF_VERSION --hash=sha256:$PKGCONF_SHA256 --hash=sha256:$PKGCONF_SHA256_LINUX --hash=sha256:$PKGCONF_SHA256_WINDOWS
colorama==$COLORAMA_VERSION; sys_platform == "win32" --hash=sha256:$COLORAMA_SHA256
REQ
  "$VENV_BIN/python" -m pip install --quiet --disable-pip-version-check --no-cache-dir \
    --only-binary :all: --require-hashes -r "$TOOLS/requirements.txt"
  echo "$TOOLS_STAMP" > "$VENV/stamp"
fi
PATH="$VENV_BIN:$PATH"
export PATH

# On Windows the DLLs must not need libgcc_s, libwinpthread or zlib1. -static would also refuse the
# import libraries the DLLs link each other with, so ld finds static copies here first instead.
if [ -n "$WINDOWS" ]; then
  STATIC_LIBS=$TOOLS/static-libs
  rm -rf "$STATIC_LIBS"
  mkdir -p "$STATIC_LIBS"
  for lib in libz.a libwinpthread.a libpthread.a; do
    cp "$(gcc -print-file-name="$lib")" "$STATIC_LIBS/$lib"
  done
  STATIC_LDFLAGS="-L$(cygpath -m "$STATIC_LIBS") -static-libgcc"
  STATIC_LDFLAGS_NOTE="--bindir=lib -Dc_link_args=-L<repo>/target/ffmpeg-tools/static-libs -static-libgcc"
fi

# dav1d, rebuilt only when missing or when its pinned version (or the macOS it is for) changes.
DAV1D_STAMP="dav1d $DAV1D_VERSION $DAV1D_SHA256$TARGET_STAMP"
if [ -n "$FORCE" ] || [ "$(cat "$DAV1D_PREFIX/stamp" 2>/dev/null)" != "$DAV1D_STAMP" ] \
  || [ ! -f "$DAV1D_PREFIX/lib/$DAV1D_LIB" ]; then
  rm -rf "$BUILD" "$DAV1D_PREFIX"
  mkdir -p "$BUILD"
  cd "$BUILD"
  curl -fsSL -o "dav1d-$DAV1D_VERSION.tar.xz" "$DAV1D_URL"
  echo "$DAV1D_SHA256  dav1d-$DAV1D_VERSION.tar.xz" | $SHA256SUM -c -
  tar -xJf "dav1d-$DAV1D_VERSION.tar.xz"
  # On Windows the DLL goes in lib, as the other systems' library does, not in bin.
  if [ -n "$WINDOWS" ]; then
    set -- --bindir=lib "-Dc_link_args=$STATIC_LDFLAGS"
  else
    set --
  fi
  # Meson gives the library an @rpath/libdav1d.7.dylib install name, like FFmpeg's.
  CC=$DAV1D_CC meson setup "$@" "dav1d-$DAV1D_VERSION/build" "dav1d-$DAV1D_VERSION" \
    --prefix="$DAV1D_PREFIX" --libdir=lib --buildtype=release \
    -Ddefault_library=shared -Denable_asm=true -Denable_tools=false -Denable_tests=false \
    -Denable_examples=false -Denable_docs=false
  ninja -C "dav1d-$DAV1D_VERSION/build"
  ninja -C "dav1d-$DAV1D_VERSION/build" install
  cp "dav1d-$DAV1D_VERSION/COPYING" "$DAV1D_PREFIX/COPYING"
  if [ "$(uname -s)" = Darwin ]; then
    otool -D "$DAV1D_PREFIX/lib/$DAV1D_LIB" | grep -q "^@rpath/$DAV1D_LIB\$" \
      || install_name_tool -id "@rpath/$DAV1D_LIB" "$DAV1D_PREFIX/lib/$DAV1D_LIB"
  fi
  echo "$DAV1D_STAMP" > "$DAV1D_PREFIX/stamp"
  # dav1d changed, so FFmpeg links it again.
  FORCE=1
fi

# FFmpeg, rebuilt only when missing or when its version, dav1d or this script changes.
FFMPEG_STAMP="ffmpeg $VERSION $SHA256 $DAV1D_STAMP script $($SHA256SUM "$ROOT/scripts/build-ffmpeg.sh" | cut -c1-16)"
if [ -z "$FORCE" ] && [ "$(cat "$PREFIX/.stamp" 2>/dev/null)" = "$FFMPEG_STAMP" ] \
  && [ -f "$PREFIX/lib/$AVCODEC_LIB" ] && [ -f "$PREFIX/lib/$AVFILTER_LIB" ] && [ -f "$PREFIX/lib/$DAV1D_LIB" ]; then
  echo "FFmpeg $VERSION with dav1d $DAV1D_VERSION is already installed in $PREFIX"
  exit 0
fi

rm -rf "$BUILD"
mkdir -p "$BUILD"
cd "$BUILD"
if [ -n "${FFMPEG_TARBALL:-}" ]; then
  cp "$FFMPEG_TARBALL" "ffmpeg-$VERSION.tar.xz"
else
  curl -fsSL --connect-timeout 20 --retry 2 -o "ffmpeg-$VERSION.tar.xz" "$URL" \
    || curl -fsSL --connect-timeout 20 --retry 2 -o "ffmpeg-$VERSION.tar.xz" "$MIRROR_URL"
fi
echo "$SHA256  ffmpeg-$VERSION.tar.xz" | $SHA256SUM -c -
tar -xJf "ffmpeg-$VERSION.tar.xz"
cd "ffmpeg-$VERSION"

# Libraries carry an @rpath install name and an @loader_path rpath ($ORIGIN off
# macOS), so one can be loaded by absolute path from target/ffmpeg/lib or from the
# app's own folder and still find its siblings. Windows has neither: falcon asks the loader
# to look in the folder the DLL is in (crates/falcon-ffmpeg/src/ffi.rs).
if [ "$(uname -s)" = Darwin ]; then
  set -- --install-name-dir=@rpath --extra-ldflags=-Wl,-rpath,@loader_path --cc=clang --arch=arm64 --disable-x86asm
elif [ -n "$WINDOWS" ]; then
  # DLLs would go in bin. Threads use Win32's own, not winpthread.
  set -- --shlibdir="$PREFIX/lib" --extra-ldflags="$STATIC_LDFLAGS" --enable-w32threads
else
  # Escaped for configure's eval, make's two expansions, then the shell.
  set -- '--extra-ldsoflags=-Wl,-rpath,\\\$\$\$\$ORIGIN'
fi
PKG_CONFIG_PATH="$DAV1D_PREFIX/lib/pkgconfig" ./configure \
  "$@" \
  --prefix="$PREFIX" \
  --enable-shared \
  --disable-static \
  --disable-programs \
  --disable-doc \
  --disable-debug \
  --disable-avdevice \
  --disable-postproc \
  --disable-filters \
  --enable-filter=atempo \
  --disable-encoders \
  --disable-muxers \
  --disable-network \
  --disable-autodetect \
  --enable-zlib \
  --enable-libdav1d \
  --pkg-config=pkgconf \
  --disable-protocols \
  --enable-protocol=file \
  --disable-decoders \
  --enable-decoder="$VIDEO_DECODERS,$AUDIO_DECODERS" \
  --disable-demuxers \
  --enable-demuxer="$DEMUXERS"

if grep -q '^#define CONFIG_GPL 1' config.h || grep -q '^#define CONFIG_NONFREE 1' config.h; then
  echo "FFmpeg was configured with GPL or nonfree parts; refusing to build" >&2
  exit 1
fi
grep -E '^#define CONFIG_(GPL|NONFREE|VERSION3|AVFILTER) ' config.h
grep -q '^#define CONFIG_GPL 0' config.h || { echo "FFmpeg's GPL switch isn't off" >&2; exit 1; }
grep -q '^#define CONFIG_AVFILTER 1' config.h || { echo "FFmpeg was configured without libavfilter" >&2; exit 1; }
grep -q '^#define CONFIG_ATEMPO_FILTER 1' config_components.h || { echo "FFmpeg was configured without atempo" >&2; exit 1; }
[ "$(sed -n 's/^#define CONFIG_\(.*\)_FILTER 1$/\1/p' config_components.h)" = ATEMPO ] \
  || { echo "FFmpeg was configured with filters other than atempo" >&2; exit 1; }
if grep -Eq '^#define CONFIG_[A-Z0-9_]+_(ENCODER|MUXER) 1$' config_components.h; then
  echo "FFmpeg was configured with encoders or muxers; refusing to build" >&2
  exit 1
fi
grep -q '^#define CONFIG_LIBDAV1D_DECODER 1' config_components.h || { echo "FFmpeg was configured without libdav1d" >&2; exit 1; }
if [ -n "$WINDOWS" ]; then
  grep -q '^#define HAVE_W32THREADS 1' config.h || { echo "FFmpeg was configured without Win32 threads" >&2; exit 1; }
fi

make -j"$JOBS"
rm -rf "$PREFIX"
make install
cp COPYING.LGPLv2.1 "$PREFIX/COPYING.LGPLv2.1"
# libavcodec links @rpath/libdav1d.7.dylib and finds it next to itself through its
# @loader_path rpath, here and in Falcon.app/Contents/Frameworks.
cp "$DAV1D_PREFIX/lib/$DAV1D_LIB" "$PREFIX/lib/$DAV1D_LIB"
cp "$DAV1D_PREFIX/COPYING" "$PREFIX/COPYING.dav1d"
if [ "$(uname -s)" = Darwin ]; then
  otool -L "$PREFIX/lib/$AVCODEC_LIB" | grep -q "@rpath/$DAV1D_LIB" \
    || { echo "libavcodec doesn't link @rpath/$DAV1D_LIB" >&2; exit 1; }
elif [ -n "$WINDOWS" ]; then
  objdump -p "$PREFIX/lib/$AVCODEC_LIB" | grep -q "DLL Name: $DAV1D_LIB" \
    || { echo "avcodec doesn't link $DAV1D_LIB" >&2; exit 1; }
  # Each DLL may need its siblings and what every Windows has: nothing from the build machine.
  for dll in "$PREFIX"/lib/*.dll; do
    for dep in $(objdump -p "$dll" | sed -n 's/^[[:space:]]*DLL Name: //p'); do
      case "$(echo "$dep" | tr '[:upper:]' '[:lower:]')" in
        avutil-59.dll | avcodec-61.dll | avformat-61.dll | avfilter-10.dll | swscale-8.dll | swresample-5.dll | "$DAV1D_LIB") ;;
        kernel32.dll | advapi32.dll | bcrypt.dll | ole32.dll | user32.dll | shell32.dll | ws2_32.dll | secur32.dll) ;;
        api-ms-win-crt-*.dll) ;;
        *) echo "${dll##*/} needs $dep, which a clean Windows lacks" >&2; exit 1 ;;
      esac
    done
  done
else
  readelf -d "$PREFIX/lib/$AVCODEC_LIB" | grep -q "$DAV1D_LIB" \
    || { echo "libavcodec doesn't link $DAV1D_LIB" >&2; exit 1; }
fi
# What was built and how, for the notice that ships next to the libraries
# (the install prefix is left out of the recorded configure line).
CONF=$(sed -n 's/^#define FFMPEG_CONFIGURATION "\(.*\)"$/\1/p' config.h | sed -e 's|--prefix=[^ ]*|--prefix=<repo>/target/ffmpeg|' \
  -e 's|--shlibdir=[^ ]*|--shlibdir=<repo>/target/ffmpeg/lib|' -e "s|--extra-ldflags='-L[^ ]*|--extra-ldflags='-L<repo>/target/ffmpeg-tools/static-libs|")
cat > "$PREFIX/BUILD-INFO.txt" <<INFO
FFmpeg $VERSION, built for Falcon by scripts/build-ffmpeg.sh.
Licence: GNU Lesser General Public License, version 2.1 or later (COPYING.LGPLv2.1).
Source: $URL
SHA-256 of the source: $SHA256
Falcon loads these libraries at run time and does not modify FFmpeg. They decode, and
libavfilter has one filter, atempo, for keeping the pitch at other playback speeds.
Configured with:
$CONF

dav1d $DAV1D_VERSION (lib/$DAV1D_LIB), the AV1 decoder libavcodec links (decoder libdav1d).
Licence: BSD 2-Clause (COPYING.dav1d).
Source: $DAV1D_URL
SHA-256 of the source: $DAV1D_SHA256
Built with meson $MESON_VERSION and ninja $NINJA_VERSION:
-Ddefault_library=shared -Denable_asm=true -Denable_tools=false -Denable_tests=false -Denable_examples=false -Denable_docs=false --buildtype=release
INFO
if [ -n "$WINDOWS" ]; then
  # A little of winpthreads ends up inside avutil, through the C runtime's clock functions, and its
  # licence asks for its text to ship along.
  cp "$(dirname "$(dirname "$(command -v gcc)")")/share/licenses/winpthreads/COPYING" "$PREFIX/COPYING.winpthreads"
  cat >> "$PREFIX/BUILD-INFO.txt" <<INFO

Both were built on Windows with $(gcc --version | head -n 1), with libgcc (GCC Runtime Library
Exception), zlib (zlib licence, https://zlib.net) and winpthreads (COPYING.winpthreads) linked
into the DLLs. dav1d also: $STATIC_LDFLAGS_NOTE
INFO
fi
echo "$FFMPEG_STAMP" > "$PREFIX/.stamp"
echo "FFmpeg $VERSION with dav1d $DAV1D_VERSION installed in $PREFIX"
