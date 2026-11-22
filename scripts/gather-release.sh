#!/usr/bin/env bash
# Gathers what a release offers from a universal macOS build: the disk image people download, the
# signed app archive the app updates itself from, latest.json that tells the app about it, and
# SHA256SUMS. One universal build serves both Apple silicon and Intel Macs.
# Usage: gather-release.sh <version> <bundle dir> <out dir> <owner/repo>
set -euo pipefail

version="${1:?version, such as 0.0.1}"
bundle="${2:?the bundle folder tauri build wrote}"
out="${3:?where to put the release files}"
repo="${4:?owner/repo}"

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "not a version: $version" >&2
  exit 1
fi
dmg="$bundle/dmg/Folyvo_${version}_universal.dmg"
archive="$bundle/macos/Folyvo.app.tar.gz"
for f in "$dmg" "$archive" "$archive.sig"; do
  if [ ! -f "$f" ]; then
    echo "missing from the build: $f" >&2
    exit 1
  fi
done

mkdir -p "$out"
name="Folyvo_${version}_universal"
cp "$dmg" "$out/$name.dmg"
cp "$archive" "$out/$name.app.tar.gz"

url="https://github.com/$repo/releases/download/v$version/$name.app.tar.gz"
jq -n \
  --arg version "$version" \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg url "$url" \
  --arg signature "$(tr -d '\n' <"$archive.sig")" \
  '{version: $version, notes: "See the release notes on GitHub.", pub_date: $date,
    platforms: {"darwin-aarch64": {signature: $signature, url: $url},
                "darwin-x86_64": {signature: $signature, url: $url}}}' >"$out/latest.json"

(cd "$out" && shasum -a 256 "$name.dmg" "$name.app.tar.gz" >SHA256SUMS)
ls "$out"
