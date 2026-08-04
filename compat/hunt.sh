#!/usr/bin/env bash
# compat/hunt.sh — clone hunt-tier OSS repos and diff guff vs golangci-lint.
#
# Usage:
#   ./compat/hunt.sh                 # all entries in corpus/hunt.json
#   ./compat/hunt.sh --name restic   # one repo
#   ./compat/hunt.sh --no-warm       # skip go mod download / go list
#
# Results land under compat/results/hunt-<stamp>/ (not the CI gate).
# Unexpected diffs are printed; exit 1 if any target has unexpected diffs
# or a tool failure. Prefer fixing guff + isolate fixtures over allowlists.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPAT_DIR="$ROOT/compat"
HUNT_JSON="${HUNT_JSON:-$ROOT/corpus/hunt.json}"
CACHE="${CORPUS_CACHE:-$ROOT/corpus/cache}"
NORMALIZE="$COMPAT_DIR/normalize.py"
PATCH_UNLIMITED="$ROOT/corpus/patch_unlimited_issues.py"
RESULTS_DIR="$COMPAT_DIR/results"
ALLOWLIST_DIR="$COMPAT_DIR/allowlists"
mkdir -p "$CACHE" "$RESULTS_DIR" "$ALLOWLIST_DIR"

NAME_FILTER=""
WARM=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --name) NAME_FILTER="$2"; shift 2 ;;
    --name=*) NAME_FILTER="${1#*=}"; shift ;;
    --no-warm) WARM=0; shift ;;
    -h|--help) sed -n '2,14p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

die() { echo "error: $*" >&2; exit 1; }

resolve_guff() {
  if [[ -n "${GUFF_BIN:-}" ]]; then echo "$GUFF_BIN"
  elif [[ -x "$ROOT/target/release/guff" ]]; then echo "$ROOT/target/release/guff"
  elif command -v guff >/dev/null 2>&1; then command -v guff
  else die "guff not found; cargo build --release -p guff-lint"
  fi
}

resolve_golangci() {
  if [[ -n "${GOLANGCI_LINT_BIN:-}" ]]; then echo "$GOLANGCI_LINT_BIN"
  elif command -v golangci-lint >/dev/null 2>&1; then command -v golangci-lint
  else die "golangci-lint not on PATH"
  fi
}

GUFF="$(resolve_guff)"
GOLANGCI="$(resolve_golangci)"
command -v git >/dev/null || die "git not found"
command -v go >/dev/null || die "go not found"
command -v python3 >/dev/null || die "python3 not found"
[[ -f "$HUNT_JSON" ]] || die "missing $HUNT_JSON"
[[ -f "$NORMALIZE" ]] || die "missing $NORMALIZE"
[[ -f "$PATCH_UNLIMITED" ]] || die "missing $PATCH_UNLIMITED"

is_v2_config() {
  python3 - "$1" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8", errors="replace").read()
for line in text.splitlines():
    s = line.strip()
    if s.startswith("#"):
        continue
    if re.match(r'^version:\s*["\']?2["\']?\s*(?:#.*)?$', s):
        sys.exit(0)
sys.exit(1)
PY
}

discover_config() {
  local dir="$1" override="$2"
  if [[ -n "$override" ]]; then
    [[ -f "$dir/$override" ]] || die "config override not found: $dir/$override"
    echo "$dir/$override"
    return
  fi
  for cand in .golangci.yml .golangci.yaml; do
    if [[ -f "$dir/$cand" ]]; then
      echo "$dir/$cand"
      return
    fi
  done
  die "no .golangci.yml/.yaml in $dir"
}

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$RESULTS_DIR/hunt-$STAMP"
mkdir -p "$RUN_DIR"
MANIFEST="$RUN_DIR/manifest.tsv"
: >"$MANIFEST"

echo "guff compat hunt"
echo "  guff:     $($GUFF version --short 2>/dev/null || echo unknown) ($GUFF)"
echo "  golangci: $($GOLANGCI version --short 2>/dev/null || echo unknown)"
echo "  hunt:     $HUNT_JSON"
echo "  results:  $RUN_DIR"
echo

FAILED=0
UNEXPECTED=0

HUNT_TSV="$(mktemp "${TMPDIR:-/tmp}/guff-hunt-list.XXXXXX")"
if ! python3 - "$HUNT_JSON" "$NAME_FILTER" >"$HUNT_TSV" <<'PY'
import json, sys
repos = json.load(open(sys.argv[1], encoding="utf-8"))
want = sys.argv[2]
n = 0
for r in repos:
    if want and r["name"] != want:
        continue
    print("\t".join([
        r["name"], r["url"], r["ref"], r.get("packages") or "./...",
        r.get("timeout") or "15m", r.get("config") or "",
    ]))
    n += 1
if n == 0:
    sys.exit(2)
PY
then
  rm -f "$HUNT_TSV"
  die "no hunt targets selected"
fi

