#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
script="$here/../ci-gate.sh"

gate() { NEEDS_JSON="$1" "$script"; }
case_for() { expect_status "$1" "$2" gate "$3"; }

case_for "all jobs succeeded" 0 '{"fmt":{"result":"success"},"test":{"result":"success"}}'
case_for "skipped jobs count as success" 0 '{"fmt":{"result":"success"},"test":{"result":"skipped"}}'
case_for "only skipped jobs still pass" 0 '{"test":{"result":"skipped"}}'
case_for "one failure fails the gate" 1 '{"fmt":{"result":"success"},"test":{"result":"failure"}}'
case_for "a cancelled job fails the gate" 1 '{"fmt":{"result":"cancelled"},"test":{"result":"success"}}'
case_for "failure wins over skipped" 1 '{"a":{"result":"skipped"},"b":{"result":"failure"}}'
case_for "no needed jobs fails" 1 '{}'
case_for "invalid JSON fails" 1 'not json'
case_for "an unknown result fails" 1 '{"a":{"result":"mystery"}}'
expect_status "missing variable fails" 1 env -u NEEDS_JSON "$script"

finish
