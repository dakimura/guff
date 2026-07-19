#!/usr/bin/env bash
# regress/run.sh — Prometheus regression gate (local only, 24GB-safe defaults).
#
# Measures guff wall-clock + peak RSS on a local prometheus checkout using
# prometheus's own .golangci.yml, diffs findings vs golangci-lint, and fails
# when metrics regress relative to regress/baseline.json.
#
# Memory-safe defaults (for ~24GB hosts):
#   - packages: ./tsdb/...   (override: REGRESS_PACKAGES='./...')
#   - concurrency: -j 1
#   - RAYON_NUM_THREADS=2
#   - reuse system GOCACHE (set REGRESS_ISOLATE_GOCACHE=1 only on large hosts)
#   - live RSS kill limit ~12GB (REGRESS_RSS_LIMIT_BYTES)
#
# Usage:
#   ./regress/run.sh                    # measure + gate
#   ./regress/run.sh --update-baseline  # rewrite baseline from this run
#   ./regress/run.sh --skip-golangci    # guff metrics only
#
# Env:
#   GUFF_BIN / GOLANGCI_LINT_BIN / PROMETHEUS_DIR
#   REGRESS_PACKAGES / REGRESS_JOBS / REGRESS_RAYON_THREADS
#   REGRESS_ISOLATE_GOCACHE / REGRESS_RSS_LIMIT_BYTES / REGRESS_TIMEOUT
#
# Requires: release guff, golangci-lint, go, python3, /usr/bin/time
# Does NOT clone prometheus — set PROMETHEUS_DIR or keep repo-root `prometheus/` symlink.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REGRESS_DIR="$ROOT/regress"
MEASURE="$REGRESS_DIR/measure.py"
GATE="$REGRESS_DIR/gate.py"
NORMALIZE="$ROOT/compat/normalize.py"
BASELINE="$REGRESS_DIR/baseline.json"
RESULTS_DIR="$REGRESS_DIR/results"
mkdir -p "$RESULTS_DIR"

UPDATE_BASELINE=0
SKIP_GOLANGCI=0

for arg in "$@"; do
  case "$arg" in
    --update-baseline) UPDATE_BASELINE=1 ;;
    --skip-golangci) SKIP_GOLANGCI=1 ;;
    -h|--help)
      sed -n '2,28p' "$0"
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

resolve_prometheus() {
  if [[ -n "${PROMETHEUS_DIR:-}" ]]; then
    echo "$PROMETHEUS_DIR"
  elif [[ -d "$ROOT/prometheus" ]]; then
    cd "$ROOT/prometheus" && pwd -P
  else
    die "prometheus checkout not found; set PROMETHEUS_DIR or create $ROOT/prometheus symlink"
  fi
}

[[ -f "$MEASURE" ]] || die "missing $MEASURE"
[[ -f "$GATE" ]] || die "missing $GATE"
[[ -f "$NORMALIZE" ]] || die "missing $NORMALIZE"
command -v go >/dev/null 2>&1 || die "go not found"
command -v python3 >/dev/null 2>&1 || die "python3 not found"
[[ -x /usr/bin/time ]] || die "/usr/bin/time not found"

GUFF="$(resolve_guff)"
GOLANGCI="$(resolve_golangci)"
PROM="$(resolve_prometheus)"
CONFIG="$PROM/.golangci.yml"
[[ -f "$CONFIG" ]] || die "missing config: $CONFIG"

if [[ "$SKIP_GOLANGCI" -eq 0 && -z "$GOLANGCI" ]]; then
  die "golangci-lint not on PATH (required; set GOLANGCI_LINT_BIN or pass --skip-golangci)"
fi
if [[ "$UPDATE_BASELINE" -eq 1 && "$SKIP_GOLANGCI" -eq 1 ]]; then
  die "--update-baseline requires golangci-lint (omit --skip-golangci)"
fi
if [[ "$UPDATE_BASELINE" -eq 0 && ! -f "$BASELINE" ]]; then
  die "missing $BASELINE — run once with --update-baseline to capture metrics"
fi

# --- 24GB-safe defaults -------------------------------------------------------
# Full ./... + empty GOCACHE + high concurrency blew past 40GB. Default to the
# R25 tsdb subtree with serial DAG, warm system GOCACHE, and a hard RSS kill.
PACKAGES_RAW="${REGRESS_PACKAGES:-./tsdb/...}"
# shellcheck disable=SC2206
PACKAGES=($PACKAGES_RAW)
JOBS="${REGRESS_JOBS:-1}"
RAYON_THREADS="${REGRESS_RAYON_THREADS:-2}"
ISOLATE_GOCACHE="${REGRESS_ISOLATE_GOCACHE:-0}"
# ~12 GiB default headroom on 24GB hosts (OS + Cursor + golangci later).
RSS_LIMIT="${REGRESS_RSS_LIMIT_BYTES:-$((12 * 1024 * 1024 * 1024))}"
TIMEOUT="${REGRESS_TIMEOUT:-15m}"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$RESULTS_DIR/$STAMP"
mkdir -p "$RUN_DIR"

