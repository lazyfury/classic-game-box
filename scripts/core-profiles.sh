#!/usr/bin/env bash
# Shared core-set selection for scripts/build-cores.sh and scripts/package-macos.sh.
#
# A "core set" is a list of core keys, each naming a `cores/<key>/` directory
# with a `build.sh`. Two ways to pick one:
#
#   --minimal        the redistributable set below
#   --only a,b,c     an explicit list
#
# With neither, every `cores/*/build.sh` is selected (the full set).
#
# The minimal set is what is clean to bundle and cannot be fetched at runtime:
# the permissive/own cores plus the two the libretro buildbot has no arm64 build
# for (custom_nes_core, freej2me_plus). Every other core is offered as a runtime
# download (`<app data>/cores/downloaded.json`) instead of being shipped. See
# cores/README.md ("Bundled set vs. downloaded cores") and the licensing survey.

# Keep this list in sync with the docs. One key per word; `cores/<key>/build.sh`
# must exist. Mesen is the one third-party NES core in the set (Nestopia, the
# other NES core, is a download); `custom_nes_core` is the self-authored one.
CGB_MINIMAL_CORES="mesen mgba custom_nes_core freej2me_plus"

# Every core that has a build script, one key per line.
cgb_all_cores() {
	local root="$1" script
	for script in "$root"/cores/*/build.sh; do
		[ -e "$script" ] || continue
		basename "$(dirname "$script")"
	done
}

# Print the selected core keys, one per line.
#
#     cgb_select_cores <root> <minimal 0|1> <only list or "">
cgb_select_cores() {
	local root="$1" minimal="$2" only="$3" key
	if [ -n "$only" ]; then
		# Accept comma- and/or space-separated keys.
		for key in ${only//,/ }; do
			printf '%s\n' "$key"
		done
	elif [ "$minimal" = "1" ]; then
		for key in $CGB_MINIMAL_CORES; do
			printf '%s\n' "$key"
		done
	else
		cgb_all_cores "$root"
	fi
}

# Whether a key is in a newline-separated selection list.
#     printf '%s\n' "$selected" | cgb_list_has <key>
cgb_list_has() {
	grep -qx "$1"
}
