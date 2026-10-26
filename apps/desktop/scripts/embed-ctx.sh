#!/usr/bin/env bash
# Builds the ctx CLI in release mode and copies it to src-tauri/binaries as
# the Tauri sidecar (`ctx-<target-triple>`), so `npm run tauri build --
# --config src-tauri/tauri.bundle.conf.json` bundles it next to the app.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
if [ "$target" = "$(rustc -vV | sed -n 's/^host: //p')" ]; then
  (cd "$root" && cargo build --release -p ctx-cli)
  built="$root/target/release/ctx"
else
  (cd "$root" && cargo build --release -p ctx-cli --target "$target")
  built="$root/target/$target/release/ctx"
fi
ext=""
case "$target" in *windows*) ext=".exe";; esac
mkdir -p "$here/src-tauri/binaries"
cp "$built$ext" "$here/src-tauri/binaries/ctx-$target$ext"
echo "embedded $here/src-tauri/binaries/ctx-$target$ext"
