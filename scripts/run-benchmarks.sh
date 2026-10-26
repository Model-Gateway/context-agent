#!/usr/bin/env bash
# Run the benchmark suite against the configured model.
# Usage: scripts/run-benchmarks.sh [variants] [extra ctx eval args...]
#   scripts/run-benchmarks.sh A,D,E --filter py-discount-bug-fix
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
VARIANTS="${1:-A,B,C,D,E}"
shift || true
cargo build --release -p ctx-cli
./target/release/ctx eval run benchmarks/tasks --variants "$VARIANTS" "$@"
./target/release/ctx eval retrieval benchmarks/retrieval/queries.jsonl
