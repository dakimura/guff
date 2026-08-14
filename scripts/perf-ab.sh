#!/usr/bin/env bash
# scripts/perf-ab.sh — interleaved A/B of two guff binaries on one target.
#
# Why interleaved: `benchmarks/run.sh` times one binary's whole sweep before
# the other's, so anything that starts in between reads as a regression. On
# 2026-08-14 `mediaanalysisd` did exactly that and made helm look 47% slower
# than it was (docs/PERF_TASKS_V3.md §7.1.5). Alternating A/B/A/B lets the
# background load hit both sides equally: the absolutes still wander, the ratio
# does not.
#
# Three modes, because different changes show up in different numbers:
#
#   wall      what users feel. The default. Needs a reasonably quiet machine —
#             check with scripts/perf-guard.sh first.
#   cpu       user+sys summed. Nearly immune to other load, so it can resolve a
#             change that only removes work when wall cannot. Says nothing
#             about parallelism: removing a lock shows in wall, not here.
#   analyzer  one analyzer's summed CPU, from GUFF_DEBUG_CACHE=1. Use when the
#             change touches a single check — 0.08s inside ~18s is invisible in
#             the other two modes.
#
# Usage:
#   scripts/perf-ab.sh A B                        # wall, 5 rounds, prometheus ./...
#   scripts/perf-ab.sh A B --mode cpu --rounds 8
#   scripts/perf-ab.sh A B --mode analyzer --analyzer gocritic
#   scripts/perf-ab.sh A B --dir ~/src/helm --config .golangci.yml
#
# Env: GUFF_TARGET_DIR (default: the repo-root `prometheus` symlink)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() { echo "perf-ab: $*" >&2; exit 2; }

[[ $# -ge 2 ]] || die "need two binaries: perf-ab.sh <binA> <binB> [opts]"
A="$1"; B="$2"; shift 2
[[ -x "$A" ]] || die "not executable: $A"
[[ -x "$B" ]] || die "not executable: $B"

MODE=wall
ROUNDS=5
ANALYZER=gocritic
DIR="${GUFF_TARGET_DIR:-$ROOT/prometheus}"
CONFIG=.golangci.yml
PKGS="./..."

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode) MODE="$2"; shift 2 ;;
    --rounds) ROUNDS="$2"; shift 2 ;;
    --analyzer) ANALYZER="$2"; shift 2 ;;
    --dir) DIR="$2"; shift 2 ;;
    --config) CONFIG="$2"; shift 2 ;;
    --packages) PKGS="$2"; shift 2 ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) die "unknown arg: $1" ;;
  esac
done

[[ -d "$DIR" ]] || die "target dir not found: $DIR (set GUFF_TARGET_DIR or --dir)"
case "$MODE" in wall|cpu|analyzer) ;; *) die "unknown --mode: $MODE" ;; esac

# One cold run. Prints the mode's number on stdout.
# A fresh GUFF_CACHE per run is deliberate: it keeps `native_list`/`modmeta`
# cold so load_graph is measured, not skipped.
one() {
  local bin="$1" cache err
  cache="$(mktemp -d "${TMPDIR:-/tmp}/guff-ab.XXXXXX")"
  err="$(mktemp)"
  local -a env_extra=()
  [[ "$MODE" == analyzer ]] && env_extra=("GUFF_DEBUG_CACHE=1")
  (
    cd "$DIR"
    env "GUFF_CACHE=$cache" "GOLANGCI_LINT_CACHE=$cache" "${env_extra[@]+"${env_extra[@]}"}" \
      /usr/bin/time -l "$bin" run -c "$CONFIG" --out-format json \
      --issues-exit-code 0 --no-cache --timeout 15m $PKGS
  ) >/dev/null 2>"$err"
  case "$MODE" in
    wall)     awk '/ real / { printf "%.3f\n", $1 }' "$err" ;;
    cpu)      awk '/ real / { printf "%.2f\n", $3 + $5 }' "$err" ;;
    analyzer) awk -v n="$ANALYZER" '$1 == n { print $2; exit }' "$err" | tr -d 's' ;;
  esac
  rm -rf "$cache" "$err"
}

echo "perf-ab: mode=$MODE rounds=$ROUNDS dir=$DIR"
[[ "$MODE" == analyzer ]] && echo "  analyzer: $ANALYZER"
as=(); bs=()
for ((r = 1; r <= ROUNDS; r++)); do
  ta="$(one "$A")"; tb="$(one "$B")"
  [[ -n "$ta" && -n "$tb" ]] || die "no measurement parsed (analyzer '$ANALYZER' never ran?)"
  as+=("$ta"); bs+=("$tb")
  printf '  r%-2d A=%-8s B=%-8s\n' "$r" "$ta" "$tb"
done

python3 - "$MODE" "${as[*]}" "${bs[*]}" <<'PY'
import statistics, sys
mode = sys.argv[1]
a = [float(x) for x in sys.argv[2].split()]
b = [float(x) for x in sys.argv[3].split()]
ma, mb = statistics.median(a), statistics.median(b)
print(f"\nA {mode}: median {ma:.3f}  min {min(a):.3f}")
print(f"B {mode}: median {mb:.3f}  min {min(b):.3f}")
print(f"delta: {mb - ma:+.3f} ({(mb / ma - 1) * 100:+.1f}%)   min-to-min {min(b) - min(a):+.3f}")
if (mb - ma) * (min(b) - min(a)) < 0:
    print("\nWARNING: median and minimum disagree on the sign — this is noise, "
          "not a result. Re-run on a quieter machine, or use --mode cpu.")
PY
