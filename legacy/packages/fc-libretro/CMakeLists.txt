# ---------------------------------------------------------------------------
# fc-libretro —— libretro 包装（core）
#
# 这是 monorepo 里的第二个独立项目。它只做翻译：
#
#   retro_* 调用  ->  fc_core 的 Machine 方法
#
# 它不新增任何模拟逻辑。这样 RetroArch、Lakka 以及任何 libretro 前端都能
# 加载这个模拟器，而不需要知道它的存在。
#
#   macOS   -> fc_libretro.dylib      (PREFIX 故意留空)
#   Linux   -> fc_libretro.so
#   Windows -> fc_libretro.dll
#
# 它依赖 fc_core。单独配置时会把 ../fc-core 作为子项目拉进来；在 monorepo
# 里，fc_core 这个 target 通常已经由根目录建好了。
#
#   单独构建：cmake -S packages/fc-libretro -B build-libretro
# ---------------------------------------------------------------------------
cmake_minimum_required(VERSION 3.20)

include(${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/Version.cmake)
project(fc_libretro
    VERSION ${FC_PROJECT_VERSION}
    DESCRIPTION "libretro core for the FC / NES emulator"
    LANGUAGES CXX)

# 单独立项构建时的设置；被 monorepo 组合时由根目录给出。
if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
    set(CMAKE_CXX_STANDARD 20)
    set(CMAKE_CXX_STANDARD_REQUIRED ON)
    set(CMAKE_CXX_EXTENSIONS OFF)
    if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
        set(CMAKE_BUILD_TYPE Debug CACHE STRING "Build type" FORCE)
    endif()
    set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
    add_compile_options(-Wall -Wextra -Wpedantic)
endif()

option(FC_BUILD_TESTS "Build the unit tests" ON)
option(FC_BUILD_LIBRETRO "Build the native libretro core" ON)

# ---------------------------------------------------------------------------
# 它包装的那个核心。
#
# 在 monorepo 里 fc_core 已经存在；单独配置时把 fc-core 作为子项目加进来。
# 两条路都得到同一个 target 名，所以下面不需要再分情况。
# ---------------------------------------------------------------------------
if(NOT TARGET fc_core)
    add_subdirectory(
        ${CMAKE_CURRENT_SOURCE_DIR}/../fc-core
        ${CMAKE_BINARY_DIR}/fc-core)
endif()

# ---------------------------------------------------------------------------
# 适配层的源码，只有一份。
#
# 编成一个 OBJECT library，而不是直接写进每个最终产物：原生 .dylib 和
# wasm 模块用同一批目标文件，所以两份产物不可能漂移。头文件目录设为
# PUBLIC，测试和 wasm 模块要用的 libretro.h / fc_libretro_ext.h 才能被看到。
#
# 版本号在这里注入。library_version 若与构建它的项目不一致，就是一份不可
# 信的构建报告。
# ---------------------------------------------------------------------------
add_library(fc_libretro_core OBJECT
    src/libretro/fc_libretro.cpp
    src/libretro/cheat_codes.cpp
)
target_link_libraries(fc_libretro_core PUBLIC fc_core)
target_include_directories(fc_libretro_core PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/src/libretro
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party/libretro
)
target_compile_definitions(fc_libretro_core PRIVATE
    FC_LIBRETRO_VERSION="${FC_PROJECT_VERSION}"
)
set_target_properties(fc_libretro_core PROPERTIES POSITION_INDEPENDENT_CODE ON)

# ---------------------------------------------------------------------------
# 原生共享对象。
#
# wasm 下不建：WebAssembly 沙箱加载不了 .dylib（见
# docs/architecture/libretro-migration.md 第 §7 节）。那条路走
# wasm/fc_libretro_wasm，是同一个 OBJECT library 的另一种链接方式。
# ---------------------------------------------------------------------------
if(NOT EMSCRIPTEN AND FC_BUILD_LIBRETRO)
    add_library(fc_libretro MODULE)
    target_link_libraries(fc_libretro PRIVATE fc_libretro_core)

    # libretro core 叫 fc_libretro.dylib，不叫 libfc_libretro.dylib：每个
    # 前端的 core 扫描器都按前者找。CMake 给 MODULE 的 macOS 默认后缀是
    # .so，前端也不会去找，所以后缀也一并定死。
    set_target_properties(fc_libretro PROPERTIES
        PREFIX ""
        POSITION_INDEPENDENT_CODE ON
        # 产在 build/ 根目录，也就是 fc_libretro_probe 与文档期望的位置。
        LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}"
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}"
    )
    if(APPLE)
        set_target_properties(fc_libretro PROPERTIES SUFFIX ".dylib")
    endif()
endif()

# ---------------------------------------------------------------------------
# 测试
# ---------------------------------------------------------------------------
if(FC_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()
