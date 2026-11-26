#!/usr/bin/env bash
# Keeps `target/` from silently rotting into a several-hundred-thousand-file
# directory, which is what makes `cargo test` appear to hang forever.
#
# Cargo never garbage-collects `target/<profile>/deps` or `.fingerprint`: every
# rebuild leaves the previous `.rlib` / `.rmeta` / test binary behind, keyed by a
# fresh metadata hash. Nothing deletes them, so a long-lived checkout drifts from
# a few thousand entries to a few hundred thousand. Twice now this repo has hit
# 229k and 871k files under `target/debug/deps`, and a third time 172k.
#
# The symptom does not look like a full disk. It looks like a build that never
# starts: no `rustc` process is running at all, and `syspolicyd` — macOS's
# Gatekeeper daemon, which is consulted per executable — sits at the top of
# `ps -r`. That is the tell. If rustc is idle and syspolicyd is hot, the problem
# is the file count, not the code.
#
#   ./scripts/target-hygiene.sh          report, and fail past the threshold
#   ./scripts/target-hygiene.sh --prune  delete stale entries, then report
#   ./scripts/target-hygiene.sh --hook   silent unless bloated; emits the JSON a
#                                        Claude Code PreToolUse hook expects
#
# `--prune` removes `deps` / `.fingerprint` / `incremental` entries not touched
# in $KEEP_DAYS days. That is always safe: cargo rebuilds whatever it cannot
# find. It is not the same as `cargo clean`, which throws away the warm artifacts
# too and costs a full rebuild.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
LIMIT="${TARGET_DEPS_LIMIT:-40000}"
KEEP_DAYS="${TARGET_KEEP_DAYS:-7}"

prune=0
hook=0
case "${1:-}" in
  --prune) prune=1 ;;
  --hook)  hook=1 ;;
esac

count_deps() {
  local dir="$1"
  [[ -d "$dir" ]] || { echo 0; return; }
  # `ls | wc -l` rather than `find`: one readdir, no per-entry stat.
  ls "$dir" | wc -l | tr -d ' '
}

if [[ "$prune" -eq 1 ]]; then
  for profile in debug release; do
    for sub in deps .fingerprint incremental; do
      dir="$TARGET/$profile/$sub"
      [[ -d "$dir" ]] || continue
      before="$(count_deps "$dir")"
      find "$dir" -mindepth 1 -maxdepth 1 -mtime "+$KEEP_DAYS" -exec rm -rf {} + 2>/dev/null || true
      after="$(count_deps "$dir")"
      if [[ "$before" != "$after" ]]; then
        echo "pruned $profile/$sub: $before -> $after"
      fi
    done
  done
fi

status=0
report=""
for profile in debug release; do
  dir="$TARGET/$profile/deps"
  [[ -d "$dir" ]] || continue
  n="$(count_deps "$dir")"
  if [[ "$n" -gt "$LIMIT" ]]; then
    report+="target/$profile/deps: $n files (limit $LIMIT) — BLOATED"$'\n'
    status=1
  else
    report+="target/$profile/deps: $n files (limit $LIMIT)"$'\n'
  fi
done

# Hook mode says nothing when the tree is healthy — a hook that speaks on every
# build is a hook that gets ignored on the one build that mattered.
if [[ "$hook" -eq 1 ]]; then
  [[ "$status" -eq 0 ]] && exit 0
  msg="$report"$'\n'"This build will look like it hangs. Run ./scripts/target-hygiene.sh --prune first."
  MSG="$msg" python3 -c 'import json,os;m=os.environ["MSG"];print(json.dumps({"systemMessage":m,"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":m}}))'
  exit 0
fi

printf '%s' "$report"

if [[ "$status" -ne 0 ]]; then
  cat >&2 <<'MSG'

A build here will look like it hangs. Prune before running the test suite:

    ./scripts/target-hygiene.sh --prune

If that does not bring the count down (everything is recent), the directory has
genuinely earned its size and `rm -rf target/debug` is the answer — the release
tree is usually healthy and worth keeping.
MSG
fi
exit "$status"
