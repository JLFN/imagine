#!/usr/bin/env bash
# tests/qa-gate.sh — rule 19 guardrail mount for imagegen (canonical source:
# the shared review-gate template, parameterized for this repository).
#
# Before a unit is committed for delivery or pushed, an AI QA-tester (model
# chosen per project, recorded in qa-evidence/qa-report.md) must verify every new
# surface of the unit and leave a FRESH qa-evidence/qa-report.md whose LAST
# non-empty line is exactly "verdict: overall PASS". This mount fails the local
# gate until that holds, and skips in real CI where the QA tester cannot run.
#
# Surfaces this project extracts from the base..HEAD diff:
#   ENDPOINT <path>        an axum route added in crates/bridge/src/main.rs
#   BINARY <name>          a workspace member (binary target) added to Cargo.toml
#   CONFIG KEY <key>       a key added to imagegen.toml
#
# Env knobs:
#   QA_BASE      ref the surfaces are diffed against. Default: dev, because this
#                repository has no remote and dev is the integration branch.
#   QA_MODEL     the per-project QA model name, used in messages only.
#   QA_REPORT    the report path (default qa-evidence/qa-report.md).
cd "$(dirname "$0")/.." || exit 1

# Real CI: the QA tester is a local-session step, not provisioned there.
if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
  echo "SKIP tests/qa-gate.sh (AI QA-tester is a local-session step, not provisioned in CI)"
  exit 0
fi

BASE=${QA_BASE:-dev}
MODEL=${QA_MODEL:-$(grep -oE 'qa-model=[^ ]+' qa-evidence/qa-report.md 2>/dev/null | tail -1 || true)}
MODEL=${MODEL:-analytical-check}
REPORT=${QA_REPORT:-qa-evidence/qa-report.md}

BASE_SHA=$(git rev-parse "$BASE" 2>/dev/null || echo "")
HEAD_SHA=$(git rev-parse HEAD 2>/dev/null || echo "")
if [[ -z "$BASE_SHA" || "$BASE_SHA" == "$HEAD_SHA" ]]; then
  echo "QA GATE: ALL PASS (HEAD is at ${BASE}; nothing new to QA)"
  exit 0
fi

# ---- surface extraction ---------------------------------------------------
surfaces=""

# axum routes added anywhere in the bridge
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  surfaces+="ENDPOINT ${line}"$'\n'
done < <(git diff "${BASE}"..HEAD -- crates/bridge/src/main.rs 2>/dev/null \
  | grep -E '^\+' | grep -oE 'route\("[^"]+"' | sed -E 's/route\("//; s/"$//' | sort -u)

# workspace members added to Cargo.toml. A member that already existed at BASE is
# NOT a new surface: editing the members list (e.g. adding a second crate to a
# one-line array) makes every existing member appear in the added line, and
# demanding QA evidence for a pre-existing binary would be wrong.
while IFS= read -r member; do
  [[ -z "$member" ]] && continue
  # exists at BASE -> not new in this unit
  if git cat-file -e "${BASE_SHA}:crates/${member}/Cargo.toml" 2>/dev/null; then
    continue
  fi
  name=$(grep -m1 -E '^name *= *"' "crates/${member}/Cargo.toml" 2>/dev/null | sed -E 's/.*"(.*)".*/\1/')
  [[ -n "$name" ]] && surfaces+="BINARY ${name}"$'\n'
done < <(git diff "${BASE}"..HEAD -- Cargo.toml 2>/dev/null \
  | grep -E '^\+.*crates/' | grep -oE 'crates/[a-z0-9_-]+' | sed 's|crates/||' | sort -u)

# configuration keys added to imagegen.toml
while IFS= read -r key; do
  [[ -z "$key" ]] && continue
  surfaces+="CONFIG KEY ${key}"$'\n'
done < <(git diff "${BASE}"..HEAD -- imagegen.toml 2>/dev/null \
  | grep -E '^\+[a-z_]+ *=' | sed -E 's/^\+([a-z_]+) *=.*/\1/' | sort -u)

surfaces=$(echo "$surfaces" | sed '/^$/d')
if [[ -z "$surfaces" ]]; then
  echo "QA GATE: ALL PASS (no new endpoints, binaries, or config keys relative to ${BASE})"
  exit 0
fi

echo "New surfaces to be QA'd (${MODEL}):"
echo "$surfaces" | sed 's/^/  /'

failures=0
check() { if eval "$2"; then echo "PASS  $1"; else echo "FAIL  $1"; failures=$((failures+1)); fi; }

check "QA report exists ($(basename "$REPORT"))" "[[ -f '$REPORT' ]]"
base_sec=$(git log -1 --format=%ct "$BASE" 2>/dev/null || echo 0)
check "QA report is fresh (>= commit time of ${BASE})" \
  "[[ -f '$REPORT' ]] && [[ '$(stat -c %Y "$REPORT" 2>/dev/null || echo 0)' -ge '$base_sec' ]]"

# The OPERATIVE verdict is the report's LAST non-empty line and nothing else: a
# FAILed report, or one with trailing prose after the verdict, does not pass.
last_verdict=$(grep -v '^[[:space:]]*$' "$REPORT" 2>/dev/null | tail -1 | tr 'A-Z' 'a-z' | tr -s ' ' | sed 's/[[:space:]]*$//')
check "operative verdict is PASS (report's last non-empty line: ${last_verdict:-none})" \
  "[[ '$last_verdict' == 'verdict: overall pass' ]]"

while IFS= read -r surf; do
  [[ -z "$surf" ]] && continue
  check "report covers surface: $surf" "grep -qF '$surf' '$REPORT'"
done <<< "$surfaces"

if (( failures == 0 )); then
  echo "QA GATE: overall PASS (${MODEL} evidence fresh and covering)"
  exit 0
else
  echo "QA GATE: FAIL (${failures} check(s) red). Run the AI QA-tester on ${MODEL},"
  echo "have it verify the surfaces above live, and update $REPORT with"
  echo "'verdict: overall PASS' before committing/pushing."
  exit 1
fi
