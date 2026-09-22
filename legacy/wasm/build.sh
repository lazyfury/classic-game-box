#!/bin/bash
# ---------------------------------------------------------------------------
# Build the WebAssembly module.
#
# The output is wasm/dist/fc_core.mjs plus wasm/dist/fc_core.wasm. That one
# pair is loaded by three things:
#
#   * the Electron renderer, so it can draw and play sound
#   * `node wasm/headless.mjs`, which is how we regression test it
#   * a plain web page, if you want to put it on one
#
# Same artifact in all three. That is the reason for doing this in wasm rather
# than as a native helper process: what we test is what we ship.
#
# Usage:
#   ./wasm/build.sh              Release
#   ./wasm/build.sh Debug        Debug (slow, but readable in DevTools)
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EMSDK="$ROOT/third_party/emsdk"
BUILD="$ROOT/build-wasm"
CONFIG="${1:-Release}"

if [ ! -d "$EMSDK/upstream/emscripten" ]; then
    echo "error: emsdk not found at $EMSDK" >&2
    echo >&2
    echo "  git clone --depth 1 https://github.com/emscripten-core/emsdk.git third_party/emsdk" >&2
    echo "  cd third_party/emsdk && ./emsdk install 6.0.9 && ./emsdk activate 6.0.9" >&2
    exit 1
fi

# emsdk_env.sh prints a banner on stdout, and it is chatty. Keep its output
# out of ours unless something goes wrong.
# shellcheck disable=SC1091
source "$EMSDK/emsdk_env.sh" > /dev/null 2>&1

echo "==> emscripten $(emcc --version | head -1 | sed 's/.*) //')"

emcmake cmake -S "$ROOT" -B "$BUILD" -G Ninja \
    -DCMAKE_BUILD_TYPE="$CONFIG" \
    -DFC_BUILD_TESTS=OFF \
    > /dev/null

cmake --build "$BUILD"

echo "==> done"
ls -la "$ROOT/wasm/dist"
