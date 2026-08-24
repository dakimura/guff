#!/usr/bin/env bash
# compat/fix/regen.sh — re-record what `golangci-lint --fix` writes.
#
#   ./compat/fix/regen.sh            # all cases
#   ./compat/fix/regen.sh godot      # one case
#   ./compat/fix/regen.sh --pending  # re-record what *guff* writes for the gaps
#   ./compat/fix/regen.sh --pending godot
#
# Expectations are never hand-written: this runs the pinned golangci-lint with
# --fix and records the bytes it left behind. Review the resulting git diff —
# every changed line is a user-visible change to what a `--fix` run does to
# somebody's source file.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ "${1:-}" == "--pending" ]]; then
  shift
  if [[ $# -gt 0 ]]; then
    exec "$HERE/run.sh" --record-pending --case "$1"
  fi
  exec "$HERE/run.sh" --record-pending
fi
if [[ $# -gt 0 ]]; then
  exec "$HERE/run.sh" --regen --case "$1"
fi
exec "$HERE/run.sh" --regen
