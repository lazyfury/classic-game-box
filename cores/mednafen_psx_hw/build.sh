#!/bin/bash
# ---------------------------------------------------------------------------
# Build Beetle PSX HW (Sony PlayStation) as a native macOS libretro core.
#
# Output:   cores/dist/mednafen_psx_hw_libretro.dylib
# Manifest: cores/cores.json, key `mednafen_psx_hw`, system `ps1`.
#
# `HAVE_OPENGL=1` builds the hardware renderer (the core then calls itself
# `mednafen_psx_hw`); without it the Makefile builds the software
# `mednafen_psx`. It links desktop OpenGL and rides the front end's offscreen
# CGL path, like ParaLLEl-N64 and PPSSPP. `LIGHTREC_DEBUG=0` keeps lightrec's
# debug build off.
#
# A real PlayStation BIOS is optional: Beetle runs HLE / OpenBIOS without one.
# To use a real BIOS, drop `scph5500.bin` / `scph5501.bin` / `scph5502.bin` into
# the app's system directory (`<app data>/system/`); the firmware scan finds it.
#
# Usage:
#   ./cores/mednafen_psx_hw/build.sh
#   BEETLE_PSX_SRC=/path/to/beetle-psx-libretro ./cores/mednafen_psx_hw/build.sh
#
# Requires: network (once), Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${BEETLE_PSX_SRC:-$ROOT/cores/sources/mednafen_psx_hw}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/beetle-psx-libretro into $SRC"
    git clone --depth 1 https://github.com/libretro/beetle-psx-libretro "$SRC"
fi

echo "==> building Beetle PSX HW (platform=osx, $(uname -m))"
make -C "$SRC" platform=osx HAVE_OPENGL=1 HAVE_VULKAN=0 LIGHTREC_DEBUG=0 -j"$JOBS"

cp "$SRC/mednafen_psx_hw_libretro.dylib" "$OUT/mednafen_psx_hw_libretro.dylib"
echo "==> done: $OUT/mednafen_psx_hw_libretro.dylib"
