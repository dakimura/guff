#!/usr/bin/env bash
# regress/run.sh — Prometheus regression gate (local only).
#
# Measures guff wall-clock + peak RSS on a local prometheus checkout using
# prometheus's own .golangci.yml, diffs findings vs golangci-lint, and fails
# when metrics regress relative to a checked-in baseline.
#
# Profiles:
#   tsdb (default) — ./tsdb/... ; baseline.json ; ~12 GiB RSS kill
#   full           — ./...      ; baseline.full.json ; ~18 GiB RSS kill
#                    (warm GOCACHE; historically cold+parallel blew past 40GB)
#
# Concurrency defaults to auto (available_parallelism). Reuse system GOCACHE
# unless REGRESS_ISOLATE_GOCACHE=1 (memory-heavy; large hosts only).
#
# Usage:
#   ./regress/run.sh                         # tsdb profile gate
#   ./regress/run.sh --profile full          # full ./... gate
#   ./regress/run.sh --profile full --update-baseline
#   ./regress/run.sh --skip-golangci         # guff metrics only
#
# Env:
#   GUFF_BIN / GOLANGCI_LINT_BIN / PROMETHEUS_DIR
#   REGRESS_PACKAGES / REGRESS_JOBS / REGRESS_RAYON_THREADS
#   REGRESS_ISOLATE_GOCACHE / REGRESS_RSS_LIMIT_BYTES / REGRESS_TIMEOUT
#   REGRESS_PROFILE  (tsdb|full; overridden by --profile)
#
# Requires: release guff, golangci-lint, go, python3, /usr/bin/time
# Does NOT clone prometheus — set PROMETHEUS_DIR or keep repo-root `prometheus/` symlink.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REGRESS_DIR="$ROOT/regress"
MEASURE="$REGRESS_DIR/measure.py"
GATE="$REGRESS_DIR/gate.py"
NORMALIZE="$ROOT/compat/normalize.py"
RESULTS_DIR="$REGRESS_DIR/results"
mkdir -p "$RESULTS_DIR"

UPDATE_BASELINE=0
SKIP_GOLANGCI=0
PROFILE="${REGRESS_PROFILE:-tsdb}"

args=("$@")
i=0
while [[ $i -lt $# ]]; do
  arg="${args[$i]}"
  case "$arg" in
    --update-baseline) UPDATE_BASELINE=1 ;;
    --skip-golangci) SKIP_GOLANGCI=1 ;;
    --profile)
      i=$((i + 1))
      [[ $i -lt $# ]] || { echo "error: --profile needs a value (tsdb|full)" >&2; exit 2; }
      PROFILE="${args[$i]}"
      ;;
    --profile=*)
      PROFILE="${arg#--profile=}"
      ;;
    -h|--help)
      sed -n '2,32p' "$0"
      exit 0
      ;;
    *)
      echo "unknown arg: $arg" >&2
      exit 2
      ;;
  esac
  i=$((i + 1))
done

case "$PROFILE" in
  tsdb)
    DEFAULT_PACKAGES="./tsdb/..."
    DEFAULT_RSS_LIMIT=$((12 * 1024 * 1024 * 1024))
    BASELINE="$REGRESS_DIR/baseline.json"
    RESULTS_SNAPSHOT="$RESULTS_DIR/RESULTS.md"
    PROFILE_LABEL="tsdb (24GB-safe subtree)"
    ;;
  full)
    DEFAULT_PACKAGES="./..."
    DEFAULT_RSS_LIMIT=$((18 * 1024 * 1024 * 1024))
    BASELINE="$REGRESS_DIR/baseline.full.json"
    RESULTS_SNAPSHOT="$RESULTS_DIR/RESULTS.full.md"
    PROFILE_LABEL="full ./... (warm GOCACHE)"
    ;;
  *)
    echo "error: unknown profile '$PROFILE' (want tsdb|full)" >&2
    exit 2
    ;;
esac

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

