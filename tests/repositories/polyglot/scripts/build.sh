#!/usr/bin/env bash
# Run the monorepo's test suites.
#
#   scripts/build.sh          # Python + JavaScript tests (fast, no artefacts)
#   scripts/build.sh --all    # also build and test the Rust `tools` crate
#
# Exits non-zero on the first failing suite.

set -euo pipefail

cd "$(dirname "$0")/.."

with_rust=0
for arg in "$@"; do
  case "$arg" in
    --all|--rust) with_rust=1 ;;
    -h|--help)
      sed -n '2,8p' "$0"
      exit 0
      ;;
    *)
      echo "build.sh: unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

echo "==> services/api (python3 -m unittest)"
python3 -m unittest discover -s services/api -p 'test_*.py'

echo "==> web (node --test)"
node --test web/*.test.mjs

if [ "$with_rust" -eq 1 ]; then
  cargo_bin="${CARGO:-$HOME/.cargo/bin/cargo}"
  echo "==> tools (cargo test)"
  "$cargo_bin" test --manifest-path tools/Cargo.toml --quiet
  echo "==> tools version"
  "$cargo_bin" run --manifest-path tools/Cargo.toml --quiet -- version
fi

echo "all suites passed"
