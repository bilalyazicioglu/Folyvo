#!/usr/bin/env bash
# Builds the Finder extension and checks what macOS will read from it. macOS only.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
build="$here/../../apps/desktop/finder/build.sh"

if [ "$(uname)" != "Darwin" ]; then
  echo "skipped: the Finder extension builds only on macOS"
  exit 0
fi

out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
expect_status "the extension builds" 0 bash "$build" "$out"
appex="$out/FolyvoFinderSync.appex"
plist="$appex/Contents/Info.plist"

if [ -x "$appex/Contents/MacOS/FolyvoFinderSync" ]; then pass "the binary is there"; else fail "the binary is there"; fi
read_key() { /usr/libexec/PlistBuddy -c "Print :$1" "$plist" 2>/dev/null || true; }
if [ "$(read_key CFBundleIdentifier)" = "dev.folyvo.app.findersync" ]; then pass "bundle id"; else fail "bundle id"; fi
if [ "$(read_key NSExtension:NSExtensionPointIdentifier)" = "com.apple.FinderSync" ]; then
  pass "a Finder Sync extension"
else
  fail "a Finder Sync extension"
fi
expect_status "the signature verifies" 0 codesign --verify --strict "$appex"
codesign -d --entitlements - --xml "$appex" >"$out/ent.xml" 2>/dev/null || true
assert_grep "sandboxed" "com.apple.security.app-sandbox" "$out/ent.xml"
if grep -q "application-groups" "$out/ent.xml"; then fail "no App Group"; else pass "no App Group"; fi
finish
