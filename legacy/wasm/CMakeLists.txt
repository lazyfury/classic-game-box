# ---------------------------------------------------------------------------
# The WebAssembly module.
#
# Reached only via emcmake, from the block in the top level CMakeLists.txt.
#
#   source third_party/emsdk/emsdk_env.sh
#   emcmake cmake -S . -B build-wasm -G Ninja -DCMAKE_BUILD_TYPE=Release
#   cmake --build build-wasm
#   # -> wasm/dist/fc_core.mjs + wasm/dist/fc_core.wasm
#
# or just:  ./wasm/build.sh
#
# The whole module is one .wasm plus a thin ES module loader. It is loaded
# both by the Electron renderer and by `node wasm/headless.mjs`, which is the
# point of doing it this way: the thing we regression test is the thing we
# ship.
# ---------------------------------------------------------------------------

add_executable(fc_wasm
    ${CMAKE_CURRENT_SOURCE_DIR}/glue.cpp
)

target_link_libraries(fc_wasm PRIVATE fc_ffi)

# The emulator has no UI, so it has no business pulling in a DOM, a filesystem
# or a wall clock. Emscripten will happily add all three; say no.
#
#   FILESYSTEM=0    we hand ROM bytes in through memory, so there is nothing
#                   to mount. Saves a surprising amount of generated code.
#   ALLOW_MEMORY_GROWTH=0
#                   a fixed heap. Growth detaches every cached typed array
#                   view into the heap, which is the classic wasm emulator
#                   bug: the picture freezes or tears a frame later. A fixed
#                   64MB removes the whole class of problem, and the largest
#                   cartridge we support is 2MB.
#   INITIAL_MEMORY  64MB: framebuffer 240KB, ROM up to a few MB, CHR RAM,
#                   2KB of console RAM, and the APU's sample queue.
#
# `--no-entry` because there is no main(). The module is a bag of functions
# that the front end calls in whatever order it likes.
set(FC_WASM_EXPORTS
    _fc_create
    _fc_destroy
    _fc_load_rom
    _fc_last_error
    _fc_rom_summary
    _fc_reset
    _fc_run_frame
    _fc_is_halted
    _fc_frame_count
    _fc_framebuffer
    _fc_pixel
    _fc_sample_rate
    _fc_take_samples
    _fc_samples_pending
    _fc_clear_samples
    _fc_set_button
    _fc_release_all_buttons
    _fc_total_cycles
    _fc_cpu_pc
    _fc_peek
    _fc_poke
    _fc_set_cheats
    _fc_cheat_count
    _fc_save_state
    _fc_save_state_into
    _fc_state_size
    _fc_load_state
    _fc_free_state
    _fc_mapper_saves_state
    _fc_wasm_abi
    _fc_wasm_screen_width
    _fc_wasm_screen_height
    _fc_wasm_sample_rate
    _malloc
    _free
)

# The heap views. packages/fc-core/src/ffi/emulator_api.h hands back bare
# pointers -- the framebuffer at 0x00RRGGBB per pixel, and APU samples as float
# -- and a pointer is only useful on the JavaScript side if we can turn it back
# into a typed array. That is what HEAPU8 and HEAPF32 are.
#
# They are not exposed by default any more, so ask for them by name. UTF8ToString
# is how a `const char*` becomes a JavaScript string.
set(FC_WASM_RUNTIME_METHODS
    HEAPU8
    HEAPF32
    UTF8ToString
    stringToUTF8
    lengthBytesUTF8
)

string(REPLACE ";" "," FC_WASM_EXPORTS_CSV "${FC_WASM_EXPORTS}")
string(REPLACE ";" "," FC_WASM_RUNTIME_CSV "${FC_WASM_RUNTIME_METHODS}")

target_link_options(fc_wasm PRIVATE
    --no-entry
    -sMODULARIZE=1
    # EXPORT_ES6 means `import createFcCore from './fc_core.mjs'` works in both
    # Node and a browser. Node needs .mjs (or a package.json with
    # "type":"module"), which is why the output is named that way below.
    -sEXPORT_ES6=1
    -sEXPORT_NAME=createFcCore
    -sFILESYSTEM=0
    -sALLOW_MEMORY_GROWTH=0
    -sINITIAL_MEMORY=67108864
    -sSTACK_SIZE=1048576
    # Do NOT set -sEXPORT_KEEPALIVE=0 here. It reads like a harmless "we are
    # not using EMSCRIPTEN_KEEPALIVE" switch, but it also stops Emscripten
    # from attaching anything in EXPORTED_FUNCTIONS to the module object: the
    # .wasm still exports them, the glue still copies them into local
    # variables, and `Module._fc_create` is silently undefined.
    "-sEXPORTED_FUNCTIONS=[${FC_WASM_EXPORTS_CSV}]"
    "-sEXPORTED_RUNTIME_METHODS=[${FC_WASM_RUNTIME_CSV}]"
)

