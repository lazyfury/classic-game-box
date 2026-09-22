#!/bin/bash
# ---------------------------------------------------------------------------
# Build Mesen as a libretro WebAssembly core.
#
# Output: wasm/dist/mesen_libretro.mjs + mesen_libretro.wasm
#
# This is the second NES core, and the first time this project has had *two*
# cores for the *same* console. wasm/libretro.mjs drives it exactly like the
# home-grown fc_libretro.wasm, because both sides of that file only know the
# libretro ABI -- which is the point of the exercise: another NES core is
# another module, not another front end.
#
# Why not the original Mesen2
# ---------------------------
# SourMesen/Mesen2 is the current emulator, but its core is C++20 and its
# libretro port is not self-contained. libretro/Mesen is the older Mesen 1.x
# (C++11) with a libretro Makefile that already has an `emscripten` target, and
# it renders XRGB8888 into a 256x240 buffer the way this front end expects.
#
# Why this is a script and not a CMake target
# -------------------------------------------
# Same reason as wasm/mgba/build.sh: Mesen is a third-party project with its
# own Makefile, dozens of megabytes of source and a multi-minute build. It is
# fetched and built on demand, and the artifact lands in wasm/dist next to the
# other cores.
#
# Usage:
#   ./wasm/mesen/build.sh                    # Release
#   MESEN_SRC=/path/to/Mesen ./wasm/mesen/build.sh   # use an existing checkout
#
# Requires: network (once, to clone), and the vendored emsdk.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EMSDK="$ROOT/third_party/emsdk"
OUT="$ROOT/wasm/dist"
WORK="${MESEN_WORK:-$ROOT/build-mesen}"
SRC="${MESEN_SRC:-$WORK/Mesen}"

if [ ! -d "$EMSDK/upstream/emscripten" ]; then
    echo "error: emsdk not found at $EMSDK" >&2
    exit 1
fi

# shellcheck disable=SC1091
source "$EMSDK/emsdk_env.sh" > /dev/null 2>&1
echo "==> emscripten $(emcc --version | head -1 | sed 's/.*) //')"

# 1. the source ------------------------------------------------------------
if [ ! -d "$SRC" ]; then
    echo "==> cloning libretro/Mesen into $SRC"
    mkdir -p "$(dirname "$SRC")"
    git clone --depth 1 https://github.com/libretro/Mesen "$SRC"
fi

cd "$SRC"

# 2. one change to libretro.cpp --------------------------------------------
#
# Mesen builds a VirtualFile from the *path* in retro_game_info, and only
# takes the in-memory bytes through the newer GET_GAME_INFO_EXT environment
# call. There is no filesystem in this wasm build, so a front end that does
# not implement that call (this one does not) would hand Mesen a path it
# cannot open. The bytes it was given are right there in the same struct; take
# them, and keep a name so the loader can still tell the format by extension.
#
# The edit is idempotent: it carries a marker, and a second run is a no-op.
if ! grep -q 'MESEN_WASM_INMEMORY' Libretro/libretro.cpp; then
    echo "==> patching Libretro/libretro.cpp (in-memory cartridge)"
    python3 - Libretro/libretro.cpp <<'PY'
import sys

path = sys.argv[1]
source = open(path).read()
anchor = "\t\t\tgamePath = game->path;\n"
replacement = (
    "\t\t\t// MESEN_WASM_INMEMORY: no extended game info. A front end without\n"
    "\t\t\t// GET_GAME_INFO_EXT still passed the cartridge in memory, and wasm\n"
    "\t\t\t// has no filesystem to read a path from, so use the bytes.\n"
    "\t\t\tgamePath = game->path ? game->path : \"content.nes\";\n"
    "\t\t\tgameData = game->data;\n"
    "\t\t\tgameSize = game->size;\n"
)
if anchor not in source:
    raise SystemExit("error: could not find the gamePath fallback in libretro.cpp")
open(path, "w").write(source.replace(anchor, replacement, 1))
PY
fi

# 2b. exceptions on for the emscripten target ---------------------------------
#
# Mesen's loaders throw std::runtime_error and catch it themselves; that is
# how a cartridge it cannot parse becomes a failed load rather than a crash.
# Emscripten disables C++ exceptions by default, which turns the throw into an
# abort of the whole wasm module -- the machine dies instead of refusing the
# ROM. The libretro.cpp above has no try/catch of its own, so the alternative
# would be patching every loader; enabling exceptions is what upstream's
# desktop builds do and is one flag.
if ! grep -q 'MESEN_WASM_EXCEPTIONS' Libretro/Makefile; then
    echo "==> patching Libretro/Makefile (C++ exceptions)"
    python3 - Libretro/Makefile <<'PY'
