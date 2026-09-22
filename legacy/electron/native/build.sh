#!/usr/bin/env bash
#
# ---------------------------------------------------------------------------
# Build the native gamepad helper.
#
#   pnpm run build:native
#
# This script is kept for muscle memory and for anyone (or any CI step) that
# calls `bash native/build.sh` directly. The real entry point is
# scripts/build-native.mjs, which does the same thing and works on Windows,
# where `bash` may not be installed.
#
# On macOS it builds the Swift helper in native/gamepad (Apple's
# GameController framework) into native/bin/fc-gamepad. Elsewhere it builds
# native/gamepad-cpp (XInput on Windows, an empty backend elsewhere) into
# native/bin/. Both write the same JSON Lines protocol.
# ---------------------------------------------------------------------------

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec node "$here/../scripts/build-native.mjs" "$@"
