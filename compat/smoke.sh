#!/usr/bin/env bash
# Offline CI smoke for the R21 compat harness (fixture only).
# Requires golangci-lint on PATH. Exit 0 when fixture diffs ⊆ allowlist.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
if [[ ! -x target/release/guff ]]; then
  cargo build --release -p guff-lint
fi
exec ./compat/run.sh --smoke "$@"
