#!/bin/bash
# ---------------------------------------------------------------------------
# Build everything, then test everything. One command.
#
#   ./scripts/build-all.sh                 the whole pipeline
#   ./scripts/build-all.sh --fast          skip the two third-party cores
#   ./scripts/build-all.sh --verify        also run the pixel/audio parity checks
#   ./scripts/build-all.sh --skip-tests    build only
#   ./scripts/build-all.sh --clean         throw the build trees away first
#   ./scripts/build-all.sh --rom game.nes  which ROM the real-ROM tests use
#   ./scripts/build-all.sh -j 4            how many jobs (default: this machine)
#
# What "everything" means here, and why each piece is its own step:
#
#   1. native C++        build/           fc_core, fc_libretro, the eleven demos,
#                                         fc_headless / fc_testrom / the probe
#      ctest             517 tests         every unit, instruction, timing and
#                                         integration test in the two packages
#   2. wasm              wasm/dist/       fc_core + fc_libretro -- the modules
#                                         the renderer actually loads
#   3. third-party cores wasm/dist/       Mesen and mGBA as libretro modules.
#                                         Same ABI, so no front-end change
#   4. Electron          electron/dist*   main process, renderer, gamepad helper
#      node --test       111 tests         library, input, cheats, save states,
#                                         the cores table, the core host
#      test:native                         the gamepad protocol tests
#   5. smoke tests                        the libretro ABI against a synthetic
#                                         ROM, then a real ROM end to end
#
# --verify adds the two scripts that compare builds against each other rather
# than just running them: wasm/verify.sh (native vs wasm, pixels and audio, byte
# for byte, with and without a save/reload) and electron/verify.sh (the window
# against the native build). They take minutes and want a real display session,
# so they are not in the default run.
#
# Not in here, on purpose:
#
#   .dmg / .zip        that is ./scripts/release.sh, which runs the build steps
#                      it needs and then packages
#   MAME 2003-Plus     the module compiles (docs/architecture/mame-integration.md
#                      §2.2 records the recipe) but nothing can load it yet: the
#                      core requires a real path in a filesystem (need_fullpath)
#                      and this front end hands cores a pointer. The doc's §6
#                      M0/M1 is the work that comes first. Building it here would
#                      produce a 17.8 MB artifact that no code path opens.
#
# Step 3 is the slow one (minutes, plus a one-time clone) and nothing under
# packages/ can change it, which is what --fast is for. Every step is
# incremental; --clean is the only destructive option, and it only removes
# build trees.
#
# Requires: cmake, ninja, node, pnpm, git, and third_party/emsdk. The check
# below says exactly how to install whichever is missing.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ELECTRON="$ROOT/electron"
BUILD="$ROOT/build"
SUMMARY="$(mktemp -t build-all)"

FAST=0
VERIFY=0
SKIP_TESTS=0
CLEAN=0
ROM=""
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)"

usage() {
    awk 'NR > 1 && /^set -euo/ { exit } NR > 1 { sub(/^# ?/, ""); print }' "$0"
    exit "${1:-0}"
}

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
note() { printf '    %s\n' "$*"; }
ok() { printf '    \033[32mok\033[0m in %ss\n' "$1"; }
bad() { printf '    \033[31mfailed\033[0m after %ss\n' "$1" >&2; }

cleanup() { rm -f "$SUMMARY"; return 0; }
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Arguments
# ---------------------------------------------------------------------------
while [ $# -gt 0 ]; do
    case "$1" in
        --fast)        FAST=1 ;;
        --verify)      VERIFY=1 ;;
        --skip-tests)  SKIP_TESTS=1 ;;
        --clean)       CLEAN=1 ;;
        --rom)         ROM="${2:-}"; [ -n "$ROM" ] || die "--rom needs a path"; shift ;;
        -j|--jobs)     JOBS="${2:-}"; [ -n "$JOBS" ] || die "$1 needs a number"; shift ;;
        -h|--help)     usage 0 ;;
        *)             usage 2 ;;
    esac
    shift
done

# ---------------------------------------------------------------------------
# Preflight
#
# Checked up front and all at once, so a missing tool is one message at the
# start rather than a failure twenty seconds into step 4.
# ---------------------------------------------------------------------------
missing=0
for tool in cmake ninja node pnpm git; do
    if ! command -v "$tool" > /dev/null 2>&1; then
        printf 'error: %s is not on PATH\n' "$tool" >&2
        missing=1
    fi
done
if [ "$missing" -ne 0 ]; then
    die "install what is missing: brew install cmake ninja node, then 'corepack enable pnpm'"
fi

if [ ! -d "$ROOT/third_party/emsdk/upstream/emscripten" ]; then
    die "third_party/emsdk is missing, and the wasm build needs it:
    git clone --depth 1 https://github.com/emscripten-core/emsdk.git third_party/emsdk
    cd third_party/emsdk && ./emsdk install 6.0.9 && ./emsdk activate 6.0.9"
fi

# The ROM for the real-ROM tests. Same search order as wasm/verify.sh, -L so
# that the symlink in packages/fc-core/tests/data is followed to wherever the
# ROM folder now lives.
find_rom() {
    if [ -n "$ROM" ]; then
        [ -f "$ROM" ] || die "--rom '$ROM' is not a file"
        printf '%s' "$(cd "$(dirname "$ROM")" && pwd)/$(basename "$ROM")"
        return 0
    fi
    for candidate in "$ROOT/packages/fc-core/tests/data" "$HOME/Documents/Fc Game Library" "$HOME/Documents/FC games"; do
        hit="$(find -L "$candidate" -maxdepth 1 -iname '*.nes' -type f 2>/dev/null | sort | head -1)"
        if [ -n "$hit" ]; then
            printf '%s' "$hit"
            return 0
        fi
    done
    return 0
}

