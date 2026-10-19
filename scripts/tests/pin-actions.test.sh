#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
script="$here/../pin-actions.sh"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin" "$work/.github/workflows"

# A fake `gh` that answers `gh api repos/<o>/<r>/commits/<ref> --jq .sha`
# with a fixed, recognisable sha derived from the ref.
cat > "$work/bin/gh" <<'FAKE'
#!/usr/bin/env bash
[ "$1" = "api" ] || exit 2
path="$2"
ref="${path##*/commits/}"
if [ "$ref" = "missing" ]; then echo "not found" >&2; exit 1; fi
printf '%s' "$ref" | od -An -tx1 | tr -d ' \n' | cut -c1-40 | awk '{ printf "%-40s\n", $0 }' | tr ' ' '0'
FAKE
chmod +x "$work/bin/gh"

wf="$work/.github/workflows/t.yml"
run() { PATH="$work/bin:$PATH" "$script" "$work/.github"; }

# tags and branches are pinned and the human-readable ref is kept as a comment
cat > "$wf" <<'YAML'
jobs:
  a:
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: owner/repo/.github/workflows/x.yml@v1
YAML
expect_status "pinning succeeds" 0 run
expect_status "result passes the pin checker" 0 "$here/../check-pins.sh" "$work/.github"
assert_grep "tag is kept as a comment" '# v4$' "$wf"
assert_grep "branch name is kept as a comment" '# stable$' "$wf"
assert_grep "reusable workflow path is preserved" 'owner/repo/\.github/workflows/x\.yml@[0-9a-f]{40} # v1' "$wf"

# idempotent: a second run changes nothing
cp "$wf" "$work/after-first.yml"
run >/dev/null
assert_same "second run is a no-op" "$wf" "$work/after-first.yml"

# local actions, docker references and already pinned lines are untouched
sha="0123456789abcdef0123456789abcdef01234567"
cat > "$wf" <<YAML
      - uses: ./.github/actions/setup-rust
      - uses: docker://alpine:3.20
      - uses: actions/checkout@$sha # v4.2.2
YAML
cp "$wf" "$work/before.yml"
run >/dev/null || true
assert_same "local, docker and pinned lines are untouched" "$wf" "$work/before.yml"

# an unresolvable ref makes the script fail and leaves the file unchanged
cat > "$wf" <<'YAML'
      - uses: actions/checkout@missing
YAML
cp "$wf" "$work/before.yml"
expect_status "unresolvable ref fails" 1 run
assert_same "file is untouched after a failure" "$wf" "$work/before.yml"

finish
