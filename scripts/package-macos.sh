#!/usr/bin/env bash
#
# Packages the release binary as a macOS .app bundle.
#
#     ./scripts/package-macos.sh                 # build cores if needed, then assemble dist/
#     ./scripts/package-macos.sh --open          # ... and launch it afterwards
#     ./scripts/package-macos.sh --skip-cores    # do not build cores; bundle what exists
#     ./scripts/package-macos.sh --minimal       # bundle only the redistributable set
#     ./scripts/package-macos.sh --only mesen,mgba
#
# The bundle is written to `dist/` (git-ignored). It holds the executable, an
# `Info.plist`, and `Contents/Resources/{cores,assets}` — the native core
# dylibs and manifest, and the bundled arcade BIOS. The app looks for cores
# first in its app-data directory, then in the bundle's Resources (see
# `resource_dir` in `crates/cgb-app/src/app/mod.rs`), so a packaged app runs from
# Finder without a repo checkout.
#
# `--minimal` / `--only` restrict which core dylibs are copied into the bundle.
# The full `cores.json` still ships: the app drops the rows whose module is
# absent at load, and keeps the rest so the library page can recommend a
# download for a console it cannot run yet. Cores left out are fetched at
# runtime from the libretro buildbot (settings page → 下载核心, or
# `--download-core`); see `scripts/core-profiles.sh` and cores/README.md.
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

source "$ROOT/scripts/core-profiles.sh"

open_after=false
skip_cores=false
minimal=false
only=""
while [ $# -gt 0 ]; do
	case "$1" in
		--open) open_after=true ;;
		--skip-cores) skip_cores=true ;;
		--minimal) minimal=true ;;
		--only) shift; only="${1:-}" ;;
		--only=*) only="${1#--only=}" ;;
		*)
			echo "未知参数: $1" >&2
			exit 2
			;;
	esac
	shift
done

if [ "$minimal" = true ] && [ -n "$only" ]; then
	echo "--minimal 与 --only 不能同时使用" >&2
	exit 2
fi

if [ "$(uname -s)" != "Darwin" ]; then
	echo "这个脚本只在 macOS 上有意义（.app bundle 是 macOS 的概念）" >&2
	exit 1
fi

# Which cores this bundle carries. Passed through to build-cores.sh too.
selected="$(cgb_select_cores "$ROOT" "$([ "$minimal" = true ] && echo 1 || echo 0)" "$only")"
selected_has() { printf '%s\n' "$selected" | cgb_list_has "$1"; }
build_args=()
[ "$minimal" = true ] && build_args+=(--minimal)
[ -n "$only" ] && build_args+=(--only "$only")

if [ "$skip_cores" = false ] && [ -z "$(ls -A "$CORES_DIST" 2>/dev/null)" ]; then
	echo "==> 构建原生 cores（首次需网络，几分钟）"
	"$ROOT/scripts/build-cores.sh" "${build_args[@]}"
fi

echo "==> cargo build --release -p cgb-app"
cargo build --release -p cgb-app --manifest-path "$ROOT/Cargo.toml"

echo "==> 组装 $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BUILT" "$APP/Contents/MacOS/$BINARY"
cp "$PLIST" "$APP/Contents/Info.plist"

echo "==> 打包 cores"
if [ -f "$CORES_JSON" ]; then
	mkdir -p "$APP/Contents/Resources/cores"
	# The full manifest ships: the app drops rows whose module is missing at
	# load, and keeps the rest so the library page can recommend a download for
	# a console it cannot run yet. Only the selected modules are copied.
	cp "$CORES_JSON" "$APP/Contents/Resources/cores/cores.json"
	# Learn which modules the selected cores name. python3 is already a
	# dependency of scripts/update-core-catalog.sh.
	modules="$(
		python3 - "$CORES_JSON" $selected <<'PY'
import json, sys

src, *keys = sys.argv[1:]
wanted = set(keys)
with open(src, encoding="utf-8") as fh:
    doc = json.load(fh)
seen = []
for core in doc.get("cores", []):
    if core.get("key") not in wanted:
        continue
    module = core.get("dylib")
    if module and module not in seen:
        seen.append(module)
print("\n".join(seen))
PY
	)"
	missing=""
	for module in $modules; do
		if [ -f "$CORES_DIST/$module" ]; then
			cp "$CORES_DIST/$module" "$APP/Contents/Resources/cores/$module"
		else
			missing="$missing $module"
		fi
	done
	if [ -n "$missing" ]; then
		echo "   注意：以下核心没有构建产物，未打包：$missing（先跑 ./scripts/build-cores.sh ${build_args[*]:-}）" >&2
	fi
	# Strip local symbols from the copies: the source dylibs are third-party
	# build products and are left alone.
	if command -v strip >/dev/null 2>&1; then
		echo "==> strip cores"
		for dylib in "$APP/Contents/Resources/cores/"*.dylib; do
			[ -f "$dylib" ] || continue
			strip -x "$dylib" 2>/dev/null || true
		done
	fi
else
	echo "   注意：找不到 $CORES_JSON，打包后没有核心清单" >&2
fi

echo "==> 打包 freej2me（jar + 精简 JRE）"
# The J2ME core is only a shim: it needs `freej2me_plus-lr.jar` and a Java VM.
# The bundle built by cores/freej2me_plus/build.sh lives beside the dylibs; the
# app seeds the jar into the writable system dir and puts `runtime/bin` on PATH.
J2ME_DIST="$CORES_DIST/freej2me_plus"
if selected_has freej2me_plus; then
	if [ -d "$J2ME_DIST" ]; then
		mkdir -p "$APP/Contents/Resources/freej2me_plus"
		cp -R "$J2ME_DIST/." "$APP/Contents/Resources/freej2me_plus/"
	else
		echo "   注意：$J2ME_DIST 不存在，J2ME 核心将依赖系统的 java（先跑 ./cores/freej2me_plus/build.sh）" >&2
	fi
else
	echo "   跳过：freej2me_plus 不在本次核心集合里"
fi

echo "==> 打包 PPSSPP assets"
# PPSSPP reads `<system dir>/PPSSPP/` (compat.ini, fonts, shaders). The app seeds
# the bundled copy there at startup; without compat.ini it warns at init.
PPSSPP_DIST="$CORES_DIST/ppsspp"
if selected_has ppsspp; then
	if [ -f "$PPSSPP_DIST/compat.ini" ]; then
		mkdir -p "$APP/Contents/Resources/ppsspp"
		cp -R "$PPSSPP_DIST/." "$APP/Contents/Resources/ppsspp/"
	else
		echo "   注意：$PPSSPP_DIST 不存在，PPSSPP 缺少 assets 会在启动时告警（先跑 ./cores/ppsspp/build.sh）" >&2
	fi
else
	echo "   跳过：ppsspp 不在本次核心集合里"
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
# `--deep` so the nested JRE under Resources/freej2me_plus/runtime is signed
# too; ad-hoc is enough for a locally built app, not a distribution signature.
codesign --force --deep --sign - "$APP"
codesign --verify --verbose=2 "$APP"

echo "==> 完成"
echo "$APP"
du -sh "$APP" | awk '{ print "  体积: " $1 }'
echo "  运行: open \"$APP\""

if [ "$open_after" = true ]; then
	open "$APP"
fi
