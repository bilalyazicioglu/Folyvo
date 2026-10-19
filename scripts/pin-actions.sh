#!/usr/bin/env bash
# Rewrites `uses: owner/repo[/path]@ref` to `uses: owner/repo[/path]@<sha> # ref`.
# Needs an authenticated `gh`. Leaves pinned lines, local actions and docker references alone.
# If any reference cannot be resolved, nothing is written.
# Usage: pin-actions.sh [directory]   (default: .github)
set -euo pipefail

dir="${1:-.github}"
failed=0
tmp_files=()
cleanup() { [ "${#tmp_files[@]}" -eq 0 ] || rm -f "${tmp_files[@]}"; }
trap cleanup EXIT

while IFS= read -r file; do
  tmp="$(mktemp)"
  tmp_files+=("$tmp")
  changed=0
  while IFS= read -r line || [ -n "$line" ]; do
    if [[ "$line" =~ ^([[:space:]-]*uses:[[:space:]]*)([A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+)@([^[:space:]#]+)[[:space:]]*(#.*)?$ ]]; then
      prefix="${BASH_REMATCH[1]}"
      action="${BASH_REMATCH[2]}"
      ref="${BASH_REMATCH[3]}"
      comment="${BASH_REMATCH[4]}"
      if [[ "$ref" =~ ^[0-9a-f]{40}$ ]]; then
        printf '%s\n' "$line" >> "$tmp"
        continue
      fi
      owner_repo="$(printf '%s' "$action" | cut -d/ -f1-2)"
      if ! sha="$(gh api "repos/${owner_repo}/commits/${ref}" --jq .sha 2>/dev/null)" || [[ ! "$sha" =~ ^[0-9a-f]{40}$ ]]; then
        echo "cannot resolve ${action}@${ref}" >&2
        failed=1
        printf '%s\n' "$line" >> "$tmp"
        continue
      fi
      note="# ${ref}"
      [ -n "$comment" ] && note="$comment"
      printf '%s%s@%s %s\n' "$prefix" "$action" "$sha" "$note" >> "$tmp"
      changed=1
    else
      printf '%s\n' "$line" >> "$tmp"
    fi
  done < "$file"
  if [ "$changed" -eq 1 ]; then
    cp "$tmp" "$file.pinned"
  fi
done < <(find "$dir" -type f \( -name '*.yml' -o -name '*.yaml' \) | sort)

if [ "$failed" -ne 0 ]; then
  find "$dir" -name '*.pinned' -delete
  exit 1
fi
while IFS= read -r pinned; do
  mv "$pinned" "${pinned%.pinned}"
done < <(find "$dir" -name '*.pinned')
