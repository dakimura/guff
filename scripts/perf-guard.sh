#!/usr/bin/env bash
# scripts/perf-guard.sh — refuse to trust a perf measurement on a dirty machine.
#
# See docs/PERF_TASKS_V2.md §1.1/§1.2: on 2026-07-27 a background Chrome ate
# ~2 cores and every guff phase came out 2.2× slow — a full afternoon nearly
# lost chasing a regression that was really just CPU contention. This script
# turns the §1.2 manual checklist into one command so that never happens again.
#
# Exit status:
#   0  clean  — safe to measure
#   1  FAIL   — machine is contended; numbers will lie. Fix and re-run.
#   (WARN findings do not change the exit status; they are printed for context.)
#
# Usage:
#   scripts/perf-guard.sh          # check once, exit 0/1
#   scripts/perf-guard.sh --wait   # poll every 5s (max 5 min) until clean
#
# regress/run.sh calls this automatically; set PERF_GUARD=0 to skip it (CI,
# or a deliberate measurement where you accept the contention).
#
# This script NEVER kills anything (docs §S-1 "やってはいけない"): it warns and
# stops. Closing Chrome / waiting for a build is the operator's call.

set -euo pipefail

WAIT=0
for arg in "$@"; do
  case "$arg" in
    --wait) WAIT=1 ;;
    -h|--help) sed -n '2,26p' "$0"; exit 0 ;;
    *) echo "perf-guard: unknown arg: $arg" >&2; exit 2 ;;
  esac
done

# --- portable primitives (macOS + Linux) -------------------------------------
ncpu() {
  if command -v sysctl >/dev/null 2>&1 && sysctl -n hw.ncpu >/dev/null 2>&1; then
    sysctl -n hw.ncpu
  elif command -v nproc >/dev/null 2>&1; then
    nproc
  else
    echo 1
  fi
}

# 1-minute load average, portable.
load1() {
  # `uptime` tail looks like: "... load averages: 1.84 2.12 2.47" (macOS)
  # or "... load average: 0.52, 0.58, 0.59" (Linux). Normalize commas → spaces.
  uptime | sed 's/,/ /g' | awk '{ print $(NF-2) }'
}

NCPU="$(ncpu)"
# Threshold: load must be < ncpu/4 (10 cores → 2.5). Use awk for float math.
LOAD_LIMIT="$(awk -v n="$NCPU" 'BEGIN { printf "%.2f", n/4 }')"

check_once() {
  local fail=0 warn=0
  echo "perf-guard: host has $NCPU cores; load must stay under $LOAD_LIMIT"

  # --- (2) load average ------------------------------------------------------
  local l1; l1="$(load1)"
  if awk -v l="$l1" -v lim="$LOAD_LIMIT" 'BEGIN { exit !(l+0 > lim+0) }'; then
    echo "  FAIL  load average ${l1} > ${LOAD_LIMIT} (ncpu/4)"
    echo "        → something is eating CPU. Close it or wait for load to fall."
    fail=1
  else
    echo "  ok    load average ${l1} (< ${LOAD_LIMIT})"
  fi

  # --- (3) top CPU consumers that are not us ---------------------------------
  # Columns: %cpu then command. Skip our own perf toolchain by name.
  local hogs
  # `head` closing the pipe raises SIGPIPE in sort under `pipefail`; cap with awk instead.
  hogs="$(ps aux | sort -nrk3 | awk 'NR>1 && NR<=11 { print $3, $11 }')"
  local self_re='guff|cargo|rustc|perf-guard|/usr/bin/time'
  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    local cpu cmd
    cpu="${line%% *}"
    cmd="${line#* }"
    # strip to basename for readability
    local base="${cmd##*/}"
    if echo "$base" | grep -qiE "$self_re"; then
      continue
    fi
    if awk -v c="$cpu" 'BEGIN { exit !(c+0 > 50.0) }'; then
      echo "  FAIL  ${cpu}% ${base} (>50% and not ours)"
      echo "        → close it before measuring (docs §1.1: Chrome did exactly this)."
      fail=1
    elif awk -v c="$cpu" 'BEGIN { exit !(c+0 > 20.0) }'; then
      echo "  WARN  ${cpu}% ${base} (>20% and not ours)"
      warn=1
    fi
  done <<< "$hogs"

  # --- (4) macOS power / thermal state ---------------------------------------
  if command -v pmset >/dev/null 2>&1; then
    local lpm; lpm="$(pmset -g 2>/dev/null | awk '/lowpowermode/ { print $2 }')"
    if [[ -n "$lpm" && "$lpm" != "0" ]]; then
      echo "  FAIL  lowpowermode = $lpm (CPU is being throttled for battery)"
      echo "        → disable Low Power Mode in System Settings → Battery."
      fail=1
    fi
    # therm: CPU_Speed_Limit < 100 means the SoC is thermally throttled.
    local speed; speed="$(pmset -g therm 2>/dev/null | awk -F'= *' '/CPU_Speed_Limit/ { print $2 }')"
    if [[ -n "$speed" && "$speed" -lt 100 ]]; then
      echo "  FAIL  CPU_Speed_Limit = ${speed}% (thermal throttling)"
      echo "        → let the machine cool down before measuring."
      fail=1
    fi
  fi

  # --- (5) active build / agent processes ------------------------------------
  # A running cargo/rustc/go build steals every core; that is a hard FAIL.
  local builds
  builds="$(ps aux | grep -iE 'cargo (build|test|run)|[r]ustc|go build|go test' | grep -v grep || true)"
  if [[ -n "$builds" ]]; then
    echo "  FAIL  a build is running (cargo/rustc/go). It will contend for all cores:"
    echo "$builds" | awk '{ print "        ", $11, $12, $13 }' | head -3
    echo "        → wait for it to finish (docs §0-7)."
    fail=1
  fi
  # cursor-agent worker is persistently resident in this repo (it hosts the
  # session). Its mere presence is not a FAIL — but per docs §44 it can fire
  # cargo build unpredictably, so surface it as a WARN. An *active* one is
  # already caught by the >50% CPU check above.
  if ps aux | grep -iE '[c]ursor-agent .*worker' >/dev/null 2>&1; then
    echo "  WARN  cursor-agent worker present — it can spawn cargo build without warning."
    warn=1
  fi

  if [[ "$fail" -ne 0 ]]; then
    return 1
  fi
  if [[ "$warn" -ne 0 ]]; then
    echo "perf-guard: PASS with warnings — numbers are probably usable, stay alert."
  else
    echo "perf-guard: PASS — machine is clean."
  fi
  return 0
}

if [[ "$WAIT" -eq 0 ]]; then
  check_once
  exit $?
fi

# --wait: retry every 5s up to 5 minutes.
deadline=$(( $(date +%s) + 300 ))
while :; do
  if check_once; then
    exit 0
  fi
  now=$(date +%s)
  if [[ "$now" -ge "$deadline" ]]; then
    echo "perf-guard: still dirty after 5 minutes — giving up." >&2
    exit 1
  fi
  echo "perf-guard: waiting 5s for the machine to settle…"
  sleep 5
done
