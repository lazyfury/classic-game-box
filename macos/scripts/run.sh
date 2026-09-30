#!/usr/bin/env bash
# Build (if needed) and run the Swift client.
#
#   macos/scripts/run.sh /path/to/game.nes
#   macos/scripts/run.sh /path/to/game.gba cores/dist/mgba_libretro.dylib
#
# The core may also come from `CGB_CORE`.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROFILE="${CGB_RUST_PROFILE:-debug}"

"$ROOT/macos/scripts/build.sh"
cd "$ROOT"
exec "$ROOT/macos/.build/$PROFILE/cgb-mac" "$@"
