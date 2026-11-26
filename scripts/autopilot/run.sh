#!/usr/bin/env bash
# scripts/autopilot/run.sh — unattended development loop for guff.
#
#   scripts/autopilot/run.sh start     # copy scripts to $STATE/run, launch the loop detached
#   scripts/autopilot/run.sh stop      # ask the loop to stop after the current step
#   scripts/autopilot/run.sh status    # what it is doing, last results
#   scripts/autopilot/run.sh once      # one iteration in the foreground (debugging)
#   scripts/autopilot/run.sh prompt    # print the prompt the next worker would get
#
# One iteration = one fresh `claude -p` session working in a dedicated git worktree
# on exactly one task, ending with a pull request (see prompt.md). The *driver*
# — this script, no model involved — then waits for that PR's CI and squash-merges
# it only when every check that ran is green on the PR's current head. A red PR is
# left open; the next iteration sees it in its signals and fixes it first (P0).
#
# Why the split: waiting for CI costs no tokens here, the merge rule is applied the
# same way every time, and a worker cannot talk itself into merging red.
#
# Usage limits: when a session ends on a usage/rate limit the loop sleeps until the
# reset (or 30 min) and continues — it runs until `stop`, MAX_ITERS, or
# MAX_FAILS consecutive non-limit failures.
#
# Env (all optional):
#   AUTOPILOT_STATE      state dir (default ~/.guff-autopilot): journal, logs, STOP
#   AUTOPILOT_WORKTREE   worker checkout (default <repo>/../guff-autopilot)
#   AUTOPILOT_MODEL      passed to claude --model
#   AUTOPILOT_MAX_ITERS  default 10000
#   AUTOPILOT_ITER_HOURS wall-clock cap per iteration (default 5)
#   AUTOPILOT_CI_HOURS   how long to wait for a PR's CI (default 3)
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STATE="${AUTOPILOT_STATE:-$HOME/.guff-autopilot}"
RUN_DIR="$STATE/run"
mkdir -p "$STATE/logs"

# The main checkout is wherever these scripts came from; when running from the
# copied $RUN_DIR it is recorded in $STATE/main-checkout.
if [[ -f "$SELF_DIR/../../Cargo.toml" ]]; then
  MAIN="$(cd "$SELF_DIR/../.." && pwd)"
else
  MAIN="$(cat "$STATE/main-checkout")"
fi
WT="${AUTOPILOT_WORKTREE:-$(dirname "$MAIN")/guff-autopilot}"
REPO="$(git -C "$MAIN" remote get-url origin | sed -E 's#(git@github.com:|https://github.com/)##; s#\.git$##')"
MEMORY_DIR="${AUTOPILOT_MEMORY_DIR:-$HOME/.claude-personal/projects/$(echo "$MAIN" | sed 's#[/.]#-#g')/memory}"
MAX_ITERS="${AUTOPILOT_MAX_ITERS:-10000}"
ITER_SECS=$(( ${AUTOPILOT_ITER_HOURS:-5} * 3600 ))
CI_SECS=$(( ${AUTOPILOT_CI_HOURS:-3} * 3600 ))
MAX_FAILS=5

