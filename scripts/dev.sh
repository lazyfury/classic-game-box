#!/bin/bash
# Format, lint and test the Rust workspace the way the per-stage gate requires.
#
# The app itself is not launched here (it needs a display and a built core);
# run `cargo run -p cgb-app` for that.
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
