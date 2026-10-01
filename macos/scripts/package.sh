#!/usr/bin/env bash
#
# Packages the Swift/macOS host as a `.app`.
#
#     macos/scripts/package.sh            # release build, then assemble dist/
#     macos/scripts/package.sh --open     # ... and launch it afterwards
#
# The Rust host is linked **statically** into the Swift binary, so the bundle
# is self-contained:
#   Contents/MacOS/cgb-mac                       the app (Swift + Rust)
#   Contents/Resources/cores/{cores.json,*.dylib}
#   Contents/Resources/assets/…
#   Contents/Resources/{freej2me_plus,ppsspp}/…  when built
#
# `cgb-app` looks for cores first in app data, then in the bundle's
# `Resources/cores` (`resource_dir` / `resolve_module` in `crates/cgb-app`), so
# the app runs from Finder without a repo checkout.
#
# `codesign` is ad-hoc (`-`), enough for a locally built app to launch.

set -euo pipefail

APP_NAME="Classic Game Box (Swift)"
BINARY="cgb-mac"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DIST="$ROOT/dist"
APP="$DIST/$APP_NAME.app"
PLIST="$ROOT/macos/packaging/Info.plist"
CORES_DIST="$ROOT/cores/dist"
CORES_JSON="$ROOT/cores/cores.json"
ASSETS="$ROOT/assets"

# Match the Swift package's deployment target (see macos/Package.swift).
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"

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
if [ ! -f "$BUILT_SWIFT" ]; then
	echo "找不到 $BUILT_SWIFT（先跑 swift build -c release）" >&2
	exit 1
fi

echo "==> 组装 $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BUILT_SWIFT" "$APP/Contents/MacOS/$BINARY"
cp "$PLIST" "$APP/Contents/Info.plist"

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
