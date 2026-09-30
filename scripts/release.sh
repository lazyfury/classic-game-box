#!/usr/bin/env bash
#
# Cut a release: run the gate and the selfcheck, stamp the bundle version,
# package the .app and zip it under dist/.
#
#     ./scripts/release.sh
#     ./scripts/release.sh --minimal    # bundle only the redistributable set
#     ./scripts/release.sh --only mesen,mgba
#
# `--minimal` / `--only` are forwarded to package-macos.sh; see
# scripts/core-profiles.sh and cores/README.md.
#
# The version comes from the workspace `Cargo.toml` (`[workspace.package]
# version`) and is written into `packaging/Info.plist` before packaging, so the
# bundle and the crate cannot disagree. The artifact is
# `dist/Classic Game Box-<version>.zip`; its sha256 is printed.
#
# Packaging builds the native cores if they are missing (needs the network
# once), so a first release takes a few minutes. Run `./scripts/dev.sh` by hand
# for the gate while iterating; this script runs it for you.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PLIST="$ROOT/packaging/Info.plist"
DIST="$ROOT/dist"

if [ "$(uname -s)" != "Darwin" ]; then
	echo "这个脚本只在 macOS 上有意义（.app bundle 是 macOS 的概念）" >&2
	exit 1
fi

version="$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")"
if [ -z "$version" ]; then
	echo "读不到 workspace 版本（Cargo.toml 的 [workspace.package] version）" >&2
	exit 1
fi
echo "==> 版本 $version"

echo "==> gate（dev.sh）"
"$ROOT/scripts/dev.sh"

echo "==> selfcheck"
cargo run --release --manifest-path "$ROOT/Cargo.toml" -p cgb-app -- --selfcheck

echo "==> 写入 Info.plist 版本"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$PLIST"

echo "==> 打包"
"$ROOT/scripts/package-macos.sh" "$@"

APP="$DIST/Classic Game Box.app"
if [ ! -d "$APP" ]; then
	echo "打包失败：$APP 不存在" >&2
	exit 1
fi

echo "==> 压缩产物"
ZIP="$DIST/Classic Game Box-$version.zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

echo
echo "产物："
echo "  $APP"
echo "  $ZIP"
shasum -a 256 "$ZIP" | awk '{ print "  sha256: " $1 }'
