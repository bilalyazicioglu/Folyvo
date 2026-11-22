#!/usr/bin/env bash
# Gathers a release's files from a fake build and checks what the app and people will read.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
gather="$here/../gather-release.sh"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
bundle="$work/bundle"
mkdir -p "$bundle/dmg" "$bundle/macos"
echo "disk image" >"$bundle/dmg/Folyvo_0.0.1_universal.dmg"
echo "app archive" >"$bundle/macos/Folyvo.app.tar.gz"
printf 'c2lnbmF0dXJl' >"$bundle/macos/Folyvo.app.tar.gz.sig"

expect_status "gathers a built release" 0 bash "$gather" 0.0.1 "$bundle" "$work/out" owner/Repo
out="$work/out"
for f in Folyvo_0.0.1_universal.dmg Folyvo_0.0.1_universal.app.tar.gz latest.json SHA256SUMS; do
  if [ -f "$out/$f" ]; then pass "$f is there"; else fail "$f is there"; fi
done

manifest="$out/latest.json"
url="https://github.com/owner/Repo/releases/download/v0.0.1/Folyvo_0.0.1_universal.app.tar.gz"
if [ "$(jq -r .version "$manifest")" = "0.0.1" ]; then pass "the manifest's version"; else fail "the manifest's version"; fi
for arch in darwin-aarch64 darwin-x86_64; do
  if [ "$(jq -r ".platforms[\"$arch\"].url" "$manifest")" = "$url" ]; then
    pass "$arch gets the universal archive"
  else
    fail "$arch gets the universal archive"
  fi
  if [ "$(jq -r ".platforms[\"$arch\"].signature" "$manifest")" = "c2lnbmF0dXJl" ]; then
    pass "$arch carries the signature"
  else
    fail "$arch carries the signature"
  fi
done
if jq -e '.pub_date | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z$")' "$manifest" >/dev/null; then
  pass "a publication date"
else
  fail "a publication date"
fi

(cd "$out" && expect_status "the checksums verify" 0 shasum -a 256 -c SHA256SUMS)
assert_grep "the disk image is summed" "Folyvo_0.0.1_universal.dmg$" "$out/SHA256SUMS"

expect_status "refuses a build without a signature" 1 bash -c "rm '$bundle/macos/Folyvo.app.tar.gz.sig' && bash '$gather' 0.0.1 '$bundle' '$work/out2' owner/Repo"
expect_status "refuses a version that is not one" 1 bash "$gather" v0.0.1 "$bundle" "$work/out3" owner/Repo
finish