while IFS=$'\t' read -r name url ref packages timeout config_override; do
  [[ -z "${name:-}" ]] && continue
  dir="$CACHE/$name"
  echo "=== prepare $name ($ref) ==="
  if [[ -d "$dir/.git" ]]; then
    git -C "$dir" fetch --depth 1 origin "refs/tags/$ref:refs/tags/$ref" 2>/dev/null \
      || git -C "$dir" fetch --depth 1 origin "$ref" 2>/dev/null || true
    git -C "$dir" checkout -q -f "$ref" 2>/dev/null \
      || git -C "$dir" checkout -q -f "tags/$ref" \
      || die "checkout failed for $name @$ref"
  else
    rm -rf "$dir"
    git clone --depth 1 --branch "$ref" "$url" "$dir" \
      || die "clone failed for $name"
  fi

  config="$(discover_config "$dir" "$config_override")"
  is_v2_config "$config" || die "$name config is not golangci-lint v2: $config"
  echo "  config: $config"

  if [[ "$WARM" -eq 1 ]]; then
    echo "  warming modules..."
    (cd "$dir" && go mod download >/dev/null 2>&1 || true)
    (cd "$dir" && go list $packages >/dev/null 2>&1 || true)
  fi

  run_config="$RUN_DIR/${name}.config.yml"
  python3 "$PATCH_UNLIMITED" "$config" -o "$run_config"
  # golangci resolves ${base-path} relative to the config file location. The
  # patched copy lives under results/, so rewrite to the repo root (rclone
  # ruleguard `${base-path}/bin/rules.go`, etc.).
  python3 - "$run_config" "$dir" <<'PY'
import pathlib, sys
cfg, root = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]).resolve()
text = cfg.read_text(encoding="utf-8")
if "${base-path}" in text:
    cfg.write_text(text.replace("${base-path}", str(root)), encoding="utf-8")
PY

  guff_json="$RUN_DIR/${name}.guff.json"
  gcl_json="$RUN_DIR/${name}.golangci.json"
  guff_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-hunt-guff.XXXXXX")"
  gcl_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-hunt-gcl.XXXXXX")"

  echo "=== run $name ==="
  # shellcheck disable=SC2086
  if ! (
    cd "$dir"
    env "GUFF_CACHE=$guff_cache" "GOLANGCI_LINT_CACHE=$guff_cache" \
      "$GUFF" run -c "$run_config" --out-format json --issues-exit-code 0 \
      --timeout "$timeout" --no-cache $packages
  ) >"$guff_json" 2>"$RUN_DIR/${name}.guff.stderr"; then
    echo "  guff FAILED — see $RUN_DIR/${name}.guff.stderr" >&2
    tail -40 "$RUN_DIR/${name}.guff.stderr" >&2 || true
    FAILED=$((FAILED + 1))
    rm -rf "$guff_cache" "$gcl_cache"
    continue
  fi

  # shellcheck disable=SC2086
  if ! (
    cd "$dir"
    env "GOLANGCI_LINT_CACHE=$gcl_cache" "GUFF_CACHE=$gcl_cache" \
      "$GOLANGCI" run -c "$run_config" --output.json.path=stdout --path-mode abs \
      --issues-exit-code 0 --timeout="$timeout" --max-issues-per-linter=0 \
      --max-same-issues=0 --allow-parallel-runners $packages
  ) >"$gcl_json" 2>"$RUN_DIR/${name}.golangci.stderr"; then
    echo "  golangci FAILED — see $RUN_DIR/${name}.golangci.stderr" >&2
    tail -40 "$RUN_DIR/${name}.golangci.stderr" >&2 || true
    FAILED=$((FAILED + 1))
    rm -rf "$guff_cache" "$gcl_cache"
    continue
  fi

  rm -rf "$guff_cache" "$gcl_cache"
  printf '%s\t%s\t%s\t%s\n' "$name" "$dir" "$guff_json" "$gcl_json" >>"$MANIFEST"

  python3 "$NORMALIZE" diff \
    --target "$name" \
    --root "$dir" \
    --guff "$guff_json" \
    --golangci "$gcl_json" \
    --allowlist-dir "$ALLOWLIST_DIR" \
    --report "$RUN_DIR/${name}.md" \
    --json-out "$RUN_DIR/${name}.summary.json" \
    || true

  if ! python3 - "$RUN_DIR/${name}.summary.json" <<'PY'
import json, sys
from collections import Counter
s = json.load(open(sys.argv[1], encoding="utf-8"))
status = "OK" if s["ok"] else "UNEXPECTED"
print(
    f"  {s['target']}: guff={s['guff']} golangci={s['golangci']} both={s['both']} "
    f"P={s['precision']:.1%} R={s['recall']:.1%} [{status}]"
)
if not s["ok"]:
    for k in s["unexpected_guff"][:40]:
        print(f"    +guff  {k}")
    if len(s["unexpected_guff"]) > 40:
        print(f"    +guff  ... ({len(s['unexpected_guff'])} total)")
    for k in s["unexpected_golangci"][:40]:
        print(f"    +gcl   {k}")
    if len(s["unexpected_golangci"]) > 40:
        print(f"    +gcl   ... ({len(s['unexpected_golangci'])} total)")
    def lint(key):
        parts = key.split(":", 3)
        return parts[2] if len(parts) >= 3 else "?"
    cg = Counter(lint(k) for k in s["unexpected_guff"])
    cc = Counter(lint(k) for k in s["unexpected_golangci"])
    if cg:
        print("  guff-only by linter:", dict(cg.most_common(15)))
    if cc:
        print("  gcl-only by linter:", dict(cc.most_common(15)))
sys.exit(0 if s["ok"] else 1)
PY
  then
    UNEXPECTED=$((UNEXPECTED + 1))
  fi
  echo
done <"$HUNT_TSV"
rm -f "$HUNT_TSV"

python3 "$NORMALIZE" report "$MANIFEST" \
  --allowlist-dir "$ALLOWLIST_DIR" \
  --report "$RUN_DIR/REPORT.md" \
  --json-out "$RUN_DIR/summary.json" \
  || true

echo "Hunt complete: $RUN_DIR"
echo "  failures=$FAILED unexpected=$UNEXPECTED"
if [[ "$FAILED" -gt 0 || "$UNEXPECTED" -gt 0 ]]; then
  exit 1
fi
