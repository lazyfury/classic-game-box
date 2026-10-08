#!/usr/bin/env bash
#
# 把 packaging/AppIcon.png 编译成 .icns（sips + iconutil，不用额外依赖）。
#
#     macos/scripts/make-icon.sh                      # -> macos/packaging/AppIcon.icns
#     macos/scripts/make-icon.sh /path/to/out.icns    # 指定输出
#
# 源图是 1024×1024、已经按 macOS 图标网格排版（圆角正方形 880×880 居中，
# 四角透明），这里只负责按 .icns 要求的各档尺寸缩放并打包。打包脚本会现场
# 生成到 .app 里，产物不进仓库。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/packaging/AppIcon.png"
OUT="${1:-$ROOT/packaging/AppIcon.icns}"

if [ ! -f "$SRC" ]; then
	echo "找不到源图 $SRC" >&2
	exit 1
fi
if [ "$(uname -s)" != "Darwin" ]; then
	echo "这个脚本只在 macOS 上有意义（.icns 是 macOS 的概念）" >&2
	exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
ICONSET="$TMP/AppIcon.iconset"
mkdir -p "$ICONSET"

# .icns 要求 {16,32,128,256,512} 各出 1x / 2x 两档。
for base in 16 32 128 256 512; do
	size=$((base * 2))
	/usr/bin/sips -z "$base" "$base" "$SRC" --out "$ICONSET/icon_${base}x${base}.png" >/dev/null
	/usr/bin/sips -z "$size" "$size" "$SRC" --out "$ICONSET/icon_${base}x${base}@2x.png" >/dev/null
done

mkdir -p "$(dirname "$OUT")"
/usr/bin/iconutil --convert icns --output "$OUT" "$ICONSET"
echo "生成 $OUT"
