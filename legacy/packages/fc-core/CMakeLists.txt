# ---------------------------------------------------------------------------
# fc-core —— 自定义 FC / NES 核心
#
# 这是 monorepo 里的一个独立项目：
#
#   * 它有自己的 `project()`、自己的版本号、自己的测试；
#   * 它可以单独配置和构建：cmake -S packages/fc-core -B build-core
#   * 它也可以被 monorepo 根目录用 add_subdirectory() 组合进来。
#
# 它产出两个 target：
#
#   fc_core   机器本身（CPU / Bus / PPU / APU / Mapper / State）
#   fc_ffi    机器外面那层纯 C 接口（src/ffi/emulator_api.h）
#
# 依赖方向只有一条：fc_ffi -> fc_core。没有任何 UI、文件 IO、线程或异常，
# 所以同一份源码能原样编译成原生库和 WebAssembly。
# ---------------------------------------------------------------------------
cmake_minimum_required(VERSION 3.20)

include(${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/Version.cmake)
project(fc_core
    VERSION ${FC_PROJECT_VERSION}
    DESCRIPTION "FC / NES emulator core: CPU, bus, PPU, APU, mappers, state"
    LANGUAGES CXX)

# 单独立项构建时，这个文件就是根；作为软件包被组合时，这些设置由 monorepo
# 根目录给出。两种情况下都成立。
if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
    set(CMAKE_CXX_STANDARD 20)
    set(CMAKE_CXX_STANDARD_REQUIRED ON)
    set(CMAKE_CXX_EXTENSIONS OFF)
    if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
        set(CMAKE_BUILD_TYPE Debug CACHE STRING "Build type" FORCE)
    endif()
    set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
    add_compile_options(-Wall -Wextra -Wpedantic)
    option(FC_BUILD_TESTS "Build the unit tests" ON)
endif()

# ---------------------------------------------------------------------------
# 核心库：纯 C++，不依赖 UI。
# ---------------------------------------------------------------------------
add_library(fc_core STATIC
    src/core/bit.cpp
    src/core/state.cpp
    src/core/cpu/cpu.cpp
    src/core/cpu/addressing.cpp
    src/core/nes/bus.cpp
    src/core/nes/ines.cpp
    src/core/nes/cartridge.cpp
    src/core/nes/ppu.cpp
    src/core/nes/machine.cpp
    src/core/nes/apu.cpp
    src/core/cpu/opcode.cpp
    src/core/cpu/disassembler.cpp
)
target_include_directories(fc_core PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)

# ---------------------------------------------------------------------------
# C 接口：前端唯一需要链接的东西。
# ---------------------------------------------------------------------------
add_library(fc_ffi STATIC
    src/ffi/emulator_api.cpp
)
target_link_libraries(fc_ffi PUBLIC fc_core)
target_include_directories(fc_ffi PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)

# 静态库最终会被装进共享对象（libretro core）或 wasm 模块，所以以位置无关
# 代码编译。arm64 上本来每条指令都位置无关，但显式写出来是为了让将来某个
# x86_64 构建不必在链接期、在一条关于 relocation 的报错里才发现这件事。
set_target_properties(fc_core fc_ffi PROPERTIES POSITION_INDEPENDENT_CODE ON)

# ---------------------------------------------------------------------------
# 测试
# ---------------------------------------------------------------------------
if(FC_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()
