#!/usr/bin/env bash
# compat/fmt/run.sh — `fmt` gate: compare what the two tools' *formatters* write.
#
# Usage:
#   ./compat/fmt/run.sh              # check every case against its expectation
#   ./compat/fmt/run.sh --case gofmt-default
#   ./compat/fmt/run.sh --regen      # re-record from `golangci-lint fmt`
#   ./compat/fmt/run.sh --record-pending   # re-record the known-missing gaps
#
# The other tiers all go through `run`: golden keys on the finding text, fix
# diffs the bytes `run --fix` writes, reject checks the configs upstream
# refuses to start on. None of them ever invokes `golangci-lint fmt`, so
# nothing in this repo compared the two tools' *formatter* surface —
# `formatters.enable`, `formatters.settings.*`, `formatters.exclusions`. That
# is not a narrow gap: `formatters.settings.gofmt.simplify` defaults to **true**
# upstream, so it covers what happens to every user who merely enables gofmt.
#
# Both tools are driven through `fmt --stdin`, which each of them supports and
# which makes the comparison exactly "same config + same bytes in, same bytes
# out" with no directory walk, no cache and no module in the way.
#
# An expectation is upstream's answer, recorded by --regen; a run needs only
# guff, like the golden and fix tiers.
#
# Env: GUFF_BIN / GOLANGCI_LINT_BIN
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FMT_DIR="$ROOT/compat/fmt"
CASES_DIR="$FMT_DIR/cases"
EXPECTED_DIR="$FMT_DIR/expected"
PENDING_DIR="$FMT_DIR/pending"
RESULTS_DIR="$ROOT/compat/results"

REGEN=0
RECORD_PENDING=0
CASE_FILTER=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --regen) REGEN=1; shift ;;
    --record-pending) RECORD_PENDING=1; shift ;;
    --case) CASE_FILTER="$2"; shift 2 ;;
    --case=*) CASE_FILTER="${1#*=}"; shift ;;
    -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
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
GOLANGCI="${GOLANGCI_LINT_BIN:-$(command -v golangci-lint 2>/dev/null || true)}"
if [[ "$REGEN" -eq 1 && -z "$GOLANGCI" ]]; then
  die "--regen needs golangci-lint on PATH (set GOLANGCI_LINT_BIN)"
fi

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$RESULTS_DIR/fmt-$STAMP"
mkdir -p "$RUN_DIR" "$EXPECTED_DIR" "$PENDING_DIR"

GCL_VER="unknown"
if [[ -n "$GOLANGCI" ]]; then
  GCL_VER="$("$GOLANGCI" version --short 2>/dev/null || echo unknown)"
fi

echo "guff fmt gate (COMPAT-HARDENING Phase 3, fmt tier)"
echo "  guff:     $("$GUFF" version --short 2>/dev/null || echo unknown) ($GUFF)"
echo "  golangci: $GCL_VER"
if [[ "$REGEN" -eq 1 ]]; then MODE=regenerate
elif [[ "$RECORD_PENDING" -eq 1 ]]; then MODE="record pending"
else MODE=check
fi
echo "  mode:     $MODE"
echo "  results:  $RUN_DIR"
echo

FAILED=0
SELECTED=0
UNCHANGED=0
PENDING=0

