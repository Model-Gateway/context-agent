#!/usr/bin/env bash
# Build everything and run the complete test suite (what CI should run).
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --workspace --all-targets
cargo test --workspace