# ---------------------------------------------------------------------------
# The steps. Each one is a function so that `run` can time it, print its name
# and stop the whole pipeline on the first failure.
# ---------------------------------------------------------------------------

build_native() {
    cmake -S "$ROOT" -B "$BUILD" -G Ninja -DCMAKE_BUILD_TYPE=Debug
    cmake --build "$BUILD" --parallel "$JOBS"
}

test_native() {
    # FC_TEST_ROM only when it was asked for: the suite's tests skip themselves
    # when no ROM is there, and a test that skips is not a test that passed.
    [ -n "$ROM" ] && export FC_TEST_ROM="$ROM"
    ctest --test-dir "$BUILD" --output-on-failure
}

build_wasm() {
    bash "$ROOT/wasm/build.sh"
}

build_cores() {
    bash "$ROOT/wasm/mesen/build.sh"
    bash "$ROOT/wasm/mgba/build.sh"
}

build_electron() {
    # A fresh clone has no node_modules, and "the build failed" with an
    # unresolved import is a bad way to learn that pnpm install had not run.
    if [ ! -d "$ELECTRON/node_modules" ]; then
        note "electron/node_modules is missing; running pnpm install first"
        ( cd "$ELECTRON" && pnpm install )
    fi
    ( cd "$ELECTRON" && pnpm run typecheck && pnpm run build )
}

test_electron() {
    ( cd "$ELECTRON" && pnpm test && pnpm run test:native )
}

test_wasm() {
    # The ABI test carries its own synthetic NROM, so it always runs.
    node "$ROOT/wasm/libretro_test.mjs"

    rom="$(find_rom)"
    if [ -n "$rom" ]; then
        note "real ROM: $rom"
        node "$ROOT/wasm/headless.mjs" "$rom" --frames 120 --snapshots 1,60,120 --quiet
    else
        note "no .nes found; skipping the real-ROM run (--rom <path>, or link one into packages/fc-core/tests/data/)"
    fi

    # Both of these skip themselves, loudly, when handed no ROM.
    node "$ROOT/wasm/mesen_test.mjs" "$rom"
    node "$ROOT/wasm/mgba_test.mjs"
}

verify_wasm() {
    if [ -n "$ROM" ]; then
        bash "$ROOT/wasm/verify.sh" "$ROM"
    else
        bash "$ROOT/wasm/verify.sh"
    fi
}

verify_electron() {
    if [ -n "$ROM" ]; then
        bash "$ROOT/electron/verify.sh" "$ROM"
    else
        bash "$ROOT/electron/verify.sh"
    fi
}

run() {
    label="$1"
    shift
    step "$label"
    started=$SECONDS
    if "$@"; then
        elapsed=$((SECONDS - started))
        ok "$elapsed"
        printf 'ok|%s|%s\n' "$label" "$elapsed" >> "$SUMMARY"
    else
        code=$?
        elapsed=$((SECONDS - started))
        bad "$elapsed"
        printf 'failed|%s|%s\n' "$label" "$elapsed" >> "$SUMMARY"
        report
        exit "$code"
    fi
}

report() {
    printf '\n\033[1m─────────────────────────────────────────────────────────────\033[0m\n'
    while IFS='|' read -r status label elapsed; do
        if [ "$status" = "ok" ]; then
            printf '  \033[32mok\033[0m      %-34s %ss\n' "$label" "$elapsed"
        else
            printf '  \033[31mfailed\033[0m  %-34s %ss\n' "$label" "$elapsed"
        fi
    done < "$SUMMARY"
    printf '  %-40s %ss total\n' "" "$SECONDS"
}

# ---------------------------------------------------------------------------
# The pipeline
# ---------------------------------------------------------------------------
printf '\033[1mClassic Game Box -- build and test everything\033[0m\n'
note "root     : $ROOT"
note "jobs     : $JOBS"
if [ "$FAST" -eq 1 ]; then
    note "mode     : --fast (no Mesen / mGBA core rebuild)"
fi

if [ "$CLEAN" -eq 1 ]; then
    step "clean"
    rm -rf "$BUILD" "$ROOT/build-wasm" "$ROOT/electron/dist" "$ROOT/electron/dist-electron"
    note "removed build/, build-wasm/, electron/dist, electron/dist-electron"
fi

run "1/7 native C++ (core + libretro + demos)" build_native

if [ "$SKIP_TESTS" -eq 0 ]; then
    run "2/7 native C++ tests (ctest)" test_native
else
    note "--skip-tests: not running ctest"
fi

run "3/7 wasm cores (fc_core + fc_libretro)" build_wasm

if [ "$FAST" -eq 0 ]; then
    run "4/7 third-party cores (Mesen + mGBA)" build_cores
else
    note "--fast: reusing the Mesen / mGBA modules already in wasm/dist"
fi

run "5/7 Electron (main + renderer + gamepad)" build_electron

if [ "$SKIP_TESTS" -eq 0 ]; then
    run "6/7 front-end tests (node --test + native)" test_electron
    run "7/7 wasm smoke tests (ABI + real ROM)" test_wasm

    if [ "$VERIFY" -eq 1 ]; then
        run "verify: native vs wasm parity" verify_wasm
        run "verify: window vs native parity" verify_electron
    fi
else
    note "--skip-tests: not running the front-end or wasm tests"
fi

report

printf '\n'
if [ "$SKIP_TESTS" -eq 0 ] && [ "$VERIFY" -eq 0 ]; then
    note "the pixel-level parity checks are opt-in: re-run with --verify"
fi

printf '    \033[1mplay it\033[0m: cd electron && pnpm start\n'
printf '    \033[1mrelease it\033[0m: ./scripts/release.sh --dry-run\n'
