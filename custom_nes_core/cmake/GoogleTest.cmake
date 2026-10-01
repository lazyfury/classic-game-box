# ---------------------------------------------------------------------------
# Find GoogleTest, once, for whichever package is asking.
#
# Both packages have tests, and both may be configured in the same build (the
# monorepo root adds them one after the other). Without the guard at the top,
# the second one would run `find_package` again and, worse, `FetchContent` a
# second copy of googletest into its own build tree. The guard makes the file
# idempotent: the first package to be configured pays for it, the rest reuse
# the imported target.
#
# A system-installed GoogleTest is preferred (e.g. `brew install googletest`),
# because it is a compile-time dependency of a test, not of the shipped core.
# When it is missing we fall back to fetching it, so a fresh clone still has a
# working `ctest`.
# ---------------------------------------------------------------------------

if(NOT TARGET GTest::gtest_main)
    find_package(GTest QUIET)

    if(NOT GTest_FOUND)
        message(STATUS "System GoogleTest not found, fetching from GitHub...")
        include(FetchContent)
        FetchContent_Declare(
            googletest
            GIT_REPOSITORY https://github.com/google/googletest.git
            GIT_TAG        v1.15.2
        )
        set(gtest_force_shared_crt ON CACHE BOOL "" FORCE)
        FetchContent_MakeAvailable(googletest)
    endif()
endif()