import sys

path = sys.argv[1]
source = open(path).read()
anchor = "else ifeq ($(platform), emscripten)\n   TARGET := $(TARGET_NAME)_libretro_emscripten.bc\n"
replacement = (
    anchor
    + "   # MESEN_WASM_EXCEPTIONS: the loaders throw and catch, which needs\n"
    + "   # exception support; without it wasm-ld aborts on the first throw.\n"
    + "   CFLAGS += -fexceptions\n"
    + "   CXXFLAGS += -fexceptions\n"
)
if anchor not in source:
    raise SystemExit("error: could not find the emscripten block in Libretro/Makefile")
open(path, "w").write(source.replace(anchor, replacement, 1))
PY
fi

# 3. compile the objects ----------------------------------------------------
#
# LD=true leaves the Makefile's own link step with nothing to do. That
# link passes -Wl,--no-undefined, which wasm-ld rejects, and its output is not
# what we want anyway: the emscripten target links a bare shared object, while
# this project needs a MODULARIZE'd ES module with the libretro ABI exported
# by name. The objects it produces first are exactly right, and step 4 links
# them itself.
echo "==> compiling the core"
cd Libretro
emmake make -f Makefile platform=emscripten LD=true clean > /dev/null 2>&1 || true
emmake make -f Makefile platform=emscripten LD=true -j"$(sysctl -n hw.ncpu 2>/dev/null || nproc)"
cd "$SRC"

# 4. link an independent module --------------------------------------------
#
# Same flags as fc_libretro.wasm and mgba_libretro.wasm -- standalone, fixed
# heap (so cached typed-array views cannot go stale), addFunction so JavaScript
# can register the front end callbacks, every retro_* plus malloc/free
# exported -- with two differences:
#
#   * -fexceptions / DISABLE_EXCEPTION_CATCHING=0: Mesen's loaders throw and
#     catch std::runtime_error, which Emscripten otherwise turns into an abort
#     of the whole module. See step 2b.
#   * the filesystem is *not* disabled. fc_libretro.wasm and
#     mgba_libretro.wasm set -sFILESYSTEM=0 because they never touch a path.
#     Mesen does: it probes disksys.rom, MesenDB.txt and HdPacks with ifstream,
#     and without the filesystem those stubs report an empty stream as good
#     while tellg() answers -1 -- which Mesen turns into resize(0xFFFFFFFF)
#     and a std::length_error. With the (in-memory, never populated) MEMFS the
#     opens fail the way the code expects and every probe is a clean miss.
#
# The heap is larger than the NES core needs because Mesen keeps HdPack and
# NTSC-filter buffers alongside the machine.
echo "==> linking $OUT/mesen_libretro.wasm"
EXPORTS='["_retro_api_version","_retro_get_system_info","_retro_get_system_av_info","_retro_set_controller_port_device","_retro_reset","_retro_run","_retro_serialize_size","_retro_serialize","_retro_unserialize","_retro_cheat_reset","_retro_cheat_set","_retro_load_game","_retro_load_game_special","_retro_unload_game","_retro_get_region","_retro_get_memory_data","_retro_get_memory_size","_retro_set_environment","_retro_set_video_refresh","_retro_set_audio_sample","_retro_set_audio_sample_batch","_retro_set_input_poll","_retro_set_input_state","_retro_init","_retro_deinit","_malloc","_free"]'
RUNTIME='["HEAPU8","HEAP16","HEAP32","HEAPU32","HEAPF32","HEAPF64","addFunction","UTF8ToString","stringToUTF8","lengthBytesUTF8"]'
OBJECTS="$(find SevenZip Core Utilities Libretro -name '*.o')"

# shellcheck disable=SC2086
em++ $OBJECTS -O3 -fexceptions -sDISABLE_EXCEPTION_CATCHING=0 \
    --no-entry -sMODULARIZE=1 -sEXPORT_ES6=1 -sEXPORT_NAME=createMesenLibretro \
    -sALLOW_MEMORY_GROWTH=0 -sINITIAL_MEMORY=268435456 -sSTACK_SIZE=1048576 \
    -sALLOW_TABLE_GROWTH=1 \
    "-sEXPORTED_FUNCTIONS=$EXPORTS" \
    "-sEXPORTED_RUNTIME_METHODS=$RUNTIME" \
    -o "$OUT/mesen_libretro.mjs"

echo "==> done"
ls -la "$OUT"/mesen_libretro.*
