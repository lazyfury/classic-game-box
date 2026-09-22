#!/bin/bash
# Build the native libretro core(s) into cores/dist.
#
# Mesen (NES) is the Q1 target and is built by default.
#
# mGBA (GB/GBA) is deferred to Q4 — pass --with-mgba to build it too. Its
# script is kept in tree but is not exercised yet; see cores/mgba/build.sh.
#
# Takes minutes per core on first run and needs the network once. Run it before
# `cargo run`: the app refuses to start a game whose core is missing, with a
# message pointing back here.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

WITH_MGBA=0
for arg in "$@"; do
    case "$arg" in
        --with-mgba) WITH_MGBA=1 ;;
        *) echo "usage: $0 [--with-mgba]" >&2; exit 2 ;;
    esac
done

"$ROOT/cores/mesen/build.sh"

if [ "$WITH_MGBA" = "1" ]; then
    "$ROOT/cores/mgba/build.sh"
else
    echo
    echo "==> mGBA skipped (GB/GBA is deferred; pass --with-mgba to build it)"
fi

echo
echo "cores:"
ls -la "$ROOT/cores/dist"
