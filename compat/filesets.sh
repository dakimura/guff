#!/usr/bin/env bash
# compat/filesets.sh — do both tools analyze the same .go files? (Phase 1c)
#
# Usage:
#   ./compat/filesets.sh                 # OSS pr tier
#   ./compat/filesets.sh --tier nightly
#   ./compat/filesets.sh --isolate       # every isolate fixture
#
# Runs guff and golangci-lint with a `goheader` template that cannot match, so
# each tool reports exactly once per file it analyzed, then diffs the two file
# sets. See compat/filesets.py for why this probe rather than a debug flag.
#
# Exit 0 when every target's file sets are identical.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPAT_DIR="$ROOT/compat"
FILESETS="$COMPAT_DIR/filesets.py"
HEALTH="$COMPAT_DIR/health.py"
PREPARE="$ROOT/corpus/prepare.sh"
RESULTS_DIR="$COMPAT_DIR/results"
ISOLATE_FIXTURES="$COMPAT_DIR/isolate/fixtures"

TIER="pr"
ISOLATE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tier) TIER="$2"; shift 2 ;;
    --tier=*) TIER="${1#*=}"; shift ;;
    --isolate) ISOLATE=1; shift ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

die() { echo "error: $*" >&2; exit 1; }

GUFF="${GUFF_BIN:-$ROOT/target/release/guff}"
[[ -x "$GUFF" ]] || GUFF="$(command -v guff || true)"
[[ -n "$GUFF" && -x "$GUFF" ]] || die "guff not found; cargo build --release -p guff-lint"
GOLANGCI="${GOLANGCI_LINT_BIN:-$(command -v golangci-lint || true)}"
[[ -n "$GOLANGCI" ]] || die "golangci-lint not on PATH"
[[ -f "$FILESETS" ]] || die "missing $FILESETS"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$RESULTS_DIR/filesets-$STAMP"
mkdir -p "$RUN_DIR"

echo "guff file-set gate (COMPAT-HARDENING Phase 1c)"
echo "  guff:     $("$GUFF" version --short 2>/dev/null || echo unknown)"
echo "  golangci: $("$GOLANGCI" version --short 2>/dev/null || echo unknown)"
echo "  mode:     $([[ "$ISOLATE" -eq 1 ]] && echo isolate || echo "oss tier=$TIER")"
echo "  results:  $RUN_DIR"
echo

FAILED=0
COUNT=0

probe_target() {
  local name="$1" dir="$2" source_config="$3" packages="$4" timeout="$5"
  COUNT=$((COUNT + 1))
  local cfg guff_json gcl_json cache
  cfg="$RUN_DIR/${name}.probe.yml"
  guff_json="$RUN_DIR/${name}.guff.json"
  gcl_json="$RUN_DIR/${name}.golangci.json"

  python3 "$FILESETS" config --source "$source_config" -o "$cfg"

  cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-fileset.XXXXXX")"
  # shellcheck disable=SC2086
  (
    cd "$dir"
    env "GUFF_CACHE=$cache" "GUFF_DEBUG_ILL_TYPED=1" \
      "$GUFF" run -c "$cfg" --out-format json --issues-exit-code 0 \
      --timeout "$timeout" --no-cache $packages
  ) >"$guff_json" 2>"$RUN_DIR/${name}.guff.stderr" || {
    echo "  $name: guff failed; see $RUN_DIR/${name}.guff.stderr" >&2
    rm -rf "$cache"; FAILED=$((FAILED + 1)); return
  }
  rm -rf "$cache"

  cache="$(mktemp -d "${TMPDIR:-/tmp}/gcl-fileset.XXXXXX")"
  # shellcheck disable=SC2086
  (
    cd "$dir"
    env "GOLANGCI_LINT_CACHE=$cache" \
      "$GOLANGCI" run -c "$cfg" --output.json.path=stdout --path-mode abs \
      --issues-exit-code 0 --timeout="$timeout" \
      --max-issues-per-linter=0 --max-same-issues=0 --allow-parallel-runners $packages
  ) >"$gcl_json" 2>"$RUN_DIR/${name}.golangci.stderr" || {
    echo "  $name: golangci-lint failed; see $RUN_DIR/${name}.golangci.stderr" >&2
    rm -rf "$cache"; FAILED=$((FAILED + 1)); return
  }
  rm -rf "$cache"

  # An ill-typed package is skipped by goheader too, so it shrinks guff's file
  # set and would read as a loader difference. Surface the real cause.
  python3 "$HEALTH" check --target "$name" --stderr "$RUN_DIR/${name}.guff.stderr" || true

  python3 "$FILESETS" diff \
    --target "$name" --root "$dir" \
    --guff "$guff_json" --golangci "$gcl_json" || FAILED=$((FAILED + 1))
}

if [[ "$ISOLATE" -eq 1 ]]; then
  for d in "$ISOLATE_FIXTURES"/*/; do
    [[ -f "$d/go.mod" ]] || continue
    probe_target "isolate-$(basename "$d")" "$d" "" "./..." "5m"
  done
else
  [[ -x "$PREPARE" ]] || die "missing $PREPARE"
  echo "Preparing OSS corpus (tier=$TIER)..."
  prep="$(mktemp "${TMPDIR:-/tmp}/guff-filesets.XXXXXX")"
  "$PREPARE" --tier "$TIER" >"$prep"
  while IFS=$'\t' read -r name dir config packages timeout tier; do
    [[ -z "${name:-}" ]] && continue
    probe_target "$name" "$dir" "$config" "$packages" "$timeout"
  done <"$prep"
  rm -f "$prep"
fi

echo
if [[ "$COUNT" -eq 0 ]]; then
  die "no targets probed"
fi
if [[ "$FAILED" -gt 0 ]]; then
  echo "FAIL: $FAILED/$COUNT target(s) analyzed different file sets" >&2
  exit 1
fi
echo "OK: $COUNT target(s) analyzed identical file sets"
