#!/usr/bin/env bash
# Attribute the cost of guff's `go list` invocation to its parts.
#
# This is the GO/NO-GO harness for docs/PERF_TASKS_V2.md §C-3.0. It answers
# "which part of the `go list` wall time is work guff actually asked for?" by
# stripping one flag at a time off the exact command `golist.rs` builds.
#
# Run from a Go module root, with a warm GOCACHE and a quiet machine
# (PERF_TASKS_V2 §1.2). Variants are interleaved, never batched per variant
# (§X-3: batching invents regressions that are not there).
#
#   cd prometheus && N=3 TESTS=false ../scripts/golist-breakdown.sh
#
# Env: N (runs, default 3), TESTS (go list -test=, default false),
#      PATTERN (default ./...)
set -uo pipefail

N="${N:-3}"
TESTS="${TESTS:-false}"
PATTERN="${PATTERN:-./...}"

# Every field `JsonPackage` in crates/guff-packages/src/golist.rs deserializes.
# Keep in sync with that struct — a field missing here silently becomes empty.
FIELDS="Name,ImportPath,Error,Standard,Dir,GoFiles,IgnoredGoFiles,IgnoredOtherFiles"
FIELDS="$FIELDS,CFiles,CgoFiles,CXXFiles,MFiles,HFiles,FFiles,SFiles,SwigFiles"
FIELDS="$FIELDS,SwigCXXFiles,SysoFiles,EmbedFiles,EmbedPatterns,CompiledGoFiles"
FIELDS="$FIELDS,Export,DepOnly,Imports,ImportMap,Deps,Module,ForTest,Target"
FIELDS_NO_DEPS="${FIELDS/,Deps/}"

COMMON="-e -test=$TESTS -export=false -deps=true -find=false"

# The first five form a chain: each adds one thing to the line above it, so the
# delta column reads as a cost attribution. The last row is an aside (it branches
# off "+ guff's fields") and is excluded from the chain.
names=(
  "floor: go version"
  "resolve+load (ImportPath only)"
  "+ guff's fields"
  "+ -compiled=true"
  "+ -json (all fields) = CURRENT"
  "aside: guff's fields minus Deps"
)
argv=(
  "version"
  "list $COMMON -json=ImportPath -compiled=false -- $PATTERN"
  "list $COMMON -json=$FIELDS -compiled=false -- $PATTERN"
  "list $COMMON -json=$FIELDS -compiled=true -- $PATTERN"
  "list $COMMON -json -compiled=true -- $PATTERN"
  "list $COMMON -json=$FIELDS_NO_DEPS -compiled=false -- $PATTERN"
)
CHAIN=5

now() { python3 -c 'import time;print(time.time())'; }

best=(); bytes=(); runs=()
for i in "${!names[@]}"; do best[i]=999; runs[i]=""; done

tmp=$(mktemp)
for _ in $(seq 1 "$N"); do
  for i in "${!names[@]}"; do
    s=$(now)
    # shellcheck disable=SC2086
    go ${argv[i]} > "$tmp" 2>/dev/null
    e=$(now)
    el=$(python3 -c "print(f'{$e-$s:.3f}')")
    bytes[i]=$(wc -c < "$tmp" | tr -d ' ')
    best[i]=$(python3 -c "print(min('$el','${best[i]}',key=float))")
    runs[i]="${runs[i]} $el"
  done
done
rm -f "$tmp"

printf '\ngo list breakdown  (pattern=%s tests=%s runs=%s)\n\n' "$PATTERN" "$TESTS" "$N"
printf '%-34s %8s %12s   %s\n' "variant" "best(s)" "stdout(B)" "all runs"
prev=""
for i in "${!names[@]}"; do
  delta=""
  if [ "$i" -lt "$CHAIN" ] && [ -n "$prev" ]; then
    delta=$(python3 -c "print(f'{float(\"${best[i]}\")-float(\"$prev\"):+.3f}')")
  fi
  printf '%-34s %8s %12s  %6s  %s\n' \
    "${names[i]}" "${best[i]}" "${bytes[i]}" "$delta" "${runs[i]}"
  [ "$i" -lt "$CHAIN" ] && prev="${best[i]}"
done
echo
echo "The two deltas guff can reclaim without replacing go list: '+ -compiled=true'"
echo "and '+ -json (all fields)'. See docs/PERF_TASKS_V2.md §C-3a."