# --- profile defaults (overridable via env) -----------------------------------
# Concurrency defaults to auto (available_parallelism); pin with REGRESS_JOBS /
# REGRESS_RAYON_THREADS if RSS climbs (e.g. REGRESS_JOBS=1 REGRESS_RAYON_THREADS=2).
PACKAGES_RAW="${REGRESS_PACKAGES:-$DEFAULT_PACKAGES}"
# shellcheck disable=SC2206
PACKAGES=($PACKAGES_RAW)
JOBS="${REGRESS_JOBS:-}"
RAYON_THREADS="${REGRESS_RAYON_THREADS:-}"
ISOLATE_GOCACHE="${REGRESS_ISOLATE_GOCACHE:-0}"
RSS_LIMIT="${REGRESS_RSS_LIMIT_BYTES:-$DEFAULT_RSS_LIMIT}"
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

JOBS_LABEL="${JOBS:-auto}"
RAYON_LABEL="${RAYON_THREADS:-auto}"

echo "guff prometheus regress harness"
echo "  profile:      $PROFILE — $PROFILE_LABEL"
echo "  host:         $(uname -srm)"
echo "  guff:         $GUFF_VER ($GUFF)"
echo "  golangci:     $GCL_VER"
echo "  prometheus:   $PROM"
echo "  sha:          $PROM_SHA"
echo "  config:       $CONFIG"
echo "  packages:     ${PACKAGES[*]}"
echo "  concurrency:  -j $JOBS_LABEL / RAYON_NUM_THREADS=$RAYON_LABEL"
echo "  isolate_gocache: $ISOLATE_GOCACHE"
echo "  rss_limit:    $RSS_LIMIT bytes"
echo "  baseline:     $BASELINE"
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
guff_env=(
  env
  "GUFF_CACHE=$guff_cache"
  "GOLANGCI_LINT_CACHE=$guff_cache"
  "GOCACHE=$GOCACHE_VALUE"
)
if [[ -n "$RAYON_THREADS" ]]; then
  guff_env+=("RAYON_NUM_THREADS=$RAYON_THREADS")
fi
guff_cmd=(
  "$GUFF" run
  -c "$CONFIG"
  --out-format json
  --issues-exit-code 0
  --no-cache
  --timeout "$TIMEOUT"
)
if [[ -n "$JOBS" ]]; then
  guff_cmd+=(-j "$JOBS")
fi
guff_cmd+=("${PACKAGES[@]}")
python3 "$MEASURE" run \
  --cwd "$PROM" \
  --stdout "$GUFF_JSON" \
  --stderr-out "$GUFF_TIME_ERR" \
  --json-out "$GUFF_MEAS" \
  --rss-limit-bytes "$RSS_LIMIT" \
  -- \
  "${guff_env[@]}" \
  "${guff_cmd[@]}"
guff_rc=$?
set -e
if [[ "$guff_rc" -ne 0 ]]; then
  echo "guff failed (exit $guff_rc); see $GUFF_TIME_ERR" >&2
  if python3 -c 'import json,sys; m=json.load(open(sys.argv[1])); sys.exit(0 if m.get("killed_for_rss") else 1)' "$GUFF_MEAS" 2>/dev/null; then
    echo "hint: RSS limit hit. Shrink REGRESS_PACKAGES further, set REGRESS_JOBS=1 REGRESS_RAYON_THREADS=2, or raise REGRESS_RSS_LIMIT_BYTES on a larger machine." >&2
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
# Record 0 for auto (omit -j / RAYON_NUM_THREADS).
JOBS_REC="${JOBS:-0}"
RAYON_REC="${RAYON_THREADS:-0}"
python3 - "$NORMALIZE" "$PROM" "$GUFF_JSON" "$GCL_JSON" "$GUFF_MEAS" "$MEASURED" \
  "$PROM_SHA" "$CONFIG" "$SKIP_GOLANGCI" "$PACKAGES_CSV" "$JOBS_REC" "$RAYON_REC" \
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
  cp "$REPORT" "$RESULTS_SNAPSHOT"
  echo
  echo "Baseline updated: $BASELINE"
  echo "Wrote $RESULTS_SNAPSHOT"
  exit 0
fi

set +e
python3 "$GATE" check --baseline "$BASELINE" --measured "$MEASURED" --report "$REPORT"
gate_rc=$?
set -e
cp "$REPORT" "$RESULTS_SNAPSHOT"
echo
echo "Wrote $REPORT"
echo "Wrote $RESULTS_SNAPSHOT"
exit "$gate_rc"
