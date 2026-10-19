#!/usr/bin/env bash
# Decides the verdict of the ci-success job from the `needs` context.
# Passes when at least one job was needed and every job succeeded or was skipped.
# Input: the environment variable NEEDS_JSON, set to ${{ toJSON(needs) }}.
set -euo pipefail

: "${NEEDS_JSON:?NEEDS_JSON is not set}"

if ! printf '%s' "$NEEDS_JSON" | jq -e 'type == "object"' >/dev/null 2>&1; then
  echo "NEEDS_JSON is not a JSON object" >&2
  exit 1
fi

count="$(printf '%s' "$NEEDS_JSON" | jq 'length')"
if [ "$count" -eq 0 ]; then
  echo "no jobs were needed: refusing to pass an empty gate" >&2
  exit 1
fi

printf '%s' "$NEEDS_JSON" | jq -r 'to_entries[] | "\(.key): \(.value.result)"'

bad="$(printf '%s' "$NEEDS_JSON" | jq -r 'to_entries[] | select(.value.result != "success" and .value.result != "skipped") | .key' | paste -sd, -)"
if [ -n "$bad" ]; then
  echo "failed, cancelled or unknown: $bad" >&2
  exit 1
fi
echo "all required jobs passed"
