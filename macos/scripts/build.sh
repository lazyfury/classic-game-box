#!/usr/bin/env bash
# Build the Rust FFI library, then the Swift executable.
#
#   macos/scripts/build.sh              # debug
#   CGB_RUST_PROFILE=release macos/scripts/build.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROFILE="${CGB_RUST_PROFILE:-debug}"
# Match the Swift package's deployment target so the staticlib's objects are
# not built for a newer SDK than the app links against.
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"

cd "$ROOT"
if [[ "$PROFILE" == "release" ]]; then
    cargo build --release
    swift build --package-path macos -c release
else
    cargo build
    swift build --package-path macos
fi

echo "built: $ROOT/macos/.build/$PROFILE/cgb-mac"
