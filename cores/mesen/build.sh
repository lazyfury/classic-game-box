#!/bin/bash
# ---------------------------------------------------------------------------
# Build Mesen as a native macOS libretro core.
#
# Output: cores/dist/mesen_libretro.dylib
#
# Mesen is the NES core. It is a third-party project with its own libretro
# Makefile and an `osx` platform target, so it is cloned and built on demand
# instead of vendored. This replaces the old wasm/mesen build script, which
# built the same core for Emscripten.
#
# The two wasm-only patches are deliberately absent: with a real filesystem
# Mesen can open disksys.rom / MesenDB.txt itself, and native builds already
# have C++ exceptions on.
#
# Usage:
#   ./cores/mesen/build.sh
#   MESEN_SRC=/path/to/Mesen ./cores/mesen/build.sh
#
# Requires: network (once), Xcode command line tools, and an arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${MESEN_SRC:-$ROOT/cores/sources/Mesen}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/Mesen into $SRC"
    git clone --depth 1 https://github.com/libretro/Mesen "$SRC"
fi

echo "==> building Mesen (platform=osx, $(uname -m))"
cd "$SRC/Libretro"
make -f Makefile platform=osx -j"$JOBS"

# The Makefile names the product after its TARGET_NAME; copy whatever .dylib it
# produced under the name the app looks for.
BUILT="$(find "$SRC" -name '*_libretro.dylib' -maxdepth 3 | head -1)"
if [ -z "$BUILT" ]; then
    echo "error: no *_libretro.dylib produced" >&2
    exit 1
fi
cp "$BUILT" "$OUT/mesen_libretro.dylib"
echo "==> done: $OUT/mesen_libretro.dylib"
