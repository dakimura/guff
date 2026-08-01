#!/usr/bin/env bash
# compat/run.sh — guff vs golangci-lint finding-set diff harness (R21).
#
# Usage:
#   ./compat/run.sh              # fixture + benchmarks/local (standard.yml)
#   ./compat/run.sh --smoke      # fixture only (CI gate)
#   ./compat/run.sh --oss --tier pr
#   ./compat/run.sh --oss --tier nightly
#   ./compat/run.sh --update-allowlist   # rewrite allowlists from current diffs
#
# OSS targets use each checkout's real golangci-lint v2 config (via corpus/).
# Fixture / local keep compat/standard.yml.
#
# Env:
#   GUFF_BIN / GOLANGCI_LINT_BIN / CORPUS_CACHE
#
# Exit 0 when every target's unexpected-diff set is empty (allowlist covers
# all known mismatches). Exit 1 on unexpected diffs or tool failure.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPAT_DIR="$ROOT/compat"
CONFIG_STANDARD="$COMPAT_DIR/standard.yml"
ALLOWLIST_DIR="$COMPAT_DIR/allowlists"
# Legacy single-file allowlist (still honored if present and dir missing entries).
ALLOWLIST_LEGACY="$COMPAT_DIR/allowlist.txt"
RESULTS_DIR="$COMPAT_DIR/results"
NORMALIZE="$COMPAT_DIR/normalize.py"
PREPARE="$ROOT/corpus/prepare.sh"
PATCH_UNLIMITED="$ROOT/corpus/patch_unlimited_issues.py"
mkdir -p "$RESULTS_DIR" "$ALLOWLIST_DIR"

SMOKE=0
OSS=0
UPDATE_ALLOWLIST=0
TIER="pr"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --smoke) SMOKE=1; shift ;;
    --oss) OSS=1; shift ;;
    --update-allowlist) UPDATE_ALLOWLIST=1; shift ;;
    --tier)
      TIER="$2"
      shift 2
      ;;
    --tier=*)
      TIER="${1#*=}"
      shift
      ;;
    -h|--help)
      sed -n '2,20p' "$0"
      exit 0
      ;;
    *)
      echo "unknown arg: $1" >&2
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
[[ -f "$CONFIG_STANDARD" ]] || die "missing $CONFIG_STANDARD"
[[ -d "$ALLOWLIST_DIR" ]] || die "missing $ALLOWLIST_DIR"

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
echo "  standard: $CONFIG_STANDARD"
echo "  allowlists:$ALLOWLIST_DIR"
echo "  results:  $RUN_DIR"
echo

