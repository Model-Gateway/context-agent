#!/bin/bash
# Build, sign with Developer ID, notarize and staple Context.app, then package
# it as dist/Context-<version>.zip and dist/Context-<version>.dmg, and write
# dist/appcast.xml, the Sparkle feed that installed copies read to update
# themselves (App/Updater.swift). The app's SUFeedURL is
# https://github.com/<repo>/releases/latest/download/appcast.xml, so every
# GitHub release must carry its appcast.xml next to the zip it points at.
#
# One-time setup:
#   1. A "Developer ID Application" certificate in the login keychain
#      (Xcode > Settings > Accounts > Manage Certificates > + > Developer ID Application).
#   2. A notarytool keychain profile:
#        xcrun notarytool store-credentials context-notary \
#          --apple-id <apple id> --team-id <team id> --password <app-specific password>
#   3. The Sparkle EdDSA private key in the login keychain. Its public half is
#      SUPublicEDKey in apps/macos/Info.plist; without the private key no
#      installed copy accepts an update. Sparkle's tools are in the build's
#      SourcePackages/artifacts/sparkle/Sparkle/bin: `generate_keys -x <file>`
#      exports a backup, `generate_keys -f <file>` imports it on another Mac.
#
# Usage: apps/macos/scripts/release.sh [version]
# Environment: SIGN_IDENTITY (default: the first Developer ID Application identity),
#              NOTARY_PROFILE (default: context-notary), SKIP_NOTARIZE=1 to sign only,
#              NOTES=<file.md> release notes (the update window and the GitHub release),
#              PUBLISH=1 to create the GitHub release v<version> with the zip, dmg and
#              appcast.xml (needs a clean checkout of the pushed commit being released),
#              RELEASE_REPO (default: Model-Gateway/context-agent).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
macos="$(cd "$here/.." && pwd)"
repo="$(cd "$macos/../.." && pwd)"
version="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/Cargo.toml" | head -1)}"
profile="${NOTARY_PROFILE:-context-notary}"
slug="${RELEASE_REPO:-Model-Gateway/context-agent}"
notes="${NOTES:-}"
if [ -n "$notes" ] && [ ! -f "$notes" ]; then
  echo "release.sh: NOTES file not found: $notes" 1>&2
  exit 1
fi
if [ "${PUBLISH:-0}" = "1" ] && [ -n "$(git -C "$repo" status --porcelain)" ]; then
  echo "release.sh: PUBLISH=1 needs a clean checkout of the commit being released (try a git worktree)" 1>&2
  exit 1
fi
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
  MARKETING_VERSION="$version" CURRENT_PROJECT_VERSION="$version" \
  build | tail -3

# The project signs ad hoc (with get-task-allow, which notarization rejects).
# Re-sign everything inside-out with Developer ID, the hardened runtime and a
# secure timestamp; no entitlements are needed.
echo "==> signing"
for bundle in "$app"/Contents/Resources/*.bundle; do
  codesign --force --sign "$identity" --timestamp "$bundle"
done
# Sparkle's helpers first, then the framework (Sparkle's documented order;
# the downloader keeps its own entitlements).
sparkle="$app/Contents/Frameworks/Sparkle.framework"
codesign --force --sign "$identity" --options runtime --timestamp "$sparkle/Versions/B/XPCServices/Installer.xpc"
codesign --force --sign "$identity" --options runtime --timestamp --preserve-metadata=entitlements "$sparkle/Versions/B/XPCServices/Downloader.xpc"
codesign --force --sign "$identity" --options runtime --timestamp "$sparkle/Versions/B/Autoupdate"
codesign --force --sign "$identity" --options runtime --timestamp "$sparkle/Versions/B/Updater.app"
codesign --force --sign "$identity" --options runtime --timestamp "$sparkle"
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

# The feed lists only this release: Sparkle compares sparkle:version with the
# installed CFBundleVersion (both are the release version) and checks the
# zip's EdDSA signature against SUPublicEDKey before unpacking it.
echo "==> writing the appcast"
appcast="$dist/appcast.xml"
signature="$("$build/SourcePackages/artifacts/sparkle/Sparkle/bin/sign_update" "$zip")"
{
  cat <<XML
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Context</title>
    <link>https://github.com/$slug</link>
    <item>
      <title>Context $version</title>
      <pubDate>$(LC_ALL=C date -u "+%a, %d %b %Y %H:%M:%S +0000")</pubDate>
      <sparkle:version>$version</sparkle:version>
      <sparkle:shortVersionString>$version</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>26.0</sparkle:minimumSystemVersion>
      <sparkle:fullReleaseNotesLink>https://github.com/$slug/releases</sparkle:fullReleaseNotesLink>
XML
  if [ -n "$notes" ]; then
    printf '      <description sparkle:format="markdown"><![CDATA[\n'
    sed 's/]]>/]]]]><![CDATA[>/g' "$notes"
    printf '\n]]></description>\n'
  fi
  cat <<XML
      <enclosure url="https://github.com/$slug/releases/download/v$version/Context-$version.zip" type="application/octet-stream" $signature />
    </item>
  </channel>
</rss>
XML
} > "$appcast"
xmllint --noout "$appcast"

if [ "${PUBLISH:-0}" = "1" ]; then
  echo "==> publishing v$version on $slug"
  if [ -n "$notes" ]; then notes_args=(--notes-file "$notes"); else notes_args=(--generate-notes); fi
  gh release create "v$version" "$zip" "$dmg" "$appcast" --repo "$slug" \
    --target "$(git -C "$repo" rev-parse HEAD)" --title "Context $version" "${notes_args[@]}"
fi

shasum -a 256 "$zip" "$dmg"
echo "==> done: $dist"
