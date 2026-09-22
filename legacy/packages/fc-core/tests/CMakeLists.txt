# ---------------------------------------------------------------------------
# fc-core 的单元测试
#
# 这些测试只碰 fc_core 与 fc_ffi —— 机器本身，以及它那层纯 C 接口。
# libretro 适配层的测试在同级仓库的另一个项目里
# （packages/fc-libretro/tests），因为它测的是另一道门。
# ---------------------------------------------------------------------------

include(${CMAKE_CURRENT_SOURCE_DIR}/../../../cmake/GoogleTest.cmake)

add_executable(fc_core_tests
    test_bit.cpp
    test_alu.cpp
    test_registers.cpp
    test_cpu.cpp
    test_disassembler.cpp
    test_addressing.cpp
    test_instruction_set.cpp
    test_cycles.cpp
    test_nes_bus.cpp
    test_cartridge.cpp
    test_cheats.cpp
    test_real_rom.cpp
    test_ppu.cpp
    test_controller.cpp
    test_apu.cpp
    test_state.cpp
    test_ffi.cpp
)

target_link_libraries(fc_core_tests PRIVATE
    fc_ffi
    fc_core
    GTest::gtest_main
)

# 真实 ROM 测试去哪里找 .nes 文件。仓库里不提交任何 ROM；文件夹为空时
# 那些测试会自己 skip。
target_compile_definitions(fc_core_tests PRIVATE
    FC_TEST_DATA_DIR="${CMAKE_CURRENT_SOURCE_DIR}/data"
)

include(GoogleTest)
gtest_discover_tests(fc_core_tests)
