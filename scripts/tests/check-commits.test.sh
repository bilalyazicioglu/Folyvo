#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
script="$here/../check-commits.sh"

repo="$(mktemp -d)"
trap 'rm -rf "$repo"' EXIT
git -C "$repo" init -q -b main
git -C "$repo" config user.name "Test"
git -C "$repo" config user.email "test@example.com"
git -C "$repo" commit -q --allow-empty -m "chore: root"
base="$(git -C "$repo" rev-parse HEAD)"

run_range() { bash -c 'cd "$1" && "$2" "$3..HEAD"' _ "$repo" "$script" "$base"; }

subject_case() { # name wanted subject
  git -C "$repo" checkout -q main
  git -C "$repo" reset -q --hard "$base"
  git -C "$repo" commit -q --allow-empty -m "$3"
  expect_status "$1" "$2" run_range
}

subject_case "feat" 0 "feat: add a thing"
subject_case "fix with scope" 0 "fix(core): handle empty files"
subject_case "breaking marker" 0 "feat(core)!: change the export layout"
subject_case "scope with slash and dot" 0 "ci(.github/workflows): pin actions"
subject_case "release-please subject" 0 "chore(main): release 0.1.0"
subject_case "dependabot subject" 0 "build(deps): bump tokio from 1.40 to 1.41"
subject_case "git revert subject" 0 'Revert "feat: add a thing"'
subject_case "plain sentence is rejected" 1 "Add a thing"
subject_case "missing space after colon" 1 "feat:add a thing"
subject_case "uppercase type" 1 "Feat: add a thing"
subject_case "empty scope" 1 "feat(): add a thing"
subject_case "unknown type" 1 "wip: half done"
subject_case "fixup commit" 1 "fixup! feat: add a thing"
subject_case "squash commit" 1 "squash! feat: add a thing"
subject_case "empty description" 1 "feat: "
long="feat: $(printf 'x%.0s' $(seq 1 100))"
subject_case "subject over 100 characters" 1 "$long"

# a body with other lines does not matter, only the subject does
git -C "$repo" checkout -q main
git -C "$repo" reset -q --hard "$base"
git -C "$repo" commit -q --allow-empty -m "feat: add a thing" -m "Free text body.\nSigned-off-by: someone"
expect_status "body text is ignored" 0 run_range

# an empty range passes
# shellcheck disable=SC2016
expect_status "empty range" 0 bash -c 'cd "$1" && "$2" "HEAD..HEAD"' _ "$repo" "$script"

# several commits: one bad subject fails the range
git -C "$repo" reset -q --hard "$base"
git -C "$repo" commit -q --allow-empty -m "feat: good one"
git -C "$repo" commit -q --allow-empty -m "oops"
git -C "$repo" commit -q --allow-empty -m "fix: good two"
expect_status "one bad commit among several fails" 1 run_range

# merge commits are rejected even with a valid subject
git -C "$repo" reset -q --hard "$base"
git -C "$repo" checkout -q -b side
git -C "$repo" commit -q --allow-empty -m "feat: on the side"
git -C "$repo" checkout -q main
git -C "$repo" commit -q --allow-empty -m "feat: on main"
git -C "$repo" merge -q --no-ff side -m "feat: merge side"
expect_status "merge commit is rejected" 1 run_range

finish
