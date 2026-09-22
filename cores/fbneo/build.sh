#!/bin/bash
# ---------------------------------------------------------------------------
# Build FinalBurn Neo (arcade) as a native macOS libretro core.
#
# Output: cores/dist/fbneo_libretro.dylib
# Declared in cores/cores.json as key `fbneo`, system `arcade`.
#
# FBNeo is the arcade core that matches the common "standard" Neo Geo romsets
# (the encrypted 4 MiB C-ROMs), which MAME 2003-Plus does not — it wants the
# decrypted variants. See cores/README.md for the family overview.
#
# The libretro port lives in the `libretro/FBNeo` fork (upstream
# `finalburnneo/FBNeo` has no libretro Makefile), and builds from
# `src/burner/libretro` with a plain `platform=osx` target.
#
# Usage:
#   ./cores/fbneo/build.sh
#   FBNEO_SRC=/path/to/source ./cores/fbneo/build.sh
#
# Requires: network (once), Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${FBNEO_SRC:-$ROOT/cores/sources/fbneo}"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/FBNeo into $SRC"
    git clone --depth 1 https://github.com/libretro/FBNeo "$SRC"
fi

echo "==> building FBNeo libretro core (platform=osx, $(uname -m))"
make -C "$SRC/src/burner/libretro" platform=osx -j"$JOBS"

cp "$SRC/src/burner/libretro/fbneo_libretro.dylib" "$OUT/fbneo_libretro.dylib"
echo "==> done: $OUT/fbneo_libretro.dylib"
