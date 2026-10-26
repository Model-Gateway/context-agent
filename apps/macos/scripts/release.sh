#!/bin/bash
# Build, sign with Developer ID, notarize and staple Context.app, then package
# it as dist/Context-<version>.zip and dist/Context-<version>.dmg.
#
# One-time setup:
#   1. A "Developer ID Application" certificate in the login keychain
#      (Xcode > Settings > Accounts > Manage Certificates > + > Developer ID Application).
#   2. A notarytool keychain profile:
#        xcrun notarytool store-credentials context-notary \
#          --apple-id <apple id> --team-id <team id> --password <app-specific password>
#
# Usage: apps/macos/scripts/release.sh [version]
# Environment: SIGN_IDENTITY (default: the first Developer ID Application identity),
#              NOTARY_PROFILE (default: context-notary), SKIP_NOTARIZE=1 to sign only.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
macos="$(cd "$here/.." && pwd)"
repo="$(cd "$macos/../.." && pwd)"
version="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/Cargo.toml" | head -1)}"
profile="${NOTARY_PROFILE:-context-notary}"
identity="${SIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | head -1)}"
if [ -z "$identity" ]; then
  echo "release.sh: no Developer ID Application certificate in the keychain (see the header of this script)" 1>&2
  exit 1
fi

build="$repo/target/macos-release"
dist="$repo/dist"
app="$build/Build/Products/Release/Context.app"
rm -rf "$build" && mkdir -p "$dist"

echo "==> building Context $version (signing as $identity)"
export PATH="$HOME/.cargo/bin:$PATH"
xcodebuild -project "$macos/ContextAgent.xcodeproj" -scheme ContextAgent -configuration Release \
  -derivedDataPath "$build" -destination "platform=macOS,arch=$(uname -m)" \
  MARKETING_VERSION="$version" \
  build | tail -3

# The project signs ad hoc (with get-task-allow, which notarization rejects).
# Re-sign everything inside-out with Developer ID, the hardened runtime and a
# secure timestamp; no entitlements are needed.
echo "==> signing"
for bundle in "$app"/Contents/Resources/*.bundle; do
  codesign --force --sign "$identity" --timestamp "$bundle"
done
codesign --force --sign "$identity" --options runtime --timestamp "$app/Contents/Helpers/ctx"
codesign --force --sign "$identity" --options runtime --timestamp "$app"
codesign --verify --deep --strict --verbose=2 "$app"

zip="$dist/Context-$version.zip"
dmg="$dist/Context-$version.dmg"
rm -f "$zip" "$dmg"

if [ "${SKIP_NOTARIZE:-0}" != "1" ]; then
  echo "==> notarizing the app"
  ditto -c -k --keepParent "$app" "$build/notarize.zip"
  xcrun notarytool submit "$build/notarize.zip" --keychain-profile "$profile" --wait
  xcrun stapler staple "$app"
fi

echo "==> packaging"
ditto -c -k --keepParent "$app" "$zip"
staging="$build/dmg"
mkdir -p "$staging" && cp -R "$app" "$staging/" && ln -s /Applications "$staging/Applications"
hdiutil create -volname "Context" -srcfolder "$staging" -ov -format UDZO "$dmg" >/dev/null
codesign --force --sign "$identity" --timestamp "$dmg"
if [ "${SKIP_NOTARIZE:-0}" != "1" ]; then
  echo "==> notarizing the disk image"
  xcrun notarytool submit "$dmg" --keychain-profile "$profile" --wait
  xcrun stapler staple "$dmg"
  spctl --assess --type execute --verbose "$app"
fi

shasum -a 256 "$zip" "$dmg"
echo "==> done: $dist"
