#!/bin/bash
# ---------------------------------------------------------------------------
# Build mGBA as a libretro WebAssembly core.
#
# Output: wasm/dist/mgba_libretro.mjs + mgba_libretro.wasm
#
# The result is a standalone Emscripten module that exports the libretro ABI,
# exactly like fc_libretro.wasm, so wasm/libretro.mjs can drive either core
# without knowing which one it has. That is the whole point: adding a console
# becomes "compile another core", not "write another front end".
#
# Why this is a script and not a CMake target
# -------------------------------------------
# mGBA is a third-party project of its own with its own Makefile-based libretro
# build, and it is not vendored here: cloning it is 94MB and building it takes
# minutes. So it is fetched and built on demand, and the artifact lands in
# wasm/dist next to fc_libretro.wasm.
#
# Usage:
#   ./wasm/mgba/build.sh                 # Release
#   MGBA_SRC=/path/to/mgba ./wasm/mgba/build.sh   # use an existing checkout
#
# Requires: network (once, to clone), and the vendored emsdk.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EMSDK="$ROOT/third_party/emsdk"
OUT="$ROOT/wasm/dist"
WORK="${MGBA_WORK:-$ROOT/build-mgba}"
SRC="${MGBA_SRC:-$WORK/mgba}"

if [ ! -d "$EMSDK/upstream/emscripten" ]; then
    echo "error: emsdk not found at $EMSDK" >&2
    exit 1
fi

# shellcheck disable=SC1091
source "$EMSDK/emsdk_env.sh" > /dev/null 2>&1
echo "==> emscripten $(emcc --version | head -1 | sed 's/.*) //')"

# 1. the source ------------------------------------------------------------
#
# EmulatorJS's fork is upstream mGBA plus the small Emscripten changes the
# libretro Makefile needs (the `platform=emscripten` target). Using it saves
# carrying those patches here.
if [ ! -d "$SRC" ]; then
    echo "==> cloning EmulatorJS/mgba into $SRC"
    mkdir -p "$(dirname "$SRC")"
    git clone --depth 1 https://github.com/EmulatorJS/mgba "$SRC"
fi

cd "$SRC"

# 2. two changes to the build flags ----------------------------------------
#
#   COLOR_16_BIT  makes mGBA render RGB565. This project's front end and its
#                 parity tests work in 32-bit XRGB8888, which is what the NES
#                 core produces; dropping the define makes mGBA match.
#   HAVE_CRC32    tells mGBA's crc32.c to expect zlib to provide crc32(), and
#                 nothing here links zlib, so the link fails on the symbol.
#                 Removing it lets crc32.c carry its own implementation.
#
# Both are one-line edits to a third-party file, so they are applied only if
# still present, and the build is a no-op if it is already patched.
COMMON="libretro-build/Makefile.common"
if grep -q 'DCOLOR_16_BIT' "$COMMON"; then
    echo "==> patching $COMMON (32-bit colour)"
    sed -i.bak 's/ -DCOLOR_16_BIT//' "$COMMON"
fi
if grep -q 'RETRODEFS += -DHAVE_CRC32' "$COMMON"; then
    echo "==> patching $COMMON (own crc32)"
    sed -i.bak '/RETRODEFS += -DHAVE_CRC32/d' "$COMMON"
fi

# 3. compile ---------------------------------------------------------------
#
# HAVE_VFS_FD=0 keeps mGBA off POSIX file descriptors, which do not exist in a
# WebAssembly build with no filesystem. The ROM arrives in memory
# (need_fullpath is false), which is what that path is for.
echo "==> compiling the core"
emmake make -f Makefile.libretro platform=emscripten HAVE_VFS_FD=0 clean > /dev/null 2>&1 || true
emmake make -f Makefile.libretro platform=emscripten HAVE_VFS_FD=0 -j"$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

# The Makefile's emscripten target archives its objects rather than linking a
# shared object, because libretro's own emscripten front end links them into
# one big module. We link our own, exporting the ABI by name.
cp mgba_libretro_emscripten.bc libmgba.a

# 4. link an independent module --------------------------------------------
#
# Same flags as fc_libretro.wasm: standalone, fixed heap (so cached typed-array
# views cannot go stale), addFunction so JavaScript can register the front end
# callbacks, and every retro_* plus malloc/free exported.
echo "==> linking $OUT/mgba_libretro.wasm"
EXPORTS='["_retro_api_version","_retro_get_system_info","_retro_get_system_av_info","_retro_set_controller_port_device","_retro_reset","_retro_run","_retro_serialize_size","_retro_serialize","_retro_unserialize","_retro_cheat_reset","_retro_cheat_set","_retro_load_game","_retro_load_game_special","_retro_unload_game","_retro_get_region","_retro_get_memory_data","_retro_get_memory_size","_retro_set_environment","_retro_set_video_refresh","_retro_set_audio_sample","_retro_set_audio_sample_batch","_retro_set_input_poll","_retro_set_input_state","_retro_init","_retro_deinit","_malloc","_free"]'
RUNTIME='["HEAPU8","HEAP16","HEAP32","HEAPU32","HEAPF32","HEAPF64","addFunction","UTF8ToString","stringToUTF8","lengthBytesUTF8"]'

emcc libmgba.a -O3 --no-entry -sMODULARIZE=1 -sEXPORT_ES6=1 -sEXPORT_NAME=createMgbaLibretro \
    -sFILESYSTEM=0 -sALLOW_MEMORY_GROWTH=0 -sINITIAL_MEMORY=268435456 -sSTACK_SIZE=1048576 \
    -sALLOW_TABLE_GROWTH=1 \
    "-sEXPORTED_FUNCTIONS=$EXPORTS" \
    "-sEXPORTED_RUNTIME_METHODS=$RUNTIME" \
    -o "$OUT/mgba_libretro.mjs"

echo "==> done"
ls -la "$OUT"/mgba_libretro.*
