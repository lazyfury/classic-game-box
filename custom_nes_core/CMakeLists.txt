# ---------------------------------------------------------------------------
# custom_nes_core —— 自研 FC / NES 核心（单个 CMake 项目）
#
# 结构是标准 src 布局：
#
#   src/core/      机器本身：CPU / Bus / PPU / APU / Mapper / State
#   src/ffi/       历史对照的纯 C 接口（fc_* 私有 ABI，不再参与产品构建）
#   src/libretro/  libretro 包装：retro_* 调用 → Machine 方法
#
# 依赖方向只有一条：src/libretro → src/core。没有任何 UI、文件 IO、线程或
# 异常，所以同一份源码能原样编译成原生库和 libretro core。
#
# 产出：
#
#   custom_nes_core             静态库（core + ffi，供测试与历史对照）
#   custom_nes_core_libretro    libretro 模块（cores.json 的 `dylib` 名）
#
# 配置与构建：
#
#   cmake -S custom_nes_core -B custom_nes_core/build
#   cmake --build custom_nes_core/build
#   ctest --test-dir custom_nes_core/build
#
# 产品路径的正式入口是 ./cores/custom_nes_core/build.sh（Release、跳过测试、
# 拷贝 dylib 到 cores/dist/）。
# ---------------------------------------------------------------------------
cmake_minimum_required(VERSION 3.20)

# 单一版本号来源：core 在 get_system_info / library_version 里上报它。
include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/Version.cmake)

project(custom_nes_core
    VERSION ${FC_PROJECT_VERSION}
    DESCRIPTION "FC / NES emulator core and its libretro wrapper"
    LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Debug CACHE STRING "Build type" FORCE)
endif()
add_compile_options(-Wall -Wextra -Wpedantic)

option(CGB_BUILD_TESTS "Build the unit tests" ON)

# ---------------------------------------------------------------------------
# 核心库：纯 C++，不依赖 UI。src/ffi 是历史对照的私有 C ABI，一起编进来，
# 让测试（test_ffi）不用两个 target 就能链接到它。
# ---------------------------------------------------------------------------
add_library(custom_nes_core STATIC
    src/core/bit.cpp
    src/core/state.cpp
    src/core/cpu/cpu.cpp
    src/core/cpu/opcode.cpp
    src/core/cpu/addressing.cpp
    src/core/cpu/disassembler.cpp
    src/core/nes/bus.cpp
    src/core/nes/ines.cpp
    src/core/nes/cartridge.cpp
    src/core/nes/machine.cpp
    src/core/nes/ppu.cpp
    src/core/nes/apu.cpp
    src/ffi/emulator_api.cpp
)
target_include_directories(custom_nes_core
    PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/src
)

# ---------------------------------------------------------------------------
# libretro 适配层：先编成 OBJECT library，产品 dylib 与 libretro ABI 测试用
# 同一批目标文件。版本号在这里注入；library_version 若与构建它的项目不一致，
# 就是一份不可信的构建报告。
# ---------------------------------------------------------------------------
add_library(retro_adapter OBJECT
    src/libretro/fc_libretro.cpp
    src/libretro/cheat_codes.cpp
)
target_link_libraries(retro_adapter PUBLIC custom_nes_core)
target_include_directories(retro_adapter
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/src/libretro
        ${CMAKE_CURRENT_SOURCE_DIR}/third_party/libretro
)
target_compile_definitions(retro_adapter PRIVATE
    FC_LIBRETRO_VERSION="${FC_PROJECT_VERSION}"
)

# ---------------------------------------------------------------------------
# libretro 模块。libretro core 叫 custom_nes_core_libretro.dylib，不带 `lib`
# 前缀（每个前端的 core 扫描器都按前者找）；CMake 给 MODULE 的 macOS 默认
# 后缀是 .so，前端也不会去找，所以后缀也一并定死。产物就是
# cores/custom_nes_core/build.sh 拷贝进 cores/dist/ 的那份。
# ---------------------------------------------------------------------------
add_library(custom_nes_core_libretro MODULE)
target_link_libraries(custom_nes_core_libretro PRIVATE retro_adapter)
set_target_properties(custom_nes_core_libretro PROPERTIES
    OUTPUT_NAME "custom_nes_core_libretro"
    PREFIX ""
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}"
)
if(APPLE)
    set_target_properties(custom_nes_core_libretro PROPERTIES SUFFIX ".dylib")
endif()

# ---------------------------------------------------------------------------
# 测试：libretro ABI 测试（retro_* 那扇门）与 core 单元测试（Machine 与
# ffi 那扇门）各自独立；一个改动了 fc_* 却悄悄弄坏 retro_* 的提交，会在
# ctest 里立刻暴露。
# ---------------------------------------------------------------------------
if(CGB_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()
