#!/usr/bin/env bash
# compat/reject/run.sh — the "upstream refuses to start" tier (Phase 4).
#
# Usage:
#   ./compat/reject/run.sh                    # every case
#   ./compat/reject/run.sh --case output-path-mode-rel
#   ./compat/reject/run.sh --regen            # re-record golangci-lint's reasons
#
# Each case is a config golangci-lint validates and rejects before it lints
# anything. The gate asserts three things per case: golangci-lint still refuses
# it, guff refuses it too, and both give the *same reason*. `cases/_control/`
# is the negative: a config both tools must run, so a tier that started failing
# everything could not pass.
#
# Expected reasons are generated (`--regen`), never hand-written — same rule as
# compat/golden.
#
# Env: GUFF_BIN / GOLANGCI_LINT_BIN
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REJECT_DIR="$ROOT/compat/reject"
CASES_DIR="$REJECT_DIR/cases"
REJECT_PY="$REJECT_DIR/reject.py"
RESULTS_DIR="$ROOT/compat/results"

REGEN=0
CASE_FILTER=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --regen) REGEN=1; shift ;;
    --case) CASE_FILTER="$2"; shift 2 ;;
    --case=*) CASE_FILTER="${1#*=}"; shift ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

die() { echo "error: $*" >&2; exit 1; }

if [[ -n "${GUFF_BIN:-}" ]]; then GUFF="$GUFF_BIN"
elif [[ -x "$ROOT/target/release/guff" ]]; then GUFF="$ROOT/target/release/guff"
elif command -v guff >/dev/null 2>&1; then GUFF="$(command -v guff)"
else die "guff not found; build with: cargo build --release -p guff-lint"
fi

GOLANGCI="${GOLANGCI_LINT_BIN:-$(command -v golangci-lint 2>/dev/null || true)}"
[[ -n "$GOLANGCI" ]] || die "golangci-lint not on PATH (set GOLANGCI_LINT_BIN)"
command -v python3 >/dev/null 2>&1 || die "python3 not found"
[[ -f "$REJECT_PY" ]] || die "missing $REJECT_PY"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$RESULTS_DIR/reject-$STAMP"
mkdir -p "$RUN_DIR"

echo "guff reject gate (configs golangci-lint refuses to start on)"
echo "  guff:     $("$GUFF" version --short 2>/dev/null || echo unknown) ($GUFF)"
echo "  golangci: $("$GOLANGCI" version --short 2>/dev/null || echo unknown)"
echo "  mode:     $([[ "$REGEN" -eq 1 ]] && echo regenerate || echo check)"
echo

FAILED=0
SELECTED=0

for case_dir in "$CASES_DIR"/*/; do
  name="$(basename "$case_dir")"
  [[ -n "$CASE_FILTER" && "$name" != "$CASE_FILTER" ]] && continue
  [[ -f "$case_dir/config.yml" ]] || die "$name: missing config.yml"
  SELECTED=$((SELECTED + 1))

  gcl_out="$RUN_DIR/$name.golangci.txt"
  guff_out="$RUN_DIR/$name.guff.txt"

  # Both tools are pointed at the same one-package module. Neither should get
  # as far as reading it, which is the point; a case that starts passing
  # because the *code* changed would be a case that stopped testing the config.
  gcl_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-reject-gcl.XXXXXX")"
  (
    cd "$REJECT_DIR"
    env "GOLANGCI_LINT_CACHE=$gcl_cache" \
      "$GOLANGCI" run -c "$case_dir/config.yml" --issues-exit-code 0 ./...
  ) >"$gcl_out" 2>&1
  gcl_rc=$?
  rm -rf "$gcl_cache"

  guff_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-reject-guff.XXXXXX")"
  (
    cd "$REJECT_DIR"
    env "GUFF_CACHE=$guff_cache" \
      "$GUFF" run -c "$case_dir/config.yml" --issues-exit-code 0 --no-cache ./...
  ) >"$guff_out" 2>&1
  guff_rc=$?
  rm -rf "$guff_cache"

  if [[ -f "$case_dir/accepts" ]]; then
    python3 "$REJECT_PY" accept \
      --case "$name" --guff-rc "$guff_rc" --golangci-rc "$gcl_rc" \
      || FAILED=$((FAILED + 1))
    continue
  fi

  if [[ "$REGEN" -eq 1 ]]; then
    python3 "$REJECT_PY" write \
      --case "$name" \
      --golangci-output "$gcl_out" \
      --golangci-rc "$gcl_rc" \
      -o "$case_dir/expected.txt" \
      || FAILED=$((FAILED + 1))
    continue
  fi

  [[ -f "$case_dir/expected.txt" ]] || die "$name: missing expected.txt (run with --regen)"
  python3 "$REJECT_PY" check \
    --case "$name" \
    --expected "$case_dir/expected.txt" \
    --guff-output "$guff_out" --guff-rc "$guff_rc" \
    --golangci-output "$gcl_out" --golangci-rc "$gcl_rc" \
    || FAILED=$((FAILED + 1))
done

[[ -n "$CASE_FILTER" && "$SELECTED" -eq 0 ]] && die "case '$CASE_FILTER' not found under $CASES_DIR"
[[ "$SELECTED" -eq 0 ]] && die "no cases found under $CASES_DIR"

echo
if [[ "$REGEN" -eq 1 ]]; then
  echo "Recorded $SELECTED case(s). Review the diff before committing."
  [[ "$FAILED" -gt 0 ]] && { echo "FAIL: $FAILED case(s) produced nothing to record" >&2; exit 1; }
  exit 0
fi
if [[ "$FAILED" -gt 0 ]]; then
  echo "FAIL: $FAILED/$SELECTED case(s) — see $RUN_DIR" >&2
  exit 1
fi
echo "OK: $SELECTED case(s) refused for the same reason as golangci-lint"
