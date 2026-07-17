#!/usr/bin/env bash
# compat/run.sh — guff vs golangci-lint finding-set diff harness (R21).
#
# Usage:
#   ./compat/run.sh              # fixture + benchmarks/local
#   ./compat/run.sh --smoke      # fixture only (CI gate)
#   ./compat/run.sh --oss        # also clone/compare repos.txt
#   ./compat/run.sh --update-allowlist   # rewrite allowlist from current diffs
#
# Env:
#   GUFF_BIN / GOLANGCI_LINT_BIN / COMPAT_CORPUS / SKIP_GOLANGCI=1
#
# Exit 0 when every target's unexpected-diff set is empty (allowlist covers
# all known mismatches). Exit 1 on unexpected diffs or tool failure.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPAT_DIR="$ROOT/compat"
CORPUS="${COMPAT_CORPUS:-$COMPAT_DIR/corpus}"
CONFIG="$COMPAT_DIR/standard.yml"
REPOS_FILE="$COMPAT_DIR/repos.txt"
ALLOWLIST="$COMPAT_DIR/allowlist.txt"
RESULTS_DIR="$COMPAT_DIR/results"
NORMALIZE="$COMPAT_DIR/normalize.py"
mkdir -p "$CORPUS" "$RESULTS_DIR"

SMOKE=0
OSS=0
UPDATE_ALLOWLIST=0

for arg in "$@"; do
  case "$arg" in
    --smoke) SMOKE=1 ;;
    --oss) OSS=1 ;;
    --update-allowlist) UPDATE_ALLOWLIST=1 ;;
    -h|--help)
      sed -n '2,16p' "$0"
      exit 0
      ;;
    *)
      echo "unknown arg: $arg" >&2
      exit 2
      ;;
  esac
done

die() { echo "error: $*" >&2; exit 1; }

resolve_guff() {
  if [[ -n "${GUFF_BIN:-}" ]]; then
    echo "$GUFF_BIN"
  elif [[ -x "$ROOT/target/release/guff" ]]; then
    echo "$ROOT/target/release/guff"
  elif command -v guff >/dev/null 2>&1; then
    command -v guff
  else
    die "guff not found; build with: cargo build --release -p guff-lint"
  fi
}

resolve_golangci() {
  if [[ -n "${GOLANGCI_LINT_BIN:-}" ]]; then
    echo "$GOLANGCI_LINT_BIN"
  elif command -v golangci-lint >/dev/null 2>&1; then
    command -v golangci-lint
  else
    echo ""
  fi
}

GUFF="$(resolve_guff)"
GOLANGCI="$(resolve_golangci)"
if [[ -z "$GOLANGCI" ]]; then
  die "golangci-lint not on PATH (required for compat diffs; set GOLANGCI_LINT_BIN)"
fi
if [[ "${SKIP_GOLANGCI:-0}" == "1" ]]; then
  die "SKIP_GOLANGCI=1 is not supported for compat (need both tools)"
fi

command -v go >/dev/null 2>&1 || die "go not found"
command -v python3 >/dev/null 2>&1 || die "python3 not found"
[[ -f "$NORMALIZE" ]] || die "missing $NORMALIZE"
[[ -f "$CONFIG" ]] || die "missing $CONFIG"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$RESULTS_DIR/$STAMP"
mkdir -p "$RUN_DIR"
MANIFEST="$RUN_DIR/manifest.tsv"
: >"$MANIFEST"

GUFF_VER="$("$GUFF" version --short 2>/dev/null || echo unknown)"
GCL_VER="$("$GOLANGCI" version --short 2>/dev/null || "$GOLANGCI" version 2>/dev/null | head -1 || echo unknown)"

echo "guff compat harness (R21)"
echo "  guff:     $GUFF_VER ($GUFF)"
echo "  golangci: $GCL_VER ($GOLANGCI)"
echo "  config:   $CONFIG"
echo "  allowlist:$ALLOWLIST"
echo "  results:  $RUN_DIR"
echo

