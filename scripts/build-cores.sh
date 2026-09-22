#!/bin/bash
# Build both native libretro cores into cores/dist.
#
# Takes minutes per core on first run and needs the network once. Run it before
# `cargo run`: the app refuses to start a game whose core is missing, with a
# message pointing back here.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

"$ROOT/cores/mesen/build.sh"
"$ROOT/cores/mgba/build.sh"

echo
echo "cores:"
ls -la "$ROOT/cores/dist"
