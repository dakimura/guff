#!/usr/bin/env bash
# compat/golden/run.sh — check-level golden gate (COMPAT-HARDENING Phase 3).
#
# Usage:
#   ./compat/golden/run.sh                 # check every case against its golden
#   ./compat/golden/run.sh --case gocritic # one case
#   ./compat/golden/run.sh --regen         # regenerate goldens from golangci-lint
#
# The gate compares guff's findings to `cases/<name>/expected.golden` with
# **no normalization**, on `path:line:col:linter:severity:text`. There is no
# allowlist: a diff is either a guff bug to fix or a reviewed regeneration.
#
# Env: GUFF_BIN / GOLANGCI_LINT_BIN
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GOLDEN_DIR="$ROOT/compat/golden"
CASES_DIR="$GOLDEN_DIR/cases"
GOLDEN_PY="$GOLDEN_DIR/golden.py"
HEALTH="$ROOT/compat/health.py"
WORK_ROOT="$GOLDEN_DIR/.work"
RESULTS_DIR="$ROOT/compat/results"

REGEN=0
CASE_FILTER=""
# Regeneration only: how many identical golangci-lint runs a golden needs before
# it is written, and how many runs we are willing to spend looking for them.
REGEN_CONFIRMATIONS="${GOLDEN_REGEN_CONFIRMATIONS:-2}"
REGEN_ATTEMPTS="${GOLDEN_REGEN_ATTEMPTS:-8}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --regen) REGEN=1; shift ;;
    --case) CASE_FILTER="$2"; shift 2 ;;
    --case=*) CASE_FILTER="${1#*=}"; shift ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
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
[[ -f "$GOLDEN_PY" ]] || die "missing $GOLDEN_PY"
[[ -f "$HEALTH" ]] || die "missing $HEALTH"

GOLANGCI="${GOLANGCI_LINT_BIN:-$(command -v golangci-lint 2>/dev/null || true)}"
if [[ "$REGEN" -eq 1 && -z "$GOLANGCI" ]]; then
  die "--regen needs golangci-lint on PATH (set GOLANGCI_LINT_BIN)"
fi

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$RESULTS_DIR/golden-$STAMP"
mkdir -p "$RUN_DIR" "$WORK_ROOT"

GCL_VER="unknown"
if [[ -n "$GOLANGCI" ]]; then
  GCL_VER="$("$GOLANGCI" version --short 2>/dev/null || echo unknown)"
fi

echo "guff golden gate (COMPAT-HARDENING Phase 3)"
echo "  guff:     $("$GUFF" version --short 2>/dev/null || echo unknown) ($GUFF)"
echo "  golangci: $GCL_VER"
echo "  mode:     $([[ "$REGEN" -eq 1 ]] && echo regenerate || echo check)"
echo "  results:  $RUN_DIR"
echo

# Materialize a case into $WORK_ROOT/<name>: go.mod + the sources listed in
# sources.txt, copied from their canonical location in the repo.
materialize() {
  local name="$1" case_dir="$2" work="$3"
  rm -rf "$work"
  mkdir -p "$work"
  cp "$case_dir/go.mod" "$work/go.mod"
  while IFS= read -r raw || [[ -n "$raw" ]]; do
    local line dest src
    line="${raw%%#*}"
    line="$(echo "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
    [[ -z "$line" ]] && continue
    # The two columns are separated by a run of two or more spaces, not by a
    # single one: either path may itself contain a space (revive's
    # filename-format fixture is literally named "bad file.go").
    if [[ "$line" =~ ^(.*[^[:space:]])[[:space:]][[:space:]]+(.+)$ ]]; then
      dest="${BASH_REMATCH[1]}"; src="${BASH_REMATCH[2]}"
    else
      die "$name: sources.txt needs two or more spaces between the columns: $raw"
    fi
    [[ -f "$ROOT/$src" ]] || die "$name: missing source $src"
    mkdir -p "$(dirname "$work/$dest")"
    cp "$ROOT/$src" "$work/$dest"
  done <"$case_dir/sources.txt"
}

FAILED=0
SELECTED=0

