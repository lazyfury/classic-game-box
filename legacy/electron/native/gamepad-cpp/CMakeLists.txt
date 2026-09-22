# ---------------------------------------------------------------------------
# fc-gamepad, in C++.
#
#   cmake -S native/gamepad-cpp -B native/gamepad-cpp/build
#   cmake --build native/gamepad-cpp/build --config Release
#
# The output is <build>/out/fc-gamepad (fc-gamepad.exe on Windows), which
# scripts/build-native.mjs copies to native/bin/ -- the fixed path the main
# process spawns.
#
# On Windows the backend is XInput; on every other platform it is the empty
# backend in src/backend_stub.cpp, which exists so the portable half can be
# built and tested anywhere. macOS uses the Swift helper instead; see
# native/build.sh.
# ---------------------------------------------------------------------------
cmake_minimum_required(VERSION 3.20)

project(fc-gamepad
    VERSION 0.1.0
    DESCRIPTION "Reads game controllers and writes JSON Lines to stdout"
    LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# A helper has no debug configuration worth the name: it either reads the pad
# or it does not. Release unless told otherwise, and never leave the build
# type unset (Windows multi-config generators choose it themselves).
if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "Build type" FORCE)
endif()

if(MSVC)
    add_compile_options(/W4)
else()
    add_compile_options(-Wall -Wextra -Wpedantic)
endif()

if(WIN32)
    set(FC_GAMEPAD_BACKEND src/backend_windows.cpp)
else()
    set(FC_GAMEPAD_BACKEND src/backend_stub.cpp)
endif()

add_executable(fc-gamepad
    src/main.cpp
    src/protocol.cpp
    src/reporter.cpp
    ${FC_GAMEPAD_BACKEND}
)
target_include_directories(fc-gamepad PRIVATE src)

if(WIN32)
    # xinput: the pad API. winmm: timeBeginPeriod, without which a 16ms sleep
    # can be served as two 15.6ms timer ticks and the helper reports at 30Hz.
    target_link_libraries(fc-gamepad PRIVATE xinput winmm)
endif()

# One deterministic path for the built binary, whatever generator is in use.
# A multi-config generator (Visual Studio) appends the configuration name to
# RUNTIME_OUTPUT_DIRECTORY unless each configuration is set explicitly, and
# the build script wants one path to copy from, not four to guess between.
set(FC_GAMEPAD_OUTPUT_DIR "${CMAKE_BINARY_DIR}/out")
foreach(config "" _DEBUG _RELEASE _RELWITHDEBINFO _MINSIZEREL)
    set_target_properties(fc-gamepad PROPERTIES
        "RUNTIME_OUTPUT_DIRECTORY${config}" "${FC_GAMEPAD_OUTPUT_DIR}")
endforeach()

# ---------------------------------------------------------------------------
# Tests. The reporter and the protocol are the parts with decisions in them;
# reading a real pad needs a pad and Windows, and is not something a test can
# do.
# ---------------------------------------------------------------------------
option(FC_GAMEPAD_TEST "Build the fc-gamepad tests" ON)
if(FC_GAMEPAD_TEST)
    enable_testing()
    add_executable(fc-gamepad-test
        test/reporter_test.cpp
        src/protocol.cpp
        src/reporter.cpp
    )
    target_include_directories(fc-gamepad-test PRIVATE src)
    add_test(NAME fc-gamepad-report COMMAND fc-gamepad-test)
endif()
