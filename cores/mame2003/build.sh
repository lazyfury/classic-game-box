#!/bin/bash
# ---------------------------------------------------------------------------
# Build MAME 2003-Plus (arcade) as a native macOS libretro core.
#
# Output: cores/dist/mame2003_plus_libretro.dylib
# Declared in cores/cores.json as key `mame2003`, system `arcade`.
#
# MAME 2003-Plus is the tractable member of the MAME family: C, a plain
# Makefile with an `osx` target, and a ~150MB checkout. (Upstream
# `libretro/mame` is the current MAME — Makefile.libretro + genie + python3 and
# a multi-GB source tree; add it the same way if you need current drivers.)
#
# Usage:
#   ./cores/mame2003/build.sh
#   MAME2003_SRC=/path/to/source ./cores/mame2003/build.sh
#
# Requires: network (once), Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${MAME2003_SRC:-$ROOT/cores/sources/mame2003}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/mame2003-plus-libretro into $SRC"
    git clone --depth 1 https://github.com/libretro/mame2003-plus-libretro "$SRC"
fi

# The bundled 2003-era zlib defines `fdopen(fd,mode)` to NULL under
# TARGET_OS_MAC, which the macOS 26+ SDK headers then fail to parse. Drop it.
ZUTIL="$SRC/src/lib/zlib/zutil.h"
if [ -f "$ZUTIL" ] && grep -q 'define fdopen(fd,mode) NULL' "$ZUTIL"; then
    echo "==> patching $ZUTIL (drop the fdopen macro)"
    sed -i.bak '/# *define fdopen(fd,mode) NULL/d' "$ZUTIL"
fi

echo "==> building MAME 2003-Plus (platform=osx, $(uname -m))"
# `fpic` is overridden because the Makefile appends -mmacosx-version-min=10.1 on
# macOS 26+ (it reads the *minor* version as a single digit), which arm64
# rejects. Overriding the variable keeps the append out.
make -C "$SRC" -f Makefile platform=osx fpic=-fPIC -j"$JOBS"

cp "$SRC/mame2003_plus_libretro.dylib" "$OUT/mame2003_plus_libretro.dylib"
echo "==> done: $OUT/mame2003_plus_libretro.dylib"
