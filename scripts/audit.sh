#!/bin/bash
# Dependency audit: known vulnerabilities, yanked releases, licenses and
# sources for the Rust graph (config in `deny.toml`).
#
# Deliberately **not** part of `scripts/dev.sh`: `cargo-deny` is not a rustup
# component and the check fetches the RustSec advisory DB over the network, so
# the per-stage gate stays install-free and offline-capable. Run this before a
# release, or from CI.
#
#   cargo install cargo-deny --locked
#   ./scripts/audit.sh
#
# It scans the Rust dependencies only; the libretro cores are downloaded at
# runtime and are outside this graph.
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v cargo-deny >/dev/null 2>&1; then
    echo "cargo-deny is not installed. Install it with:" >&2
    echo "  cargo install cargo-deny --locked" >&2
    exit 1
fi

exec cargo deny check
