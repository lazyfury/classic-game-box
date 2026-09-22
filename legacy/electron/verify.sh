#!/bin/bash
# ---------------------------------------------------------------------------
# End to end parity: the window against the native build.
#
#   ./electron/verify.sh                       # auto-detect a ROM folder
#   ./electron/verify.sh "/path/to/roms"
#   ./electron/verify.sh game.nes
#
# What it checks
# --------------
# For each scenario, run a ROM through ./build/fc_headless, hash the pixels of
# every snapshot frame. Run the same ROM through the real Electron application
# with the same scenario, capture the picture off the canvas, hash the same
# pixels. The hashes must be equal.
#
# That is one step further out than wasm/verify.sh, and it is the step that
# matters, because it covers everything the wasm check does not: the emscripten
# loader finding its .wasm over the app:// protocol, the renderer's 2D context,
# the JavaScript that reshuffles B,G,R,X into R,G,B,A, and the canvas itself.
#
# A cycle count alone would not catch a display bug, and a screenshot would not
# catch a one-pixel shift. A hash of every pixel catches both.
#
# The second scenario presses buttons. Without it, every test in this
# repository would still pass on an emulator that ignored input entirely.
#
# The window is hidden for this, so nothing flashes on screen while it runs.
# ---------------------------------------------------------------------------
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ELECTRON="$ROOT/electron"

# name | frames | script | snapshot frames
#
# The no-input run is the baseline, and the input run is checked against it in
# a second way: if the two produce the same hash, the buttons never reached the
# console, and a test that only compared Electron against native would not
# notice, because both would be equally deaf.
scenarios=(
    "no-input|600||90,250,400,600"
    "input|600|100:START=1,105:START=0,300:RIGHT=1,480:RIGHT=0|90,250,400,600"
)

ROMS=()

