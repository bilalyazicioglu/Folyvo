#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
script="$here/../check-pins.sh"

sha="0123456789abcdef0123456789abcdef01234567"
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
mkdir -p "$dir/workflows" "$dir/actions/setup"

case_for() { # name want file-content
  printf '%s\n' "$3" > "$dir/workflows/t.yml"
  expect_status "$1" "$2" "$script" "$dir"
}

case_for "pinned action passes" 0 "      - uses: actions/checkout@$sha # v4.2.2"
case_for "tag reference fails" 1 "      - uses: actions/checkout@v4"
case_for "branch reference fails" 1 "      - uses: dtolnay/rust-toolchain@stable"
case_for "short sha fails" 1 "      - uses: actions/checkout@0123456"
case_for "local action passes" 0 "      - uses: ./.github/actions/setup-rust"
case_for "docker with digest passes" 0 "      - uses: docker://alpine@sha256:$(printf 'a%.0s' $(seq 1 64))"
case_for "docker with tag fails" 1 "      - uses: docker://alpine:3.20"
case_for "reusable workflow with path, pinned" 0 "    uses: owner/repo/.github/workflows/x.yml@$sha"
case_for "reusable workflow with path, unpinned" 1 "    uses: owner/repo/.github/workflows/x.yml@v1"
case_for "quoted value is still checked" 1 "      - uses: 'actions/checkout@v4'"
case_for "no uses lines at all passes" 0 "name: nothing here"

printf '      - uses: actions/setup-node@v4\n' > "$dir/actions/setup/action.yml"
printf '%s\n' "      - uses: actions/checkout@$sha # v4" > "$dir/workflows/t.yml"
expect_status "composite action files are checked too" 1 "$script" "$dir"

finish
