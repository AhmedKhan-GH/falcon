#!/bin/sh
# Fetches the web-ifc geometry engine and the header-only libraries it builds against, for
# the falcon-ifc crate, which compiles them with the `cc` crate when they are present.
#
# Every archive is a GitHub source tarball at a pinned tag or commit and is checked against
# a pinned SHA-256 before it is unpacked. The sources land in target/web-ifc (never
# committed); nothing is installed system-wide and no CMake is needed.
#
# web-ifc is MPL-2.0; its source stays available from the URL below. The libraries:
# glm (MIT), tinynurbs (BSD-3-Clause), fast_float (MIT OR Apache-2.0 OR BSL-1.0),
# earcut.hpp (ISC), CDT (MPL-2.0), spdlog (MIT), stduuid (MIT), unordered_dense (MIT).
#
# Environment:
#   WEB_IFC_DIR   where the sources go (default: <repo>/target/web-ifc, wiped first)
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEST=${WEB_IFC_DIR:-$ROOT/target/web-ifc}

# name  repository  tag-or-commit  sha256
SOURCES='web-ifc ThatOpen/engine_web-ifc 0.78 cccd3bbe4058c2c2c118ae40df31f8cfde1b1850f88a031fdbbf23fd6494ae8d
fast_float fastfloat/fast_float b0ab987b3dfdde13fa1915f65ef2a5c068d9208c 3abf993dc066593bdef1b046da615338c70ff563e3640fda8cf1c22ba185d364
tinynurbs QuimMoya/tinynurbs 47115cd9b6e922b27bbc4ab01fdeac2e9ea597a4 1b65bb6598f7af0cc4283f89798688065fd916549e49e5f6679c944a36b26e43
glm g-truc/glm 8d1fd52e5ab5590e2c81768ace50c72bae28f2ed 36eefbae41503e6822e8d9e1b4cc85c6a6621f17676f6258aaf0a64c2de0a09c
earcut mapbox/earcut.hpp c68c8835ccff2b7532d31d8fa8dfcf398f629498 f597583136be63b7d8aa921b7af19621d725e6070d740fddc1b959382e6cad19
cdt artem-ogre/CDT 2068d015b9db3c92481e869b0c1f669b96a1d70a a06e75f930ca1c95be0b024678bc92f3b75cd9ab0879d1e8a3d26f79ebf8d4a9
spdlog gabime/spdlog 79524ddd08a4ec981b7fea76afd08ee05f83755d c6dee844e35a54cc80e3ab76aae2310763e0b49c7397549dfa4d4657d9737138
stduuid mariusbancila/stduuid 3afe7193facd5d674de709fccc44d5055e144d7a e11f9bf30c7f9c03d8e9a3a3fd7fe016eb5d8d9b89a2fe2c11b5f049e1d97916
unordered_dense martinus/unordered_dense 1c8636e810d7e4a485d17bb9cbd6dccb5fe4e284 b21b3d002c1d5a7c3987763ec92361949e93130b0d39ccd03c55f79d83d84a2f'

rm -rf "$DEST"
mkdir -p "$DEST/archives"
echo "$SOURCES" | while read -r name repo rev sha; do
  file="$DEST/archives/$name.tar.gz"
  # A tag is fetched as refs/tags/<tag>, a commit by its hash.
  case "$rev" in
    *.*) url="https://github.com/$repo/archive/refs/tags/$rev.tar.gz" ;;
    *) url="https://github.com/$repo/archive/$rev.tar.gz" ;;
  esac
  curl -fsSL -o "$file" "$url"
  echo "$sha  $file" | shasum -a 256 -c - >/dev/null
  mkdir -p "$DEST/$name"
  tar -xzf "$file" -C "$DEST/$name" --strip-components=1
  echo "$name $repo $rev $url" >>"$DEST/SOURCES.txt"
done
rm -rf "$DEST/archives"
echo "web-ifc sources are in $DEST"
