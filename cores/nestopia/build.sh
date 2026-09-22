#!/bin/bash
# ---------------------------------------------------------------------------
# Build Nestopia (NES / FC) as a custom libretro core.
#
# Output: cores/dist/nestopia_libretro.dylib
# Declared in cores/cores.json as key `nestopia`.
#
# This is the worked example of the custom-core flow: clone, build with the
# project's own libretro Makefile, copy the module into cores/dist. See
# cores/README.md.
#
# Usage:
#   ./cores/nestopia/build.sh
#   NESTOPIA_SRC=/path/to/nestopia ./cores/nestopia/build.sh
#
# Requires: network (once), Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${NESTOPIA_SRC:-$ROOT/cores/sources/nestopia}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/nestopia into $SRC"
    git clone --depth 1 https://github.com/libretro/nestopia "$SRC"
fi

echo "==> building Nestopia (platform=osx, $(uname -m))"
# Nestopia derives its deployment target from the *minor* macOS version and
# falls back to 10.4 when that parses as a single-digit number (macOS 26/27
# read as "0"), which arm64 rejects. Pin a floor arm64 accepts.
make -C "$SRC/libretro" -f Makefile platform=osx -j"$JOBS" MINVERSION=-mmacosx-version-min=11.0

cp "$SRC/libretro/nestopia_libretro.dylib" "$OUT/nestopia_libretro.dylib"
echo "==> done: $OUT/nestopia_libretro.dylib"
