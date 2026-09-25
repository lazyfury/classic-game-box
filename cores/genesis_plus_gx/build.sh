#!/bin/bash
# ---------------------------------------------------------------------------
# Build Genesis Plus GX as a native macOS libretro core.
#
# Output: cores/dist/genesis_plus_gx_libretro.dylib
# Declared in cores/cores.json under key `genesis_plus_gx`, for the consoles
# `genesis` (Mega Drive / Genesis), `sms` (Master System), `gg` (Game Gear) and
# `sg1000` (SG-1000) — one module, four manifest rows, exactly like mGBA on
# GBA and GB.
#
# The libretro port lives in the `libretro/Genesis-Plus-GX` fork; it builds
# from `Makefile.libretro` at the repo root with `platform=osx`.
#
# Usage:
#   ./cores/genesis_plus_gx/build.sh
#   GENESIS_PLUS_GX_SRC=/path/to/source ./cores/genesis_plus_gx/build.sh
#
# Requires: network (once), Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${GENESIS_PLUS_GX_SRC:-$ROOT/cores/sources/genesis_plus_gx}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/Genesis-Plus-GX into $SRC"
    git clone --depth 1 https://github.com/libretro/Genesis-Plus-GX "$SRC"
fi

echo "==> building Genesis Plus GX libretro core (platform=osx, $(uname -m))"
# The Makefile may derive its deployment target from the macOS minor version and
# fall back to 10.4 on macOS 26+, which arm64 rejects; MINVERSION pins it.
make -C "$SRC" -f Makefile.libretro platform=osx -j"$JOBS" \
    MINVERSION=-mmacosx-version-min=11.0

cp "$SRC/genesis_plus_gx_libretro.dylib" "$OUT/genesis_plus_gx_libretro.dylib"
echo "==> done: $OUT/genesis_plus_gx_libretro.dylib"
