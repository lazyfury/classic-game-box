# ---------------------------------------------------------------------------
# Classic Game Box —— monorepo 根目录
#
# 这里不定义任何模拟逻辑，只负责把几个独立项目组装起来并给出全局设置。
#
#   packages/fc-core/       自定义 FC / NES 核心     (fc_core, fc_ffi)
#   packages/fc-libretro/   libretro 包装            (fc_libretro, fc_libretro_core)
#   wasm/                   同一份核心的 WebAssembly 构建
#   tools/                  教学 demo 与命令行工具
#   electron/               Electron + TypeScript 前端
#
# 每个 packages/* 都是一个真正的 CMake 项目：有自己的 project()、自己的
# 版本号、自己的测试，也可以单独配置构建（见各自的 CMakeLists.txt）。
# 根目录只是把它们拼在一起，所以两者永远不会成为两份会漂移的事实。
#
# 依赖方向：
#
#   wasm / tools / electron  ->  fc-libretro, fc-ffi, fc-core
#   fc-libretro              ->  fc-core
#   fc-core                  ->  （没有）
# ---------------------------------------------------------------------------
cmake_minimum_required(VERSION 3.20)

# 项目版本只写在一个地方，见 cmake/Version.cmake。
include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/Version.cmake)

project(ClassicGameBox
    VERSION ${FC_PROJECT_VERSION}
    DESCRIPTION "Classic Game Box - NES / FC emulator for macOS, a computer science learning project"
    LANGUAGES CXX)

# ---------------------------------------------------------------------------
# C++ standard
# ---------------------------------------------------------------------------
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Default to a build type so tests are debuggable.
if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Debug CACHE STRING "Build type" FORCE)
endif()

# Export compile_commands.json.
#
# This file is a "compilation database": for every .cpp it records the exact
# compiler invocation, including -std=c++20 and every -I path.
#
# Editors (VS Code / clangd / Emacs) read it instead of guessing. Without it,
# IntelliSense defaults to an older C++ standard and reports false errors such
# as:  namespace "std" has no member "unsigned_integral"
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# Warnings: they are how the compiler teaches you about your own bugs.
add_compile_options(-Wall -Wextra -Wpedantic)

# ---------------------------------------------------------------------------
# Options. Defined here (before the packages are added) so every package sees
# the same values whether it is built by this file or on its own.
# ---------------------------------------------------------------------------
option(FC_BUILD_TESTS "Build the unit tests" ON)
option(FC_BUILD_LIBRETRO "Build the native libretro core" ON)

if(FC_BUILD_TESTS)
    # Enables testing for this directory and everything below it, so the tests
    # each package adds show up in the build root's ctest run.
    enable_testing()
endif()

# ---------------------------------------------------------------------------
# Packages.
#
# fc-core first: it depends on nothing, and both other things depend on it.
# ---------------------------------------------------------------------------
add_subdirectory(packages/fc-core)

# The libretro wrapper is a native shared object for every ordinary build, so
# FC_BUILD_LIBRETRO decides whether it is built at all. The wasm build needs its
# source target regardless; the package itself skips the .dylib there.
if(FC_BUILD_LIBRETRO OR EMSCRIPTEN)
    add_subdirectory(packages/fc-libretro)
endif()

# ---------------------------------------------------------------------------
# The WebAssembly build.
#
# A second build of the *same* packages. Nothing under packages/fc-core changed
# to make this work, and that is the point: the core has no file I/O, no threads
# and no exceptions, so it cross compiles untouched. Everything platform
# specific lives on the far side of packages/fc-core/src/ffi/emulator_api.h.
#
# The demos and the tests below are native only (they use <fstream> and
# <filesystem>), so the wasm build stops here.
# ---------------------------------------------------------------------------
if(EMSCRIPTEN)
    add_subdirectory(wasm)
    return()
endif()

# ---------------------------------------------------------------------------
# Teaching demos / command line tools.
# ---------------------------------------------------------------------------
add_executable(demo_bitwise tools/demo_bitwise.cpp)
target_link_libraries(demo_bitwise PRIVATE fc_core)

add_executable(demo_overflow tools/demo_overflow.cpp)
target_link_libraries(demo_overflow PRIVATE fc_core)

add_executable(demo_cpu tools/demo_cpu.cpp)
target_link_libraries(demo_cpu PRIVATE fc_core)

add_executable(demo_disasm tools/demo_disasm.cpp)
target_link_libraries(demo_disasm PRIVATE fc_core)

add_executable(demo_addressing tools/demo_addressing.cpp)
target_link_libraries(demo_addressing PRIVATE fc_core)

add_executable(demo_instructions tools/demo_instructions.cpp)
target_link_libraries(demo_instructions PRIVATE fc_core)

add_executable(demo_bus tools/demo_bus.cpp)
target_link_libraries(demo_bus PRIVATE fc_core)

add_executable(demo_cartridge tools/demo_cartridge.cpp)
target_link_libraries(demo_cartridge PRIVATE fc_core)

add_executable(demo_ppu tools/demo_ppu.cpp)
target_link_libraries(demo_ppu PRIVATE fc_core)

add_executable(demo_input tools/demo_input.cpp)
target_link_libraries(demo_input PRIVATE fc_core)

add_executable(demo_apu tools/demo_apu.cpp)
target_link_libraries(demo_apu PRIVATE fc_core)

# The native half of the headless parity tests: run a ROM through the C API
# with scripted input, and write PPMs. wasm/headless.mjs and the Electron self
# test do the same thing; this one shares no code with either, which is what
# makes the comparison meaningful.
add_executable(fc_headless tools/fc_headless.cpp)
target_link_libraries(fc_headless PRIVATE fc_ffi)

# The outside world's test ROMs. Links the core directly rather than the C
# interface, because it needs to step one instruction at a time and read the
# bus -- which is exactly what a debugger does, and exactly what the C
# interface is not for.
add_executable(fc_testrom tools/fc_testrom.cpp)
target_link_libraries(fc_testrom PRIVATE fc_core)

# The libretro acceptance probe. Unlike every other tool here it does not link
# the emulator: it dlopen()s fc_libretro.dylib the way a front end does, and
# drives it through a stand-in front end. That is what makes it a test of the
# loaded core rather than of the functions in this build.
#
# It links dlopen and nothing else. The two include directories are for the
# header types only (libretro.h, fc_libretro_ext.h); the emulator is fetched at
# run time, which is the entire point of a probe.
add_executable(fc_libretro_probe tools/fc_libretro_probe.cpp)
target_include_directories(fc_libretro_probe PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/packages/fc-libretro/src/libretro
    ${CMAKE_CURRENT_SOURCE_DIR}/packages/fc-libretro/third_party/libretro
)
target_link_libraries(fc_libretro_probe PRIVATE ${CMAKE_DL_LIBS})
