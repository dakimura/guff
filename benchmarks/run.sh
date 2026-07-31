#!/usr/bin/env bash
# benchmarks/run.sh — wall-clock harness: guff vs golangci-lint.
#
# Usage:
#   ./benchmarks/run.sh              # fixture + benchmarks/local (standard.yml)
#   ./benchmarks/run.sh --smoke      # fixture only (offline, fast)
#   ./benchmarks/run.sh --oss --tier pr
#   ./benchmarks/run.sh --oss --tier pr,nightly
#   ./benchmarks/run.sh --quick      # 1 sample (default: 3)
#
# OSS targets use each checkout's real golangci-lint v2 config (via corpus/).
# Fixture / local keep benchmarks/standard.yml.
#
# Env:
#   GUFF_BIN / GOLANGCI_LINT_BIN / BENCH_SAMPLES / SKIP_GOLANGCI=1 / CORPUS_CACHE
#
# Output: benchmarks/results/<timestamp>.{tsv,md} and SCOREBOARD.md (when --oss)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BENCH_DIR="$ROOT/benchmarks"
CONFIG_STANDARD="$BENCH_DIR/standard.yml"
RESULTS_DIR="$BENCH_DIR/results"
PREPARE="$ROOT/corpus/prepare.sh"
PATCH_UNLIMITED="$ROOT/corpus/patch_unlimited_issues.py"
mkdir -p "$RESULTS_DIR"

SMOKE=0
OSS=0
SAMPLES="${BENCH_SAMPLES:-3}"
TIER="pr"
PERF_GATE=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --smoke) SMOKE=1; shift ;;
    --oss) OSS=1; shift ;;
    --quick) SAMPLES=1; shift ;;
    --tier)
      TIER="$2"
      shift 2
      ;;
    --tier=*)
      TIER="${1#*=}"
      shift
      ;;
    --no-perf-gate)
      PERF_GATE=0
      shift
      ;;
    -h|--help)
      sed -n '2,22p' "$0"
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
if [[ -z "$GOLANGCI" && "${SKIP_GOLANGCI:-0}" != "1" ]]; then
  echo "warn: golangci-lint not on PATH; timing guff only (set SKIP_GOLANGCI=1 to silence)" >&2
  SKIP_GOLANGCI=1
fi

command -v go >/dev/null 2>&1 || die "go not found"
command -v python3 >/dev/null 2>&1 || die "python3 not found"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
TSV="$RESULTS_DIR/${STAMP}.tsv"
HOST="$(uname -srm)"
GO_VER="$(go env GOVERSION 2>/dev/null || go version)"
GUFF_VER="$("$GUFF" version --short 2>/dev/null || echo unknown)"
GCL_VER="n/a"
if [[ "${SKIP_GOLANGCI:-0}" != "1" ]]; then
  GCL_VER="$("$GOLANGCI" version --short 2>/dev/null || "$GOLANGCI" version 2>/dev/null | head -1 || echo unknown)"
fi

echo -e "target\ttool\tmode\tsample\tseconds\texit_code\tissues\tok\tconfig" >"$TSV"

RESULT_LINE=""
time_cmd() {
  local target_dir="$1"
  shift
  local out_file status_file elapsed
  out_file="$(mktemp)"
  status_file="$(mktemp)"
  elapsed="$(
    cd "$target_dir"
    python3 -c '
import subprocess, sys, time
out, status = sys.argv[1], sys.argv[2]
cmd = sys.argv[3:]
t0 = time.perf_counter()
with open(out, "wb") as fh:
    p = subprocess.run(cmd, stdout=fh, stderr=subprocess.STDOUT)
open(status, "w").write(str(p.returncode))
print("%.6f" % (time.perf_counter() - t0))
' "$out_file" "$status_file" "$@"
  )"
  local exit_code issues
  exit_code="$(cat "$status_file")"
  issues="$(
    python3 - "$out_file" <<'PY'
import json, sys
raw = open(sys.argv[1], "rb").read()
text = raw.decode("utf-8", errors="replace")
candidates = [text]
brace = text.rfind("{")
if brace >= 0:
    candidates.append(text[brace:])
for c in candidates:
    try:
        data = json.loads(c)
        if isinstance(data, dict) and "Issues" in data:
            print(len(data["Issues"] or []))
            raise SystemExit
    except Exception:
        pass
