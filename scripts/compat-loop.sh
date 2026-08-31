#!/usr/bin/env bash
# scripts/compat-loop.sh — run guff at the 100-target compat goal, unattended.
#
#   ./scripts/compat-loop.sh            # until done, or 50 iterations
#   ./scripts/compat-loop.sh 5          # five iterations
#   ./scripts/compat-loop.sh 1 --dry    # print what one iteration would do
#
# One iteration = one task from `corpus/status.py next` = one pull request.
# The loop merges that PR itself once CI is green, so the next iteration starts
# from a main that already has the fix. That is the whole reason it converges:
# a task done on a branch nobody merged is a task the next iteration picks up
# again, having thrown away the first attempt.
#
# It stops, rather than continuing, when CI goes red. An unattended loop that
# works around its own broken commit is worse than one that waits: the second
# costs a morning, the first costs the reason anyone trusts the gate.
#
# State lives in files, not in a conversation — `docs/COMPAT-HARDENING.md` says
# a new session only needs to read it, and `corpus/status.json` is the queue.
# Every iteration is a fresh `claude -p` with no memory of the last one.
set -uo pipefail

ROOT="${COMPAT_LOOP_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$ROOT" || exit 1

# Re-exec from a copy outside the work tree.
#
# The loop's first act each iteration is `git checkout main`, which rewrites
# every file this script needs — including this script. bash reads a script
# incrementally, so a checkout mid-run can feed it the wrong bytes or, as
# happened the first time this was run, delete the prompt out from under it and
# leave the loop spinning on a file that is no longer there. Copying both out
# of the tree first makes the loop independent of whatever branch is checked
# out, which is the only way a thing that changes branches can be safe.
if [[ -z "${COMPAT_LOOP_REEXEC:-}" ]]; then
  _tmp="$(mktemp -d "${TMPDIR:-/tmp}/compat-loop.XXXXXX")"
  cp "$ROOT/scripts/compat-loop.sh" "$_tmp/compat-loop.sh" 2>/dev/null \
    || { echo "cannot copy self out of the work tree" >&2; exit 1; }
  cp "$ROOT/scripts/compat-loop-prompt.md" "$_tmp/prompt.md" 2>/dev/null \
    || { echo "missing scripts/compat-loop-prompt.md" >&2; exit 1; }
  export COMPAT_LOOP_REEXEC=1 COMPAT_LOOP_ROOT="$ROOT" COMPAT_LOOP_PROMPT="$_tmp/prompt.md"
  exec bash "$_tmp/compat-loop.sh" "$@"
fi

ITERATIONS="${1:-50}"
DRY=0
[[ "${2:-}" == "--dry" ]] && DRY=1

PROMPT_FILE="${COMPAT_LOOP_PROMPT:-$ROOT/scripts/compat-loop-prompt.md}"
LOG="$ROOT/compat-loop.log"
MODEL="${COMPAT_LOOP_MODEL:-opus}"
# Which Claude account the loop works as.
#
# This machine keeps more than one, selected by an alias rather than by the
# environment (`alias claude-personal="CLAUDE_CONFIG_DIR=~/.claude-personal
# claude …"`), so a script started from a plain terminal inherits nothing and
# `claude` looks in the default `~/.claude`, which has no credentials. The
# first run of this loop failed exactly there and reported it as "no pull
# request" — a login problem wearing the costume of an empty iteration.
#
# Pin it here instead of hoping the caller exported it.
export CLAUDE_CONFIG_DIR="${COMPAT_LOOP_CLAUDE_CONFIG_DIR:-${CLAUDE_CONFIG_DIR:-$HOME/.claude-personal}}"
# How long to wait for a pull request's checks. oss-pr alone runs ~15 minutes
# and the whole set has been ~25; 60 gives headroom without hanging forever.
CI_WAIT_MINUTES="${COMPAT_LOOP_CI_WAIT:-60}"
# The corpus grows by a clone plus its module downloads on every adoption.
MIN_FREE_GB="${COMPAT_LOOP_MIN_FREE_GB:-25}"
# How often to say the iteration is still alive. `claude -p` buffers its answer
# to the end, so the log standing still means nothing and the terminal is
# otherwise silent for half an hour at a time.
HEARTBEAT_SECONDS="${COMPAT_LOOP_HEARTBEAT:-300}"

log() { printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$LOG"; }
die() { log "STOP: $*"; exit 1; }

command -v claude >/dev/null || die "claude not on PATH"
command -v gh >/dev/null || die "gh not on PATH"
[[ -f "$PROMPT_FILE" ]] || die "missing $PROMPT_FILE"

free_gb() { df -g "$ROOT" | awk 'NR==2 {print $4}'; }

# Merge a pull request once every check has finished and none of them failed.
# Returns 0 merged, 1 still running / red, 2 no such PR.
land_pr() {
  local pr="$1" waited=0
  while :; do
    local states
    # Same silence as the claude phase, for the same reason: this polls every
    # thirty seconds and said nothing, so `claude exited 0` was the last line
    # for twenty minutes while CI ran. Report every heartbeat interval.
    if [[ $waited -gt 0 && $((waited % (HEARTBEAT_SECONDS / 30))) -eq 0 ]]; then
      log "  … waiting on PR #$pr checks ($((waited / 2))m)"
    fi
    states="$(gh pr checks "$pr" --json state --jq '.[].state' 2>/dev/null)" || return 2
    if [[ -z "$states" ]]; then
      sleep 30
      waited=$((waited + 1))
      [[ $waited -gt $((CI_WAIT_MINUTES * 2)) ]] && { log "PR #$pr: no checks after ${CI_WAIT_MINUTES}m"; return 1; }
      continue
    fi
    if grep -qE '^(PENDING|QUEUED|IN_PROGRESS)$' <<<"$states"; then
      sleep 30
      waited=$((waited + 1))
      if [[ $waited -gt $((CI_WAIT_MINUTES * 2)) ]]; then
        log "PR #$pr: still pending after ${CI_WAIT_MINUTES}m"
        return 1
      fi
      continue
    fi
    if grep -qE '^(FAILURE|ERROR|CANCELLED|TIMED_OUT|ACTION_REQUIRED)$' <<<"$states"; then
      log "PR #$pr: CI is red"
      return 1
    fi
    gh pr merge "$pr" --squash --delete-branch >>"$LOG" 2>&1 || return 1
    log "PR #$pr: merged"
    return 0
  done
}

# One cheap call before the first iteration. Everything downstream of a failed
# login looks like a task that produced nothing, and that is the one failure
# this loop must not spend iterations discovering.
probe="$(claude -p --model "$MODEL" 'reply with exactly: OK' 2>&1 | tr -d '\r\n')"
if [[ "$probe" != *OK* ]]; then
  die "claude is not usable as $CLAUDE_CONFIG_DIR — it said: ${probe:-<nothing>}"
fi

barren=0
log "=== compat-loop start (max $ITERATIONS iterations, model=$MODEL) ==="

for i in $(seq 1 "$ITERATIONS"); do
  log "----- iteration $i -----"

  # Dirty check *before* the checkout, not after: `git checkout main` on a dirty
  # tree either fails or carries the changes across, and both are worse than
  # refusing. This ran in the wrong order once and moved a person off their
  # branch mid-session.
  if ! git diff --quiet || ! git diff --cached --quiet; then
    die "working tree is dirty — commit or stash before starting the loop"
  fi
  here="$(git rev-parse --abbrev-ref HEAD)"
  [[ "$here" != "main" ]] && log "switching from $here to main"
  git checkout -q main || die "cannot check out main"
  git pull -q --ff-only || die "cannot fast-forward main"

  # The loop runs from main, so its own tooling has to be *on* main. The first
  # run of this script died here without saying so: status.py and the prompt
  # existed only on the branch of the pull request that added them, so the
  # checkout removed them and every iteration called claude with an empty task.
  [[ -x "$ROOT/corpus/status.py" ]] \
    || die "corpus/status.py is not on main — merge the pull request that adds it first"

  free="$(free_gb)"
  if [[ "$free" -lt "$MIN_FREE_GB" ]]; then
    die "only ${free}GB free, need ${MIN_FREE_GB}GB (the corpus grows on every adoption)"
  fi

  # cargo never garbage-collects `target/*/deps`, and every iteration runs the
  # whole test suite, so a long unattended run walks straight into the failure
  # `scripts/target-hygiene.sh` was written for: a build that never starts, no
  # rustc running at all, and macOS's `syspolicyd` at the top of `ps`. Prune the
  # stale entries; if everything is recent — which it will be, because this loop
  # is what made it recent — the debug tree has earned its size and has to go.
  # The release tree stays: it is small, it is warm, and the measurements use it.
  if [[ -x scripts/target-hygiene.sh ]]; then
    scripts/target-hygiene.sh --prune >>"$LOG" 2>&1 || true
    deps="$(ls target/debug/deps 2>/dev/null | wc -l | tr -d ' ')"
    if [[ "${deps:-0}" -gt "${TARGET_DEPS_LIMIT:-40000}" ]]; then
      log "target/debug/deps still $deps after pruning — removing target/debug"
      rm -rf target/debug
    fi
  fi

  ./corpus/status.py probe >>"$LOG" 2>&1
  if ./corpus/status.py check >>"$LOG" 2>&1; then
    log "GOAL REACHED: $(./corpus/status.py check)"
    exit 0
  fi
  if ! task="$(./corpus/status.py next)" || [[ -z "$task" ]]; then
    die "corpus/status.py next produced nothing — the queue is broken, not empty"
  fi
  log "task: $task"

  if [[ "$DRY" -eq 1 ]]; then
    log "(dry run — stopping before calling claude)"
    exit 0
  fi

  branch="loop/$(date -u +%Y%m%dT%H%M%SZ)"
  # `caffeinate` because an iteration is measured in tens of minutes and a
  # sleeping laptop is the most common way this loop dies.
  caffeinate -ims claude -p \
    --model "$MODEL" \
    --dangerously-skip-permissions \
    "$(sed "s|<<TASK>>|$task|; s|<<BRANCH>>|$branch|" "$PROMPT_FILE")" \
    >>"$LOG" 2>&1 &
  cpid=$!
  started=$SECONDS

  # `claude -p` writes its answer when it is finished, so the log not growing is
  # not a symptom and the process is the only honest liveness signal. Without
  # this the terminal says nothing for twenty to thirty minutes, and the only
  # way to tell a working iteration from a wedged one is `ps` — which is how
  # this was found.
  while kill -0 "$cpid" 2>/dev/null; do
    sleep "$HEARTBEAT_SECONDS"
    kill -0 "$cpid" 2>/dev/null || break
    log "  … working on '$task' ($(( (SECONDS - started) / 60 ))m elapsed)"
  done
  wait "$cpid"
  rc=$?
  log "claude exited $rc after $(( (SECONDS - started) / 60 ))m"

  pr="$(gh pr list --head "$branch" --json number --jq '.[0].number' 2>/dev/null)"
  if [[ $rc -ne 0 && ( -z "$pr" || "$pr" == "null" ) ]]; then
    # claude itself failed and nothing was pushed. That is a broken tool, not a
    # quiet iteration, and repeating it only produces the same error twice.
    tail -n 5 "$LOG" >&2
    die "claude exited $rc and landed nothing — see $LOG"
  fi
  if [[ -z "$pr" || "$pr" == "null" ]]; then
    barren=$((barren + 1))
    log "no pull request for $branch — nothing landed this iteration ($barren in a row)"
    # One is survivable: an iteration that measured a target and found it clean
    # may have had nothing to open a pull request about. Two in a row is not a
    # quiet iteration, it is a loop that cannot produce anything — which is
    # exactly what fifty four-second iterations looked like the first time.
    if [[ $barren -ge 2 ]]; then
      die "two iterations in a row produced no pull request — see $LOG"
    fi
    continue
  fi
  barren=0

  if ! land_pr "$pr"; then
    die "PR #$pr did not land — fix it, then re-run this script"
  fi
done

log "=== compat-loop finished $ITERATIONS iterations ==="
./corpus/status.py report | tee -a "$LOG"
