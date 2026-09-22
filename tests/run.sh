#!/usr/bin/env bash
# tests/run.sh — the project's single test entry point.
#
# Runs the Rust test suite, then the rule 19 QA guardrail mount. A non-zero exit
# from either fails the run, so this is what a unit boundary should call before
# committing for delivery or pushing.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

rc=0

echo "== cargo test (workspace) =="
if cargo test --workspace 2>&1 | tail -25; then
  echo "cargo test: ok"
else
  echo "cargo test: FAILED"
  rc=1
fi

echo
echo "== rule 19 QA gate =="
if bash tests/qa-gate.sh; then
  : # message already printed
else
  rc=1
fi

echo
if (( rc == 0 )); then
  echo "tests/run.sh: PASS"
else
  echo "tests/run.sh: FAIL"
fi
exit "$rc"