if "panic" in text.lower() or "lint worker exited" in text:
    print(0)
else:
    print(sum(1 for line in raw.splitlines() if line.strip()))
PY
  )"
  rm -f "$out_file" "$status_file"
  local ok=0
  [[ "$exit_code" == "0" ]] && ok=1
  RESULT_LINE="${elapsed}"$'\t'"${exit_code}"$'\t'"${issues}"$'\t'"${ok}"
}

bench_one() {
  local target_name="$1"
  local target_dir="$2"
  local tool="$3"
  local mode="$4"
  local sample="$5"
  local cache_dir="$6"
  local config="$7"
  local packages="$8"
  local timeout="$9"
  local config_label="${10:-$config}"

  local -a cmd
  # shellcheck disable=SC2206
  local -a pkg_args=($packages)
  if [[ "$tool" == "guff" ]]; then
    cmd=(
      env "GUFF_CACHE=$cache_dir" "GOLANGCI_LINT_CACHE=$cache_dir"
      "$GUFF" run
      -c "$config"
      --out-format json
      --issues-exit-code 0
      --timeout "$timeout"
      "${pkg_args[@]}"
    )
  else
    cmd=(
      env "GOLANGCI_LINT_CACHE=$cache_dir" "GUFF_CACHE=$cache_dir"
      "$GOLANGCI" run
      -c "$config"
      --output.json.path=stdout
      --issues-exit-code 0
      --timeout="$timeout"
      --max-issues-per-linter=0
      --max-same-issues=0
      --allow-parallel-runners
      "${pkg_args[@]}"
    )
  fi

  time_cmd "$target_dir" "${cmd[@]}"
  local seconds exit_code issues ok
  IFS=$'\t' read -r seconds exit_code issues ok <<<"$RESULT_LINE"
  echo -e "${target_name}\t${tool}\t${mode}\t${sample}\t${seconds}\t${exit_code}\t${issues}\t${ok}\t${config_label}" >>"$TSV"
  local mark=""
  [[ "$ok" != "1" ]] && mark=" FAIL"
  printf '  %-16s %-7s #%d  %8ss  exit=%s  issues≈%s%s\n' \
    "$tool" "$mode" "$sample" "$seconds" "$exit_code" "$issues" "$mark"
}

