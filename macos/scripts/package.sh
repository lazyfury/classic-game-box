#!/usr/bin/env bash
#
# Packages the experimental Swift/macOS host as a `.app`.
#
#     macos/scripts/package.sh            # release build, then assemble dist/
#     macos/scripts/package.sh --open     # ... and launch it afterwards
#
# The bundle holds:
#   Contents/MacOS/cgb-mac                       the Swift host
#   Contents/Frameworks/libcgb_mac.dylib         the Rust app (UI + emulator)
#   Contents/Resources/cores/{cores.json,*.dylib}
#   Contents/Resources/assets/…
#   Contents/Resources/{freej2me_plus,ppsspp}/…  when built
#
# `cgb-app` looks for cores first in app data, then in the bundle's
# `Resources/cores` (`resource_dir` / `resolve_module` in `src/app`), so the app
# runs from Finder without a repo checkout.
#
# The Rust dylib is bundled and referenced by `@rpath`; `codesign` is ad-hoc
# (`-`), enough for a locally built app to launch.

set -euo pipefail

APP_NAME="Classic Game Box (Swift)"
BINARY="cgb-mac"
LIB="libcgb_mac.dylib"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DIST="$ROOT/dist"
APP="$DIST/$APP_NAME.app"
PLIST="$ROOT/macos/packaging/Info.plist"
CORES_DIST="$ROOT/cores/dist"
CORES_JSON="$ROOT/cores/cores.json"
ASSETS="$ROOT/assets"

open_after=false
if [ "${1:-}" = "--open" ]; then
	open_after=true
fi

if [ "$(uname -s)" != "Darwin" ]; then
	echo "这个脚本只在 macOS 上有意义（.app bundle 是 macOS 的概念）" >&2
	exit 1
fi

echo "==> cargo build --release -p cgb-mac"
cargo build --release -p cgb-mac --manifest-path "$ROOT/Cargo.toml"

echo "==> swift build -c release"
# The Swift package links the matching Rust profile.
CGB_RUST_PROFILE=release swift build --package-path "$ROOT/macos" -c release

BUILT_SWIFT="$ROOT/macos/.build/release/$BINARY"
RUST_LIB="$ROOT/target/release/deps/$LIB"
if [ ! -f "$RUST_LIB" ]; then
	RUST_LIB="$ROOT/target/release/$LIB"
fi
if [ ! -f "$RUST_LIB" ]; then
	echo "找不到 $LIB（先跑 cargo build --release -p cgb-mac）" >&2
	exit 1
fi

echo "==> 组装 $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$BUILT_SWIFT" "$APP/Contents/MacOS/$BINARY"
cp "$PLIST" "$APP/Contents/Info.plist"
cp "$RUST_LIB" "$APP/Contents/Frameworks/$LIB"

# Point the Rust dylib at @rpath and the Swift binary at the bundled copy.
install_name_tool -id "@rpath/$LIB" "$APP/Contents/Frameworks/$LIB"
old_ref="$(otool -L "$APP/Contents/MacOS/$BINARY" | awk '/libcgb_mac\.dylib/ {print $1; exit}')"
if [ -n "$old_ref" ] && [ "$old_ref" != "@rpath/$LIB" ]; then
	install_name_tool -change "$old_ref" "@rpath/$LIB" "$APP/Contents/MacOS/$BINARY"
fi
if ! otool -l "$APP/Contents/MacOS/$BINARY" | grep -q '@executable_path/../Frameworks'; then
	install_name_tool -add_rpath "@executable_path/../Frameworks" "$APP/Contents/MacOS/$BINARY"
fi

echo "==> 打包 cores"
if [ -f "$CORES_JSON" ]; then
	mkdir -p "$APP/Contents/Resources/cores"
	cp "$CORES_JSON" "$APP/Contents/Resources/cores/cores.json"
	for dylib in "$CORES_DIST"/*.dylib; do
		[ -f "$dylib" ] || continue
		cp "$dylib" "$APP/Contents/Resources/cores/"
	done
else
	echo "   注意：找不到 $CORES_JSON，打包后没有核心清单" >&2
fi

echo "==> 打包 assets"
mkdir -p "$APP/Contents/Resources/assets"
cp -R "$ASSETS/." "$APP/Contents/Resources/assets/"

if [ -d "$CORES_DIST/freej2me_plus" ]; then
	echo "==> 打包 freej2me（jar + 精简 JRE）"
	cp -R "$CORES_DIST/freej2me_plus" "$APP/Contents/Resources/freej2me_plus"
fi
if [ -d "$CORES_DIST/ppsspp" ]; then
	echo "==> 打包 PPSSPP assets"
	cp -R "$CORES_DIST/ppsspp" "$APP/Contents/Resources/ppsspp"
fi

# The executable name and the plist must agree, or the app launches nothing.
declared="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")"
if [ "$declared" != "$BINARY" ]; then
	echo "Info.plist 的 CFBundleExecutable ($declared) 与二进制名 ($BINARY) 不一致" >&2
	exit 1
fi
/usr/bin/plutil -lint "$APP/Contents/Info.plist"

echo "==> codesign (ad-hoc)"
codesign --force --deep --sign - "$APP"
codesign --verify --verbose=2 "$APP"

echo "==> 完成"
echo "$APP"
du -sh "$APP" | awk '{ print "  体积: " $1 }'
echo "  运行: open \"$APP\""

if [ "$open_after" = true ]; then
	open "$APP"
fi
