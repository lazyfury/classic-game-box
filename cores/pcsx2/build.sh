#!/bin/bash
# ---------------------------------------------------------------------------
# Install LRPS2 (PCSX2) as a native macOS arm64 libretro core.
#
# Output:   cores/dist/pcsx2_libretro.dylib
# Manifest: cores/cores.json, key `pcsx2`, system `ps2`.
#
# LRPS2 is the libretro hard-fork of PCSX2. Its `required_hw_api` is
# "Direct3D >= 11 | OpenGL Core >= 3.3 | OpenGL >= 3.0", so it satisfies both
# the front end's `GET_PREFERRED_HW_RENDER` (OpenGL core) and the offscreen CGL
# 4.1 context the PSP / N64 cores already use. It is downloaded from the
# libretro buildbot rather than built: the buildbot's `apple/osx/arm64`
# artifact is the maintained arm64 build.
#
# A real PS2 BIOS is required (LRPS2 has no HLE BIOS): drop `scph*.bin` /
# `rom1.bin` / `erom.bin` dumps into `<app data>/system/pcsx2/bios/` before
# playing. The BIOS is not downloaded or bundled here.
#
# Usage:
#   ./cores/pcsx2/build.sh
#   PCSX2_URL=https://… ./cores/pcsx2/build.sh   # pin a different build
#
# Requires: network, Xcode command line tools, arm64 Mac.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
NAME="pcsx2_libretro.dylib"
URL="${PCSX2_URL:-https://buildbot.libretro.com/nightly/apple/osx/arm64/latest/pcsx2_libretro.dylib.zip}"

mkdir -p "$OUT"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> downloading $URL"
curl -fL --retry 3 -o "$TMP/pcsx2.zip" "$URL"

echo "==> extracting $NAME"
unzip -oq "$TMP/pcsx2.zip" -d "$TMP"
cp "$TMP/$NAME" "$OUT/$NAME"

# Verify what we installed: an arm64 Mach-O exporting the libretro ABI.
file "$OUT/$NAME"
count="$(nm -gU "$OUT/$NAME" | grep -c '_retro_')"
if [ "$count" -lt 20 ]; then
    echo "error: $NAME does not look like a libretro core ($count retro_* symbols)" >&2
    exit 1
fi
echo "==> done: $OUT/$NAME ($count retro_* symbols)"
echo "==> PS2 BIOS: drop scph*.bin / rom1.bin / erom.bin into"
echo "    <app data>/system/pcsx2/bios/ before playing (LRPS2 has no HLE BIOS)."
