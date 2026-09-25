#!/bin/bash
# ---------------------------------------------------------------------------
# Build PicoDrive as a native macOS libretro core.
#
# Output: cores/dist/picodrive_libretro.dylib
# Declared in cores/cores.json under key `picodrive`, for the consoles
# `genesis` (Mega Drive / Genesis), `sms` (Master System), `gg` (Game Gear) and
# `sg1000` (SG-1000) — one module, four manifest rows, exactly like Genesis
# Plus GX and mGBA.
#
# PicoDrive is the lightweight alternative to Genesis Plus GX: it also covers
# 32X and Sega/Mega CD (which this app does not model yet). It builds from
# `Makefile.libretro` at the libretro fork root with `platform=osx` and renders
# RGB565, which the host accepts and converts.
#
# The libretro fork pulls libpicofe / cyclone / libchdr / dr_libs as git
# submodules, so the clone recurses into them.
#
# Usage:
#   ./cores/picodrive/build.sh
#   PICODRIVE_SRC=/path/to/source ./cores/picodrive/build.sh
#
# Requires: network (once), Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${PICODRIVE_SRC:-$ROOT/cores/sources/picodrive}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/picodrive into $SRC"
    # No --shallow-submodules: this fork pins submodule commits that are not
    # the submodule branch tip, and a depth-1 shallow clone cannot reach them
    # (libchdr fails with "Unable to find current revision").
    git clone --depth 1 --recurse-submodules \
        https://github.com/libretro/picodrive "$SRC"
fi

# A pre-existing checkout (or one made before this script) may still have empty
# submodule directories; make sure they are populated. Harmless when they are.
git -C "$SRC" submodule update --init --recursive

echo "==> building PicoDrive libretro core (platform=osx, $(uname -m))"
# PicoDrive does not derive its deployment target from the macOS minor version,
# so it needs no MINVERSION pin (unlike Nestopia).
make -C "$SRC" -f Makefile.libretro platform=osx -j"$JOBS"

cp "$SRC/picodrive_libretro.dylib" "$OUT/picodrive_libretro.dylib"
echo "==> done: $OUT/picodrive_libretro.dylib"
