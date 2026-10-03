#!/usr/bin/env bash
#
# check-boundaries.sh — enforce the module contract in CONVENTIONS.md.
#
# Each rule has a matching [Bn] entry in CONVENTIONS.md; keep the two in sync.
# Run it directly, or as part of the per-stage gate via ./scripts/dev.sh.
#
# The checks are grep-based on purpose: they look at the code as written, so a
# boundary can only be crossed by editing this file too — and CONVENTIONS.md
# says that edit needs an ADR.
set -uo pipefail

cd "$(dirname "$0")/.."

if ! command -v rg >/dev/null 2>&1; then
    echo "check-boundaries: ripgrep (rg) is required" >&2
    exit 2
fi

fail=0

# forbidden <id> <desc> <pattern> <path...>
# Fails when <pattern> matches a line in <path...>.
forbidden() {
    local id="$1" desc="$2" pattern="$3"
    shift 3
    local hits
    hits="$(rg -n --no-heading -e "$pattern" "$@" 2>/dev/null || true)"
    if [ -n "$hits" ]; then
        printf 'FAIL [%s] %s\n' "$id" "$desc"
        printf '%s\n' "$hits" | sed 's/^/       /'
        fail=1
    else
        printf 'PASS [%s] %s\n' "$id" "$desc"
    fi
}

# forbidden_code — like `forbidden`, but ignores lines whose content starts with
# `//` (doc and code comments), so a comment that *names* an edge is not counted
# as a dependency.
forbidden_code() {
    local id="$1" desc="$2" pattern="$3"
    shift 3
    local hits
    hits="$(rg -n --no-heading -e "$pattern" "$@" 2>/dev/null \
        | grep -vE ':[0-9]+:[[:space:]]*//' || true)"
    if [ -n "$hits" ]; then
        printf 'FAIL [%s] %s\n' "$id" "$desc"
        printf '%s\n' "$hits" | sed 's/^/       /'
        fail=1
    else
        printf 'PASS [%s] %s\n' "$id" "$desc"
    fi
}

# The `[dependencies]` block of a Cargo.toml (normal deps only).
deps_block() {
    awk '/^\[dependencies\]/{f=1;next} /^\[/{f=0} f' "$1"
}

echo "module boundary checks (CONVENTIONS.md)"

# B1 — src/ui is the pure view: no app state, no devices, no implementation.
forbidden_code B1 "src/ui does not depend on app state / devices / impl" \
    'crate::(app|audio|cores|library|native|session)\b' src/ui

# B2 — src/ui uses igui's public view API and libretro's pure domain types only;
# it must not reach the libretro front-end or the GPU backend.
forbidden_code B2 "src/ui uses no libretro front-end / GPU backend" \
    'cgb_libretro::(CoreHost|Frame|CoreLibrary|AvInfo|CoreOption|InputDescriptor|MemoryRegion|SystemInfo|LibretroError)|igui_backend_wgpu|\bwgpu::' \
    src/ui

# B3 — the emulator boundary stays device/UI-free: only libloading + thiserror.
if deps_block crates/cgb-libretro/Cargo.toml \
    | rg -q 'cgb-app|igui|cpal|diesel|libsqlite3-sys|ureq|zip|png|dirs|rfd|gilrs|arboard'; then
    printf 'FAIL [%s] %s\n' B3 "cgb-libretro normal deps stay at libloading + thiserror"
    deps_block crates/cgb-libretro/Cargo.toml | sed 's/^/       /'
    fail=1
else
    printf 'PASS [%s] %s\n' B3 "cgb-libretro normal deps stay at libloading + thiserror"
fi

# B4 — one-way dependency: the member package may only use the root package as
# a dev-dependency (tests), never in the normal build.
b4_ok=1
for manifest in crates/*/Cargo.toml; do
    if deps_block "$manifest" | rg -q 'cgb-app'; then
        printf 'FAIL [%s] %s\n' B4 "member packages depend on the root only as a dev-dep"
        printf '       %s [dependencies] contains cgb-app\n' "$manifest"
        b4_ok=0
    fi
done
if [ "$b4_ok" -eq 1 ]; then
    printf 'PASS [%s] %s\n' B4 "member packages depend on the root only as a dev-dep"
else
    fail=1
fi

# B5 — only src/native touches the GPU/OS layer (wgpu::, objc2, Metal).
forbidden B5 "only src/native uses wgpu:: / objc2 / metal::" \
    '\bwgpu::|objc2|metal::' src -g '!**/native/**'

# B6 — no build artifacts are tracked (they live under gitignored dirs).
tracked="$(git ls-files 2>/dev/null \
    | rg '(^|/)(target|dist|cores/sources)/|\.(dylib|o)$' || true)"
if [ -n "$tracked" ]; then
    printf 'FAIL [%s] %s\n' B6 "no build artifacts committed"
    printf '%s\n' "$tracked" | sed 's/^/       /'
    fail=1
else
    printf 'PASS [%s] %s\n' B6 "no build artifacts committed"
fi

echo
if [ "$fail" -eq 0 ]; then
    echo "check-boundaries: all rules hold"
else
    echo "check-boundaries: boundary violations above — see CONVENTIONS.md"
fi
exit "$fail"
