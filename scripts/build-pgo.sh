#!/usr/bin/env bash
# scripts/build-pgo.sh — build a Profile-Guided Optimization (PGO) release binary.
#
# See docs/PERF_TASKS_V2.md §A-8b and docs/DEVELOPMENT.md §9.5.
#
# PGO is a *local* speed-up. Never feed a PGO binary into regress --update-baseline:
# the gate always compares against a plain `cargo build --release` binary.
#
# Usage:
#   ./scripts/build-pgo.sh
#   ./scripts/build-pgo.sh /path/to/workload/dir   # default: ./prometheus
#
# Outputs:
#   target/release/guff              — PGO-optimized binary
#   target/release/guff.generic.bak  — plain release binary saved before PGO
#
# Requires: rustc with -Cprofile-generate/use, llvm-profdata (xcrun on macOS).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORKLOAD="${1:-$ROOT/prometheus}"
if [[ ! -d "$WORKLOAD" ]]; then
  echo "build-pgo: workload dir not found: $WORKLOAD" >&2
  echo "  pass a Go module with .golangci.yml (default: ./prometheus)" >&2
  exit 1
fi
if [[ ! -f "$WORKLOAD/.golangci.yml" ]]; then
  echo "build-pgo: missing $WORKLOAD/.golangci.yml" >&2
  exit 1
fi

PROFDIR="${GUFF_PGO_DIR:-/tmp/guff-pgo-data}"
rm -rf "$PROFDIR"
mkdir -p "$PROFDIR"

if command -v xcrun >/dev/null 2>&1 && xcrun --find llvm-profdata >/dev/null 2>&1; then
  LLVM_PROFDATA=(xcrun llvm-profdata)
elif command -v llvm-profdata >/dev/null 2>&1; then
  LLVM_PROFDATA=(llvm-profdata)
else
  echo "build-pgo: llvm-profdata not found (install Xcode CLT or LLVM)" >&2
  exit 1
fi

# Keep a plain release binary for A/B measurement / restore.
if [[ ! -x "$ROOT/target/release/guff.generic.bak" ]]; then
  if [[ -x "$ROOT/target/release/guff" ]]; then
    echo "build-pgo: saving current release binary → guff.generic.bak"
    cp -f "$ROOT/target/release/guff" "$ROOT/target/release/guff.generic.bak"
  else
    echo "build-pgo: building plain release (generic) for backup"
    cargo build --release -p guff-lint
    cp -f "$ROOT/target/release/guff" "$ROOT/target/release/guff.generic.bak"
  fi
fi

echo "build-pgo: [1/4] instrumented release build → profile-generate=$PROFDIR"
cargo clean -p guff-lint >/dev/null 2>&1 || true
# More value-profile counters avoids "Running out of static counters" during training.
RUSTFLAGS="-Cprofile-generate=${PROFDIR} -Cllvm-args=-vp-counters-per-site=10" \
  cargo build --release -p guff-lint

GUFFBIN="$ROOT/target/release/guff"

echo "build-pgo: [2/4] training workload (cold --no-cache + warm) in $WORKLOAD"
CACHE=$(mktemp -d)
cleanup() { rm -rf "$CACHE"; }
trap cleanup EXIT

(
  cd "$WORKLOAD"
  # guff exits non-zero when findings exist — that is expected during training.
  GUFF_CACHE="$CACHE" "$GUFFBIN" run --no-cache -c .golangci.yml ./... >/dev/null || true
  GUFF_CACHE="$CACHE" "$GUFFBIN" run -c .golangci.yml ./... >/dev/null || true
  GUFF_CACHE="$CACHE" "$GUFFBIN" run -c .golangci.yml ./... >/dev/null || true
)

echo "build-pgo: [3/4] merge profiles with ${LLVM_PROFDATA[*]}"
MERGED="$PROFDIR/merged.profdata"
# shellcheck disable=SC2046
set -- $(find "$PROFDIR" -name '*.profraw' 2>/dev/null)
if [[ $# -eq 0 ]]; then
  echo "build-pgo: no .profraw under $PROFDIR — was the instrumented binary run?" >&2
  ls -laR "$PROFDIR" >&2 || true
  exit 1
fi
"${LLVM_PROFDATA[@]}" merge -o "$MERGED" "$@"

echo "build-pgo: [4/4] optimized release build → profile-use=$MERGED"
cargo clean -p guff-lint >/dev/null 2>&1 || true
RUSTFLAGS="-Cprofile-use=${MERGED}" cargo build --release -p guff-lint

echo "build-pgo: done → $ROOT/target/release/guff"
echo "build-pgo: generic backup → $ROOT/target/release/guff.generic.bak"
echo "build-pgo: REMINDER — do NOT --update-baseline with this binary."
