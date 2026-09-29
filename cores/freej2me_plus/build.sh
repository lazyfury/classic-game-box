#!/bin/bash
# ---------------------------------------------------------------------------
# Build FreeJ2ME-Plus, the J2ME (Java ME) core, as a native macOS libretro core.
#
# FreeJ2ME is unusual: the libretro module is only a shim that `fork/exec`s a
# Java VM and talks to it over stdin/stdout. So this script produces three
# things:
#
#   cores/dist/freej2me_plus_libretro.dylib     the core (a C shim)
#   cores/dist/freej2me_plus/freej2me_plus-lr.jar
#                                               the Java program the shim runs
#   cores/dist/freej2me_plus/runtime/           a jlink-trimmed JRE the app puts
#                                               first on PATH at startup
#
# The app seeds the jar into the writable `<app data>/system` directory (where
# the core looks for it) and prepends `runtime/bin` to PATH, so no system Java
# is required. See `crates/cgb-app/src/app.rs` (`j2me_dir`, `prepend_path`).
#
# Output: cores/dist/freej2me_plus_libretro.dylib (+ the bundle dir above).
# Manifest: the `freej2me_plus` row in cores/cores.json.
#
# Usage:
#   ./cores/freej2me_plus/build.sh
#   FREEJ2ME_SRC=/path/to/freej2me-plus ./cores/freej2me_plus/build.sh
#   JAVA_HOME=/path/to/jdk ./cores/freej2me_plus/build.sh
#
# Requires: network (once), Xcode command line tools, and a JDK 9+ (for `jlink`;
# the classes are compiled with `--release 8`). The upstream Ant build instead
# wants JDK 8 — we sidestep it and compile with `javac --release 8`, which any
# modern JDK accepts.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/cores/dist"
SRC="${FREEJ2ME_SRC:-$ROOT/cores/sources/freej2me-plus}"
BUNDLE="$OUT/freej2me_plus"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