PROM_SHA="$(git -C "$PROM" rev-parse HEAD 2>/dev/null || echo unknown)"
GUFF_VER="$("$GUFF" version --short 2>/dev/null || echo unknown)"
GCL_VER="skipped"
if [[ "$SKIP_GOLANGCI" -eq 0 ]]; then
  GCL_VER="$("$GOLANGCI" version --short 2>/dev/null || "$GOLANGCI" version 2>/dev/null | head -1 || echo unknown)"
fi

echo "guff prometheus regress harness (24GB-safe defaults)"
echo "  host:         $(uname -srm)"
echo "  guff:         $GUFF_VER ($GUFF)"
echo "  golangci:     $GCL_VER"
echo "  prometheus:   $PROM"
echo "  sha:          $PROM_SHA"
echo "  config:       $CONFIG"
echo "  packages:     ${PACKAGES[*]}"
echo "  concurrency:  -j $JOBS / RAYON_NUM_THREADS=$RAYON_THREADS"
echo "  isolate_gocache: $ISOLATE_GOCACHE"
echo "  rss_limit:    $RSS_LIMIT bytes"
echo "  timeout:      $TIMEOUT"
echo "  results:      $RUN_DIR"
echo

# Warm module graph / export data using the *system* GOCACHE so we do not
# compile the world under memory pressure during the timed run.
(
  cd "$PROM"
  go list "${PACKAGES[@]}" >/dev/null
)

guff_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-regress-guff.XXXXXX")"
gcl_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-regress-gcl.XXXXXX")"
gocache_isolated=""
# Always set GOCACHE explicitly (avoid ``set -u`` + empty-array issues, and
# keep the warm system cache unless isolation is requested).
GOCACHE_VALUE="$(go env GOCACHE)"
cleanup() {
  rm -rf "$guff_cache" "$gcl_cache"
  if [[ -n "$gocache_isolated" ]]; then
    rm -rf "$gocache_isolated"
  fi
}
trap cleanup EXIT

if [[ "$ISOLATE_GOCACHE" == "1" ]]; then
  gocache_isolated="$(mktemp -d "${TMPDIR:-/tmp}/guff-regress-gocache.XXXXXX")"
  GOCACHE_VALUE="$gocache_isolated"
  echo "warning: REGRESS_ISOLATE_GOCACHE=1 — cold go build may use many GB of RAM" >&2
fi

GUFF_JSON="$RUN_DIR/guff.json"
GUFF_TIME_ERR="$RUN_DIR/guff.time.stderr"
GUFF_MEAS="$RUN_DIR/guff.measure.json"
GCL_JSON="$RUN_DIR/golangci.json"
GCL_ERR="$RUN_DIR/golangci.stderr"
MEASURED="$RUN_DIR/measured.json"

echo "=== guff (cold tool-cache, measured) ==="
set +e
python3 "$MEASURE" run \
  --cwd "$PROM" \
  --stdout "$GUFF_JSON" \
  --stderr-out "$GUFF_TIME_ERR" \
  --json-out "$GUFF_MEAS" \
  --rss-limit-bytes "$RSS_LIMIT" \
  -- \
  env \
    "GUFF_CACHE=$guff_cache" \
    "GOLANGCI_LINT_CACHE=$guff_cache" \
    "RAYON_NUM_THREADS=$RAYON_THREADS" \
    "GOCACHE=$GOCACHE_VALUE" \
  "$GUFF" run \
    -c "$CONFIG" \
    -j "$JOBS" \
    --out-format json \
    --issues-exit-code 0 \
    --no-cache \
    --timeout "$TIMEOUT" \
    "${PACKAGES[@]}"
guff_rc=$?
set -e
if [[ "$guff_rc" -ne 0 ]]; then
  echo "guff failed (exit $guff_rc); see $GUFF_TIME_ERR" >&2
  if python3 -c 'import json,sys; m=json.load(open(sys.argv[1])); sys.exit(0 if m.get("killed_for_rss") else 1)' "$GUFF_MEAS" 2>/dev/null; then
    echo "hint: RSS limit hit. Shrink REGRESS_PACKAGES further, lower REGRESS_RAYON_THREADS, or raise REGRESS_RSS_LIMIT_BYTES on a larger machine." >&2
  fi
  tail -n 40 "$GUFF_TIME_ERR" >&2 || true
  exit 1
fi
python3 - "$GUFF_MEAS" <<'PY'
import json, sys
m = json.load(open(sys.argv[1], encoding="utf-8"))
gb = m["peak_rss_bytes"] / (1024 ** 3)
print(f"  wall={m['wall_seconds']:.3f}s  peak_rss={m['peak_rss_bytes']:,} bytes ({gb:.2f} GiB)")
PY

