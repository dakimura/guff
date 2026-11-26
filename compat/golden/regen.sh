#!/usr/bin/env bash
# compat/golden/regen.sh — regenerate golden finding sets from golangci-lint.
#
#   ./compat/golden/regen.sh            # all cases
#   ./compat/golden/regen.sh gocritic   # one case
#
# Goldens are never hand-written: this runs the pinned golangci-lint and records
# what it actually reported. Review the resulting git diff — every changed line
# is a user-visible behaviour change on one side or the other.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ $# -gt 0 ]]; then
  exec "$HERE/run.sh" --regen --case "$1"
fi
exec "$HERE/run.sh" --regen
