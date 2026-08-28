#!/usr/bin/env bash
# compat/fix/run.sh — `--fix` gate: compare what the two tools *write*.
#
# Usage:
#   ./compat/fix/run.sh                 # check every case against its expectation
#   ./compat/fix/run.sh --case godot    # one case
#   ./compat/fix/run.sh --regen         # re-record from golangci-lint --fix
#   ./compat/fix/run.sh --record-pending # re-record the known-missing gaps
#
# The corpus is compat/golden/cases — the same 193 cases the golden tier gates,
# read through the same materialize.sh. The question is different: golden keys
# on `path:line:col:linter:severity:text`, which does not contain a suggested
# fix's replacement text, so a linter can report perfectly and rewrite the file
# wrongly (or not at all) with the golden tier fully green.
#
# An absent expected/<case>.diff means upstream's --fix changes nothing there,
# and guff must change nothing either.
#
# Env: GUFF_BIN / GOLANGCI_LINT_BIN
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIX_DIR="$ROOT/compat/fix"
GOLDEN_DIR="$ROOT/compat/golden"
CASES_DIR="$GOLDEN_DIR/cases"
EXPECTED_DIR="$FIX_DIR/expected"
PENDING_DIR="$FIX_DIR/pending"
DIVERGENT_DIR="$FIX_DIR/divergent"
FIXDIFF="$FIX_DIR/fixdiff.py"
HEALTH="$ROOT/compat/health.py"
WORK_ROOT="$FIX_DIR/.work"
RESULTS_DIR="$ROOT/compat/results"

REGEN=0
RECORD_PENDING=0
CASE_FILTER=""
# Regeneration only: same guard as the golden tier. An under-recorded --fix is
# worse than an under-recorded golden — it pins *fewer edits than upstream
# makes*, and every run afterwards reads as guff over-fixing.
REGEN_CONFIRMATIONS="${FIX_REGEN_CONFIRMATIONS:-2}"
REGEN_ATTEMPTS="${FIX_REGEN_ATTEMPTS:-8}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --regen) REGEN=1; shift ;;
    --record-pending) RECORD_PENDING=1; shift ;;
    --case) CASE_FILTER="$2"; shift 2 ;;
    --case=*) CASE_FILTER="${1#*=}"; shift ;;
    -h|--help) sed -n '2,19p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

die() { echo "error: $*" >&2; exit 1; }

resolve_guff() {
  if [[ -n "${GUFF_BIN:-}" ]]; then echo "$GUFF_BIN"
  elif [[ -x "$ROOT/target/release/guff" ]]; then echo "$ROOT/target/release/guff"
  elif command -v guff >/dev/null 2>&1; then command -v guff
  else die "guff not found; build with: cargo build --release -p guff-lint"
  fi
}

GUFF="$(resolve_guff)"
command -v go >/dev/null 2>&1 || die "go not found"
command -v python3 >/dev/null 2>&1 || die "python3 not found"
[[ -f "$FIXDIFF" ]] || die "missing $FIXDIFF"
[[ -f "$HEALTH" ]] || die "missing $HEALTH"

GOLANGCI="${GOLANGCI_LINT_BIN:-$(command -v golangci-lint 2>/dev/null || true)}"
if [[ "$REGEN" -eq 1 && -z "$GOLANGCI" ]]; then
  die "--regen needs golangci-lint on PATH (set GOLANGCI_LINT_BIN)"
fi

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$RESULTS_DIR/fix-$STAMP"
mkdir -p "$RUN_DIR" "$WORK_ROOT" "$EXPECTED_DIR" "$PENDING_DIR"

GCL_VER="unknown"
if [[ -n "$GOLANGCI" ]]; then
  GCL_VER="$("$GOLANGCI" version --short 2>/dev/null || echo unknown)"
fi

echo "guff --fix gate (COMPAT-HARDENING Phase 3, fix tier)"
echo "  guff:     $("$GUFF" version --short 2>/dev/null || echo unknown) ($GUFF)"
echo "  golangci: $GCL_VER"
if [[ "$REGEN" -eq 1 ]]; then MODE=regenerate
elif [[ "$RECORD_PENDING" -eq 1 ]]; then MODE="record pending"
else MODE=check
fi
echo "  mode:     $MODE"
echo "  results:  $RUN_DIR"
echo

# shellcheck source=compat/golden/materialize.sh
source "$GOLDEN_DIR/materialize.sh"

FAILED=0
SELECTED=0
CHANGED=0
BROKEN=0

# Does the tree still build? A --fix that rewrites a call and leaves out the
# import it now needs writes code that does not compile, which no finding-set
# comparison can express and the byte diff only shows to a reader who notices
# the missing line. Asked of the pristine tree first: several fixtures are
# deliberately un-buildable (an unused variable *is* the finding), and there is
# nothing to break in a tree that was already broken.
tree_compiles() {
  (cd "$1" && env ${case_env[@]+"${case_env[@]}"} go build ./... >/dev/null 2>&1)
}