if [[ "$SKIP_GOLANGCI" -eq 0 ]]; then
  echo "=== golangci-lint (finding-set; not RSS-gated) ==="
  set +e
  (
    cd "$PROM"
    env \
      "GOLANGCI_LINT_CACHE=$gcl_cache" \
      "GUFF_CACHE=$gcl_cache" \
      "GOCACHE=$GOCACHE_VALUE" \
      "$GOLANGCI" run \
      -c "$CONFIG" \
      --output.json.path=stdout \
      --path-mode abs \
      --issues-exit-code 0 \
      --timeout="$TIMEOUT" \
      "${PACKAGES[@]}"
  ) >"$GCL_JSON" 2>"$GCL_ERR"
  gcl_rc=$?
  set -e
  if [[ "$gcl_rc" -ne 0 ]]; then
    echo "golangci-lint failed (exit $gcl_rc); see $GCL_ERR" >&2
    tail -n 40 "$GCL_ERR" >&2 || true
    exit 1
  fi
fi

# Encode packages as a single CLI-safe string for the packer.
PACKAGES_CSV="$(printf '%s,' "${PACKAGES[@]}")"
PACKAGES_CSV="${PACKAGES_CSV%,}"

echo "=== normalize + measure pack ==="
python3 - "$NORMALIZE" "$PROM" "$GUFF_JSON" "$GCL_JSON" "$GUFF_MEAS" "$MEASURED" \
  "$PROM_SHA" "$CONFIG" "$SKIP_GOLANGCI" "$PACKAGES_CSV" "$JOBS" "$RAYON_THREADS" \
  "$ISOLATE_GOCACHE" <<'PY'
import json, sys
from pathlib import Path

sys.path.insert(0, str(Path(sys.argv[1]).parent))
from normalize import diff_sets, issue_keys, load_issues  # noqa: E402

(
    _normalize,
    root,
    guff_json,
    gcl_json,
    guff_meas_path,
    out_path,
    sha,
    config,
    skip_gcl,
    packages_csv,
    jobs,
    rayon_threads,
    isolate_gocache,
) = sys.argv[1:14]

guff_m = json.loads(Path(guff_meas_path).read_text(encoding="utf-8"))
guff_keys = issue_keys(load_issues(guff_json), root)
packages = [p for p in packages_csv.split(",") if p]

if skip_gcl == "1":
    compat = {
        "guff_issues": len(guff_keys),
        "golangci_issues": 0,
        "both": 0,
        "guff_only": len(guff_keys),
        "golangci_only": 0,
        "precision": 1.0,
        "recall": 1.0,
    }
else:
    gcl_keys = issue_keys(load_issues(gcl_json), root)
    d = diff_sets("prometheus", guff_keys, gcl_keys, [])
    compat = {
        "guff_issues": len(d.guff),
        "golangci_issues": len(d.golangci),
        "both": len(d.both),
        "guff_only": len(d.guff_only),
        "golangci_only": len(d.golangci_only),
        "precision": d.precision,
        "recall": d.recall,
    }
    print(
        f"  guff={compat['guff_issues']} golangci={compat['golangci_issues']} "
        f"both={compat['both']} guff_only={compat['guff_only']} "
        f"golangci_only={compat['golangci_only']} "
        f"P={compat['precision']:.1%} R={compat['recall']:.1%}"
    )

measured = {
    "prometheus_git_sha": sha,
    "config": ".golangci.yml",
    "config_path": config,
    "packages": packages,
    "concurrency": int(jobs),
    "rayon_threads": int(rayon_threads),
    "isolate_gocache": isolate_gocache == "1",
    "guff": {
        "wall_seconds": float(guff_m["wall_seconds"]),
        "peak_rss_bytes": int(guff_m["peak_rss_bytes"]),
        "exit_code": int(guff_m["exit_code"]),
    },
    "compat": compat,
}
Path(out_path).write_text(json.dumps(measured, indent=2) + "\n", encoding="utf-8")
print(f"  wrote {out_path}")
PY

REPORT="$RUN_DIR/REPORT.md"

if [[ "$UPDATE_BASELINE" -eq 1 ]]; then
  python3 "$GATE" update-baseline --baseline "$BASELINE" --measured "$MEASURED"
  python3 "$GATE" check --baseline "$BASELINE" --measured "$MEASURED" --report "$REPORT"
  cp "$REPORT" "$RESULTS_DIR/RESULTS.md"
  echo
  echo "Baseline updated: $BASELINE"
  echo "Wrote $RESULTS_DIR/RESULTS.md"
  exit 0
fi

set +e
python3 "$GATE" check --baseline "$BASELINE" --measured "$MEASURED" --report "$REPORT"
gate_rc=$?
set -e
cp "$REPORT" "$RESULTS_DIR/RESULTS.md"
echo
echo "Wrote $REPORT"
echo "Wrote $RESULTS_DIR/RESULTS.md"
exit "$gate_rc"
