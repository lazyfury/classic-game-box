#!/bin/bash
# ---------------------------------------------------------------------------
# Build mGBA as a native macOS libretro core.
#
# Output: cores/dist/mgba_libretro.dylib
#
# ---------------------------------------------------------------------------
# STATUS: DEFERRED to Q4 (not yet verified).
#
# Two facts found while trying to run this:
#   * upstream `libretro/mgba` no longer ships a Makefile.libretro; it builds
#     the core through CMake with -DBUILD_LIBRETRO=ON, which needs cmake.
#   * the EmulatorJS/mgba fork still ships the Makefile.libretro + osx target
#     (the same fork legacy/wasm/mgba/build.sh used), and needs no cmake.
# This script targets the fork. It has NOT been run end to end yet, because
# cloning the fork needs the network and it was not exercised in Q1.
# ---------------------------------------------------------------------------
#
# mGBA runs Game Boy, Game Boy Color and Game Boy Advance.
#
# Two patches carried over from the wasm script, both still needed here:
#   * remove -DCOLOR_16_BIT  -> render XRGB8888, matching the host and Mesen.
#     NOTE: the upstream CMake build hardcodes COLOR_16_BIT|COLOR_5_6_5, so
#     when this core is picked up the host must accept RGB565 — cgb-libretro
#     already converts it, but its environment handler must be made to accept
#     it too (it currently only accepts XRGB8888).
#   * remove -DHAVE_CRC32    -> let mGBA's crc32.c carry its own implementation
#     (nothing here links zlib).
#
# Usage:
#   ./cores/mgba/build.sh
#   MGBA_SRC=/path/to/mgba ./cores/mgba/build.sh
# ---------------------------------------------------------------------------
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${MGBA_SRC:-$ROOT/cores/sources/mgba}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning EmulatorJS/mgba into $SRC"
    git clone --depth 1 https://github.com/EmulatorJS/mgba "$SRC"
fi

cd "$SRC"

COMMON="libretro-build/Makefile.common"
if [ -f "$COMMON" ]; then
    if grep -q 'DCOLOR_16_BIT' "$COMMON"; then
        echo "==> patching $COMMON (32-bit colour)"
        sed -i.bak 's/ -DCOLOR_16_BIT//' "$COMMON"
    fi
    if grep -q 'RETRODEFS += -DHAVE_CRC32' "$COMMON"; then
        echo "==> patching $COMMON (own crc32)"
        sed -i.bak '/RETRODEFS += -DHAVE_CRC32/d' "$COMMON"
    fi
fi

echo "==> building mGBA (platform=osx, $(uname -m))"
make -f Makefile.libretro platform=osx -j"$JOBS"

BUILT="$(find "$SRC" -name '*_libretro.dylib' -maxdepth 2 | head -1)"
if [ -z "$BUILT" ]; then
    echo "error: no *_libretro.dylib produced" >&2
    exit 1
fi
cp "$BUILT" "$OUT/mgba_libretro.dylib"
echo "==> done: $OUT/mgba_libretro.dylib"
