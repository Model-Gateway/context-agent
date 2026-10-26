#!/bin/bash
# Run Script phase ("Embed ctx backend"): build the Rust backend and copy it
# into the app bundle at Contents/Helpers/ctx before Xcode signs the app.
# The phase declares that path as an output so the CodeSign step re-runs
# whenever the binary changes (otherwise the app seal goes stale).
set -euo pipefail
repo="$(cd "$SRCROOT/../.." && pwd)"
dest="$CODESIGNING_FOLDER_PATH/Contents/Helpers"
bin="$repo/target/release/ctx"
cargo="$(PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH" command -v cargo || true)"
if [ -n "$cargo" ] && [ "${CTX_SKIP_CARGO:-0}" != "1" ]; then
  echo "embed-ctx.sh: building ctx with cargo (set CTX_SKIP_CARGO=1 to reuse target/release/ctx)" 1>&2
  env -i HOME="$HOME" PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin" \
    "$cargo" build --release -p ctx-cli --manifest-path "$repo/Cargo.toml" 1>&2
fi
if [ ! -x "$bin" ]; then
  echo "warning: ctx binary not found at $bin; the app will look for ctx in ~/.cargo/bin, Homebrew or the path set in Settings" 1>&2
  exit 0
fi
mkdir -p "$dest"
cp -f "$bin" "$dest/ctx"
chmod 755 "$dest/ctx"
# Nested code must carry a valid signature before the app is signed; "-" (ad hoc) is fine.
codesign --force --sign "${EXPANDED_CODE_SIGN_IDENTITY:--}" --timestamp=none "$dest/ctx"
echo "embed-ctx.sh: embedded $bin at $dest/ctx" 1>&2
