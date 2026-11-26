#!/usr/bin/env bash
# scripts/autopilot/gate.sh — the local pre-PR gate, as one deterministic command.
#
# Mirrors .github/workflows/compat.yml (unit / smoke / golden / fix / fmt / reject /
# isolate / filesets / oss-pr) plus the regress gate from docs/DEVELOPMENT.md §4.3.1.
# The one deliberate omission is the OSS *nightly* tier: on this machine it was
# killed for memory twice (Phase 9), and CI runs it on every push to main.
#
# Usage:
#   scripts/autopilot/gate.sh            # full gate, stop at first failure
#   scripts/autopilot/gate.sh --perf     # also regress --profile full (perf / refactor work)
#   scripts/autopilot/gate.sh --from isolate   # resume from a step after a fix
#   scripts/autopilot/gate.sh --list     # print step names
#
# Writes one log per step to $GATE_OUT (default: a fresh mktemp dir) and prints a
# summary table. Exit 0 only when every step passed.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT" || exit 2

PERF=0
FROM=""
LIST=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --perf) PERF=1 ;;
    --from) FROM="$2"; shift ;;
    --list) LIST=1 ;;
    *) echo "gate: unknown arg $1" >&2; exit 2 ;;
  esac
  shift
done

STEPS="hygiene unit-py build build-fmt golden fix fmt reject smoke cargo-test isolate filesets oss-pr regress"
[[ $PERF == 1 ]] && STEPS="$STEPS regress-full"
if [[ $LIST == 1 ]]; then echo "$STEPS" | tr ' ' '\n'; exit 0; fi

PIN="$(python3 -c 'import json;print(json.load(open("compat/pins.json"))["golangci_lint"])')"
GL="$ROOT/compat/.tools/golangci-lint-$PIN/golangci-lint"
if [[ ! -x "$GL" ]]; then
  echo "gate: pinned golangci-lint $PIN missing at $GL" >&2
  exit 2
fi
export GOLANGCI_LINT_BIN="$GL"
export GOTOOLCHAIN=local
# golines / swag at the versions golangci-lint embeds (compat/pins.json).
TOOLS="${AUTOPILOT_TOOLS:-$HOME/.guff-autopilot/bin}"
[[ -d "$TOOLS" ]] && export PATH="$TOOLS:$PATH"
want_golines="$(python3 -c 'import json;print(json.load(open("compat/pins.json"))["golines"].lstrip("v"))')"
if ! golines --version 2>/dev/null | grep -q "version $want_golines"; then
  echo "gate: golines on PATH is not $want_golines (isolate would report false diffs)" >&2
  exit 2
fi

OUT="${GATE_OUT:-$(mktemp -d "${TMPDIR:-/tmp}/guff-gate.XXXXXX")}"
mkdir -p "$OUT"
SUMMARY="$OUT/summary.txt"
: > "$SUMMARY"

cmd_for() {
  case "$1" in
    hygiene)      echo "./scripts/target-hygiene.sh --prune" ;;
    unit-py)      echo "python3 -m unittest discover -s compat/tests" ;;
    build)        echo "cargo build --release --locked -p guff-lint" ;;
    build-fmt)    echo "cargo build --release --locked -p guff-fmt --bin guff-fmt-native" ;;
    golden)       echo "./compat/golden/run.sh" ;;
    fix)          echo "./compat/fix/run.sh" ;;
    fmt)          echo "./compat/fmt/run.sh" ;;
    reject)       echo "./compat/reject/run.sh" ;;
    smoke)        echo "./compat/smoke.sh" ;;
    cargo-test)   echo "cargo test --workspace --locked" ;;
    isolate)      echo "./compat/run.sh --isolate" ;;
    filesets)     echo "./compat/filesets.sh --isolate" ;;
    oss-pr)       echo "./compat/run.sh --oss --tier pr" ;;
    regress)      echo "./regress/run.sh" ;;
    regress-full) echo "./regress/run.sh --profile full" ;;
  esac
}

started=0
[[ -z "$FROM" ]] && started=1
rc_all=0
for s in $STEPS; do
  if [[ $started == 0 ]]; then
    [[ "$s" == "$FROM" ]] && started=1 || continue
  fi
  c="$(cmd_for "$s")"
  log="$OUT/$s.log"
  t0=$(date +%s)
  printf -- '-- gate: %-12s %s\n' "$s" "$c"
  bash -c "$c" > "$log" 2>&1
  rc=$?
  dt=$(( $(date +%s) - t0 ))
  if [[ $rc == 0 ]]; then
    printf '%-12s PASS %5ss\n' "$s" "$dt" | tee -a "$SUMMARY"
  else
    printf '%-12s FAIL %5ss  (exit %s, log %s)\n' "$s" "$dt" "$rc" "$log" | tee -a "$SUMMARY"
    echo "---- last 40 lines of $log"
    tail -40 "$log"
    rc_all=1
    break
  fi
done

if [[ $started == 0 ]]; then
  echo "gate: --from $FROM matched no step" >&2
  exit 2
fi
echo "gate: logs in $OUT"
if [[ $rc_all == 0 ]]; then echo "GATE: PASS"; else echo "GATE: FAIL"; fi
exit $rc_all
