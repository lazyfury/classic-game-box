#!/usr/bin/env bash
#
# Packages the release binary as a macOS .app bundle.
#
#     ./scripts/package-macos.sh            # build cores if needed, then assemble dist/
#     ./scripts/package-macos.sh --open     # ... and launch it afterwards
#     ./scripts/package-macos.sh --skip-cores  # do not build cores; bundle what exists
#
# The bundle is written to `dist/` (git-ignored). It holds the executable, an
# `Info.plist`, and `Contents/Resources/{cores,assets}` — the native core
# dylibs and manifest, and the bundled arcade BIOS. The app looks for cores
# first in its app-data directory, then in the bundle's Resources (see
# `resource_dir` in `crates/cgb-app/src/app.rs`), so a packaged app runs from
# Finder without a repo checkout.
#
# `codesign` runs ad-hoc (`-`): enough for a locally built app to launch
# without the "damaged" Gatekeeper error, not a distribution signature.

set -euo pipefail

APP_NAME="Classic Game Box"
BINARY="classic-game-box"

# Resolve paths from this script, so it works from any cwd.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLIST="$ROOT/packaging/Info.plist"
DIST="$ROOT/dist"
APP="$DIST/$APP_NAME.app"
BUILT="$ROOT/target/release/$BINARY"
CORES_DIST="$ROOT/cores/dist"
CORES_JSON="$ROOT/cores/cores.json"
ASSETS="$ROOT/assets"

open_after=false
skip_cores=false
for arg in "$@"; do
	case "$arg" in
		--open) open_after=true ;;
		--skip-cores) skip_cores=true ;;
		*)
			echo "未知参数: $arg" >&2
			exit 2
			;;
	esac
done

if [ "$(uname -s)" != "Darwin" ]; then
	echo "这个脚本只在 macOS 上有意义（.app bundle 是 macOS 的概念）" >&2
	exit 1
fi

if [ "$skip_cores" = false ] && [ -z "$(ls -A "$CORES_DIST" 2>/dev/null)" ]; then
	echo "==> 构建原生 cores（首次需网络，几分钟）"
	"$ROOT/scripts/build-cores.sh"
fi

echo "==> cargo build --release -p cgb-app"
cargo build --release -p cgb-app --manifest-path "$ROOT/Cargo.toml"

echo "==> 组装 $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BUILT" "$APP/Contents/MacOS/$BINARY"
cp "$PLIST" "$APP/Contents/Info.plist"

echo "==> 打包 cores"
if [ -n "$(ls -A "$CORES_DIST"/*.dylib 2>/dev/null)" ]; then
	mkdir -p "$APP/Contents/Resources/cores"
	cp "$CORES_DIST"/*.dylib "$APP/Contents/Resources/cores/"
	[ -f "$CORES_JSON" ] && cp "$CORES_JSON" "$APP/Contents/Resources/cores/cores.json"
else
	echo "   注意：cores/dist 里没有 .dylib，打包后没有可运行核心（先跑 ./scripts/build-cores.sh）" >&2
fi

echo "==> 打包 assets"
mkdir -p "$APP/Contents/Resources/assets"
cp -R "$ASSETS/." "$APP/Contents/Resources/assets/"

# The executable name and the plist must agree, or the app launches nothing.
declared="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")"
if [ "$declared" != "$BINARY" ]; then
	echo "Info.plist 的 CFBundleExecutable ($declared) 与二进制名 ($BINARY) 不一致" >&2
	exit 1
fi
/usr/bin/plutil -lint "$APP/Contents/Info.plist"

echo "==> codesign (ad-hoc)"
codesign --force --sign - "$APP"
codesign --verify --verbose=2 "$APP"

echo "==> 完成"
echo "$APP"
du -sh "$APP" | awk '{ print "  体积: " $1 }'
echo "  运行: open \"$APP\""

if [ "$open_after" = true ]; then
	open "$APP"
fi
