#!/bin/bash
# ---------------------------------------------------------------------------
# Build Snes9x (Super Nintendo / SFC) as a native macOS arm64 libretro core.
#
# Output:   cores/dist/snes9x_libretro.dylib
# Manifest: cores/cores.json, key `snes9x`, system `snes`.
#
# Snes9x is software-rendered: it asks for RETRO_PIXEL_FORMAT_RGB565 (the host
# converts it to RGBA8) and never calls SET_HW_RENDER, so it rides the plain
# frame path — no OpenGL. SuperFX / SA-1 / CX4 / SDD-1 / SPC7110 / MSU-1 and
# the DSP-1..4 mixers are all emulated in-tree (the DSP firmware table is
# compiled in), so a regular SNES game needs no BIOS or firmware ROM. Only BS-X
# Satellaview (`BS-X.bin`) and Sufami Turbo (`STBIOS.bin`) want an optional
# BIOS in the app's system directory; the app does not expose those extensions.
#
# The libretro Makefile lives in `libretro/`, not the repo root, and has no git
# submodules — `libretro-common` is vendored. It has no deployment-target quirk
# on arm64 (`MINVERSION` is empty unless the macOS minor version is <= 9).
#
# Usage:
#   ./cores/snes9x/build.sh
#   SNES9X_SRC=/path/to/snes9x ./cores/snes9x/build.sh
#
# Requires: network (once), Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${SNES9X_SRC:-$ROOT/cores/sources/snes9x}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/snes9x into $SRC"
    git clone --depth 1 https://github.com/libretro/snes9x "$SRC"
fi

echo "==> building Snes9x (platform=osx, $(uname -m))"
make -C "$SRC/libretro" platform=osx -j"$JOBS"

cp "$SRC/libretro/snes9x_libretro.dylib" "$OUT/snes9x_libretro.dylib"
echo "==> done: $OUT/snes9x_libretro.dylib"
