#!/usr/bin/env bash
# Build the Rust FFI library, then the Swift executable.
#
#   macos/scripts/build.sh              # debug
#   CGB_RUST_PROFILE=release macos/scripts/build.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROFILE="${CGB_RUST_PROFILE:-debug}"

cd "$ROOT"
if [[ "$PROFILE" == "release" ]]; then
    cargo build -p cgb-mac --release
    swift build --package-path macos -c release
else
    cargo build -p cgb-mac
    swift build --package-path macos
fi

echo "built: $ROOT/macos/.build/$PROFILE/cgb-mac"
