#!/usr/bin/env bash
# Builds Folyvo's Finder Sync extension without Xcode or an Apple account: swiftc, then an ad-hoc
# signature with the app sandbox, which Finder requires. Tauri copies the result into the app bundle.
# Usage: build.sh [out_dir]   (default: apps/desktop/finder/build)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="${1:-$here/build}"

if [ "$(uname)" != "Darwin" ]; then
  echo "The Finder extension is macOS only; nothing to build."
  exit 0
fi

# Built and signed in a private temporary folder: in a folder synced by iCloud (the Desktop, say) the
# system tags a new .appex with Finder attributes, and codesign refuses to sign those.
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
appex="$work/FolyvoFinderSync.appex"
mkdir -p "$appex/Contents/MacOS"
cp "$here/Info.plist" "$appex/Contents/Info.plist"
swiftc -swift-version 5 -O -application-extension -module-name FolyvoFinderSync -parse-as-library \
  -framework FinderSync -Xlinker -e -Xlinker _NSExtensionMain \
  -o "$appex/Contents/MacOS/FolyvoFinderSync" "$here/FolyvoFinderSync.swift"
codesign --force --sign - --entitlements "$here/FolyvoFinderSync.entitlements" "$appex"

mkdir -p "$out"
rm -rf "$out/FolyvoFinderSync.appex"
ditto "$appex" "$out/FolyvoFinderSync.appex"
# The copy is tagged again in a synced folder; the tags are not part of the signature, so drop them.
xattr -cr "$out/FolyvoFinderSync.appex"
echo "$out/FolyvoFinderSync.appex"
