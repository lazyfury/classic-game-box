# ---------------------------------------------------------------------------
# Tests for the custom FC / NES core.
#
# Two gates, each of which can fail on its own:
#
#   custom_nes_core_tests      the machine + its private fc_* C ABI
#   custom_nes_core_abi_tests  the libretro ABI (retro_*)
#
# Both link the same object libraries the shipped core is built from, so a
# change that quietly breaks retro_* is caught here rather than only by a
# dlopen probe inside a front end.
# ---------------------------------------------------------------------------

include(${CMAKE_CURRENT_SOURCE_DIR}/../cmake/GoogleTest.cmake)

add_executable(custom_nes_core_tests
    core/test_bit.cpp
    core/test_alu.cpp
    core/test_registers.cpp
    core/test_cpu.cpp
    core/test_disassembler.cpp
    core/test_addressing.cpp
    core/test_instruction_set.cpp
    core/test_cycles.cpp
    core/test_nes_bus.cpp
    core/test_cartridge.cpp
    core/test_cheats.cpp
    core/test_real_rom.cpp
    core/test_ppu.cpp
    core/test_controller.cpp
    core/test_apu.cpp
    core/test_state.cpp
    core/test_ffi.cpp
)
target_link_libraries(custom_nes_core_tests PRIVATE
    custom_nes_core
    GTest::gtest_main
)

# Real-ROM tests find a .nes file here; nothing is committed, so those tests
# skip on their own when the folder is empty.
target_compile_definitions(custom_nes_core_tests PRIVATE
    FC_TEST_DATA_DIR="${CMAKE_CURRENT_SOURCE_DIR}/data"
)

add_executable(custom_nes_core_abi_tests
    libretro/test_libretro.cpp
    libretro/test_cheat_codes.cpp
)
target_link_libraries(custom_nes_core_abi_tests PRIVATE
    retro_adapter
    GTest::gtest_main
)

include(GoogleTest)
gtest_discover_tests(custom_nes_core_tests)
gtest_discover_tests(custom_nes_core_abi_tests)