log()  { echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*"; }
note() { echo "- $(date '+%m-%d %H:%M') $*" >> "$STATE/driver-notes.md"; log "NOTE: $*"; }
status() { echo "$(date '+%Y-%m-%d %H:%M:%S') $*" > "$STATE/status.txt"; }
stopping() { [[ -f "$STATE/STOP" ]]; }

# Sleep in small steps so `stop` is honoured promptly.
nap() {
  local end=$(( $(date +%s) + $1 ))
  while (( $(date +%s) < end )); do
    stopping && return 1
    sleep 30
  done
  return 0
}

ensure_worktree() {
  if [[ ! -e "$WT/.git" ]]; then
    log "creating worktree $WT"
    git -C "$MAIN" fetch -q origin
    git -C "$MAIN" worktree add --detach "$WT" origin/main || return 1
  fi
  # Untracked, machine-local inputs the gates need. Shared read-mostly with the
  # main checkout; the loop is serial so nothing writes them concurrently.
  local p
  for p in compat/.tools corpus/cache benchmarks/corpus prometheus; do
    if [[ -e "$MAIN/$p" && ! -e "$WT/$p" ]]; then
      mkdir -p "$(dirname "$WT/$p")"
      ln -s "$(cd "$MAIN/$p" && pwd -P)" "$WT/$p"
    fi
    # .gitignore's `/dir/` patterns do not match a symlink; without this every
    # iteration would stash the links as leftovers.
    local ex; ex="$(git -C "$MAIN" rev-parse --git-common-dir)/info/exclude"
    [[ "$ex" == /* ]] || ex="$MAIN/$ex"
    grep -qxF "/$p" "$ex" 2>/dev/null || echo "/$p" >> "$ex"
  done
}

# Put the worker checkout on a fresh detached origin/main. Leftovers from a
# crashed iteration are stashed, never discarded.
reset_worktree() {
  git -C "$WT" fetch -q --prune origin || return 1
  if [[ -n "$(git -C "$WT" status --porcelain)" ]]; then
    local tag="autopilot-leftover-$(date +%Y%m%dT%H%M%S)"
    git -C "$WT" stash push -u -m "$tag" >/dev/null && note "stashed leftovers in worktree as '$tag'"
  fi
  git -C "$WT" switch -q --detach origin/main || return 1
}

render_prompt() {  # $1 = iteration label; prints the prompt
  local signals
  signals="$(cd "$WT" && AUTOPILOT_STATE="$STATE" "$RUN_DIR/signals.sh" 2>&1)"
  SIGNALS="$signals" ITER_LABEL="$1" python3 - "$RUN_DIR/prompt.md" <<EOF
import os, sys
s = open(sys.argv[1]).read()
for k, v in {
    "MEMORY_DIR": "$MEMORY_DIR", "STATE_DIR": "$STATE", "REPO": "$REPO",
    "GATE": "$RUN_DIR/gate.sh", "ITER_LABEL": os.environ["ITER_LABEL"],
    "SIGNALS": os.environ["SIGNALS"],
}.items():
    s = s.replace("{{" + k + "}}", v)
print(s)
EOF
}

worker_settings() {
  cat <<EOF
{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[
  {"type":"command","command":"python3 $RUN_DIR/guard.py","timeout":10},
  {"type":"command","if":"Bash(cargo *)","command":"$WT/scripts/target-hygiene.sh --hook","timeout":15}
]}]}}
EOF
}

# Runs one worker session. Sets RESULT_LINE, RESULT_KIND, LIMIT_UNTIL.
run_worker() {
  local label="$1" logf="$2" prompt claude_bin pid t0
  prompt="$(render_prompt "$label")"
  echo "$prompt" > "${logf%.jsonl}.prompt.md"
  claude_bin="$(command -v claude)"
  local args=(-p "$prompt" --dangerously-skip-permissions --output-format stream-json --verbose
              --settings "$(worker_settings)" --add-dir "$MEMORY_DIR" --add-dir "$STATE")
  [[ -n "${AUTOPILOT_MODEL:-}" ]] && args+=(--model "$AUTOPILOT_MODEL")
  t0=$(date +%s)
  ( cd "$WT" && exec "$claude_bin" "${args[@]}" ) > "$logf" 2>&1 < /dev/null &
  pid=$!
  echo "$pid" > "$STATE/worker.pid"
  while kill -0 "$pid" 2>/dev/null; do
    if (( $(date +%s) - t0 > ITER_SECS )); then
      note "$label exceeded ${AUTOPILOT_ITER_HOURS:-5}h; terminating worker"
      kill -TERM "$pid" 2>/dev/null; sleep 20; kill -KILL "$pid" 2>/dev/null
      break
    fi
    if [[ -f "$STATE/STOP-NOW" ]]; then
      kill -TERM "$pid" 2>/dev/null; break
    fi
    sleep 15
  done
  wait "$pid" 2>/dev/null
  rm -f "$STATE/worker.pid"

  eval "$(python3 - "$logf" "$t0" <<'EOF'
import json, re, shlex, sys, time
path, t0 = sys.argv[1], int(sys.argv[2])
result, is_err, cost, turns, raw = None, True, 0.0, 0, open(path, errors="replace").read()
for line in raw.splitlines():
    try:
        ev = json.loads(line)
    except Exception:
        continue
    if ev.get("type") == "result":
        result = ev.get("result") or ""
        is_err = bool(ev.get("is_error"))
        cost = ev.get("total_cost_usd") or 0.0
        turns = ev.get("num_turns") or 0
text = result if result is not None else raw[-4000:]
m = None
for l in (result or "").splitlines():
    if l.strip().startswith("AUTOPILOT_RESULT:"):
        m = l.strip()
limit_until = 0
if re.search(r"usage limit|rate limit|hit your limit|limit reached|resets? (at )?\d|overloaded", text, re.I) and (is_err or not m):
    e = re.search(r"\|(\d{10})\b", text)
    limit_until = int(e.group(1)) if e else int(time.time()) + 1800
kind = "error"
if m:
    kind = m.split()[1] if len(m.split()) > 1 else "error"
elif limit_until:
    kind = "limit"
last = text.strip().splitlines()[-1][:300] if text.strip() else ""
print("RESULT_LINE=" + shlex.quote(m or last))
print("RESULT_KIND=" + shlex.quote(kind))
print("LIMIT_UNTIL=%d" % limit_until)
print("COST=%s TURNS=%s" % (cost, turns))
EOF
)"
  printf '%s\t%s\t%ss\t$%s\t%s turns\t%s\n' "$(date '+%F %T')" "$label" "$(( $(date +%s) - t0 ))" \
    "${COST:-0}" "${TURNS:-0}" "$RESULT_LINE" >> "$STATE/iterations.tsv"
}

# Wait for a PR's checks; merge when every check that ran passed on the current head.
# Returns 0 merged, 1 red/blocked (left open), 2 stopped.
wait_and_merge() {
  local n="$1" t0 js verdict head branch
  t0=$(date +%s)
  status "waiting for CI on #$n"
  while :; do
    stopping && return 2
    if (( $(date +%s) - t0 > CI_SECS )); then
      note "PR #$n: CI did not finish within ${AUTOPILOT_CI_HOURS:-3}h; left open"
      return 1
    fi
    js="$(gh pr view "$n" -R "$REPO" --json state,isDraft,headRefOid,headRefName,mergeable,statusCheckRollup 2>/dev/null)" \
      || { nap 60 || return 2; continue; }
    verdict="$(echo "$js" | python3 -c '
import json, sys, time
p = json.load(sys.stdin)
if p["state"] != "OPEN":
    print("closed " + p["state"]); sys.exit()
if p["isDraft"]:
    print("draft"); sys.exit()
checks = p.get("statusCheckRollup") or []
if not checks:
    print("nochecks"); sys.exit()
pend, bad, ok = [], [], []
for c in checks:
    name = c.get("name") or c.get("context") or "?"
    if c.get("__typename") == "StatusContext":
        st = c.get("state")
        (ok if st == "SUCCESS" else pend if st in ("PENDING", "EXPECTED") else bad).append(name)
        continue
    if c.get("status") != "COMPLETED":
        pend.append(name)
    elif c.get("conclusion") in ("SUCCESS", "SKIPPED", "NEUTRAL"):
        ok.append(name)
    else:
        bad.append(name + ":" + str(c.get("conclusion")))
if pend:
    print("pending " + ",".join(pend))
elif bad:
    print("red " + ",".join(bad))
elif p.get("mergeable") == "CONFLICTING":
    print("conflict")
else:
    print("green")
')"
    head="$(echo "$js" | python3 -c 'import json,sys;print(json.load(sys.stdin)["headRefOid"])')"
    branch="$(echo "$js" | python3 -c 'import json,sys;print(json.load(sys.stdin)["headRefName"])')"
    case "$verdict" in
      green)
        log "PR #$n green at ${head:0:10}; merging"
        if gh pr merge "$n" -R "$REPO" --squash --match-head-commit "$head"; then
          sleep 5
          if [[ "$(gh pr view "$n" -R "$REPO" --json state --jq .state)" == "MERGED" ]]; then
            note "merged PR #$n ($branch) at ${head:0:10}"
            git -C "$MAIN" push -q origin --delete "$branch" 2>/dev/null || true
            return 0
          fi
        fi
        note "PR #$n: merge command failed; left open for the next iteration"
        return 1 ;;
      red*)      note "PR #$n CI red on ${head:0:10}: ${verdict#red }"; return 1 ;;
      conflict)  note "PR #$n conflicts with main; rebase onto origin/main and push (no force: merge main in)"; return 1 ;;
      draft)     note "PR #$n is draft; not merging"; return 1 ;;
      closed*)   note "PR #$n is ${verdict#closed }"; return 0 ;;
      nochecks)  if (( $(date +%s) - t0 > 900 )); then note "PR #$n: no checks started in 15 min"; return 1; fi
                 nap 60 || return 2 ;;
      *)         nap 120 || return 2 ;;
    esac
  done
}

# Open autopilot PRs left by a previous run (e.g. after a restart) get one CI pass.
drain_open_prs() {
  local n
  for n in $(gh pr list -R "$REPO" --state open --json number,headRefName,isDraft \
              --jq '.[] | select(.headRefName|startswith("autopilot/")) | select(.isDraft|not) | .number'); do
    wait_and_merge "$n"
  done
}

loop() {
  if ! mkdir "$STATE/lock.d" 2>/dev/null; then
    if kill -0 "$(cat "$STATE/lock.d/pid" 2>/dev/null)" 2>/dev/null; then
      log "another driver is running (pid $(cat "$STATE/lock.d/pid"))"; exit 1
    fi
  fi
  echo $$ > "$STATE/lock.d/pid"
  trap 'rm -rf "$STATE/lock.d"; status "stopped"' EXIT
  rm -f "$STATE/STOP" "$STATE/STOP-NOW"
  [[ -f "$STATE/journal.md" ]] || printf '# guff autopilot journal\n\n' > "$STATE/journal.md"
  ensure_worktree || { log "worktree setup failed"; exit 1; }
  log "driver up: repo=$REPO worktree=$WT state=$STATE"

  local i=0 fails=0 idle=0 label logf pr
  drain_open_prs
  while (( i < MAX_ITERS )); do
    stopping && break
    i=$((i + 1))
    label="iter-$(date +%Y%m%d-%H%M)"
    logf="$STATE/logs/$label.jsonl"
    reset_worktree || { note "worktree reset failed"; nap 600 || break; continue; }
    status "$label running (worker log $logf)"
    log "$label start"
    run_worker "$label" "$logf"
    log "$label -> [$RESULT_KIND] $RESULT_LINE (cost \$${COST:-?}, ${TURNS:-?} turns)"

    case "$RESULT_KIND" in
      pr-open)
        fails=0; idle=0
        pr="$(echo "$RESULT_LINE" | sed -nE 's/.*#([0-9]+).*/\1/p' | head -1)"
        if [[ -n "$pr" ]]; then wait_and_merge "$pr"; [[ $? == 2 ]] && break
        else note "$label said pr-open without a number: $RESULT_LINE"; fi ;;
      no-task)
        fails=0; idle=$((idle + 1))
        status "idle (no task) — next check in $(( idle * 2 ))h"
        nap $(( idle > 6 ? 43200 : idle * 7200 )) || break ;;
      blocked)
        fails=0
        note "$label blocked: $RESULT_LINE" ;;
      limit)
        local now; now=$(date +%s)
        local wait=$(( LIMIT_UNTIL > now ? LIMIT_UNTIL - now + 120 : 1800 ))
        status "usage limit — sleeping until $(date -r $((now + wait)) '+%F %T')"
        log "usage limit; sleeping ${wait}s"
        nap "$wait" || break ;;
      *)
        fails=$((fails + 1))
        note "$label ended without a result line ($fails/$MAX_FAILS): $RESULT_LINE"
        (( fails >= MAX_FAILS )) && { note "too many consecutive failures; driver exiting"; break; }
        nap $(( fails * 300 )) || break ;;
    esac
  done
  log "driver exiting after $i iteration(s)"
}

