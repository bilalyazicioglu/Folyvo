#!/usr/bin/env bash
# Fails when a workflow or composite action uses a third-party action that is not pinned
# to a full commit SHA. Local actions and docker references with a digest are accepted.
# Usage: check-pins.sh [directory]   (default: .github)
set -euo pipefail

dir="${1:-.github}"
status=0

while IFS= read -r file; do
  while IFS= read -r line; do
    value="$(printf '%s' "$line" | sed -E "s/^[[:space:]-]*uses:[[:space:]]*//; s/[[:space:]]+#.*$//; s/^['\"]//; s/['\"]$//")"
    case "$value" in
      ./*) continue ;;
      docker://*@sha256:*)
        digest="${value##*@sha256:}"
        if [[ "$digest" =~ ^[0-9a-f]{64}$ ]]; then continue; fi
        ;;
      *@*)
        ref="${value##*@}"
        if [[ "$ref" =~ ^[0-9a-f]{40}$ ]]; then continue; fi
        ;;
    esac
    echo "not pinned: $file: $value"
    status=1
  done < <(grep -E '^[[:space:]-]*uses:' "$file" || true)
done < <(find "$dir" -type f \( -name '*.yml' -o -name '*.yaml' \) | sort)

exit "$status"
