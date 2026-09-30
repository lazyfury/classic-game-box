#!/bin/bash
# ---------------------------------------------------------------------------
# Regenerate the built-in downloadable-core catalog (`cores/catalog.json`).
#
# The app embeds this snapshot at compile time so core search works offline.
# It is a join of:
#   * the libretro buildbot's macOS arm64 `latest/` listing (which cores exist),
#     and
#   * the `libretro/libretro-core-info` metadata (display name / systemid /
#     extensions).
#
# Run by hand when the buildbot gains cores you care about; `--force-update`
# refreshes the *user* cache at runtime without touching this file.
#
# Usage:  ./scripts/update-core-catalog.sh
# Needs:  network, curl, tar, python3.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/cores/catalog.json"
PLATFORM="${CATALOG_PLATFORM:-apple/osx/arm64}"
LISTING="https://buildbot.libretro.com/nightly/$PLATFORM/latest/"
CORE_INFO="https://github.com/libretro/libretro-core-info/archive/refs/heads/master.tar.gz"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> fetching buildbot listing ($PLATFORM)"
curl -fsSL --retry 3 -o "$TMP/listing.html" "$LISTING"
grep -oE '[a-z0-9_]+_libretro\.(dylib|so|dll)\.zip' "$TMP/listing.html" \
    | sed -E 's/_libretro\..*//' | sort -u > "$TMP/names.txt"
echo "    $(wc -l < "$TMP/names.txt" | tr -d ' ') cores"

echo "==> fetching libretro-core-info"
curl -fsSL --retry 3 -o "$TMP/core-info.tar.gz" "$CORE_INFO"
tar xzf "$TMP/core-info.tar.gz" -C "$TMP"

echo "==> writing $OUT"
python3 - "$TMP" "$OUT" <<'PY'
import datetime, glob, json, os, sys

tmp, out = sys.argv[1], sys.argv[2]

def parse_info(path):
    fields = {}
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, value = line.split("=", 1)
            fields[key.strip()] = value.strip().strip('"')
    return fields

infos = {}
for path in glob.glob(os.path.join(tmp, "libretro-core-info-*", "*.info")):
    base = os.path.basename(path)
    key = base[: -len("_libretro.info")] if base.endswith("_libretro.info") else base[:-5]
    infos[key] = parse_info(path)

names = [line.strip() for line in open(os.path.join(tmp, "names.txt")) if line.strip()]
cores = []
for name in sorted(names):
    info = infos.get(name, {})
    cores.append(
        {
            "name": name,
            "display_name": info.get("display_name", name),
            "system": info.get("systemid", ""),
            "extensions": info.get("supported_extensions", ""),
        }
    )

catalog = {
    "generated": datetime.date.today().isoformat(),
    "source": "https://buildbot.libretro.com/nightly",
    "cores": cores,
}
with open(out, "w", encoding="utf-8") as fh:
    json.dump(catalog, fh, indent=2, ensure_ascii=False)
    fh.write("\n")
print(f"    {len(cores)} cores -> {out}")
PY
