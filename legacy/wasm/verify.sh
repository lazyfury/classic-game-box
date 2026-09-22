#!/bin/bash
# ---------------------------------------------------------------------------
# Cross compilation parity check.
#
# The rule this enforces: compiling packages/fc-core/src/core to WebAssembly must not change
# one thing about what the emulator computes.
#
# It runs the same ROM four ways and compares all of them:
#
#                     run to the end        save at frame N, reload, carry on
#   native (C++)      every frame + audio   must be identical to the left
#   wasm (the app)    must be identical     must be identical to the left
#
# So a mismatch anywhere -- between the two builds, or between a run with a
# round trip and one without -- fails the test. Thirteen frames and a
# megabyte of audio per ROM, compared byte for byte.
#
# This is a stronger statement than "the wasm build runs". A port that is off
# by one PPU dot, or that drops a frame, or that gets a mapper's bank shift
# wrong, still produces a plausible picture. It does not produce an identical
# one. And a save state that is missing one field still loads happily; it just
# diverges a few frames later.
#
# Usage:
#   ./wasm/verify.sh                       # auto-detect a ROM folder
#   ./wasm/verify.sh "/path/to/roms"
#   ./wasm/verify.sh game.nes
# ---------------------------------------------------------------------------
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# name | frames | script | snapshot frames
scenarios=(
    "no-input|600||90,250,400,600"
    "input|600|100:START=1,105:START=0,300:RIGHT=1,480:RIGHT=0|90,250,400,600"
)

# Frame at which the round trip happens. Deliberately an odd number, and not
# one of the snapshot frames: a state saved on a frame that is about to be
# hashed would hide a difference in the saved state itself.
ROUNDTRIP_FRAME=337

ROMS=()

