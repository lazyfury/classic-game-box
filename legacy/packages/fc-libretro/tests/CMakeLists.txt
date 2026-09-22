# ---------------------------------------------------------------------------
# fc-libretro 的单元测试
#
# 这里测的是 ABI：retro_* 那扇门。它和 packages/fc-core/tests 里的
# test_ffi.cpp 问的是同一个问题，但走另一道门，两者可以各自失败 —— 一个
# 改动了 fc_* 却悄悄弄坏 retro_* 的提交，只会在别人的电脑、别人的模拟器上
# 出现。
#
# 适配层的源码被直接编进测试二进制（通过 fc_libretro_core 这个 OBJECT
# library），所以 ABI 由普通的 ctest 覆盖，而不是只有 fc_libretro_probe
# 那个 dlopen 探针才发现问题。
# ---------------------------------------------------------------------------

include(${CMAKE_CURRENT_SOURCE_DIR}/../../../cmake/GoogleTest.cmake)

add_executable(fc_libretro_tests
    test_libretro.cpp
    test_cheat_codes.cpp
)

target_link_libraries(fc_libretro_tests PRIVATE
    fc_libretro_core
    GTest::gtest_main
)

include(GoogleTest)
gtest_discover_tests(fc_libretro_tests)
