#!/bin/sh
# Builds the LGPL, decode-only FFmpeg that falcon's fallback video player loads at run time.
#
# Downloads the release tarball, checks it against a pinned SHA-256, configures it without
# GPL or nonfree parts, without encoders, muxers, programs, avdevice, avfilter or postproc,
# and installs shared libraries and headers into target/ffmpeg (never system-wide).
# Nothing it produces is committed. scripts/bundle.sh copies the libraries into Falcon.app.
#
# Environment:
#   FFMPEG_PREFIX     install prefix (default: <repo>/target/ffmpeg)
#   FFMPEG_BUILD_DIR  empty scratch directory for the download and the build
#                     (default: <repo>/target/ffmpeg-build, wiped first)
set -eu

VERSION=7.1.5
SHA256=de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f
URL="https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz"

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PREFIX=${FFMPEG_PREFIX:-$ROOT/target/ffmpeg}
BUILD=${FFMPEG_BUILD_DIR:-$ROOT/target/ffmpeg-build}

VIDEO_DECODERS=h264,hevc,mpeg1video,mpeg2video,mpeg4,msmpeg4v1,msmpeg4v2,msmpeg4v3,\
wmv1,wmv2,wmv3,vc1,h263,h263i,h263p,flv,vp6,vp6a,vp6f,vp8,vp9,theora,\
rv10,rv20,rv30,rv40,mjpeg,mjpegb,dvvideo,prores,dnxhd,cinepak,huffyuv,ffvhuff,ffv1,\
rawvideo,indeo3,indeo4,indeo5,svq1,svq3,msvideo1,msrle,qtrle,utvideo,magicyuv,png
AUDIO_DECODERS=aac,aac_latm,mp1,mp1float,mp2,mp2float,mp3,mp3float,ac3,eac3,dca,truehd,mlp,\
vorbis,opus,flac,alac,wmav1,wmav2,wmapro,wmalossless,cook,ra_144,ra_288,sipr,atrac3,\
nellymoser,amrnb,amrwb,ape,adpcm_ms,adpcm_ima_wav,adpcm_ima_qt,adpcm_swf,gsm_ms,\
pcm_s8,pcm_u8,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be,\
pcm_f32le,pcm_f32be,pcm_f64le,pcm_alaw,pcm_mulaw,pcm_bluray,pcm_dvd
DEMUXERS=matroska,avi,asf,flv,live_flv,ogg,mpegts,mpegps,mpegvideo,mov,rm,\
h264,hevc,m4v,vc1,ivf,nut,dv,mxf,yuv4mpegpipe,\
aac,mp3,flac,wav,w64,aiff,ac3,eac3,dts,truehd,amr,ape

rm -rf "$BUILD"
mkdir -p "$BUILD"
cd "$BUILD"
curl -fsSL -o "ffmpeg-$VERSION.tar.xz" "$URL"
echo "$SHA256  ffmpeg-$VERSION.tar.xz" | shasum -a 256 -c -
tar -xJf "ffmpeg-$VERSION.tar.xz"
cd "ffmpeg-$VERSION"

# Libraries carry an @rpath install name and an @loader_path rpath, so one can be
# loaded by absolute path from target/ffmpeg/lib or from Falcon.app/Contents/Frameworks
# and still find its siblings.
./configure \
  --prefix="$PREFIX" \
  --install-name-dir=@rpath \
  --extra-ldflags=-Wl,-rpath,@loader_path \
  --cc=clang \
  --arch=arm64 \
  --enable-shared \
  --disable-static \
  --disable-programs \
  --disable-doc \
  --disable-debug \
  --disable-avdevice \
  --disable-postproc \
  --disable-avfilter \
  --disable-encoders \
  --disable-muxers \
  --disable-network \
  --disable-autodetect \
  --enable-zlib \
  --disable-x86asm \
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
grep -E '^#define CONFIG_(GPL|NONFREE|VERSION3) ' config.h

make -j"$(sysctl -n hw.ncpu)"
rm -rf "$PREFIX"
make install
cp COPYING.LGPLv2.1 "$PREFIX/COPYING.LGPLv2.1"
# What was built and how, for the notice that ships next to the libraries
# (the install prefix is left out of the recorded configure line).
CONF=$(sed -n 's/^#define FFMPEG_CONFIGURATION "\(.*\)"$/\1/p' config.h | sed 's|--prefix=[^ ]*|--prefix=<repo>/target/ffmpeg|')
cat > "$PREFIX/BUILD-INFO.txt" <<INFO
FFmpeg $VERSION, built for Falcon by scripts/build-ffmpeg.sh.
Licence: GNU Lesser General Public License, version 2.1 or later (COPYING.LGPLv2.1).
Source: $URL
SHA-256 of the source: $SHA256
Falcon loads these libraries at run time and does not modify FFmpeg.
Configured with:
$CONF
INFO
echo "FFmpeg $VERSION installed in $PREFIX"
