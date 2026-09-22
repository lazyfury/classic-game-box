#!/bin/bash
# ---------------------------------------------------------------------------
# Build mGBA (GB / GBC / GBA) as a native macOS libretro core.
#
# Output: cores/dist/mgba_libretro.dylib
#
# Source: upstream libretro/mgba (a CMake project; there is no Makefile any
# more). The libretro target needs `-DBUILD_LIBRETRO=ON`.
#
# Note on pixel format: upstream hardcodes `COLOR_16_BIT;COLOR_5_6_5`, so the
# core renders **RGB565** and asks the front end for it via
# RETRO_ENVIRONMENT_SET_PIXEL_FORMAT. The host accepts both XRGB8888 and RGB565
# (see cgb-libretro/src/host.rs), so no source patch is needed. The old
# EmulatorJS-based script patched COLOR_16_BIT out to force XRGB8888; that is
# no longer required.
#
# Usage:
#   ./cores/mgba/build.sh
#   MGBA_SRC=/path/to/mgba ./cores/mgba/build.sh
#
# Requires: cmake, network (once), Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${MGBA_SRC:-$ROOT/cores/sources/mgba}"
BUILD="$SRC/build"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/mgba into $SRC"
    git clone --depth 1 https://github.com/libretro/mgba "$SRC"
fi

echo "==> configuring mGBA libretro core (cmake)"
cmake -S "$SRC" -B "$BUILD" \
    -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_LIBRETRO=ON \
    -DBUILD_QT=OFF -DBUILD_SDL=OFF \
    -DBUILD_TEST=OFF -DBUILD_SUITE=OFF \
    -DBUILD_GL=OFF -DBUILD_GLES2=OFF -DBUILD_GLES3=OFF \
    -DUSE_LUA=OFF -DUSE_JSON_C=OFF -DUSE_ELF=OFF -DUSE_DISCORD_RPC=OFF

echo "==> building mGBA libretro core"
cmake --build "$BUILD" --target mgba_libretro -j"$JOBS"

cp "$BUILD/mgba_libretro.dylib" "$OUT/mgba_libretro.dylib"
echo "==> done: $OUT/mgba_libretro.dylib"
