#!/bin/bash
# ---------------------------------------------------------------------------
# Build mGBA as a native macOS libretro core.
#
# Output: cores/dist/mgba_libretro.dylib
#
# mGBA runs Game Boy, Game Boy Color and Game Boy Advance. Same story as
# Mesen: clone on demand, build with the in-tree libretro Makefile's `osx`
# target. Replaces `legacy/wasm/mgba/build.sh`.
#
# Two patches carried over from the wasm script, both still needed natively:
#   * remove -DCOLOR_16_BIT  -> render XRGB8888, matching the host and Mesen.
#   * remove -DHAVE_CRC32    -> let mGBA's crc32.c carry its own implementation
#                               (nothing here links zlib).
#
# Usage:
#   ./cores/mgba/build.sh
#   MGBA_SRC=/path/to/mgba ./cores/mgba/build.sh
#
# Requires: network (once), Xcode command line tools, and an arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${MGBA_SRC:-$ROOT/cores/sources/mgba}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/mgba into $SRC"
    git clone --depth 1 https://github.com/libretro/mgba "$SRC"
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