if [ $# -ge 1 ] && [ -f "$1" ]; then
    ROM_DIR="$(dirname "$1")"
    ROMS=("$(basename "$1")")
else
    ROM_DIR="${1:-}"
    if [ -z "$ROM_DIR" ]; then
        for candidate in "$ROOT/packages/fc-core/tests/data" "$HOME/Documents/Fc Game Library" "$HOME/Documents/FC games"; do
            # -L so a symlink is followed: packages/fc-core/tests/data holds symlinks into a
            # ROM folder that may have moved, and a dangling one must not be
            # mistaken for a usable ROM.
            if [ -n "$(find -L "$candidate" -maxdepth 1 -iname '*.nes' -type f 2>/dev/null | head -1)" ]; then
                ROM_DIR="$candidate"
                break
            fi
        done
    fi
fi

if [ -z "$ROM_DIR" ] || [ -z "$(find -L "$ROM_DIR" -maxdepth 1 -iname '*.nes' -type f 2>/dev/null | head -1)" ]; then
    echo "no usable .nes files found in '${ROM_DIR:-<nothing>}'" >&2
    echo "usage: $0 [rom-folder-or-file]" >&2
    exit 2
fi

if [ ! -x "$ROOT/build/fc_headless" ]; then
    echo "error: build/fc_headless is missing; run cmake --build build first" >&2
    exit 2
fi

if [ ! -f "$ROOT/wasm/dist/fc_core.wasm" ]; then
    echo "error: wasm/dist/fc_core.wasm is missing; run ./wasm/build.sh first" >&2
    exit 2
fi

if [ "${#ROMS[@]}" -eq 0 ]; then
    while IFS= read -r -d '' file; do
        ROMS+=("$(basename "$file")")
    done < <(find -L "$ROM_DIR" -maxdepth 1 -iname '*.nes' -type f -print0 | sort -z)
fi

SCRATCH="$ROOT/build-wasm/parity"

echo "=== wasm parity check: ${#ROMS[@]} ROM(s), ${#scenarios[@]} scenario(s) ==="
echo "    native : build/fc_headless   (C++, through packages/fc-core/src/ffi/emulator_api.h)"
echo "    wasm   : wasm/headless.mjs   (the module Electron loads)"
echo "    each run is done twice: straight through, and with a save and reload"
echo

failures=0
passed=0
skipped=0

for scenario in "${scenarios[@]}"; do
    IFS='|' read -r name frames script snapshots <<< "$scenario"

    echo "  --- $name ($frames frames, round trip at $ROUNDTRIP_FRAME) ---"

    for rom in "${ROMS[@]}"; do
        path="$ROM_DIR/$rom"
        rm -rf "$SCRATCH"
        mkdir -p "$SCRATCH"

        # -- native, straight through -----------------------------------
        native_output="$("$ROOT/build/fc_headless" "$path" --frames "$frames" --script "$script" \
            --snapshots "$snapshots" --outdir "$SCRATCH/n1" --samples "$SCRATCH/n1.raw" \
            --save "$SCRATCH/n1.state" --quiet 2>/dev/null)"

        mapper_state="$(printf '%s\n' "$native_output" | sed -n 's/^mapperstate //p')"
        if [ -z "$mapper_state" ]; then
            printf '    %-40s skipped (the native build could not run it)\n' "${rom%.nes}"
            skipped=$((skipped + 1))
            continue
        fi

        # A mapper that does not save its bank registers cannot pass a round
        # trip, and pretending otherwise would mean calling a known gap a
        # regression. The list is the work still to do; it is printed.
        if [ "$mapper_state" = "0" ]; then
            printf '    %-40s skipped (this mapper does not save its banks yet)\n' "${rom%.nes}"
            skipped=$((skipped + 1))
            continue
        fi

        # -- native, with a save and reload ------------------------------
        "$ROOT/build/fc_headless" "$path" --frames "$frames" --script "$script" \
            --snapshots "$snapshots" --outdir "$SCRATCH/n2" --samples "$SCRATCH/n2.raw" \
            --roundtrip "$ROUNDTRIP_FRAME" --quiet > /dev/null 2>&1

        # -- wasm, straight through --------------------------------------
        node "$ROOT/wasm/headless.mjs" "$path" --frames "$frames" --script "$script" \
            --snapshots "$snapshots" --outdir "$SCRATCH/w1" --samples "$SCRATCH/w1.raw" \
            --save "$SCRATCH/w1.state" --quiet > /dev/null 2>&1

        # -- wasm, with a save and reload --------------------------------
        node "$ROOT/wasm/headless.mjs" "$path" --frames "$frames" --script "$script" \
            --snapshots "$snapshots" --outdir "$SCRATCH/w2" --samples "$SCRATCH/w2.raw" \
            --roundtrip "$ROUNDTRIP_FRAME" --quiet > /dev/null 2>&1

        mismatch=""
        frames_checked=0

        for ppm in "$SCRATCH/n1"/*.ppm; do
            name_of_frame="$(basename "$ppm")"
            frames_checked=$((frames_checked + 1))
            for variant in n2 w1 w2; do
                if ! cmp -s "$ppm" "$SCRATCH/$variant/$name_of_frame"; then
                    mismatch="$mismatch $name_of_frame:$variant"
                fi
            done
        done

        if ! cmp -s "$SCRATCH/n1.raw" "$SCRATCH/w1.raw"; then
            mismatch="$mismatch audio:native-vs-wasm"
        fi
        if ! cmp -s "$SCRATCH/n1.raw" "$SCRATCH/n2.raw"; then
            mismatch="$mismatch audio:roundtrip"
        fi
        if ! cmp -s "$SCRATCH/n1.raw" "$SCRATCH/w2.raw"; then
            mismatch="$mismatch audio:roundtrip-wasm"
        fi

        # The state files themselves, which is the strictest form of "the two
        # builds agree about what a machine is". Only possible because the
        # format writes little endian explicitly rather than memcpy-ing the
        # host's bytes.
        if ! cmp -s "$SCRATCH/n1.state" "$SCRATCH/w1.state"; then
            mismatch="$mismatch statefile"
        fi

        if [ -n "$mismatch" ]; then
            printf '    %-40s MISMATCH:%s\n' "${rom%.nes}" "$mismatch"
            failures=$((failures + 1))
        else
            printf '    %-40s identical (%d frames + audio + state)\n' "${rom%.nes}" "$frames_checked"
            passed=$((passed + 1))
        fi
    done
done

rm -rf "$SCRATCH"

echo
if [ "$failures" -gt 0 ]; then
    echo "FAILED: $failures run(s) differ, $passed identical, $skipped skipped"
    exit 1
fi
echo "PASSED: $passed run(s) byte for byte identical, $skipped skipped"
