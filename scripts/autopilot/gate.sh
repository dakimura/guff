#!/usr/bin/env bash
# scripts/autopilot/gate.sh — the local pre-PR gate, as one deterministic command.
#
# Mirrors .github/workflows/compat.yml (unit / smoke / golden / fix / fmt / reject /
# isolate / filesets / oss-pr) plus the regress gate from docs/DEVELOPMENT.md §4.3.1.
# The one deliberate omission is the OSS *nightly* tier: on this machine it was
# killed for memory twice (Phase 9), and CI runs it on every push to main.
#
# Usage:
#   gate.sh                  # full gate, stop at first failure; records the result
#   gate.sh --keep-going     # full gate, run every step even after a failure; records
#   gate.sh --perf           # also regress --profile full (perf / refactor work)
#   gate.sh --quick          # dev loop: build + golden + fix + cargo test of the crates
#                            # changed vs origin/main. Never recorded.
#   gate.sh --from isolate   # resume from a step (not recorded: partial)
#   gate.sh --only build,regress   # just these steps (recorded as "only", used by the
#                                  # driver's main-health check)
#   gate.sh --list
#
# Recording: a full run (no --from / --quick) writes
#   $AUTOPILOT_STATE/gates/<tree>.tsv      one "step<TAB>PASS|FAIL<TAB>secs" per step run
# where <tree> is the git tree of the working directory *when the gate started*
# (tracked + untracked-but-not-ignored files). The driver merges a PR that touches
# code only if a full record exists for the PR head's tree — i.e. the gate ran on
# exactly what was pushed. Files the gate itself rewrites (RESULTS*.md) must not be
# committed, or the tree no longer matches.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT" || exit 2
STATE="${AUTOPILOT_STATE:-$HOME/.guff-autopilot}"

PERF=0; FROM=""; LIST=0; ONLY=""; QUICK=0; KEEP=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --perf) PERF=1 ;;
    --from) FROM="$2"; shift ;;
    --only) ONLY="$2"; shift ;;
    --quick) QUICK=1 ;;
    --keep-going) KEEP=1 ;;
    --list) LIST=1 ;;
    *) echo "gate: unknown arg $1" >&2; exit 2 ;;
  esac
  shift
done

STEPS="hygiene unit-py build build-fmt golden fix fmt reject smoke cargo-test isolate filesets oss-pr regress"
[[ $PERF == 1 ]] && STEPS="$STEPS regress-full"
if [[ $LIST == 1 ]]; then echo "$STEPS regress-full" | tr ' ' '\n'; exit 0; fi

# Crates changed vs origin/main (committed or not). Empty => whole workspace.
changed_crates() {
  local files
  files="$( { git diff --name-only origin/main...HEAD; git diff --name-only; git ls-files --others --exclude-standard; } 2>/dev/null | sort -u)"
  if echo "$files" | grep -qE '^(Cargo\.(toml|lock)|crates/[^/]+/Cargo\.toml)$'; then return; fi
  echo "$files" | sed -nE 's#^crates/([^/]+)/.*#\1#p' | sort -u | tr '\n' ' '
}

MODE=full
if [[ $QUICK == 1 ]]; then
  MODE=quick
  STEPS="build golden fix cargo-test-changed"
elif [[ -n "$ONLY" ]]; then
  MODE=only
  STEPS="$(echo "$ONLY" | tr ',' ' ')"
elif [[ -n "$FROM" ]]; then
  MODE=partial
fi

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

# The tree under test, taken before any step can rewrite tracked files.
TREE=""
if [[ $MODE == full || $MODE == only ]]; then
  idx="$(mktemp "${TMPDIR:-/tmp}/gate-index.XXXXXX")"
  if GIT_INDEX_FILE="$idx" git read-tree HEAD && GIT_INDEX_FILE="$idx" git add -A . \
     && TREE="$(GIT_INDEX_FILE="$idx" git write-tree)"; then :; else TREE=""; fi
  rm -f "$idx"
fi

OUT="${GATE_OUT:-$(mktemp -d "${TMPDIR:-/tmp}/guff-gate.XXXXXX")}"
mkdir -p "$OUT"
SUMMARY="$OUT/summary.txt"
: > "$SUMMARY"
git diff --name-only > "$OUT/dirty-before.txt" 2>/dev/null

