#!/usr/bin/env bash
# Tiny assertion helpers shared by the script tests. Source this file; do not execute it.
FAILURES=0

pass() { printf 'ok   %s\n' "$1"; }
fail() { printf 'FAIL %s\n' "$1"; FAILURES=$((FAILURES + 1)); }

# expect_status <name> <wanted exit status> <command...>
expect_status() {
  local name="$1" want="$2"
  shift 2
  local out got
  set +e
  out="$("$@" 2>&1)"
  got=$?
  set -e
  if [ "$got" -eq "$want" ]; then
    pass "$name"
  else
    fail "$name (wanted exit $want, got $got)"
    printf '%s\n' "$out" | sed 's/^/     /'
  fi
}

# assert_grep <name> <extended regex> <file>
assert_grep() {
  if grep -qE -- "$2" "$3"; then pass "$1"; else fail "$1"; fi
}

# assert_same <name> <file a> <file b>
assert_same() {
  if cmp -s "$2" "$3"; then pass "$1"; else fail "$1"; fi
}

finish() {
  if [ "$FAILURES" -ne 0 ]; then
    printf '%d failure(s)\n' "$FAILURES"
    exit 1
  fi
  echo "all passed"
}
