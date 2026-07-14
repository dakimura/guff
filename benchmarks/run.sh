#!/usr/bin/env bash
# benchmarks/run.sh — wall-clock harness: guff vs golangci-lint (standard preset).
#
# Usage:
#   ./benchmarks/run.sh              # fixture + benchmarks/local
#   ./benchmarks/run.sh --smoke      # fixture only (offline, fast)
#   ./benchmarks/run.sh --oss        # also clone/bench repos.txt (may FAIL on guff;
#                                    staticcheck→SSA gaps until R17)
#   ./benchmarks/run.sh --quick      # 1 sample (default: 3)
#
# Env:
#   GUFF_BIN / GOLANGCI_LINT_BIN / BENCH_CORPUS / BENCH_SAMPLES / SKIP_GOLANGCI=1
#
# Output: benchmarks/results/<timestamp>.{tsv,md}
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BENCH_DIR="$ROOT/benchmarks"
CORPUS="${BENCH_CORPUS:-$BENCH_DIR/corpus}"
CONFIG="$BENCH_DIR/standard.yml"
REPOS_FILE="$BENCH_DIR/repos.txt"
RESULTS_DIR="$BENCH_DIR/results"
mkdir -p "$CORPUS" "$RESULTS_DIR"

SMOKE=0
OSS=0
SAMPLES="${BENCH_SAMPLES:-3}"

for arg in "$@"; do
  case "$arg" in
    --smoke) SMOKE=1 ;;
    --oss) OSS=1 ;;
    --quick) SAMPLES=1 ;;
    -h|--help)
      sed -n '2,18p' "$0"
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

echo -e "target\ttool\tmode\tsample\tseconds\texit_code\tissues\tok" >"$TSV"

RESULT_LINE=""
time_cmd() {
  local target_dir="$1"
  shift
  local out_file status_file elapsed
  out_file="$(mktemp)"
  status_file="$(mktemp)"
  elapsed="$(
    cd "$target_dir"
    go list ./... >/dev/null 2>&1 || true
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
# Prefer counting non-log lines if JSON missing (treat as 0 on crash).
if "panic" in text.lower() or "lint worker exited" in text:
    print(0)
else:
    print(sum(1 for line in raw.splitlines() if line.strip()))
PY
  )"
  rm -f "$out_file" "$status_file"
  # ok=1 when exit 0 (issues-exit-code is forced to 0)
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

  local -a cmd
  if [[ "$tool" == "guff" ]]; then
    cmd=(
      env "GUFF_CACHE=$cache_dir" "GOLANGCI_LINT_CACHE=$cache_dir"
      "$GUFF" run
      -c "$CONFIG"
      --out-format json
      --issues-exit-code 0
      --timeout 5m
      ./...
    )
  else
    cmd=(
      env "GOLANGCI_LINT_CACHE=$cache_dir" "GUFF_CACHE=$cache_dir"
      "$GOLANGCI" run
      -c "$CONFIG"
      --output.json.path=stdout
      --issues-exit-code 0
      --timeout=5m
      ./...
    )
  fi

  time_cmd "$target_dir" "${cmd[@]}"
  local seconds exit_code issues ok
  IFS=$'\t' read -r seconds exit_code issues ok <<<"$RESULT_LINE"
  echo -e "${target_name}\t${tool}\t${mode}\t${sample}\t${seconds}\t${exit_code}\t${issues}\t${ok}" >>"$TSV"
  local mark=""
  [[ "$ok" != "1" ]] && mark=" FAIL"
  printf '  %-16s %-7s #%d  %8ss  exit=%s  issues≈%s%s\n' \
    "$tool" "$mode" "$sample" "$seconds" "$exit_code" "$issues" "$mark"
}