run_target() {
  local name="$1"
  local dir="$2"
  echo "=== $name ($dir) ==="

  local guff_json gcl_json guff_cache gcl_cache
  guff_json="$RUN_DIR/${name}.guff.json"
  gcl_json="$RUN_DIR/${name}.golangci.json"
  guff_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-compat-guff.XXXXXX")"
  gcl_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-compat-gcl.XXXXXX")"

  (
    cd "$dir"
    env "GUFF_CACHE=$guff_cache" "GOLANGCI_LINT_CACHE=$guff_cache" \
      "$GUFF" run \
      -c "$CONFIG" \
      --out-format json \
      --issues-exit-code 0 \
      --timeout 5m \
      --no-cache \
      ./...
  ) >"$guff_json" 2>"$RUN_DIR/${name}.guff.stderr" || {
    echo "guff failed for $name; see $RUN_DIR/${name}.guff.stderr" >&2
    cat "$RUN_DIR/${name}.guff.stderr" >&2 || true
    rm -rf "$guff_cache" "$gcl_cache"
    return 1
  }

  (
    cd "$dir"
    env "GOLANGCI_LINT_CACHE=$gcl_cache" "GUFF_CACHE=$gcl_cache" \
      "$GOLANGCI" run \
      -c "$CONFIG" \
      --output.json.path=stdout \
      --path-mode abs \
      --issues-exit-code 0 \
      --timeout=5m \
      ./...
  ) >"$gcl_json" 2>"$RUN_DIR/${name}.golangci.stderr" || {
    echo "golangci-lint failed for $name; see $RUN_DIR/${name}.golangci.stderr" >&2
    cat "$RUN_DIR/${name}.golangci.stderr" >&2 || true
    rm -rf "$guff_cache" "$gcl_cache"
    return 1
  }

  rm -rf "$guff_cache" "$gcl_cache"
  printf '%s\t%s\t%s\t%s\n' "$name" "$dir" "$guff_json" "$gcl_json" >>"$MANIFEST"

  python3 "$NORMALIZE" diff \
    --target "$name" \
    --root "$dir" \
    --guff "$guff_json" \
    --golangci "$gcl_json" \
    --allowlist "$ALLOWLIST" \
    --report "$RUN_DIR/${name}.md" \
    --json-out "$RUN_DIR/${name}.summary.json" \
    || true

  python3 - "$RUN_DIR/${name}.summary.json" <<'PY'
import json, sys
s = json.load(open(sys.argv[1], encoding="utf-8"))
status = "OK" if s["ok"] else "UNEXPECTED"
print(
    f"  {s['target']}: guff={s['guff']} golangci={s['golangci']} both={s['both']} "
    f"P={s['precision']:.1%} R={s['recall']:.1%} [{status}]"
)
if not s["ok"]:
    for k in s["unexpected_guff"]:
        print(f"    +guff  {k}")
    for k in s["unexpected_golangci"]:
        print(f"    +gcl   {k}")
PY
}

clone_repo() {
  local name="$1"
  local url="$2"
  local ref="$3"
  local dest="$CORPUS/$name"
  command -v git >/dev/null 2>&1 || die "git not found (needed for --oss)"
  if [[ -d "$dest/.git" ]]; then
    git -C "$dest" fetch --depth 1 origin "refs/tags/${ref}:refs/tags/${ref}" >/dev/null 2>&1 \
      || git -C "$dest" fetch --depth 1 origin "$ref" >/dev/null 2>&1 \
      || true
    git -C "$dest" checkout -q "$ref" 2>/dev/null \
      || git -C "$dest" checkout -q "tags/$ref" 2>/dev/null \
      || die "cannot checkout $ref in $dest"
  else
    echo "cloning $name ($ref)..." >&2
    rm -rf "$dest"
    if ! git clone --depth 1 --branch "$ref" "$url" "$dest" >/dev/null 2>&1; then
      git clone --depth 1 "$url" "$dest" >/dev/null
      git -C "$dest" fetch --depth 1 origin tag "$ref" >/dev/null 2>&1 || true
      git -C "$dest" checkout -q "$ref" 2>/dev/null \
        || git -C "$dest" checkout -q "tags/$ref" \
        || die "clone/checkout failed for $name@$ref"
    fi
  fi
  echo "$dest"
}