run_target() {
  local name="$1"
  local dir="$2"
  local config="$3"
  local packages="$4"
  local timeout="$5"
  echo "=== $name ($dir) ==="
  echo "  config: $config"
  echo "  packages: $packages  timeout: $timeout"

  local guff_json gcl_json guff_cache gcl_cache run_config
  guff_json="$RUN_DIR/${name}.guff.json"
  gcl_json="$RUN_DIR/${name}.golangci.json"
  guff_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-compat-guff.XXXXXX")"
  gcl_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-compat-gcl.XXXXXX")"

  # Force unlimited issue caps so max-same-issues truncation cannot rotate keys.
  run_config="$RUN_DIR/${name}.config.yml"
  if [[ "$config" == "$CONFIG_STANDARD" ]]; then
    cp "$config" "$run_config"
  else
    python3 "$PATCH_UNLIMITED" "$config" -o "$run_config"
  fi

  # shellcheck disable=SC2086
  (
    cd "$dir"
    env "GUFF_CACHE=$guff_cache" "GOLANGCI_LINT_CACHE=$guff_cache" \
      "$GUFF" run \
      -c "$run_config" \
      --out-format json \
      --issues-exit-code 0 \
      --timeout "$timeout" \
      --no-cache \
      $packages
  ) >"$guff_json" 2>"$RUN_DIR/${name}.guff.stderr" || {
    echo "guff failed for $name; see $RUN_DIR/${name}.guff.stderr" >&2
    cat "$RUN_DIR/${name}.guff.stderr" >&2 || true
    rm -rf "$guff_cache" "$gcl_cache"
    return 1
  }

  # shellcheck disable=SC2086
  (
    cd "$dir"
    env "GOLANGCI_LINT_CACHE=$gcl_cache" "GUFF_CACHE=$gcl_cache" \
      "$GOLANGCI" run \
      -c "$run_config" \
      --output.json.path=stdout \
      --path-mode abs \
      --issues-exit-code 0 \
      --timeout="$timeout" \
      --max-issues-per-linter=0 \
      --max-same-issues=0 \
      --allow-parallel-runners \
      $packages
  ) >"$gcl_json" 2>"$RUN_DIR/${name}.golangci.stderr" || {
    echo "golangci-lint failed for $name; see $RUN_DIR/${name}.golangci.stderr" >&2
    cat "$RUN_DIR/${name}.golangci.stderr" >&2 || true
    rm -rf "$guff_cache" "$gcl_cache"
    return 1
  }

  rm -rf "$guff_cache" "$gcl_cache"
  printf '%s\t%s\t%s\t%s\n' "$name" "$dir" "$guff_json" "$gcl_json" >>"$MANIFEST"

  local allow_args=(--allowlist-dir "$ALLOWLIST_DIR")
  if [[ -f "$ALLOWLIST_LEGACY" ]]; then
    allow_args+=(--allowlist "$ALLOWLIST_LEGACY")
  fi

  python3 "$NORMALIZE" diff \
    --target "$name" \
    --root "$dir" \
    --guff "$guff_json" \
    --golangci "$gcl_json" \
    "${allow_args[@]}" \
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

FAILED_TARGETS=0

run_target "fixture" "$ROOT/benchmarks/fixture" "$CONFIG_STANDARD" "./..." "5m" || FAILED_TARGETS=$((FAILED_TARGETS + 1))

if [[ "$SMOKE" -eq 0 ]]; then
  run_target "local" "$ROOT/benchmarks/local" "$CONFIG_STANDARD" "./..." "5m" || FAILED_TARGETS=$((FAILED_TARGETS + 1))
fi

if [[ "$OSS" -eq 1 ]]; then
  [[ -x "$PREPARE" ]] || die "missing $PREPARE"
  echo "Preparing OSS corpus (tier=$TIER)..."
  prep_list="$(mktemp "${TMPDIR:-/tmp}/guff-compat-prep.XXXXXX")"
  "$PREPARE" --tier "$TIER" >"$prep_list"
  while IFS=$'\t' read -r name dir config packages timeout tier; do
    [[ -z "${name:-}" ]] && continue
    if ! run_target "$name" "$dir" "$config" "$packages" "$timeout"; then
      FAILED_TARGETS=$((FAILED_TARGETS + 1))
    fi
  done <"$prep_list"
  rm -f "$prep_list"
fi

REPORT="$RUN_DIR/REPORT.md"
allow_args=(--allowlist-dir "$ALLOWLIST_DIR")
if [[ -f "$ALLOWLIST_LEGACY" ]]; then
  allow_args+=(--allowlist "$ALLOWLIST_LEGACY")
fi

# Diff may be non-zero before allowlist update; final gate is below.
python3 "$NORMALIZE" report "$MANIFEST" \
  "${allow_args[@]}" \
  --report "$REPORT" \
  --json-out "$RUN_DIR/summary.json" \
  || true
if [[ "$SMOKE" -eq 0 ]]; then
  cp "$REPORT" "$RESULTS_DIR/RESULTS.md"
fi

# Rebuild per-target allowlists from full diffs when requested.
# Replaces existing entries with the current diff set (stale fixed keys drop out).
if [[ "$UPDATE_ALLOWLIST" -eq 1 ]]; then
  python3 - "$NORMALIZE" "$MANIFEST" "$ALLOWLIST_DIR" <<'PY'
import sys
from pathlib import Path

sys.path.insert(0, str(Path(sys.argv[1]).parent))
from normalize import diff_sets, issue_keys, load_issues

manifest = Path(sys.argv[2])
allow_dir = Path(sys.argv[3])
allow_dir.mkdir(parents=True, exist_ok=True)

header = """# Known finding-set diffs between guff and golangci-lint (R21).
# Format: <target> <guff-only|golangci-only> <normalized-key>
# normalized-key = relpath:line:linter:message
# Regenerated by: ./compat/run.sh --update-allowlist
#
# Prefer fixing guff over growing this list. Entries here are accepted
# mismatches (message phrasing, enable-set gaps, known DEFERRED).

"""

by_target: dict[str, list[str]] = {}
for raw in manifest.read_text(encoding="utf-8").splitlines():
    line = raw.strip()
    if not line or line.startswith("#"):
        continue
    name, root, guff_json, gcl_json = line.split("\t")
    r = diff_sets(
        name,
        issue_keys(load_issues(guff_json), root),
        issue_keys(load_issues(gcl_json), root),
        [],
    )
    lines = [f"{name} guff-only {k}" for k in sorted(r.guff_only)]
    lines += [f"{name} golangci-only {k}" for k in sorted(r.golangci_only)]
    by_target[name] = lines

default_targets = {"fixture", "local"}
default_lines: list[str] = []
for name, lines in sorted(by_target.items()):
    if name in default_targets:
        default_lines.extend(lines)
    else:
        path = allow_dir / f"{name}.txt"
        if lines:
            path.write_text(
                header + "\n".join(lines) + "\n",
                encoding="utf-8",
            )
            print(f"Updated {path} ({len(lines)} entries)")
        else:
            if path.exists():
                path.unlink()
                print(f"Removed empty {path}")
            else:
                print(f"OK {name}: no diffs")

default_path = allow_dir / "_default.txt"
default_body = "\n".join(sorted(default_lines))
default_path.write_text(
    header
    + "# Fixture / local (standard.yml) known diffs.\n\n"
    + (default_body + "\n" if default_body else ""),
    encoding="utf-8",
)
print(f"Updated {default_path} ({len(default_lines)} entries)")
PY
  cp "$ALLOWLIST_DIR/_default.txt" "$ALLOWLIST_LEGACY"
  python3 "$NORMALIZE" report "$MANIFEST" \
    --allowlist-dir "$ALLOWLIST_DIR" \
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

if [[ "$FAILED_TARGETS" -gt 0 ]]; then
  echo "FAIL: $FAILED_TARGETS target(s) failed to run" >&2
  exit 1
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