for case_dir in "$CASES_DIR"/*/; do
  name="$(basename "$case_dir")"
  [[ -n "$CASE_FILTER" && "$name" != "$CASE_FILTER" ]] && continue
  [[ -f "$case_dir/config.yml" ]] || die "$name: missing config.yml"
  [[ -f "$case_dir/input.go" ]] || die "$name: missing input.go"
  SELECTED=$((SELECTED + 1))

  expected="$EXPECTED_DIR/$name.go"

  if [[ "$REGEN" -eq 1 ]]; then
    if ! "$GOLANGCI" fmt -c "$case_dir/config.yml" --stdin \
      <"$case_dir/input.go" >"$RUN_DIR/$name.golangci.out" 2>"$RUN_DIR/$name.golangci.err"; then
      echo "  $name: golangci-lint fmt failed" >&2
      cat "$RUN_DIR/$name.golangci.err" >&2 || true
      FAILED=$((FAILED + 1))
      continue
    fi
    # An expectation identical to the input is a real answer here — it is what
    # a skipped generated file looks like — but it is also what a formatter
    # that silently did nothing looks like. Record it either way and let the
    # case's own config.yml say which one it means.
    cp "$RUN_DIR/$name.golangci.out" "$expected"
    if cmp -s "$expected" "$case_dir/input.go"; then
      echo "  $name: recorded (unchanged from input — the case must explain why)"
    else
      echo "  $name: recorded ($(diff "$case_dir/input.go" "$expected" | grep -c '^[<>]' || true) differing line(s))"
    fi
    continue
  fi

  [[ -f "$expected" ]] || die "$name: no expectation; run ./compat/fmt/run.sh --regen"
  pending="$PENDING_DIR/$name.go"

  if ! "$GUFF" fmt -c "$case_dir/config.yml" --stdin \
    <"$case_dir/input.go" >"$RUN_DIR/$name.guff.out" 2>"$RUN_DIR/$name.guff.err"; then
    echo "  $name: FAIL — guff fmt exited non-zero" >&2
    cat "$RUN_DIR/$name.guff.err" >&2 || true
    FAILED=$((FAILED + 1))
    continue
  fi

  if [[ "$RECORD_PENDING" -eq 1 ]]; then
    if cmp -s "$RUN_DIR/$name.guff.out" "$expected"; then
      echo "  $name: matches upstream — no pending baseline needed"
    else
      cp "$RUN_DIR/$name.guff.out" "$pending"
      if [[ -f "$PENDING_DIR/$name.why" ]]; then
        echo "  $name: pending baseline recorded"
      else
        echo "  $name: pending baseline recorded — now write $PENDING_DIR/$name.why"
      fi
    fi
    continue
  fi

  # A pending baseline is what guff writes *today* for a case whose parity is
  # missing. It is not an allowlist: the case's real expectation is still
  # expected/<case>.go and the diff against it is printed in full on every run.
  # The baseline only keeps CI green while the gap is worked down, and it fails
  # in *either* direction — including the day guff gets it right — so it cannot
  # quietly outlive the defect it records.
  if [[ -f "$pending" ]]; then
    # The baseline is bytes; the reason is prose, and it is not optional. A
    # pending case with no `.why` is a gap nobody has to justify, which is how
    # a baseline turns into an allowlist.
    [[ -f "$PENDING_DIR/$name.why" ]] || die "$name: pending baseline with no $PENDING_DIR/$name.why"
    if cmp -s "$RUN_DIR/$name.guff.out" "$expected"; then
      echo "  $name: guff now matches upstream — delete $pending" >&2
      FAILED=$((FAILED + 1))
    elif cmp -s "$RUN_DIR/$name.guff.out" "$pending"; then
      PENDING=$((PENDING + 1))
      echo "  $name: PENDING — still differs from upstream, at the recorded baseline"
      sed 's/^/      /' "$PENDING_DIR/$name.why"
      diff -u "$expected" "$pending" \
        --label "golangci-lint fmt" --label "guff fmt (pending)" || true
    else
      echo "  $name: FAIL — moved off its pending baseline" >&2
      diff -u "$pending" "$RUN_DIR/$name.guff.out" \
        --label "pending baseline" --label "guff fmt" >&2 || true
      FAILED=$((FAILED + 1))
    fi
    continue
  fi

  if cmp -s "$RUN_DIR/$name.guff.out" "$expected"; then
    if cmp -s "$expected" "$case_dir/input.go"; then
      UNCHANGED=$((UNCHANGED + 1))
      echo "  $name: matches (upstream writes the input back unchanged)"
    else
      echo "  $name: matches ($(diff "$case_dir/input.go" "$expected" | grep -c '^[<>]' || true) differing line(s))"
    fi
  else
    echo "  $name: FAIL — guff writes different bytes than golangci-lint fmt" >&2
    diff -u "$expected" "$RUN_DIR/$name.guff.out" \
      --label "golangci-lint fmt" --label "guff fmt" >&2 || true
    FAILED=$((FAILED + 1))
  fi
done

echo
if [[ "$SELECTED" -eq 0 ]]; then
  die "no cases selected"
fi

if [[ "$REGEN" -eq 1 ]]; then
  echo "recorded $SELECTED case(s) from golangci-lint $GCL_VER"
  exit 0
fi

if [[ "$RECORD_PENDING" -eq 1 ]]; then
  echo "pending baselines refreshed"
  exit 0
fi

if [[ "$FAILED" -gt 0 ]]; then
  echo "FAIL: $FAILED/$SELECTED case(s) format differently than golangci-lint fmt" >&2
  echo "Fix guff, or re-record with ./compat/fmt/run.sh --regen after review." >&2
  exit 1
fi

echo "OK: $SELECTED case(s) checked — $((SELECTED - PENDING)) match golangci-lint fmt exactly" \
  "($UNCHANGED of them by writing the input back), $PENDING held at a pending baseline"
