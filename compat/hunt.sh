#!/usr/bin/env bash
# compat/hunt.sh — clone hunt-tier OSS repos and diff guff vs golangci-lint.
#
# Usage:
#   ./compat/hunt.sh                    # all entries in corpus/hunt.json
#   ./compat/hunt.sh --name restic      # one repo
#   ./compat/hunt.sh --no-warm          # skip go mod download / go list
#   ./compat/hunt.sh --update-baseline  # re-record the ill-typed baselines
#
# Results land under compat/results/hunt-<stamp>/ (not the CI gate).
# Unexpected diffs are printed; exit 1 if any target has unexpected diffs,
# a tool failure, or a panic / ill-typed regression. Prefer fixing guff +
# isolate fixtures over allowlists.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPAT_DIR="$ROOT/compat"
HUNT_JSON="${HUNT_JSON:-$ROOT/corpus/hunt.json}"
CACHE="${CORPUS_CACHE:-$ROOT/corpus/cache}"
NORMALIZE="$COMPAT_DIR/normalize.py"
PATCH_UNLIMITED="$ROOT/corpus/patch_unlimited_issues.py"
RESULTS_DIR="$COMPAT_DIR/results"
ALLOWLIST_DIR="$COMPAT_DIR/allowlists"
HEALTH="$COMPAT_DIR/health.py"
# Hunt keeps its own baseline file. The OSS one is a CI gate whose rows are
# rewritten by `run.sh --update-baseline`; mixing the two tiers into it would
# let a hunt refresh silently move a gated OSS number.
HEALTH_BASELINE="$COMPAT_DIR/baselines/health-hunt.json"
mkdir -p "$CACHE" "$RESULTS_DIR" "$ALLOWLIST_DIR"

NAME_FILTER=""
WARM=1
UPDATE_BASELINE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --name) NAME_FILTER="$2"; shift 2 ;;
    --name=*) NAME_FILTER="${1#*=}"; shift ;;
    --no-warm) WARM=0; shift ;;
    --update-baseline) UPDATE_BASELINE=1; shift ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
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
[[ -f "$HEALTH" ]] || die "missing $HEALTH"

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
HEALTH_FAILED=0

HUNT_TSV="$(mktemp "${TMPDIR:-/tmp}/guff-hunt-list.XXXXXX")"
if ! python3 - "$HUNT_JSON" "$NAME_FILTER" >"$HUNT_TSV" <<'PY'
import json, sys
repos = json.load(open(sys.argv[1], encoding="utf-8"))
want = sys.argv[2]
n = 0
for r in repos:
    if want and r["name"] != want:
        continue
    # "-" for an absent optional field, not "": tab is IFS whitespace, so bash's
    # `read` collapses a run of tabs into one delimiter and an empty column in
    # the middle would shift every column after it.
    print("\t".join([
        r["name"], r["url"], r["ref"], r.get("packages") or "./...",
        r.get("timeout") or "15m", r.get("config") or "-",
        ",".join(r.get("build_tags") or []) or "-",
    ]))
    n += 1
if n == 0:
    sys.exit(2)
PY
then
  rm -f "$HUNT_TSV"
  die "no hunt targets selected"
fi