bench_target() {
  local name="$1"
  local dir="$2"
  local config="$3"
  local packages="$4"
  local timeout="$5"
  echo "=== $name ($dir) ==="
  echo "  config: $config"
  echo "  packages: $packages  timeout: $timeout"

  local run_config
  run_config="$(mktemp "${TMPDIR:-/tmp}/guff-bench-config.XXXXXX.yml")"
  if [[ "$config" == "$CONFIG_STANDARD" ]]; then
    cp "$config" "$run_config"
  else
    python3 "$PATCH_UNLIMITED" "$config" -o "$run_config"
  fi

  local guff_cache gcl_cache
  guff_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-bench-guff.XXXXXX")"
  gcl_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-bench-gcl.XXXXXX")"

  local i
  for ((i = 1; i <= SAMPLES; i++)); do
    rm -rf "${guff_cache:?}"/*
    bench_one "$name" "$dir" "guff" "cold" "$i" "$guff_cache" "$run_config" "$packages" "$timeout" "$config"
  done
  for ((i = 1; i <= SAMPLES; i++)); do
    bench_one "$name" "$dir" "guff" "warm" "$i" "$guff_cache" "$run_config" "$packages" "$timeout" "$config"
  done

  if [[ "${SKIP_GOLANGCI:-0}" != "1" ]]; then
    for ((i = 1; i <= SAMPLES; i++)); do
      rm -rf "${gcl_cache:?}"/*
      bench_one "$name" "$dir" "golangci-lint" "cold" "$i" "$gcl_cache" "$run_config" "$packages" "$timeout" "$config"
    done
    for ((i = 1; i <= SAMPLES; i++)); do
      bench_one "$name" "$dir" "golangci-lint" "warm" "$i" "$gcl_cache" "$run_config" "$packages" "$timeout" "$config"
    done
  fi

  rm -rf "$guff_cache" "$gcl_cache" "$run_config"
}

echo "guff benchmark harness"
echo "  host:     $HOST"
echo "  go:       $GO_VER"
echo "  guff:     $GUFF_VER ($GUFF)"
echo "  golangci: $GCL_VER (${GOLANGCI:-skipped})"
echo "  samples:  $SAMPLES"
echo "  standard: $CONFIG_STANDARD"
echo "  tsv:      $TSV"
echo

bench_target "fixture" "$BENCH_DIR/fixture" "$CONFIG_STANDARD" "./..." "5m"

if [[ "$SMOKE" -eq 0 ]]; then
  bench_target "local" "$BENCH_DIR/local" "$CONFIG_STANDARD" "./..." "5m"
fi

if [[ "$OSS" -eq 1 ]]; then
  [[ -x "$PREPARE" ]] || die "missing $PREPARE"
  echo "Preparing OSS corpus (tier=$TIER)..."
  prep_list="$(mktemp "${TMPDIR:-/tmp}/guff-bench-prep.XXXXXX")"
  "$PREPARE" --tier "$TIER" >"$prep_list"
  while IFS=$'\t' read -r name dir config packages timeout tier; do
    [[ -z "${name:-}" ]] && continue
    bench_target "$name" "$dir" "$config" "$packages" "$timeout"
  done <"$prep_list"
  rm -f "$prep_list"
fi

SUMMARY_MD="$RESULTS_DIR/${STAMP}.md"
SCOREBOARD="$RESULTS_DIR/SCOREBOARD.md"
python3 - "$TSV" "$SUMMARY_MD" "$SCOREBOARD" "$HOST" "$GO_VER" "$GUFF_VER" "$GCL_VER" "$SAMPLES" "$OSS" "$PERF_GATE" <<'PY'
import collections, statistics, sys
from pathlib import Path

(
    tsv,
    out,
    scoreboard,
    host,
    go_ver,
    guff_ver,
    gcl_ver,
    samples,
    oss,
    perf_gate,
) = sys.argv[1:]
oss = oss == "1"
perf_gate = perf_gate == "1"

rows = []
configs = {}
with open(tsv, encoding="utf-8") as f:
    next(f)
    for line in f:
        parts = line.rstrip("\n").split("\t")
        if len(parts) < 8:
            continue
        if len(parts) >= 9:
            target, tool, mode, _sample, seconds, exit_code, issues, ok, config = parts[:9]
            configs[target] = config
        else:
            target, tool, mode, _sample, seconds, exit_code, issues, ok = parts
        rows.append((target, tool, mode, float(seconds), int(exit_code), issues, ok == "1"))

groups = collections.defaultdict(list)
ok_flags = collections.defaultdict(list)
targets = []
for target, tool, mode, sec, _ec, _iss, ok in rows:
    groups[(target, tool, mode)].append(sec)
    ok_flags[(target, tool, mode)].append(ok)
    if target not in targets:
        targets.append(target)

def med(vals):
    return statistics.median(vals) if vals else float("nan")

def all_ok(key):
    flags = ok_flags.get(key, [])
    return bool(flags) and all(flags)

def fmt(x, ok):
    if not ok:
        return "FAIL"
    return "—" if x != x else f"{x:.3f}s"

def fmtr(x, ok):
    if not ok or x != x:
        return "—"
    return f"{x:.2f}x"

def short_config(path: str) -> str:
    if not path:
        return "—"
    p = Path(path)
    # Prefer repo-relative style for OSS checkouts under corpus/cache/<name>/...
    parts = p.parts
    if "cache" in parts:
        i = parts.index("cache")
        if i + 2 < len(parts):
            return str(Path(*parts[i + 2 :]))
    return p.name

lines = [
    "# Benchmark results",
    "",
    f"- Host: `{host}`",
    f"- Go: `{go_ver}`",
    f"- guff: `{guff_ver}`",
    f"- golangci-lint: `{gcl_ver}`",
    f"- Samples per cell: {samples} (median reported; `FAIL` if any sample exited non-zero)",
    "- Fixture/local: `benchmarks/standard.yml` (standard five linters)",
    "- OSS: each repo's real golangci-lint v2 config (own-config)",
    "- Protocol: GOCACHE warm (prepare), tool caches cold then warm; clone/mod download excluded",
    "",
    "| Target | config | guff cold | guff warm | golangci cold | golangci warm | speedup (warm) |",
    "|--------|--------|----------:|----------:|--------------:|--------------:|---------------:|",
]

gate_failures = []
for t in targets:
    keys = {
        "gc": (t, "guff", "cold"),
        "gw": (t, "guff", "warm"),
        "cc": (t, "golangci-lint", "cold"),
        "cw": (t, "golangci-lint", "warm"),
    }
    gc, gw = med(groups.get(keys["gc"], [])), med(groups.get(keys["gw"], []))
    cc, cw = med(groups.get(keys["cc"], [])), med(groups.get(keys["cw"], []))
    ok_g = all_ok(keys["gc"]) and all_ok(keys["gw"])
    ok_c = all_ok(keys["cc"]) and all_ok(keys["cw"]) if keys["cc"] in groups or keys["cw"] in groups else False
    # speedup = golangci_warm / guff_warm  (>1 means guff faster)
    ratio_ok = ok_g and ok_c and cw == cw and gw == gw and gw > 0
    speedup = (cw / gw) if ratio_ok else float("nan")
    cfg = short_config(configs.get(t, ""))
    lines.append(
        f"| {t} | `{cfg}` | {fmt(gc, all_ok(keys['gc']))} | {fmt(gw, all_ok(keys['gw']))} | "
        f"{fmt(cc, all_ok(keys['cc']))} | {fmt(cw, all_ok(keys['cw']))} | {fmtr(speedup, ratio_ok)} |"
    )
    if perf_gate and t not in ("fixture", "local"):
        if not ok_g:
            gate_failures.append(f"{t}: guff failed")
        elif ratio_ok and speedup < 1.0:
            gate_failures.append(f"{t}: speedup {speedup:.2f}x < 1.0 (guff slower than golangci-lint)")

lines.append("")
lines.append(
    "Speedup = golangci warm / guff warm. Values `>1.0x` mean guff was faster. "
    "≈20x is a SCOREBOARD claim, not a hard CI fail threshold."
)
text = "\n".join(lines) + "\n"
Path(out).write_text(text, encoding="utf-8")
print(text)

if oss:
    sb = [
        "# OSS SCOREBOARD (guff vs golangci-lint, own-config)",
        "",
        f"- Host: `{host}`",
        f"- Go: `{go_ver}`",
        f"- guff: `{guff_ver}`",
        f"- golangci-lint: `{gcl_ver}`",
        f"- Samples: {samples} (median)",
        "- Both tools use each repository's real golangci-lint **v2** config.",
        "- GOCACHE warm; linter caches measured cold then warm; clone/mod excluded.",
        "",
        "| Target | config | guff warm | golangci warm | speedup |",
        "|--------|--------|----------:|--------------:|--------:|",
    ]
    for t in targets:
        if t in ("fixture", "local"):
            continue
        gw = med(groups.get((t, "guff", "warm"), []))
        cw = med(groups.get((t, "golangci-lint", "warm"), []))
        ok_g = all_ok((t, "guff", "warm"))
        ok_c = all_ok((t, "golangci-lint", "warm"))
        ratio_ok = ok_g and ok_c and gw == gw and cw == cw and gw > 0
        speedup = (cw / gw) if ratio_ok else float("nan")
        cfg = short_config(configs.get(t, ""))
        sb.append(
            f"| {t} | `{cfg}` | {fmt(gw, ok_g)} | {fmt(cw, ok_c)} | {fmtr(speedup, ratio_ok)} |"
        )
    sb.append("")
    sb.append(f"Full run detail: `{Path(out).name}`")
    sb.append("")
    Path(scoreboard).write_text("\n".join(sb) + "\n", encoding="utf-8")
    print(f"Wrote {scoreboard}")

if gate_failures:
    print("PERF GATE FAIL:", file=sys.stderr)
    for msg in gate_failures:
        print(f"  - {msg}", file=sys.stderr)
    raise SystemExit(1)
PY

echo "Wrote $TSV"
echo "Wrote $SUMMARY_MD"
if [[ "$OSS" -eq 1 ]]; then
  echo "Wrote $SCOREBOARD"
  cp "$SUMMARY_MD" "$RESULTS_DIR/RESULTS.md"
fi
