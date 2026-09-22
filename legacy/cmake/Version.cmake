# ---------------------------------------------------------------------------
# The version the C++ core reports.
#
# This is a monorepo: `packages/fc-core` and `packages/fc-libretro` are separate
# CMake projects, each with its own `project()` call and each buildable on its
# own. Both take their version from here, and the root project does too, so the
# core and the libretro core's `library_version` cannot disagree with each
# other.
#
# There is one other file that has to carry the version, and it is not this
# one: `electron/package.json`, because npm and electron-builder read it from
# there. Two files, one for CMake and one for npm, is the minimum this
# toolchain allows -- so `scripts/release.sh` writes both, and treats a
# disagreement between them as a version to fix rather than a no-op.
#
# `FC_PROJECT_VERSION` is a normal (not cache) variable: a package built on its
# own gets the value below, and a build can override it on the command line
# with -DFC_PROJECT_VERSION=... if that is ever useful.
# ---------------------------------------------------------------------------

if(NOT DEFINED FC_PROJECT_VERSION)
    set(FC_PROJECT_VERSION "0.1.3")
endif()