while IFS=$'\t' read -r name url ref packages timeout config_override build_tags; do
  [[ -z "${name:-}" ]] && continue
  [[ "$config_override" == "-" ]] && config_override=""
  [[ "$build_tags" == "-" ]] && build_tags=""
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
    if [[ -n "${build_tags:-}" ]]; then
      (cd "$dir" && go list -tags "$build_tags" $packages >/dev/null 2>&1 || true)
    else
      (cd "$dir" && go list $packages >/dev/null 2>&1 || true)
    fi
  fi

  # `--uniq-by-line false`: the key is ON by default and keeps one finding per
  # (file, line) across all linters — so on any line two linters both report,
  # the survivor depends on arrival order, and a single missing finding on
  # either side swaps it and moves the diff somewhere unrelated. Both tools get
  # the same patched config, so turning it off makes the repo-scale comparison
  # independent of the order. The order itself is gated exactly, on pinned
  # finding sets, by compat/golden/cases/issues-uniq-by-line*.
  run_config="$RUN_DIR/${name}.config.yml"
  python3 "$PATCH_UNLIMITED" "$config" -o "$run_config" --uniq-by-line false
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

  # Some repos do not build from a plain checkout: syncthing's `lib/api/auto`
  # gets its `Assets()` from a generated file, and without it golangci-lint
  # reports the compile error *and nothing else* — one typecheck issue in place
  # of every finding in the repo, which reads as "guff invented 547 findings".
  # The repo ships a `noassets` build tag for exactly this, so the tier carries
  # the tags rather than the generator.
  #
  # A plain string, not an array: macOS ships bash 3.2, where `"${a[@]}"` on an
  # empty array is an unbound-variable error under `set -u`. Tag lists are
  # comma-separated and contain no spaces, so word splitting is exact here.
  tag_flag=""
  if [[ -n "${build_tags:-}" ]]; then
    tag_flag="--build-tags $build_tags"
    echo "  build tags: $build_tags"
  fi

  guff_json="$RUN_DIR/${name}.guff.json"
  gcl_json="$RUN_DIR/${name}.golangci.json"
  guff_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-hunt-guff.XXXXXX")"
  gcl_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-hunt-gcl.XXXXXX")"

  echo "=== run $name ==="
  # GUFF_DEBUG_ILL_TYPED makes guff name the packages every analyzer skipped;
  # health.py reads them (and any panic) back out of stderr below. Without it
  # a hunt repo can lose a whole package's findings and the diff shows only
  # "golangci-only" — which is how syncthing's lib/model hid five linters.
  # shellcheck disable=SC2086
  if ! (
    cd "$dir"
    env "GUFF_CACHE=$guff_cache" "GOLANGCI_LINT_CACHE=$guff_cache" \
      "GUFF_DEBUG_ILL_TYPED=1" \
      "$GUFF" run -c "$run_config" --out-format json --issues-exit-code 0 \
      $tag_flag --timeout "$timeout" --no-cache $packages
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
      $tag_flag --issues-exit-code 0 --timeout="$timeout" \
      --max-issues-per-linter=0 --max-same-issues=0 --allow-parallel-runners $packages
  ) >"$gcl_json" 2>"$RUN_DIR/${name}.golangci.stderr"; then
    echo "  golangci FAILED — see $RUN_DIR/${name}.golangci.stderr" >&2
    tail -40 "$RUN_DIR/${name}.golangci.stderr" >&2 || true
    FAILED=$((FAILED + 1))
    rm -rf "$guff_cache" "$gcl_cache"
    continue
  fi

  rm -rf "$guff_cache" "$gcl_cache"
  printf '%s\t%s\t%s\t%s\n' "$name" "$dir" "$guff_json" "$gcl_json" >>"$MANIFEST"

  # Silent recall losses: a panicking analyzer drops its findings, and an
  # ill-typed package is skipped whole. Neither shows up in the set-diff — an
  # ill-typed package reads as a run of golangci-only findings with no linter
  # in common, and a panic reads as nothing at all.
  health_args=(check --target "$name" --stderr "$RUN_DIR/${name}.guff.stderr"
    --baseline "$HEALTH_BASELINE")
  if [[ "$UPDATE_BASELINE" -eq 1 ]]; then
    health_args+=(--update)
  fi
  if ! python3 "$HEALTH" "${health_args[@]}"; then
    HEALTH_FAILED=$((HEALTH_FAILED + 1))
  fi

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
echo "  failures=$FAILED unexpected=$UNEXPECTED health=$HEALTH_FAILED"
if [[ "$HEALTH_FAILED" -gt 0 && "$UPDATE_BASELINE" -eq 0 ]]; then
  echo "FAIL: $HEALTH_FAILED target(s) failed the panic / ill-typed gate" >&2
  echo "See compat/health.py; hunt baselines live in $HEALTH_BASELINE" >&2
fi
if [[ "$FAILED" -gt 0 || "$UNEXPECTED" -gt 0 ]] \
  || [[ "$HEALTH_FAILED" -gt 0 && "$UPDATE_BASELINE" -eq 0 ]]; then
  exit 1
fi
