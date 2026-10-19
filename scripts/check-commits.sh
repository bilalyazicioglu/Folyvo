#!/usr/bin/env bash
# Checks that every commit in a revision range follows Conventional Commits
# and that the range contains no merge commits (history on main stays linear).
# Usage: check-commits.sh <revision range>, for example origin/main..HEAD
set -euo pipefail

range="${1:?usage: check-commits.sh <revision range>}"
types='feat|fix|perf|refactor|test|docs|build|ci|chore|revert'
pattern="^(${types})(\\([a-z0-9._/-]+\\))?!?: .*[^ ]"
status=0

while IFS= read -r sha; do
  subject="$(git log -1 --format=%s "$sha")"
  parents="$(git log -1 --format=%p "$sha" | wc -w | tr -d ' ')"
  short="${sha:0:9}"

  if [ "$parents" -gt 1 ]; then
    echo "merge commit not allowed: $short $subject"
    status=1
    continue
  fi
  if [[ "$subject" == Revert\ \"* ]]; then
    continue
  fi
  if [[ "$subject" =~ $pattern ]] && [ "${#subject}" -le 100 ]; then
    continue
  fi
  echo "subject does not follow Conventional Commits: $short $subject"
  status=1
done < <(git rev-list "$range")

exit "$status"
