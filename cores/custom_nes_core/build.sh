#!/bin/bash
# ---------------------------------------------------------------------------
# Build the legacy custom FC / NES core as a native libretro module.
#
# Output: cores/dist/custom_nes_core_libretro.dylib
# Declared in cores/cores.json as key `custom_nes_core`.
#
# Source lives in legacy/packages/{fc-core,fc-libretro} and is **read-only**
# here (AGENTS.md rule 7). That tree is a CMake project, but this machine has
# no cmake and the project chose not to require one, so this script drives
# clang++ directly with the same sources and include paths the CMake targets
# use:
#
#   fc_core          legacy/packages/fc-core/src/core/*.cpp
#   fc_libretro_core legacy/packages/fc-libretro/src/libretro/{fc_libretro,cheat_codes}.cpp
#
# src/ffi/emulator_api.cpp is not part of the core and is not built. The
# private `fc_*` extension (`fc_libretro_get_ext`) is exported but the host
# never uses it: only the standard libretro ABI matters.
#
# Usage: ./cores/custom_nes_core/build.sh
# Requires: Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
CORE="$ROOT/legacy/packages/fc-core"
LIBRETRO="$ROOT/legacy/packages/fc-libretro"

if [ ! -d "$CORE/src/core" ] || [ ! -d "$LIBRETRO/src/libretro" ]; then
    echo "error: legacy/packages/fc-{core,libretro} sources not found under $ROOT" >&2
    exit 1
fi

# The version the core reports, kept in sync with the CMake tree it comes from.
VERSION="$(sed -n 's/.*set(FC_PROJECT_VERSION "\(.*\)").*/\1/p' "$ROOT/legacy/cmake/Version.cmake")"
VERSION="${VERSION:-0.0.0}"

# bash 3.2 (macOS) has no `mapfile`; collect the core sources in a loop.
SOURCES=()
while IFS= read -r file; do
    SOURCES+=("$file")
done < <(find "$CORE/src/core" -name '*.cpp' | sort)
SOURCES+=(
    "$LIBRETRO/src/libretro/fc_libretro.cpp"
    "$LIBRETRO/src/libretro/cheat_codes.cpp"
)

mkdir -p "$OUT"

echo "==> building custom_nes_core (${#SOURCES[@]} sources, v$VERSION, $(uname -m))"
clang++ -std=c++20 -O2 -fPIC -dynamiclib -mmacosx-version-min=11.0 \
    -I "$CORE/src" \
    -I "$LIBRETRO/src/libretro" \
    -I "$LIBRETRO/third_party/libretro" \
    -DFC_LIBRETRO_VERSION="\"$VERSION\"" \
    "${SOURCES[@]}" \
    -o "$OUT/custom_nes_core_libretro.dylib"

echo "==> done: $OUT/custom_nes_core_libretro.dylib"
