#!/bin/bash
# Build native libretro cores into cores/dist.
#
# Each core is a directory with its own build.sh: cores/<name>/build.sh.
# Which cores get built is selectable:
#
#     ./scripts/build-cores.sh                    # every core (default)
#     ./scripts/build-cores.sh --minimal          # only the redistributable set
#     ./scripts/build-cores.sh --only mesen,mgba  # an explicit list
#
# `--skip-mgba` skips mGBA (it needs cmake); it still applies when mGBA is in
# the selected set. `--minimal` and `--only` are mutually exclusive.
#
# The minimal set is defined in `scripts/core-profiles.sh`; the rest are meant
# to be fetched at runtime from the libretro buildbot instead of bundled. See
# cores/README.md.
#
# Takes minutes per core on first run and needs the network once. Run it before
# `cargo run`: the app refuses to start a game whose core is missing, with a
# message pointing back here.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/core-profiles.sh"

BUILD_MGBA=1
MINIMAL=0
ONLY=""
while [ $# -gt 0 ]; do
	case "$1" in
		--skip-mgba) BUILD_MGBA=0 ;;
		--with-mgba) BUILD_MGBA=1 ;; # back-compat: mGBA is the default now
		--minimal) MINIMAL=1 ;;
		--only) shift; ONLY="${1:-}" ;;
		--only=*) ONLY="${1#--only=}" ;;
		*) echo "usage: $0 [--minimal | --only a,b,c] [--skip-mgba]" >&2; exit 2 ;;
	esac
	shift
done

if [ "$MINIMAL" = "1" ] && [ -n "$ONLY" ]; then
	echo "error: --minimal and --only are mutually exclusive" >&2
	exit 2
fi

selected="$(cgb_select_cores "$ROOT" "$MINIMAL" "$ONLY")"
if [ -z "$selected" ]; then
	echo "error: no cores selected" >&2
	exit 1
fi

for name in $selected; do
	script="$ROOT/cores/$name/build.sh"
	if [ ! -f "$script" ]; then
		echo "error: no such core: $name (missing $script)" >&2
		exit 2
	fi
	if [ "$name" = "mgba" ] && [ "$BUILD_MGBA" = "0" ]; then
		echo
		echo "==> skipped: $name (--skip-mgba)"
		continue
	fi
	echo
	echo "==> core: $name"
	"$script"
done

echo
echo "cores:"
ls -la "$ROOT/cores/dist"