CHANGED=""
[[ $QUICK == 1 ]] && CHANGED="$(changed_crates)"

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
    cargo-test-changed)
      if [[ -n "$CHANGED" ]]; then
        local a=""; for c in $CHANGED; do a="$a -p $c"; done
        echo "cargo test --locked$a"
      else
        echo "cargo test --workspace --locked"
      fi ;;
    isolate)      echo "./compat/run.sh --isolate" ;;
    filesets)     echo "./compat/filesets.sh --isolate" ;;
    oss-pr)       echo "./compat/run.sh --oss --tier pr" ;;
    # Timing gates are sensitive to load from whatever else the machine is doing:
    # up to 3 attempts, and the attempt count is printed so a flake is visible.
    regress)      echo "for a in 1 2 3; do ./regress/run.sh && { echo \"regress: PASS on attempt \$a\"; exit 0; }; echo \"regress: attempt \$a failed\"; sleep 20; done; exit 1" ;;
    regress-full) echo "for a in 1 2; do ./regress/run.sh --profile full && { echo \"regress-full: PASS on attempt \$a\"; exit 0; }; echo \"regress-full: attempt \$a failed\"; sleep 20; done; exit 1" ;;
    *) return 1 ;;
  esac
}

started=0
[[ -z "$FROM" ]] && started=1
rc_all=0
RECORD="$OUT/record.tsv"
: > "$RECORD"
for s in $STEPS; do
  if [[ $started == 0 ]]; then
    [[ "$s" == "$FROM" ]] && started=1 || continue
  fi
  c="$(cmd_for "$s")" || { echo "gate: unknown step $s" >&2; exit 2; }
  log="$OUT/$s.log"
  t0=$(date +%s)
  printf -- '-- gate: %-18s %s\n' "$s" "$c"
  bash -c "$c" > "$log" 2>&1
  rc=$?
  dt=$(( $(date +%s) - t0 ))
  if [[ $rc == 0 ]]; then
    printf '%-18s PASS %5ss\n' "$s" "$dt" | tee -a "$SUMMARY"
    printf '%s\tPASS\t%s\n' "$s" "$dt" >> "$RECORD"
  else
    printf '%-18s FAIL %5ss  (exit %s, log %s)\n' "$s" "$dt" "$rc" "$log" | tee -a "$SUMMARY"
    printf '%s\tFAIL\t%s\n' "$s" "$dt" >> "$RECORD"
    echo "---- last 40 lines of $log"
    tail -40 "$log"
    rc_all=1
    [[ $KEEP == 1 ]] || break
  fi
done

if [[ $started == 0 ]]; then
  echo "gate: --from $FROM matched no step" >&2
  exit 2
fi

# The compat / regress harnesses rewrite tracked report files (RESULTS*.md). Put back
# the ones that were clean when the gate started, so the tree stays the one tested.
for f in $(git diff --name-only -- compat/results regress/results 2>/dev/null); do
  if ! grep -qxF "$f" "$OUT/dirty-before.txt" 2>/dev/null; then
    git checkout -q -- "$f" && echo "gate: restored $f (rewritten by the harness)"
  fi
done

if [[ -n "$TREE" ]]; then
  mkdir -p "$STATE/gates"
  { printf '#mode\t%s\n#head\t%s\n#date\t%s\n#perf\t%s\n' "$MODE" "$(git rev-parse HEAD)" "$(date '+%F %T')" "$PERF"
    cat "$RECORD"; } > "$STATE/gates/$TREE.tsv"
  echo "gate: recorded $MODE result for tree $TREE"
  if [[ $MODE == full ]] && [[ -n "$(git status --porcelain)" ]]; then
    echo "gate: NOTE the working tree has uncommitted changes. Commit exactly this tree (git add -A) or the record will not match the pushed commit."
  fi
fi
echo "gate: logs in $OUT"
if [[ $rc_all == 0 ]]; then echo "GATE: PASS ($MODE)"; else echo "GATE: FAIL ($MODE)"; fi
exit $rc_all