for case_dir in "$CASES_DIR"/*/; do
  name="$(basename "$case_dir")"
  [[ -n "$CASE_FILTER" && "$name" != "$CASE_FILTER" ]] && continue
  [[ -f "$case_dir/config.yml" ]] || die "$name: missing config.yml"
  [[ -f "$case_dir/sources.txt" ]] || die "$name: missing sources.txt"
  [[ -f "$case_dir/go.mod" ]] || die "$name: missing go.mod"
  SELECTED=$((SELECTED + 1))

  # The golden tier already refuses a case whose fixtures compile differently
  # per platform (platforms.py), on this same corpus. Repeating that check here
  # would only make the fix tier fail for a reason the other one already owns.
  work="$WORK_ROOT/$name"
  pristine="$work/pristine"
  materialize_case "$name" "$case_dir" "$pristine" "$ROOT"
  read_case_env "$name" "$case_dir"
  expected="$EXPECTED_DIR/$name.diff"
  pending="$PENDING_DIR/$name.diff"
  divergent="$DIVERGENT_DIR/$name.diff"

  if [[ "$REGEN" -eq 1 ]]; then
    gcl_args=()
    attempt=0
    regen_ok=0
    while (( attempt < REGEN_ATTEMPTS )); do
      attempt=$((attempt + 1))
      run_dir_case="$work/golangci.$attempt"
      rm -rf "$run_dir_case"
      cp -R "$pristine" "$run_dir_case"
      # A cache per run, not per tool. golangci-lint keyed on file *content*
      # will answer from a previous run's entry while printing paths from a
      # directory that no longer exists — which reads as "upstream fixes
      # nothing" and would be recorded as exactly that.
      gcl_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-fix-gcl.XXXXXX")"
      (
        cd "$run_dir_case"
        env "GOLANGCI_LINT_CACHE=$gcl_cache" \
          ${case_env[@]+"${case_env[@]}"} \
          "$GOLANGCI" run \
          -c "$case_dir/config.yml" \
          --fix \
          --issues-exit-code 0 \
          --allow-parallel-runners \
          ./...
      ) >"$RUN_DIR/fix-$name.golangci.$attempt.out" \
        2>"$RUN_DIR/fix-$name.golangci.$attempt.stderr" || {
        echo "golangci-lint failed for $name; see $RUN_DIR/fix-$name.golangci.$attempt.stderr" >&2
        cat "$RUN_DIR/fix-$name.golangci.$attempt.stderr" >&2 || true
        rm -rf "$gcl_cache"
        break
      }
      rm -rf "$gcl_cache"
      gcl_diff="$RUN_DIR/fix-$name.golangci.$attempt.diff"
      python3 "$FIXDIFF" capture --before "$pristine" --after "$run_dir_case" -o "$gcl_diff"
      gcl_args+=(--run "$gcl_diff")
      (( attempt < REGEN_CONFIRMATIONS )) && continue
      if python3 "$FIXDIFF" write \
        --case "$name" \
        "${gcl_args[@]}" \
        --confirmations "$REGEN_CONFIRMATIONS" \
        --tool-version "$GCL_VER" \
        -o "$expected"; then
        regen_ok=1
        break
      fi
    done
    if [[ "$regen_ok" -eq 0 ]]; then
      echo "  $name: NOT recorded — golangci-lint --fix never agreed with itself in $attempt run(s)" >&2
      FAILED=$((FAILED + 1))
    fi
    continue
  fi

  guff_work="$work/guff"
  rm -rf "$guff_work"
  cp -R "$pristine" "$guff_work"
  guff_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-fix-guff.XXXXXX")"
  (
    cd "$guff_work"
    env "GUFF_CACHE=$guff_cache" \
      "GUFF_DEBUG_ILL_TYPED=1" \
      ${case_env[@]+"${case_env[@]}"} \
      "$GUFF" run \
      -c "$case_dir/config.yml" \
      --fix \
      --issues-exit-code 0 \
      --no-cache \
      ./...
  ) >"$RUN_DIR/fix-$name.guff.out" 2>"$RUN_DIR/fix-$name.guff.stderr" || {
    echo "guff failed for $name; see $RUN_DIR/fix-$name.guff.stderr" >&2
    cat "$RUN_DIR/fix-$name.guff.stderr" >&2 || true
    rm -rf "$guff_cache"
    FAILED=$((FAILED + 1))
    continue
  }
  rm -rf "$guff_cache"

  # A worker panic or a skipped ill-typed package produces *fewer edits*, and
  # fewer edits against an absent expectation is a pass. Same gate as golden,
  # same reason (COMPAT-HARDENING Phase 1), no baseline: these cases are clean.
  if ! python3 "$HEALTH" check \
    --target "fix-$name" \
    --stderr "$RUN_DIR/fix-$name.guff.stderr"; then
    FAILED=$((FAILED + 1))
  fi

  guff_diff="$RUN_DIR/fix-$name.guff.diff"
  python3 "$FIXDIFF" capture --before "$pristine" --after "$guff_work" -o "$guff_diff"
  if [[ -s "$guff_diff" ]]; then
    CHANGED=$((CHANGED + 1))
    if tree_compiles "$pristine" && ! tree_compiles "$guff_work"; then
      BROKEN=$((BROKEN + 1))
      echo "  $name: guff's --fix leaves a tree that does NOT COMPILE" \
           "(the same tree builds before the fix)" >&2
      (cd "$guff_work" && env ${case_env[@]+"${case_env[@]}"} go build ./... 2>&1 | head -5) >&2 || true
    fi
  fi

  if [[ "$RECORD_PENDING" -eq 1 ]]; then
    # A recorded divergence is a decision a person wrote down; the recorder has
    # no way to re-derive it and its refusal would fail the whole sweep. Skip
    # the case and say so, rather than making --record-pending unusable for the
    # other 192 the moment somebody files a divergence.
    if [[ -f "$divergent" ]]; then
      echo "  $name: skipped — deliberate divergence, see $divergent"
      continue
    fi
    # Recording is driven by the *check*, not by a separate opinion about which
    # cases are broken: a case that now matches upstream has its pending file
    # deleted here rather than left to rot into a stale expectation.
    if python3 "$FIXDIFF" check \
      --case "$name" --actual "$guff_diff" --expected "$expected" >/dev/null 2>&1; then
      if [[ -f "$pending" ]]; then
        rm -f "$pending"
        echo "  $name: matches upstream — removed $pending"
      fi
    else
      python3 "$FIXDIFF" pending --case "$name" --actual "$guff_diff" \
        --expected "$expected" -o "$pending" || FAILED=$((FAILED + 1))
    fi
    continue
  fi

  # A divergence that claims "upstream's output does not compile" is the one
  # kind allowed to write *less* than upstream, so the claim has to be checked
  # rather than believed: apply upstream's own recorded diff and see. The day
  # upstream fixes its fixer, this fails and the entry gets re-decided instead
  # of quietly licensing an under-fix forever.
  if [[ -f "$divergent" ]] && grep -q '^# upstream-breaks-build:' "$divergent"; then
    if ! tree_compiles "$pristine"; then
      echo "  $name: cannot verify the upstream-breaks-build claim — the" \
           "pristine tree already does not compile" >&2
      FAILED=$((FAILED + 1))
    else
      up_work="$work/upstream"
      rm -rf "$up_work"
      cp -R "$pristine" "$up_work"
      if ! (cd "$up_work" && git apply -p1 --unsafe-paths --directory=. "$expected" 2>/dev/null); then
        echo "  $name: could not apply $expected to check the" \
             "upstream-breaks-build claim" >&2
        FAILED=$((FAILED + 1))
      elif tree_compiles "$up_work"; then
        echo "  $name: $divergent says golangci-lint --fix breaks the build," \
             "but its recorded output compiles. The reason for writing less" \
             "than upstream no longer holds — re-read the \`# why:\`." >&2
        FAILED=$((FAILED + 1))
      fi
    fi
  fi

  python3 "$FIXDIFF" check \
    --case "$name" \
    --actual "$guff_diff" \
    --expected "$expected" \
    --pending "$pending" \
    --divergent "$divergent" || FAILED=$((FAILED + 1))
done

if [[ -n "$CASE_FILTER" && "$SELECTED" -eq 0 ]]; then
  die "case '$CASE_FILTER' not found under $CASES_DIR"
fi
if [[ "$SELECTED" -eq 0 ]]; then
  die "no cases found under $CASES_DIR"
fi

echo
if [[ "$RECORD_PENDING" -eq 1 ]]; then
  echo "Recorded the pending gaps in $PENDING_DIR. Review the diff before committing:"
  echo "every file there is a linter whose findings a user's --fix does not act on."
  exit 0
fi

if [[ "$REGEN" -eq 1 ]]; then
  echo "Re-recorded $SELECTED case(s) into $EXPECTED_DIR. Review the diff before committing."
  [[ "$FAILED" -gt 0 ]] && { echo "FAIL: $FAILED case(s) failed to run" >&2; exit 1; }
  exit 0
fi

if [[ "$FAILED" -gt 0 ]]; then
  echo "FAIL: $FAILED/$SELECTED case(s) write something other than what golangci-lint --fix writes" >&2
  [[ "$BROKEN" -gt 0 ]] && echo "      $BROKEN rewritten tree(s) no longer build" >&2
  echo "Fix guff, or re-record with ./compat/fix/regen.sh after review." >&2
  exit 1
fi
PENDING_N="$(find "$PENDING_DIR" -name '*.diff' 2>/dev/null | wc -l | tr -d ' ')"
DIVERGENT_N="$(find "$DIVERGENT_DIR" -name '*.diff' 2>/dev/null | wc -l | tr -d ' ')"
# $BROKEN counts rewritten trees, not pending cases: all of them are
# byte-identical to golangci-lint's own output (compat/fix/README.md).
echo "OK: $SELECTED case(s) checked, $CHANGED rewrote a file" \
     "($BROKEN of those trees no longer build)," \
     "$PENDING_N held at a pending baseline," \
     "$DIVERGENT_N deliberately divergent"
