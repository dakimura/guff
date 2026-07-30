#!/usr/bin/env bash
# scripts/findings-gen.sh — dump guff findings in the canonical sorted form used
# by the findings-identity check in docs/PERF_TASKS.md §2.1.
#
#   scripts/findings-gen.sh /tmp/before.txt [repo]
#
# `repo` defaults to the bundled prometheus checkout. Always runs cold
# (`--no-cache` in a throwaway GUFF_CACHE) so the output cannot come from a
# stale cache entry. Comparing counts is not enough: diff the files.

set -euo pipefail

OUT="${1:?usage: findings-gen.sh OUT [repo]}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="${2:-$ROOT/prometheus}"
GUFFBIN="$ROOT/target/release/guff"

cache="$(mktemp -d)"
trap 'rm -rf "$cache"' EXIT
json="$cache/findings.json"

# guff exits non-zero whenever it reports issues, which is the normal case here.
(cd "$REPO" && GUFF_CACHE="$cache" "$GUFFBIN" run --no-cache -c .golangci.yml \
  --out-format json ./... >"$json" 2>/dev/null) || true

python3 - "$json" "$OUT" <<'PY'
import json, sys

with open(sys.argv[1]) as fh:
    issues = json.load(fh).get("Issues") or []

lines = sorted(
    "{}:{}:{}:{}:{}".format(
        i["Pos"]["Filename"], i["Pos"]["Line"], i["Pos"]["Column"],
        i["FromLinter"], i["Text"],
    )
    for i in issues
)
with open(sys.argv[2], "w") as fh:
    fh.write("\n".join(lines) + ("\n" if lines else ""))
print(f"{len(lines)} findings -> {sys.argv[2]}")
PY