run_target "fixture" "$ROOT/benchmarks/fixture"

if [[ "$SMOKE" -eq 0 ]]; then
  run_target "local" "$ROOT/benchmarks/local"
fi

if [[ "$OSS" -eq 1 ]]; then
  while read -r name url ref; do
    [[ -z "${name:-}" || "$name" == \#* ]] && continue
    dest="$(clone_repo "$name" "$url" "$ref")"
    run_target "$name" "$dest" || true
  done <"$REPOS_FILE"
fi

REPORT="$RUN_DIR/REPORT.md"
# Diff may be non-zero before allowlist update; final gate is below.
python3 "$NORMALIZE" report "$MANIFEST" \
  --allowlist "$ALLOWLIST" \
  --report "$REPORT" \
  --json-out "$RUN_DIR/summary.json" \
  || true
if [[ "$SMOKE" -eq 0 ]]; then
  cp "$REPORT" "$RESULTS_DIR/RESULTS.md"
fi

# Rebuild allowlist from full per-target diffs when requested.
if [[ "$UPDATE_ALLOWLIST" -eq 1 ]]; then
  {
    echo "# Known finding-set diffs between guff and golangci-lint (R21)."
    echo "# Format: <target> <guff-only|golangci-only> <normalized-key>"
    echo "# normalized-key = relpath:line:linter:message"
    echo "# Regenerated by: ./compat/run.sh --update-allowlist"
    echo "#"
    echo "# Prefer fixing guff over growing this list. Entries here are accepted"
    echo "# mismatches (message phrasing, enable-set gaps, known DEFERRED)."
    echo "#"
    echo "# Notable classes:"
    echo "# - ST1000 package-comment: guff staticcheck enables it; golangci's"
    echo "#   bundled staticcheck defaults often omit stylecheck ST1000."
    echo "# - local ineffassign: guff reports more assignment sites than golangci."
    echo "# Ensure issues.max-*-issues: 0 in standard.yml (defaults truncate)."
    echo
    while IFS=$'\t' read -r name dir guff_json gcl_json; do
      python3 - "$NORMALIZE" "$name" "$dir" "$guff_json" "$gcl_json" <<'PY'
import sys
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv[1]).parent))
from normalize import diff_sets, issue_keys, load_issues
name, root, guff_json, gcl_json = sys.argv[2:6]
r = diff_sets(name, issue_keys(load_issues(guff_json), root), issue_keys(load_issues(gcl_json), root), [])
for k in sorted(r.guff_only):
    print(f"{name} guff-only {k}")
for k in sorted(r.golangci_only):
    print(f"{name} golangci-only {k}")
PY
    done <"$MANIFEST"
  } >"$ALLOWLIST"
  echo "Updated $ALLOWLIST"
  python3 "$NORMALIZE" report "$MANIFEST" \
    --allowlist "$ALLOWLIST" \
    --report "$REPORT" \
    --json-out "$RUN_DIR/summary.json" \
    || true
  if [[ "$SMOKE" -eq 0 ]]; then
    cp "$REPORT" "$RESULTS_DIR/RESULTS.md"
  fi
fi

echo
echo "Wrote $REPORT"
if [[ "$SMOKE" -eq 0 ]]; then
  echo "Wrote $RESULTS_DIR/RESULTS.md"
fi

python3 - "$RUN_DIR/summary.json" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1], encoding="utf-8"))
bad = [r for r in rows if not r["ok"]]
if bad:
    print(f"FAIL: {len(bad)} target(s) with unexpected diffs", file=sys.stderr)
    raise SystemExit(1)
print(f"OK: {len(rows)} target(s) within allowlist")
PY
