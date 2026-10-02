cmake_minimum_required(VERSION 3.20)

# The C++/Win32 shell, the Windows counterpart of `macos/`. It links the Rust
# static library (`libcgb_app.a` on macOS / `cgb_app.lib` here), which owns the
# UI, the renderer and the emulator. Build the Rust side first:
#
#   cargo build                        # -> target/debug/cgb_app.lib
#   cmake -S windows -B windows/build -A x64
#   cmake --build windows/build --config Debug
#
# `CGB_RUST_PROFILE` selects the Rust profile (debug by default).
project(ClassicGameBoxWin LANGUAGES CXX)

if(NOT CMAKE_SYSTEM_NAME STREQUAL "Windows")
    message(FATAL_ERROR "The C++ Win32 host only builds on Windows.")
endif()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Debug CACHE STRING "" FORCE)
endif()

get_filename_component(REPO_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/.." ABSOLUTE)
set(CGB_RUST_PROFILE "debug" CACHE STRING "Rust profile (debug or release)")
if(CGB_RUST_PROFILE STREQUAL "release")
    set(CGB_RUST_LIB_DIR "${REPO_ROOT}/target/release")
else()
    set(CGB_RUST_LIB_DIR "${REPO_ROOT}/target/debug")
endif()

add_executable(cgb-win WIN32
    src/main.cpp
    src/WinWindow.cpp
    src/Input.cpp
    src/Gamepads.cpp
    src/LaunchOptions.cpp
)

target_compile_definitions(cgb-win PRIVATE
    UNICODE
    _UNICODE
    NOMINMAX
    _WIN32_WINNT=0x0A00
    WINVER=0x0A00
)

target_include_directories(cgb-win PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/src"
    "${REPO_ROOT}/src/native/include"
)

target_link_directories(cgb-win PRIVATE "${CGB_RUST_LIB_DIR}")

target_link_libraries(cgb-win PRIVATE
    # The Rust static library. It carries the igui UI, wgpu and cgb-libretro.
    cgb_app
    # Win32 / IME / drag & drop / XInput. System libs the Rust std + wgpu need
    # are listed too because a static library does not carry link directives.
    user32 gdi32 shell32 ole32 oleaut32 advapi32 comctl32
    imm32 ws2_32 bcrypt userenv ntdll
    xinput1_4
)

# Match the Rust staticlib's C runtime. The `x86_64-pc-windows-msvc` target
# defaults to the dynamic CRT, so use /MD (/MDd in Debug).
set_property(TARGET cgb-win PROPERTY
    MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>DLL")

if(MSVC)
    # The Unicode entry point (`wWinMain`).
    target_link_options(cgb-win PRIVATE "/ENTRY:wWinMainCRTStartup")
endif()