for case_dir in "$CASES_DIR"/*/; do
  name="$(basename "$case_dir")"
  [[ -n "$CASE_FILTER" && "$name" != "$CASE_FILTER" ]] && continue
  [[ -f "$case_dir/config.yml" ]] || die "$name: missing config.yml"
  [[ -f "$case_dir/sources.txt" ]] || die "$name: missing sources.txt"
  [[ -f "$case_dir/go.mod" ]] || die "$name: missing go.mod"
  # Both tools default these to 50 / 3, and a golden truncated by a default is
  # a golden that silently stops comparing. The gate used to pass
  # --max-issues-per-linter=0 --max-same-issues=0 to golangci-lint, which also
  # made the two keys untestable (a CLI flag beats the case's config). Requiring
  # the case to state them instead keeps config.yml the whole truth: a case that
  # is not about the limits writes 0, and cases/issues-max-* write the value
  # they are measuring.
  for key in max-issues-per-linter max-same-issues; do
    grep -q "^[[:space:]]*$key:" "$case_dir/config.yml" \
      || die "$name: config.yml must set issues.$key (0 unless the case is about it)"
  done
  SELECTED=$((SELECTED + 1))

  work="$WORK_ROOT/$name"
  materialize "$name" "$case_dir" "$work"
  golden="$case_dir/expected.golden"

  # Optional `cases/<name>/env`: KEY=VALUE lines applied to *both* tools. A
  # check whose behaviour depends on the target platform (SA1027 returns early
  # unless the word size is 4) is unreachable on the host arch and needs
  # GOOS/GOARCH to be compared at all.
  case_env=()
  if [[ -f "$case_dir/env" ]]; then
    while IFS= read -r raw || [[ -n "$raw" ]]; do
      line="${raw%%#*}"
      line="$(echo "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
      [[ -z "$line" ]] && continue
      [[ "$line" == *=* ]] || die "$name: env line is not KEY=VALUE: $raw"
      case_env+=("$line")
    done <"$case_dir/env"
  fi

  if [[ "$REGEN" -eq 1 ]]; then
    # golangci-lint is not a deterministic function of its input: on cases/revive
    # roughly one cold-cache run in four silently drops whole packages' findings
    # (root cause in README.md, "Upstream is not a function"). A golden written
    # from such a run compares less than it claims to, and nothing downstream
    # would ever notice. So: run it repeatedly and refuse to write until two
    # runs have produced identical keys.
    gcl_args=()
    attempt=0
    regen_ok=0
    while (( attempt < REGEN_ATTEMPTS )); do
      attempt=$((attempt + 1))
      gcl_json="$RUN_DIR/golden-$name.golangci.$attempt.json"
      gcl_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-golden-gcl.XXXXXX")"
      (
        cd "$work"
        env "GOLANGCI_LINT_CACHE=$gcl_cache" \
          ${case_env[@]+"${case_env[@]}"} \
          "$GOLANGCI" run \
          -c "$case_dir/config.yml" \
          --output.json.path=stdout \
          --path-mode abs \
          --issues-exit-code 0 \
          --allow-parallel-runners \
          ./...
      ) >"$gcl_json" 2>"$RUN_DIR/golden-$name.golangci.$attempt.stderr" || {
        echo "golangci-lint failed for $name; see $RUN_DIR/golden-$name.golangci.$attempt.stderr" >&2
        cat "$RUN_DIR/golden-$name.golangci.$attempt.stderr" >&2 || true
        rm -rf "$gcl_cache"
        break
      }
      rm -rf "$gcl_cache"
      gcl_args+=(--golangci "$gcl_json")
      (( attempt < REGEN_CONFIRMATIONS )) && continue
      if python3 "$GOLDEN_PY" write \
        --case "$name" \
        --root "$work" \
        "${gcl_args[@]}" \
        --confirmations "$REGEN_CONFIRMATIONS" \
        --tool-version "$GCL_VER" \
        -o "$golden"; then
        regen_ok=1
        break
      fi
    done
    if [[ "$regen_ok" -eq 0 ]]; then
      echo "  $name: NOT regenerated — golangci-lint never agreed with itself in $attempt run(s)" >&2
      FAILED=$((FAILED + 1))
    fi
    continue
  fi

  [[ -f "$golden" ]] || die "$name: missing $golden (run with --regen)"

  guff_json="$RUN_DIR/golden-$name.guff.json"
  guff_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-golden-guff.XXXXXX")"
  (
    cd "$work"
    env "GUFF_CACHE=$guff_cache" \
      "GUFF_DEBUG_ILL_TYPED=1" \
      ${case_env[@]+"${case_env[@]}"} \
      "$GUFF" run \
      -c "$case_dir/config.yml" \
      --out-format json \
      --issues-exit-code 0 \
      --no-cache \
      ./...
  ) >"$guff_json" 2>"$RUN_DIR/golden-$name.guff.stderr" || {
    echo "guff failed for $name; see $RUN_DIR/golden-$name.guff.stderr" >&2
    cat "$RUN_DIR/golden-$name.guff.stderr" >&2 || true
    rm -rf "$guff_cache"
    FAILED=$((FAILED + 1))
    continue
  }
  rm -rf "$guff_cache"

  # A worker panic or a skipped ill-typed package leaves findings silently
  # short, which an exact golden match would then bless (COMPAT-HARDENING
  # Phase 1). Golden cases are hand-written and must be clean: no baseline.
  if ! python3 "$HEALTH" check \
    --target "golden-$name" \
    --stderr "$RUN_DIR/golden-$name.guff.stderr"; then
    FAILED=$((FAILED + 1))
  fi

  # A case that is still being brought to zero carries a ratchet.json; the
  # gate then fails only if its diff grows. Every differing finding is still
  # printed either way — nothing is suppressed.
  python3 "$GOLDEN_PY" check \
    --case "$name" \
    --root "$work" \
    --guff "$guff_json" \
    --golden "$golden" \
    --ratchet "$case_dir/ratchet.json" || FAILED=$((FAILED + 1))
done

if [[ -n "$CASE_FILTER" && "$SELECTED" -eq 0 ]]; then
  die "case '$CASE_FILTER' not found under $CASES_DIR"
fi
if [[ "$SELECTED" -eq 0 ]]; then
  die "no cases found under $CASES_DIR"
fi

echo
if [[ "$REGEN" -eq 1 ]]; then
  echo "Regenerated $SELECTED case(s). Review the diff before committing."
  [[ "$FAILED" -gt 0 ]] && { echo "FAIL: $FAILED case(s) failed to run" >&2; exit 1; }
  exit 0
fi

if [[ "$FAILED" -gt 0 ]]; then
  echo "FAIL: $FAILED/$SELECTED case(s) differ from golden" >&2
  echo "Fix guff, or regenerate with ./compat/golden/run.sh --regen after review." >&2
  exit 1
fi
echo "OK: $SELECTED case(s) match golden exactly"
