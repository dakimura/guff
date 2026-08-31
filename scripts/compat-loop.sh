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

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

ITERATIONS="${1:-50}"
DRY=0
[[ "${2:-}" == "--dry" ]] && DRY=1

PROMPT_FILE="$ROOT/scripts/compat-loop-prompt.md"
LOG="$ROOT/compat-loop.log"
MODEL="${COMPAT_LOOP_MODEL:-opus}"
# How long to wait for a pull request's checks. oss-pr alone runs ~15 minutes
# and the whole set has been ~25; 60 gives headroom without hanging forever.
CI_WAIT_MINUTES="${COMPAT_LOOP_CI_WAIT:-60}"
# The corpus grows by a clone plus its module downloads on every adoption.
MIN_FREE_GB="${COMPAT_LOOP_MIN_FREE_GB:-25}"

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

  free="$(free_gb)"
  if [[ "$free" -lt "$MIN_FREE_GB" ]]; then
    die "only ${free}GB free, need ${MIN_FREE_GB}GB (the corpus grows on every adoption)"
  fi

  ./corpus/status.py probe >>"$LOG" 2>&1
  if ./corpus/status.py check >>"$LOG" 2>&1; then
    log "GOAL REACHED: $(./corpus/status.py check)"
    exit 0
  fi
  task="$(./corpus/status.py next)"
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
    --permission-mode bypassPermissions \
    "$(sed "s|<<TASK>>|$task|; s|<<BRANCH>>|$branch|" "$PROMPT_FILE")" \
    >>"$LOG" 2>&1
  rc=$?
  log "claude exited $rc"

  pr="$(gh pr list --head "$branch" --json number --jq '.[0].number' 2>/dev/null)"
  if [[ -z "$pr" || "$pr" == "null" ]]; then
    log "no pull request for $branch — nothing landed this iteration"
    # Not fatal: an iteration that measured a target and found it clean has
    # nothing to open a PR about except the ledger, and may have pushed that
    # to a differently named branch. Carry on; `status.py` is the judge.
    continue
  fi

  if ! land_pr "$pr"; then
    die "PR #$pr did not land — fix it, then re-run this script"
  fi
done

log "=== compat-loop finished $ITERATIONS iterations ==="
./corpus/status.py report | tee -a "$LOG"
