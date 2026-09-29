#!/bin/bash
# ---------------------------------------------------------------------------
# Build ParaLLEl-N64 (Nintendo 64) as a native macOS libretro core.
#
# Output: cores/dist/parallel_n64_libretro.dylib
# Declared in cores/cores.json under key `parallel_n64`, system `n64`.
#
# Why this core and not Mupen64Plus-Next: on macOS 26 / Apple Silicon,
# Mupen64Plus-Next's GLideN64 renders black (reproduced in RetroArch too), and
# its software Angrylion is black as well. ParaLLEl-N64's GLideN64 renders
# correctly through the same front-end GL path, and it ships a working arm64
# dynarec, so it is both correct and fast here. See
# docs/architecture/n64-gl-hw-render-plan.md §7.2.
#
# `HAVE_PARALLEL=0` / `HAVE_PARALLEL_RSP=0`: drop the Vulkan parallel-rdp/RSP
# backends. The front end has no Vulkan path; GLideN64 (the default
# `gfxplugin`) is what we use.
#
# Upstream vendors mupen64plus-core / GLideN64 / Angrylion / the RSP plugins in
# the tree (no submodules), so a plain clone is enough.
#
# Usage:
#   ./cores/parallel_n64/build.sh
#   PARALLEL_N64_SRC=/path/to/source ./cores/parallel_n64/build.sh
#
# Requires: network (once), Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${PARALLEL_N64_SRC:-$ROOT/cores/sources/parallel_n64}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/parallel-n64 into $SRC"
    git clone --depth 1 https://github.com/libretro/parallel-n64 "$SRC"
fi

echo "==> building ParaLLEl-N64 libretro core (platform=osx, $(uname -m))"
# GLideN64 (the default gfxplugin) + arm64 dynarec; no Vulkan backends.
make -C "$SRC" platform=osx HAVE_PARALLEL=0 HAVE_PARALLEL_RSP=0 -j"$JOBS"

cp "$SRC/parallel_n64_libretro.dylib" "$OUT/parallel_n64_libretro.dylib"
echo "==> done: $OUT/parallel_n64_libretro.dylib"