if [ $# -ge 1 ] && [ -f "$1" ]; then
    ROM_DIR="$(dirname "$1")"
    ROMS=("$(basename "$1")")
else
    ROM_DIR="${1:-}"
    if [ -z "$ROM_DIR" ]; then
        for candidate in "$ROOT/packages/fc-core/tests/data" "$HOME/Documents/Fc Game Library" "$HOME/Documents/FC games"; do
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

if [ ! -d "$ELECTRON/node_modules" ]; then
    echo "error: electron/node_modules is missing; run (cd electron && pnpm install)" >&2
    exit 2
fi

if ! command -v pnpm > /dev/null 2>&1; then
    echo "error: pnpm is not on PATH; this front end is built with pnpm" >&2
    exit 2
fi

echo "==> building the renderer (pnpm run build)"
# Shown, not silenced. The first build compiles the Swift gamepad helper, which
# can take a minute; with its output hidden that minute looks like a hang.
(cd "$ELECTRON" && pnpm run build) || {
    echo "error: the Electron build failed; see the output above" >&2
    exit 2
}

if [ "${#ROMS[@]}" -eq 0 ]; then
    while IFS= read -r -d '' file; do
        ROMS+=("$(basename "$file")")
    done < <(find -L "$ROM_DIR" -maxdepth 1 -iname '*.nes' -type f -print0 | sort -z)
fi

SCRATCH="$ELECTRON/.verify"
rm -rf "$SCRATCH"

# The reference is build/fc_headless -- the home-grown FC core -- so this run
# must use that core too, even though Mesen is the default NES core. A
# throwaway user data directory with the FC core pinned does that, and it also
# means a developer's own config.json (which could name any core) cannot change
# what this test compares. It lives outside the repository so there is nothing
# to clean up but the directory itself.
USERDATA="$(mktemp -d "${TMPDIR:-/tmp}/classic-game-box-verify.XXXXXX")"
trap 'rm -rf "$USERDATA"' EXIT
printf '{ "cores": { "nes": "fc" } }\n' > "$USERDATA/config.json"

echo "=== electron parity check: ${#ROMS[@]} ROM(s), ${#scenarios[@]} scenario(s) ==="
echo "    native   : build/fc_headless        (the C API, no JavaScript at all)"
echo "    electron : the application itself   (hashes the canvas pixels and the APU samples)"
echo

failures=0
passed=0

for scenario in "${scenarios[@]}"; do
    IFS='|' read -r name frames script snapshots <<< "$scenario"

    echo "  --- $name ($frames frames) ---"

    for rom in "${ROMS[@]}"; do
        path="$ROM_DIR/$rom"
        rm -rf "$SCRATCH"

        # --- the native reference ---------------------------------------
        "$ROOT/build/fc_headless" "$path" --frames "$frames" --script "$script" \
            --snapshots "$snapshots" --outdir "$SCRATCH" --samples "$SCRATCH/samples.raw" \
            --quiet > /dev/null 2>&1

        native_hashes=""
        saw_frames=0
        for frame in ${snapshots//,/ }; do
            ppm="$SCRATCH/frame_$frame.ppm"
            if [ ! -f "$ppm" ]; then
                continue
            fi
            saw_frames=$((saw_frames + 1))
            # Skip the 15 byte PPM header: "P6\n256 240\n255\n". What remains
            # is the RGB triples, which is byte for byte what the renderer
            # hashes.
            native_hashes="$native_hashes$frame $(tail -c +16 "$ppm" | shasum -a 256 | cut -d' ' -f1)"$'\n'
        done

        if [ "$saw_frames" -eq 0 ]; then
            printf '    %-40s skipped (native build could not run it)\n' "${rom%.nes}"
            continue
        fi

        # The audio the APU produced, as raw float32. Hashed rather than
        # shipped back through IPC: 440,000 samples is two megabytes of
        # structured clone for a number that fits in 64 hex digits.
        native_audio_hash="$(shasum -a 256 "$SCRATCH/samples.raw" | cut -d' ' -f1)"

        # Say so before the app starts, so a slow launch is visibly working
        # rather than apparently stuck.
        printf '    %-40s native ok, running the app...\n' "${rom%.nes}"

        # --- the application ---------------------------------------------
        # --no-gamepad: no controller is part of a parity run, and on macOS
        # the native helper can keep the process tree alive after the window
        # closes -- which is exactly what a script capturing stdout must not
        # be left waiting on.
        output="$(cd "$ELECTRON" && pnpm exec electron --user-data-dir="$USERDATA" . \
            --rom "$path" --selftest "$frames" \
            --script "$script" --snapshots "$snapshots" --no-gamepad 2>/dev/null)"
        electron_hashes="$(printf '%s\n' "$output" | sed -n 's/^hash //p')"
        electron_audio_hash="$(printf '%s\n' "$output" | awk '/^audio [0-9a-f]+$/ { print $2 }' | head -1)"

        if [ -z "$electron_hashes" ]; then
            printf '    %-40s FAILED (the application reported nothing)\n' "${rom%.nes}"
            failures=$((failures + 1))
            continue
        fi

        if [ -z "$electron_audio_hash" ]; then
            printf '    %-40s FAILED (the application reported no audio hash)\n' "${rom%.nes}"
            failures=$((failures + 1))
            continue
        fi

        mismatch="$(comm -3 \
            <(printf '%s' "$native_hashes" | grep -v '^$' | sort) \
            <(printf '%s\n' "$electron_hashes" | sort) | head -4)"

        if [ "$electron_audio_hash" != "$native_audio_hash" ]; then
            mismatch="$mismatch audio"
        fi

        if [ -n "$mismatch" ]; then
            printf '    %-40s MISMATCH\n' "${rom%.nes}"
            printf '%s\n' "$mismatch" | sed 's/^/        /'
            failures=$((failures + 1))
        else
            printf '    %-40s identical (%d frames + audio)\n' "${rom%.nes}" "$saw_frames"
            passed=$((passed + 1))
        fi
    done
done

rm -rf "$SCRATCH"

echo
if [ "$failures" -gt 0 ]; then
    echo "FAILED: $failures run(s) differ, $passed identical"
    exit 1
fi
echo "PASSED: $passed run(s) identical pixel for pixel and sample for sample"
