#!/usr/bin/env bash
# Offline smoke for the R11 harness (no network, local fixture only).
# Exit 0 if guff cold+warm timings are recorded; golangci-lint is optional.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
if [[ ! -x target/release/guff ]]; then
  cargo build --release -p guff-lint
fi
exec ./benchmarks/run.sh --smoke --quick "$@"
