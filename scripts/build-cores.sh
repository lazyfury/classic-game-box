#!/bin/bash
# Build the native libretro cores into cores/dist.
#
# Mesen (NES) and mGBA (GB/GBA) are built by default; `--skip-mgba` skips
# mGBA (it needs cmake). The `--with-mgba` spelling from when mGBA was
# deferred still parses, and now changes nothing.
#
# Takes minutes per core on first run and needs the network once. Run it before
# `cargo run`: the app refuses to start a game whose core is missing, with a
# message pointing back here.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

BUILD_MGBA=1
for arg in "$@"; do
    case "$arg" in
        --skip-mgba) BUILD_MGBA=0 ;;
        --with-mgba) BUILD_MGBA=1 ;; # back-compat: mGBA is the default now
        *) echo "usage: $0 [--skip-mgba]" >&2; exit 2 ;;
    esac
done

"$ROOT/cores/mesen/build.sh"

if [ "$BUILD_MGBA" = "1" ]; then
    "$ROOT/cores/mgba/build.sh"
else
    echo
    echo "==> mGBA skipped (--skip-mgba)"
fi

# Custom cores: every cores/custom/<name>/build.sh, in name order. Each one
# drops its module in cores/dist and is declared in cores/custom/cores.json;
# see cores/custom/README.md.
shopt -s nullglob
custom=("$ROOT"/cores/custom/*/build.sh)
if [ ${#custom[@]} -eq 0 ]; then
    echo
    echo "==> no custom cores (add cores/custom/<name>/build.sh + cores/custom/cores.json)"
else
    for script in "${custom[@]}"; do
        echo
        echo "==> custom core: $(basename "$(dirname "$script")")"
        "$script"
    done
fi

echo
echo "cores:"
ls -la "$ROOT/cores/dist"
