#!/bin/bash
# ---------------------------------------------------------------------------
# Build the in-repo custom FC / NES core as a native libretro module.
#
# Output: cores/dist/custom_nes_core_libretro.dylib
# Declared in cores/cores.json as key `custom_nes_core`.
#
# Source lives in the read-only `custom_nes_core/` CMake project (AGENTS.md
# rule 7): src/{core,ffi,libretro}. When cmake is available this script drives
# it:
#
#   cmake -S custom_nes_core -B custom_nes_core/build    # Release, no tests
#   cmake --build ... --target custom_nes_core_libretro  # one .dylib
#   cp ... -> cores/dist/custom_nes_core_libretro.dylib
#
# On a machine without cmake the tree's own CMake project is still the source
# of truth, so the fallback drives clang++ with the same sources and include
# paths the targets use:
#
#   custom_nes_core   custom_nes_core/src/core/*.cpp + src/ffi (not needed by
#                     the module)
#   retro_adapter     custom_nes_core/src/libretro/{fc_libretro,cheat_codes}.cpp
#
# src/ffi/emulator_api.cpp is not part of the core and is not built. The
# private `fc_*` extension (`fc_libretro_get_ext`) is exported but the host
# never uses it: only the standard libretro ABI matters.
#
# Usage: ./cores/custom_nes_core/build.sh
# Requires: cmake + Apple clang++, arm64 Mac (Linux works with .so suffix).
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
PROJ="$ROOT/custom_nes_core"

if [ ! -d "$PROJ/src/core" ] || [ ! -d "$PROJ/src/libretro" ]; then
    echo "error: custom_nes_core/{src/core,src/libretro} not found under $ROOT" >&2
    exit 1
fi

# The version the core reports, kept in sync with the CMake tree it comes from.
VERSION="$(sed -n 's/.*set(FC_PROJECT_VERSION "\(.*\)").*/\1/p' "$PROJ/cmake/Version.cmake")"
VERSION="${VERSION:-0.0.0}"

mkdir -p "$OUT"
DYLIB_NAME="custom_nes_core_libretro.dylib"

build_with_cmake() {
    local build_dir="$PROJ/build"
    echo "==> building custom_nes_core with cmake (v$VERSION, $(uname -m))"
    cmake -S "$PROJ" -B "$build_dir" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCGB_BUILD_TESTS=OFF \
        -DFC_PROJECT_VERSION="$VERSION"
    cmake --build "$build_dir" --target custom_nes_core_libretro
    # The module's OUTPUT_NAME already matches what cores.json declares.
    cp "$build_dir/$DYLIB_NAME" "$OUT/$DYLIB_NAME"
}

build_with_clang() {
    # bash 3.2 (macOS) has no `mapfile`; collect the core sources in a loop.
    local sources=()
    local file
    while IFS= read -r file; do
        sources+=("$file")
    done < <(find "$PROJ/src/core" -name '*.cpp' | sort)
    sources+=(
        "$PROJ/src/libretro/fc_libretro.cpp"
        "$PROJ/src/libretro/cheat_codes.cpp"
    )
    echo "==> building custom_nes_core with clang++ (${#sources[@]} sources, v$VERSION, $(uname -m))"
    clang++ -std=c++20 -O2 -fPIC -dynamiclib -mmacosx-version-min=11.0 \
        -I "$PROJ/src" \
        -I "$PROJ/src/libretro" \
        -I "$PROJ/third_party/libretro" \
        -DFC_LIBRETRO_VERSION="\"$VERSION\"" \
        "${sources[@]}" \
        -o "$OUT/$DYLIB_NAME"
}

if command -v cmake >/dev/null 2>&1; then
    build_with_cmake
else
    echo "note: cmake not found; falling back to a direct clang++ build" >&2
    build_with_clang
fi

echo "==> done: $OUT/$DYLIB_NAME"