bench_target() {
  local name="$1"
  local dir="$2"
  echo "=== $name ($dir) ==="

  local guff_cache gcl_cache
  guff_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-bench-guff.XXXXXX")"
  gcl_cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-bench-gcl.XXXXXX")"

  local i
  for ((i = 1; i <= SAMPLES; i++)); do
    rm -rf "${guff_cache:?}"/*
    bench_one "$name" "$dir" "guff" "cold" "$i" "$guff_cache"
  done
  for ((i = 1; i <= SAMPLES; i++)); do
    bench_one "$name" "$dir" "guff" "warm" "$i" "$guff_cache"
  done

  if [[ "${SKIP_GOLANGCI:-0}" != "1" ]]; then
    for ((i = 1; i <= SAMPLES; i++)); do
      rm -rf "${gcl_cache:?}"/*
      bench_one "$name" "$dir" "golangci-lint" "cold" "$i" "$gcl_cache"
    done
    for ((i = 1; i <= SAMPLES; i++)); do
      bench_one "$name" "$dir" "golangci-lint" "warm" "$i" "$gcl_cache"
    done
  fi

  rm -rf "$guff_cache" "$gcl_cache"
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

echo "guff benchmark harness"
echo "  host:     $HOST"
echo "  go:       $GO_VER"
echo "  guff:     $GUFF_VER ($GUFF)"
echo "  golangci: $GCL_VER (${GOLANGCI:-skipped})"
echo "  samples:  $SAMPLES"
echo "  config:   $CONFIG"
echo "  tsv:      $TSV"
echo

bench_target "fixture" "$BENCH_DIR/fixture"

if [[ "$SMOKE" -eq 0 ]]; then
  bench_target "local" "$BENCH_DIR/local"
fi

if [[ "$OSS" -eq 1 ]]; then
  while read -r name url ref; do
    [[ -z "${name:-}" || "$name" == \#* ]] && continue
    dest="$(clone_repo "$name" "$url" "$ref")"
    bench_target "$name" "$dest"
  done <"$REPOS_FILE"
fi

SUMMARY_MD="$RESULTS_DIR/${STAMP}.md"
python3 - "$TSV" "$SUMMARY_MD" "$HOST" "$GO_VER" "$GUFF_VER" "$GCL_VER" "$SAMPLES" <<'PY'
import collections, statistics, sys
tsv, out, host, go_ver, guff_ver, gcl_ver, samples = sys.argv[1:]
rows = []
with open(tsv, encoding="utf-8") as f:
    next(f)
    for line in f:
        parts = line.rstrip("\n").split("\t")
        if len(parts) < 8:
            continue
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

lines = [
    "# Benchmark results",
    "",
    f"- Host: `{host}`",
    f"- Go: `{go_ver}`",
    f"- guff: `{guff_ver}`",
    f"- golangci-lint: `{gcl_ver}`",
    f"- Samples per cell: {samples} (median reported; `FAIL` if any sample exited non-zero)",
    "- Preset: standard five linters via `benchmarks/standard.yml`",
    "",
    "| Target | guff cold | guff warm | golangci cold | golangci warm | guff/gcl warm |",
    "|--------|----------:|----------:|--------------:|--------------:|--------------:|",
]
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
    ratio_ok = ok_g and ok_c and cw == cw and cw > 0
    ratio = (gw / cw) if ratio_ok else float("nan")
    lines.append(
        f"| {t} | {fmt(gc, all_ok(keys['gc']))} | {fmt(gw, all_ok(keys['gw']))} | "
        f"{fmt(cc, all_ok(keys['cc']))} | {fmt(cw, all_ok(keys['cw']))} | {fmtr(ratio, ratio_ok)} |"
    )
lines.append("")
lines.append("Ratio `<1.0x` means guff warm was faster than golangci-lint warm.")
lines.append("OSS targets often `FAIL` on guff until SSA gaps (R17) land; prefer `fixture` / `local`.")
text = "\n".join(lines) + "\n"
open(out, "w", encoding="utf-8").write(text)
print(text)
PY

echo "Wrote $TSV"
echo "Wrote $SUMMARY_MD"
