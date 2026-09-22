#!/bin/bash
# Build every native libretro core into cores/dist.
#
# Each core is a directory with its own build.sh: cores/<name>/build.sh.
# This script just runs them all, in name order, and leaves the modules in
# cores/dist. Which cores the app knows about is declarative — see
# cores/cores.json and cores/README.md.
#
# `--skip-mgba` skips mGBA (it needs cmake). The `--with-mgba` spelling from
# when mGBA was deferred still parses; mGBA is the default now.
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

shopt -s nullglob
scripts=("$ROOT"/cores/*/build.sh)
if [ ${#scripts[@]} -eq 0 ]; then
    echo "error: no cores/*/build.sh found" >&2
    exit 1
fi

for script in "${scripts[@]}"; do
    name="$(basename "$(dirname "$script")")"
    if [ "$name" = "mgba" ] && [ "$BUILD_MGBA" = "0" ]; then
        echo
        echo "==> skipped: $name (--skip-mgba)"
        continue
    fi
    echo
    echo "==> core: $name"
    "$script"
done

echo
echo "cores:"
ls -la "$ROOT/cores/dist"