# Emit the module straight into wasm/dist, next to the JavaScript that loads
# it, so nothing has to copy files around afterwards.
set_target_properties(fc_wasm PROPERTIES
    OUTPUT_NAME "fc_core"
    SUFFIX ".mjs"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/dist"
)

# ---------------------------------------------------------------------------
# The libretro core, as a WebAssembly module.
#
# The same packages/fc-libretro/src/libretro/fc_libretro.cpp that becomes
# fc_libretro.dylib, built for the browser instead. It is a *standalone* module,
# not a side module:
# its own linear memory, `retro_*` exported by name, and the front end's
# callbacks handed in from JavaScript with addFunction. That is route L4a in
# docs/architecture/libretro-migration.md, chosen because an Emscripten side
# module would have to import libc++ from a main module that does not use it.
#
# Because it is standalone it can keep ALLOW_MEMORY_GROWTH=0, which is the
# whole point: a fixed heap cannot detach the typed array views the front end
# caches, so the classic "the picture froze a frame after the ROM loaded" bug
# cannot happen here either.
#
#   -> wasm/dist/fc_libretro.mjs + fc_libretro.wasm
# ---------------------------------------------------------------------------

add_executable(fc_libretro_wasm
    ${CMAKE_CURRENT_SOURCE_DIR}/libretro_ext_glue.cpp
)

# fc_libretro_core is the OBJECT library the native .dylib is also built from:
# one set of object files, two linkings, so the two products cannot drift apart.
# Its PUBLIC include directories bring libretro.h and fc_libretro_ext.h along,
# and its PUBLIC link to fc_core brings the emulator itself.
target_link_libraries(fc_libretro_wasm PRIVATE fc_libretro_core)

# Every retro_* the standard ABI names, plus the custom extension, plus malloc
# and free so JavaScript can hand a ROM in and take a save state out.
set(FC_LIBRETRO_WASM_EXPORTS
    _retro_api_version
    _retro_get_system_info
    _retro_get_system_av_info
    _retro_set_controller_port_device
    _retro_reset
    _retro_run
    _retro_serialize_size
    _retro_serialize
    _retro_unserialize
    _retro_cheat_reset
    _retro_cheat_set
    _retro_load_game
    _retro_load_game_special
    _retro_unload_game
    _retro_get_region
    _retro_get_memory_data
    _retro_get_memory_size
    _retro_set_environment
    _retro_set_video_refresh
    _retro_set_audio_sample
    _retro_set_audio_sample_batch
    _retro_set_input_poll
    _retro_set_input_state
    _retro_init
    _retro_deinit
    _fc_libretro_get_ext
    _fc_ext_peek
    _fc_ext_poke
    _fc_ext_set_raw_cheats
    _fc_ext_raw_cheat_count
    _fc_ext_mapper_saves_state
    _fc_ext_rom_summary
    _fc_ext_total_cycles
    _fc_ext_cpu_pc
    _fc_ext_take_samples
    _fc_ext_last_error
    _malloc
    _free
)

# HEAP16 is where libretro's audio lives (int16 stereo); HEAPF64 is where
# retro_system_av_info's fps and sample rate live. addFunction is what turns a
# JavaScript callback into a pointer the core can call, which is the whole of
# how this module has a front end at all.
set(FC_LIBRETRO_WASM_RUNTIME_METHODS
    HEAPU8
    HEAP16
    HEAPU32
    HEAPF32
    HEAPF64
    addFunction
    UTF8ToString
    stringToUTF8
    lengthBytesUTF8
)

string(REPLACE ";" "," FC_LIBRETRO_EXPORTS_CSV "${FC_LIBRETRO_WASM_EXPORTS}")
string(REPLACE ";" "," FC_LIBRETRO_RUNTIME_CSV "${FC_LIBRETRO_WASM_RUNTIME_METHODS}")

target_link_options(fc_libretro_wasm PRIVATE
    --no-entry
    -sMODULARIZE=1
    -sEXPORT_ES6=1
    -sEXPORT_NAME=createFcLibretro
    -sFILESYSTEM=0
    -sALLOW_MEMORY_GROWTH=0
    -sINITIAL_MEMORY=67108864
    -sSTACK_SIZE=1048576
    # addFunction writes a JavaScript callback into the wasm function table,
    # and the table starts with no spare slots unless it is allowed to grow.
    -sALLOW_TABLE_GROWTH=1
    "-sEXPORTED_FUNCTIONS=[${FC_LIBRETRO_EXPORTS_CSV}]"
    "-sEXPORTED_RUNTIME_METHODS=[${FC_LIBRETRO_RUNTIME_CSV}]"
)

set_target_properties(fc_libretro_wasm PROPERTIES
    OUTPUT_NAME "fc_libretro"
    SUFFIX ".mjs"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/dist"
)
