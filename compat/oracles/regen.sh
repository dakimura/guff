#!/usr/bin/env bash
# Regenerate the Go-stdlib ground truth consumed by guff's gostd ports.
#
# Each oracle is a Go program that runs the real standard library over a
# deterministic corpus and prints `input<TAB>error`. The Rust side replays the
# same corpus through its port and must agree on every row, so an expected value
# is never written by hand — the same rule the golden gate follows.
#
# Usage: compat/oracles/regen.sh [name ...]   (default: all)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"

ALL="gotime goquote goquote-table gourl"

dest_for() {
  case "$1" in
    gotime)        echo "crates/guff-staticcheck/tests/testdata/gostd/time_parse.tsv" ;;
    goquote)       echo "crates/guff-staticcheck/tests/testdata/gostd/quote.tsv" ;;
    goquote-table) echo "crates/guff-staticcheck/src/gostd/isprint_table.rs" ;;
    gourl)         echo "crates/guff-staticcheck/tests/testdata/gostd/url_parse.tsv" ;;
    *) return 1 ;;
  esac
}

# goquote-table is the one oracle whose output is source rather than testdata:
# strconv.IsPrint is pinned to Go's Unicode version and cannot be derived from a
# crate on a different one. The tsv from `goquote` is what gates it.
dir_for() {
  case "$1" in
    goquote-table) echo "goquote" ;;
    *) echo "$1" ;;
  esac
}

args_for() {
  case "$1" in
    goquote-table) echo "-rust" ;;
    *) echo "" ;;
  esac
}

names="$*"
[ -n "$names" ] || names="$ALL"

for name in $names; do
  if ! dest="$(dest_for "$name")"; then
    echo "unknown oracle: $name (known: $ALL)" >&2
    exit 2
  fi
  out="$repo/$dest"
  mkdir -p "$(dirname "$out")"
  echo "==> $name -> $dest"
  # shellcheck disable=SC2046  # args_for is empty or a single flag, on purpose
  (cd "$here/$(dir_for "$name")" && go run . $(args_for "$name")) > "$out"
  echo "    $(wc -l < "$out" | tr -d ' ') rows ($(go version | awk '{print $3}'))"
done