case "${1:-}" in
  start)
    mkdir -p "$RUN_DIR"
    cp "$SELF_DIR"/run.sh "$SELF_DIR"/gate.sh "$SELF_DIR"/signals.sh "$SELF_DIR"/guard.py "$SELF_DIR"/prompt.md "$RUN_DIR"/
    echo "$MAIN" > "$STATE/main-checkout"
    nohup caffeinate -ims bash "$RUN_DIR/run.sh" loop >> "$STATE/driver.log" 2>&1 &
    echo "started (pid $!). log: $STATE/driver.log  status: $0 status  stop: $0 stop" ;;
  stop)
    touch "$STATE/STOP"
    [[ "${2:-}" == "--now" ]] && touch "$STATE/STOP-NOW"
    echo "stop requested (the current worker finishes unless --now)" ;;
  status)
    echo "status : $(cat "$STATE/status.txt" 2>/dev/null)"
    echo "driver : $(kill -0 "$(cat "$STATE/lock.d/pid" 2>/dev/null)" 2>/dev/null && echo running || echo not running)"
    echo; echo "recent iterations:"; tail -n 8 "$STATE/iterations.tsv" 2>/dev/null
    echo; echo "driver notes:"; tail -n 8 "$STATE/driver-notes.md" 2>/dev/null ;;
  prompt)  # preview what the next worker would be told
    RUN_DIR="$SELF_DIR"; ensure_worktree && render_prompt preview ;;
  loop) loop ;;
  once) MAX_ITERS=1 loop ;;
  *) sed -n '2,8p' "$0"; exit 2 ;;
esac
