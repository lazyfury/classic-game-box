#!/bin/bash
# ---------------------------------------------------------------------------
# Install PPSSPP (PlayStation Portable) as a native macOS arm64 libretro core.
#
# Output:   cores/dist/ppsspp_libretro.dylib
#           cores/dist/ppsspp/  (the upstream `assets/` tree the core reads from
#           `<system dir>/PPSSPP/`; the app seeds it there at startup)
# Manifest: cores/cores.json, key `ppsspp`, system `psp`.
#
# Why this downloads instead of building from source: the upstream libretro
# Makefile is not arm64-ready. `libretro/Makefile` coerces every TARGET_ARCH
# containing "64" to `x86_64` (lines 17-18), and its macOS ffmpeg path
# (`ffmpeg/macosx/$(TARGET_ARCH)`) has no `arm64` directory in
# hrydgard/ppsspp-ffmpeg (only `universal`). The libretro buildbot's
# `apple/osx/arm64` artifact is the maintained arm64 build. It links only
# desktop OpenGL.framework (no GLES/EGL, no Vulkan/MoltenVK), so it uses the
# front end's offscreen CGL GL path — the same one ParaLLEl-N64 uses. See
# docs/architecture/n64-gl-hw-render-plan.md for that path.
#
# Usage:
#   ./cores/ppsspp/build.sh
#   PPSSPP_URL=https://… ./cores/ppsspp/build.sh   # pin a different build
#
# Requires: network, Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
NAME="ppsspp_libretro.dylib"
URL="${PPSSPP_URL:-https://buildbot.libretro.com/nightly/apple/osx/arm64/latest/ppsspp_libretro.dylib.zip}"

mkdir -p "$OUT"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> downloading $URL"
curl -fL --retry 3 -o "$TMP/ppsspp.zip" "$URL"

echo "==> extracting $NAME"
unzip -oq "$TMP/ppsspp.zip" -d "$TMP"
cp "$TMP/$NAME" "$OUT/$NAME"

# Verify what we installed: an arm64 Mach-O exporting the libretro ABI.
file "$OUT/$NAME"
count="$(nm -gU "$OUT/$NAME" | grep -c '_retro_')"
if [ "$count" -lt 20 ]; then
    echo "error: $NAME does not look like a libretro core ($count retro_* symbols)" >&2
    exit 1
fi
echo "==> done: $OUT/$NAME ($count retro_* symbols)"

# PPSSPP reads `compat.ini`, fonts, shaders and translations from
# `<system dir>/PPSSPP/` and warns at init when `compat.ini` is missing. Grab
# the upstream `assets/` tree with a sparse clone so a build produces what the
# app later seeds.
ASSETS="$OUT/ppsspp"
if [ ! -f "$ASSETS/compat.ini" ]; then
    echo "==> fetching hrydgard/ppsspp assets into $ASSETS"
    SRC="$TMP/ppsspp-src"
    git clone --quiet --depth 1 --filter=blob:none --sparse \
        https://github.com/hrydgard/ppsspp "$SRC"
    git -C "$SRC" sparse-checkout set assets >/dev/null
    mkdir -p "$ASSETS"
    cp -R "$SRC/assets/." "$ASSETS/"
fi
echo "==> assets: $ASSETS ($(du -sh "$ASSETS" 2>/dev/null | cut -f1))"
