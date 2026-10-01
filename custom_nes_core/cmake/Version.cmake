# ---------------------------------------------------------------------------
# The version the C++ core reports.
#
# `custom_nes_core` reports this in `get_system_info` / `library_version`; the
# build fallback in `cores/custom_nes_core/build.sh` reads it from here so the
# clang++ build and the CMake build cannot disagree. It stays a normal (not
# cache) variable: a build can override it on the command line with
# -DFC_PROJECT_VERSION=... if that is ever useful.
# ---------------------------------------------------------------------------

if(NOT DEFINED FC_PROJECT_VERSION)
    set(FC_PROJECT_VERSION "0.1.3")
endif()