# --- a JDK with `javac` and `jlink` -------------------------------------
find_java_home() {
    if [ -n "${JAVA_HOME:-}" ] && [ -x "$JAVA_HOME/bin/jlink" ]; then
        echo "$JAVA_HOME"
        return 0
    fi
    # Homebrew's keg-only JDK.
    if command -v brew >/dev/null 2>&1; then
        local prefix
        for formula in openjdk openjdk@21 openjdk@17; do
            prefix="$(brew --prefix "$formula" 2>/dev/null || true)"
            if [ -n "$prefix" ] && [ -x "$prefix/libexec/openjdk.jdk/Contents/Home/bin/jlink" ]; then
                echo "$prefix/libexec/openjdk.jdk/Contents/Home"
                return 0
            fi
        done
    fi
    # A JDK installed the usual macOS way.
    local vm
    for vm in /Library/Java/JavaVirtualMachines/*/Contents/Home; do
        if [ -x "$vm/bin/jlink" ]; then
            echo "$vm"
            return 0
        fi
    done
    # Last resort: a `javac` already on PATH (derive the home from it).
    local javac
    if javac="$(command -v javac 2>/dev/null)"; then
        local home
        home="$(cd "$(dirname "$javac")/.." && pwd)"
        if [ -x "$home/bin/jlink" ]; then
            echo "$home"
            return 0
        fi
    fi
    return 1
}

if ! JAVA_HOME="$(find_java_home)"; then
    echo "error: 找不到带 jlink 的 JDK（设置 JAVA_HOME，或 brew install openjdk@21）" >&2
    exit 1
fi
echo "==> JDK: $JAVA_HOME"

mkdir -p "$OUT" "$(dirname "$SRC")"

if [ ! -d "$SRC" ]; then
    echo "==> cloning TASEmulators/freej2me-plus into $SRC"
    git clone --depth 1 https://github.com/TASEmulators/freej2me-plus "$SRC"
fi

# --- patch: decode the pipe paths as UTF-8 ---------------------------------
# The shim sends the game and save paths as UTF-8 bytes, but `Libretro.java`
# reads them with `new String(buffer, 0, bytesRead)` — the JVM default charset,
# which the core pins to ISO-8859-1. A non-ASCII path (e.g. a Chinese game
# name) is then double-encoded and `new File()` reports "not found", so the JVM
# exits and the frontend shows a black frame. Decode those two strings as UTF-8
# explicitly. An upstream bug, harmless for ASCII paths.
LIBRETRO_JAVA="$SRC/src/org/recompile/freej2me/Libretro.java"
# Always patch from a pristine copy, so re-running the build cannot stack the
# edits (a replacement contains the text the previous run matched).
LIBRETRO_JAVA_ORIG="$LIBRETRO_JAVA.orig"
[ -f "$LIBRETRO_JAVA_ORIG" ] || cp "$LIBRETRO_JAVA" "$LIBRETRO_JAVA_ORIG"
cp "$LIBRETRO_JAVA_ORIG" "$LIBRETRO_JAVA"
perl -pi -e 's/path = new String\(buffer, 0, bytesRead\);/path = new String(buffer, 0, bytesRead, "UTF-8");/' "$LIBRETRO_JAVA"
perl -pi -e 's/Mobile\.getPlatform\(\)\.dataPath = new String\(buffer, 0, bytesRead\);/Mobile.getPlatform().dataPath = new String(buffer, 0, bytesRead, "UTF-8");/' "$LIBRETRO_JAVA"
if ! grep -q 'new String(buffer, 0, bytesRead, "UTF-8")' "$LIBRETRO_JAVA"; then
    echo "error: 无法给 Libretro.java 打 UTF-8 路径补丁（上游改动？）" >&2
    exit 1
fi

# --- patch: rate-limit the key repeat --------------------------------------
# In libretro mode `Libretro.java` fires `keyRepeated` on *every* emulated
# frame a key is held (the frontend asks for a frame every tick), i.e. ~60
# repeats a second. Holding a direction or a menu key then races. Real phones
# wait ~400ms, then repeat ~12 times a second; do the same. The re-arm happens
# when the held key changes, so the patch is local to the repeat call.
perl -pi -e 's/private int lcdWidth, lcdHeight, lastPressedKey = -1;/private int lcdWidth, lcdHeight, lastPressedKey = -1, repeatedKey = -1;\n\tprivate long repeatTime = 0;/' "$LIBRETRO_JAVA"
perl -pi -e 's/MobilePlatform\.keyRepeated\(Mobile\.getMobileKey\(lastPressedKey\)\);/if (repeatedKey != lastPressedKey) { repeatedKey = lastPressedKey; repeatTime = System.currentTimeMillis() + 400; } else if (System.currentTimeMillis() >= repeatTime) { MobilePlatform.keyRepeated(Mobile.getMobileKey(lastPressedKey)); repeatTime = System.currentTimeMillis() + 80; }/' "$LIBRETRO_JAVA"
# Re-arm on every key edge, so a key pressed again gets the full initial delay.
perl -pi -e 's/MobilePlatform\.pressedKeys\[code\] = false;/MobilePlatform.pressedKeys[code] = false; repeatedKey = -1;/' "$LIBRETRO_JAVA"
perl -pi -e 's/\tlastPressedKey = code;/\tlastPressedKey = code; repeatedKey = -1;/' "$LIBRETRO_JAVA"
if ! grep -q 'repeatTime = System.currentTimeMillis() + 400' "$LIBRETRO_JAVA"; then
    echo "error: 无法给 Libretro.java 打按键重复限速补丁（上游改动？）" >&2
    exit 1
fi
if ! grep -q 'pressedKeys\[code\] = false; repeatedKey = -1;' "$LIBRETRO_JAVA"; then
    echo "error: 无法给 Libretro.java 打按键重臂补丁（上游改动？）" >&2
    exit 1
fi

# --- 1. the C shim (platform=osx builds a universal x86_64+arm64 dylib) ---
echo "==> building the libretro shim (platform=osx, $(uname -m))"
make -C "$SRC/src/libretro" platform=osx NOUNIVERSAL=1 -j"$JOBS"
cp "$SRC/src/libretro/freej2me_plus_libretro.dylib" "$OUT/freej2me_plus_libretro.dylib"
echo "==> done: $OUT/freej2me_plus_libretro.dylib"

# --- 2. the Java program (compiled at Java 8, so a modern JRE can run it) ---
echo "==> compiling freej2me_plus-lr.jar"
BUILD="$SRC/build/cgb"
rm -rf "$BUILD"
mkdir -p "$BUILD/classes" "$BUNDLE"
# `--release 8` keeps the upstream Ant target's compatibility while working on
# any modern JDK; `-nowarn` silences the deprecation notices the old sources
# would otherwise emit.
find "$SRC/src" -name '*.java' >"$BUILD/sources.txt"
"$JAVA_HOME/bin/javac" --release 8 -encoding utf-8 -nowarn \
    -d "$BUILD/classes" @"$BUILD/sources.txt"
cat >"$BUILD/manifest.txt" <<'EOF'
Main-Class: org.recompile.freej2me.Libretro
Implementation-Title: FreeJ2ME-Plus
EOF
"$JAVA_HOME/bin/jar" cfm "$BUNDLE/freej2me_plus-lr.jar" "$BUILD/manifest.txt" \
    -C "$BUILD/classes" . -C "$SRC/resources" . -C "$SRC/META-INF" .
echo "==> done: $BUNDLE/freej2me_plus-lr.jar"

# --- 3. a trimmed JRE to ship with the app ---------------------------------
# `java.desktop` is required (FreeJ2ME uses AWT for fonts/images); without
# `jdk.charsets` the Shift_JIS / EUC_KR / GBK encodings the core can request are
# missing, so Japanese/Korean/Chinese games break.
echo "==> building the bundled runtime (jlink)"
rm -rf "$BUNDLE/runtime"
"$JAVA_HOME/bin/jlink" \
    --module-path "$JAVA_HOME/jmods" \
    --add-modules java.base,java.desktop,jdk.charsets \
    --compress=2 \
    --no-header-files \
    --no-man-pages \
    --strip-debug \
    --output "$BUNDLE/runtime"
echo "==> done: $BUNDLE/runtime"

echo
echo "==> bundle:"
ls -la "$BUNDLE"
du -sh "$BUNDLE" | awk '{ print "  体积: " $1 }'
