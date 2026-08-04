#!/usr/bin/env bash
# Thin wrapper so PERF_GUARD does not match this process's argv for "cargo build".
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
exec ./regress/run.sh "$@"
