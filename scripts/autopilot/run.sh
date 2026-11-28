#!/usr/bin/env bash
# scripts/autopilot/run.sh — unattended development loop for guff.
#
#   scripts/autopilot/run.sh start     # copy scripts to $STATE/run, launch the loop detached
#   scripts/autopilot/run.sh stop      # ask the loop to stop after the current step
#   scripts/autopilot/run.sh status    # what it is doing, last results
#   scripts/autopilot/run.sh once      # one iteration in the foreground (debugging)
#   scripts/autopilot/run.sh prompt    # print the prompt the next worker would get
#   scripts/autopilot/run.sh release   # cut a release at the loop's next step (see release.sh)
#   scripts/autopilot/run.sh release-check  # is one due, and which version
#
# One iteration = one fresh `claude -p` *worker* session in a dedicated git worktree
# doing exactly one task and ending with a pull request (prompt.md). The *driver* —
# this script, no model — then decides the merge. While the PR's CI runs it uses the
# idle machine for two read-only sessions:
#
#   reviewer (review-prompt.md)  fresh context, judges the PR; "changes" blocks the merge
#                                and is posted on the PR for the next worker (P0)
#   scout    (scout-prompt.md)   prepares the next task; its brief goes into the next
#                                worker's signals
#
# A PR is squash-merged only when all of these hold on its current head:
#   1. every CI check that ran is green
#   2. the reviewer approved this head
#   3. if it touches code (crates/ Cargo.* compat/ regress/ corpus/): a full gate.sh
#      record exists for the head's exact tree, and every step in it passed — or failed
#      the same way on main (known-red, from the main-health check)
# After each merge the driver re-runs the local-only gate steps (build, regress) on the
# new main ("main health"), so a red that is already on main is known as such.
#
# Usage limits: when a session ends on a usage/rate limit the loop sleeps until the
# reset (or 30 min) and continues — it runs until `stop`, MAX_ITERS, or
# MAX_FAILS consecutive non-limit failures.
#
# Env (all optional):
#   AUTOPILOT_STATE        state dir (default ~/.guff-autopilot): journal, logs, STOP
#   AUTOPILOT_WORKTREE     worker checkout (default <repo>/../guff-autopilot)
#   AUTOPILOT_MODEL        worker / reviewer model (claude --model)
#   AUTOPILOT_SCOUT_MODEL  scout model (default sonnet; "off" disables the scout)
#   AUTOPILOT_REVIEW       0 disables the reviewer gate
#   AUTOPILOT_MAX_ITERS    default 10000
#   AUTOPILOT_ITER_HOURS   wall-clock cap per iteration (default 5)
#   AUTOPILOT_CI_HOURS     how long to wait for a PR's CI (default 3)
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STATE="${AUTOPILOT_STATE:-$HOME/.guff-autopilot}"
RUN_DIR="$STATE/run"
mkdir -p "$STATE/logs" "$STATE/reviews" "$STATE/scout" "$STATE/gates"

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
SCOUT_MODEL="${AUTOPILOT_SCOUT_MODEL:-sonnet}"
MAX_FAILS=5
# Steps a full gate record must contain (gate.sh's default list).
GATE_REQUIRED="hygiene unit-py build build-fmt golden fix fmt reject smoke cargo-test isolate filesets oss-pr regress"
CODE_PATHS='^(crates/|Cargo\.(toml|lock)$|compat/|regress/|corpus/)'
GATE_WHY=""

log()  { echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*"; }
note() { echo "- $(date '+%m-%d %H:%M') $*" >> "$STATE/driver-notes.md"; log "NOTE: $*"; }
status() { echo "$(date '+%Y-%m-%d %H:%M:%S') $*" > "$STATE/status.txt"; }
stopping() { [[ -f "$STATE/STOP" ]]; }
# macOS notification for the few things a human should look at.
notify() {
  log "NOTIFY: $*"
  osascript -e "display notification \"$(echo "$*" | tr '"' "'" | cut -c1-200)\" with title \"guff autopilot\"" >/dev/null 2>&1 || true
}
# Run a command with a wall-clock limit (gh has hung after a successful merge).
tmo() {
  local secs="$1"; shift
  "$@" &
  local pid=$! w
  ( sleep "$secs"; kill -TERM "$pid" 2>/dev/null ) >/dev/null 2>&1 &
  w=$!
  wait "$pid"; local rc=$?
  kill "$w" 2>/dev/null; wait "$w" 2>/dev/null
  return $rc
}

# shellcheck source=release.sh
source "$SELF_DIR/release.sh"

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

# cargo never collects target/debug/deps; past ~40k files builds crawl (memory:
# cargo-test-slow-when-target-debug-bloats). A dev-profile clean costs one debug rebuild.
target_hygiene() {
  local n
  n="$(find "$WT/target/debug/deps" -maxdepth 1 2>/dev/null | wc -l | tr -d ' ')"
  if (( ${n:-0} > 30000 )); then
    note "target/debug/deps has $n entries; cargo clean --profile dev"
    ( cd "$WT" && cargo clean --profile dev -q ) || true
  fi
}

# Fill {{KEY}} placeholders in a template from KEY=value arguments.
render() {  # $1 = template, then KEY=value...
  local tpl="$1"; shift
  python3 - "$tpl" "$@" <<'EOF'
import sys
s = open(sys.argv[1]).read()
for kv in sys.argv[2:]:
    k, _, v = kv.partition("=")
    s = s.replace("{{" + k + "}}", v)
print(s)
EOF
}

# P4 (speed) is last in the task order, so with a ledger full of needs-port rows it
# never comes up (0 of the first 32 iterations). Every PERF_EVERY-th iteration is
# a speed turn unless one of the previous PERF_EVERY-1 already was.
PERF_EVERY="${AUTOPILOT_PERF_EVERY:-4}"
next_focus() {
  (( PERF_EVERY > 0 )) || return
  local recent
  recent="$(grep -oE '^### iter-[0-9-]+ P[0-4]' "$STATE/journal.md" 2>/dev/null | awk '{print $3}' | tail -n $(( PERF_EVERY - 1 )))"
  (( $(printf '%s\n' "$recent" | grep -c '^P') >= PERF_EVERY - 1 )) || return
  grep -q '^P4$' <<<"$recent" || echo perf
}

current_signals() { (cd "$WT" && AUTOPILOT_STATE="$STATE" AUTOPILOT_FOCUS="$(next_focus)" "$RUN_DIR/signals.sh" 2>&1); }

render_prompt() {  # $1 = iteration label
  render "$RUN_DIR/prompt.md" \
    "MEMORY_DIR=$MEMORY_DIR" "STATE_DIR=$STATE" "REPO=$REPO" "GATE=$RUN_DIR/gate.sh" \
    "ITER_LABEL=$1" "DEADLINE=$(date -r $(( $(date +%s) + ITER_SECS - 1800 )) '+%Y-%m-%d %H:%M')" \
    "SIGNALS=$(current_signals)"
}

settings_for() {  # $1 = worker | readonly
  local role_arg=""
  [[ "$1" == readonly ]] && role_arg=" --role readonly"
  cat <<EOF
{"hooks":{"PreToolUse":[
  {"matcher":"Bash","hooks":[
    {"type":"command","command":"python3 $RUN_DIR/guard.py$role_arg","timeout":10},
    {"type":"command","if":"Bash(cargo *)","command":"$WT/scripts/target-hygiene.sh --hook","timeout":15}]},
  {"matcher":"Write|Edit|MultiEdit|NotebookEdit","hooks":[
    {"type":"command","command":"python3 $RUN_DIR/guard.py$role_arg","timeout":10}]}
]}}
EOF
}

# Run one claude session to completion (or timeout). Writes the stream to $logf.
#   $1 role (worker|readonly)  $2 prompt  $3 logf  $4 timeout secs  $5 model ("" = default)
run_claude() {
  local role="$1" prompt="$2" logf="$3" secs="$4" model="${5:-}" claude_bin pid t0
  claude_bin="$(command -v claude)"
  local args=(-p "$prompt" --dangerously-skip-permissions --output-format stream-json --verbose
              --settings "$(settings_for "$role")" --add-dir "$MEMORY_DIR" --add-dir "$STATE")
  [[ "$role" == readonly ]] && args+=(--disallowedTools "Edit" "Write" "MultiEdit" "NotebookEdit")
  [[ -n "$model" ]] && args+=(--model "$model")
  t0=$(date +%s)
  ( cd "$WT" && exec "$claude_bin" "${args[@]}" ) > "$logf" 2>&1 < /dev/null &
  pid=$!
  echo "$pid" > "${logf%.jsonl}.pid"
  while kill -0 "$pid" 2>/dev/null; do
    if (( $(date +%s) - t0 > secs )); then
      echo "{\"type\":\"driver\",\"timeout\":$secs}" >> "$logf"
      kill -TERM "$pid" 2>/dev/null; sleep 20; kill -KILL "$pid" 2>/dev/null
      break
    fi
    if [[ -f "$STATE/STOP-NOW" ]]; then kill -TERM "$pid" 2>/dev/null; break; fi
    sleep 15
  done
  wait "$pid" 2>/dev/null
  rm -f "${logf%.jsonl}.pid"
  echo $(( $(date +%s) - t0 )) > "${logf%.jsonl}.secs"
}

# Parse a session log. $2 = marker prefix of the machine-readable last line.
# Sets RESULT_TEXT RESULT_LINE RESULT_KIND LIMIT_UNTIL COST TURNS.
parse_session() {
  eval "$(python3 - "$1" "$2" <<'EOF'
import json, re, shlex, sys, time
path, marker = sys.argv[1], sys.argv[2]
result, is_err, cost, turns = None, True, 0.0, 0
raw = open(path, errors="replace").read()
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
    if l.strip().startswith(marker):
        m = l.strip()
limit_until = 0
if re.search(r"usage limit|rate limit|hit your limit|limit reached|resets? (at )?\d|overloaded", text, re.I) and (is_err or not m):
    e = re.search(r"\|(\d{10})\b", text)
    limit_until = int(e.group(1)) if e else int(time.time()) + 1800
kind = "error"
if m:
    rest = m[len(marker):].split()
    kind = rest[0] if rest else "error"
elif limit_until:
    kind = "limit"
last = text.strip().splitlines()[-1][:300] if text.strip() else ""
print("RESULT_TEXT=" + shlex.quote(result or ""))
print("RESULT_LINE=" + shlex.quote(m or last))
print("RESULT_KIND=" + shlex.quote(kind))
print("LIMIT_UNTIL=%d" % limit_until)
print("COST=%s TURNS=%s" % (cost, turns))
EOF
)"
}

record_session() {  # $1 label  $2 logf
  printf '%s\t%s\t%ss\t$%s\t%s turns\t%s\n' "$(date '+%F %T')" "$1" "$(cat "${2%.jsonl}.secs" 2>/dev/null)" \
    "${COST:-0}" "${TURNS:-0}" "$RESULT_LINE" >> "$STATE/iterations.tsv"
}

run_worker() {  # $1 label $2 logf
  local prompt
  prompt="$(render_prompt "$1")"
  echo "$prompt" > "${2%.jsonl}.prompt.md"
  run_claude worker "$prompt" "$2" "$ITER_SECS" "${AUTOPILOT_MODEL:-}"
  parse_session "$2" "AUTOPILOT_RESULT:"
  record_session "$1" "$2"
}

# ---------------------------------------------------------------- reviewer / scout
# Both run in the background while the driver waits for CI. One reviewer per PR head.

review_file() { echo "$STATE/reviews/$1.$2"; }   # $1 head sha, $2 jsonl|verdict|md|pid

start_review() {  # $1 pr $2 head $3 branch
  local pr="$1" head="$2" branch="$3" logf prompt
  logf="$(review_file "$head" jsonl)"
  prompt="$(render "$RUN_DIR/review-prompt.md" "PR=$pr" "REPO=$REPO" "HEAD=$head" "HEAD_SHORT=${head:0:10}" \
            "BRANCH=$branch" "WT=$WT" "MEMORY_DIR=$MEMORY_DIR")"
  log "reviewer: PR #$pr @ ${head:0:10}"
  (
    run_claude readonly "$prompt" "$logf" 3600 "${AUTOPILOT_MODEL:-}"
    parse_session "$logf" "REVIEW_VERDICT:"
    record_session "review-#$pr" "$logf"
    printf '%s\n' "$RESULT_TEXT" > "$(review_file "$head" md)"
    case "$RESULT_KIND" in approve|changes) echo "$RESULT_KIND" ;; limit) echo limit ;; *) echo error ;; esac \
      > "$(review_file "$head" verdict)"
  ) >> "$STATE/driver.log" 2>&1 &
  echo $! > "$(review_file "$head" bgpid)"
}

review_running() { local p; p="$(cat "$(review_file "$1" bgpid)" 2>/dev/null)"; [[ -n "$p" ]] && kill -0 "$p" 2>/dev/null; }

SCOUT_BG=""
start_scout() {  # $1 = pr being waited on
  [[ "$SCOUT_MODEL" == off ]] && return
  [[ -n "$SCOUT_BG" ]] && kill -0 "$SCOUT_BG" 2>/dev/null && return
  local label logf prompt
  label="scout-$(date +%Y%m%d-%H%M)"
  logf="$STATE/scout/$label.jsonl"
  prompt="$(render "$RUN_DIR/scout-prompt.md" "PR=$1" "WT=$WT" "STATE_DIR=$STATE" "RUN_DIR=$RUN_DIR" \
            "MEMORY_DIR=$MEMORY_DIR" "SIGNALS=$(current_signals)")"
  log "scout: $label"
  (
    run_claude readonly "$prompt" "$logf" 1800 "$SCOUT_MODEL"
    parse_session "$logf" "SCOUT_DONE"
    record_session "$label" "$logf"
    if [[ "$RESULT_KIND" != limit && "$RESULT_LINE" == SCOUT_DONE* ]]; then
      { echo "<!-- $label, origin/main $(git -C "$WT" rev-parse --short origin/main) -->"; printf '%s\n' "$RESULT_TEXT"; } \
        > "$STATE/next-task.md"
    fi
  ) >> "$STATE/driver.log" 2>&1 &
  SCOUT_BG=$!
}

# The worker must not start while the scout still reads the worktree.
# The driver runs from a copy in $RUN_DIR, so merged changes to scripts/autopilot
# never reached it: #18 (PR-number parse) sat unused for 8h while the old parse
# left #22 and #28 unmerged. After each worktree reset, adopt origin/main's copy
# and re-exec (same pid, so the lock stays ours; drain_open_prs then runs again).
RUN_FILES=(run.sh gate.sh signals.sh guard.py prompt.md review-prompt.md scout-prompt.md
           release.sh update_formula.py)
self_update() {
  local f changed=()
  for f in "${RUN_FILES[@]}"; do
    [[ -f "$WT/scripts/autopilot/$f" ]] || continue
    cmp -s "$WT/scripts/autopilot/$f" "$RUN_DIR/$f" || changed+=("$f")
  done
  (( ${#changed[@]} )) || return 0
  bash -n "$WT/scripts/autopilot/run.sh" || { note "self-update skipped: run.sh on main fails bash -n"; return 0; }
  wait_scout
  for f in "${RUN_FILES[@]}"; do
    [[ -f "$WT/scripts/autopilot/$f" ]] && cp "$WT/scripts/autopilot/$f" "$RUN_DIR/$f"
  done
  note "self-update from origin/main $(git -C "$WT" rev-parse --short origin/main): ${changed[*]}; re-exec"
  trap - EXIT
  exec bash "$RUN_DIR/run.sh" loop
}

wait_scout() {
  [[ -z "$SCOUT_BG" ]] && return
  local t0; t0=$(date +%s)
  while kill -0 "$SCOUT_BG" 2>/dev/null; do
    (( $(date +%s) - t0 > 900 )) && { pkill -TERM -P "$SCOUT_BG" 2>/dev/null; kill "$SCOUT_BG" 2>/dev/null; break; }
    sleep 15
  done
  SCOUT_BG=""
}

# ---------------------------------------------------------------- merge decision

# 0 when the PR's head tree has an acceptable full gate record (or needs none).
gate_ok() {  # $1 pr  $2 head ; sets GATE_WHY
  local files tree rec
  files="$(tmo 60 gh pr diff "$1" -R "$REPO" --name-only 2>/dev/null)"
  if ! echo "$files" | grep -qE "$CODE_PATHS"; then GATE_WHY="no code paths touched"; return 0; fi
  git -C "$WT" fetch -q origin "$2" 2>/dev/null
  tree="$(git -C "$WT" rev-parse "$2^{tree}" 2>/dev/null)"
  rec="$STATE/gates/$tree.tsv"
  if [[ -z "$tree" || ! -f "$rec" ]]; then GATE_WHY="no gate.sh record for head tree ${tree:0:10}"; return 1; fi
  GATE_WHY="$(python3 - "$rec" "$STATE/main-health.tsv" "$GATE_REQUIRED" <<'EOF'
import sys
rec, health, required = sys.argv[1], sys.argv[2], sys.argv[3].split()
meta, steps = {}, {}
for l in open(rec):
    p = l.rstrip("\n").split("\t")
    if l.startswith("#"):
        meta[p[0][1:]] = p[1] if len(p) > 1 else ""
    elif len(p) >= 2:
        steps[p[0]] = p[1]
known_red = set()
try:
    for l in open(health):
        p = l.rstrip("\n").split("\t")
        if not l.startswith("#") and len(p) >= 2 and p[1] == "FAIL":
            known_red.add(p[0])
except FileNotFoundError:
    pass
if meta.get("mode") != "full":
    print("gate record is mode=%s, not full" % meta.get("mode")); sys.exit(1)
missing = [s for s in required if s not in steps]
if missing:
    print("gate stopped before: " + ",".join(missing)); sys.exit(1)
bad = [s for s, r in steps.items() if r != "PASS" and s not in known_red]
if bad:
    print("gate FAIL: " + ",".join(bad)); sys.exit(1)
kr = [s for s, r in steps.items() if r != "PASS"]
print("gate PASS" + (" (known-red on main: %s)" % ",".join(kr) if kr else ""))
EOF
)"
}

comment_once() {  # $1 pr  $2 key  $3 body-file
  local mark="$STATE/reviews/commented-$1-$2"
  [[ -f "$mark" ]] && return
  tmo 60 gh pr comment "$1" -R "$REPO" --body-file "$3" >/dev/null && touch "$mark"
}

# Wait for a PR's checks and decide. Returns 0 merged/closed, 1 left open, 2 stopped.
#   $2 = "mech" for the driver's own release PRs: no reviewer, no gate record.
wait_and_merge() {
  local n="$1" mode="${2:-}" t0 js verdict head branch seen="" rv tmpf scouted=0
  t0=$(date +%s)
  while :; do
    stopping && return 2
    if (( $(date +%s) - t0 > CI_SECS )); then
      note "PR #$n: CI did not finish within ${AUTOPILOT_CI_HOURS:-3}h; left open"
      return 1
    fi
    js="$(tmo 60 gh pr view "$n" -R "$REPO" --json state,isDraft,headRefOid,headRefName,mergeable,statusCheckRollup 2>/dev/null)" \
      || { nap 60 || return 2; continue; }
    verdict="$(echo "$js" | python3 -c '
import json, sys
p = json.load(sys.stdin)
if p["state"] != "OPEN":
    print("closed " + p["state"]); sys.exit()
if p["isDraft"]:
    print("draft"); sys.exit()
checks = p.get("statusCheckRollup") or []
if not checks:
    print("nochecks"); sys.exit()
pend, bad = [], []
for c in checks:
    name = c.get("name") or c.get("context") or "?"
    if c.get("__typename") == "StatusContext":
        st = c.get("state")
        if st in ("PENDING", "EXPECTED"): pend.append(name)
        elif st != "SUCCESS": bad.append(name)
        continue
    if c.get("status") != "COMPLETED":
        pend.append(name)
    elif c.get("conclusion") not in ("SUCCESS", "SKIPPED", "NEUTRAL"):
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
    status "PR #$n @ ${head:0:10}: CI ${verdict%% *}"

    # Reviewer for this head (once), then the scout (once per wait).
    if [[ "$mode" != mech && "${AUTOPILOT_REVIEW:-1}" != 0 && "$verdict" != closed* ]]; then
      if [[ "$head" != "$seen" ]]; then
        seen="$head"
        [[ -f "$(review_file "$head" verdict)" ]] || review_running "$head" || start_review "$n" "$head" "$branch"
      fi
      if [[ $scouted == 0 ]] && ! review_running "$head"; then
        scouted=1; start_scout "$n"
      fi
    fi

    case "$verdict" in
      green)
        if [[ "$mode" != mech ]]; then
          if ! gate_ok "$n" "$head"; then
            note "PR #$n @ ${head:0:10}: not merging — $GATE_WHY"
            tmpf="$(mktemp "$STATE/reviews/gate-msg.XXXXXX")"
            printf '**autopilot driver**: not merging `%s` yet — %s.\n\nRun the full `gate.sh` on exactly this tree (commit everything it tested, nothing it rewrote) and push.\n' "${head:0:10}" "$GATE_WHY" > "$tmpf"
            comment_once "$n" "gate-${head:0:10}" "$tmpf"
            return 1
          fi
          if [[ "${AUTOPILOT_REVIEW:-1}" != 0 ]]; then
            if review_running "$head"; then nap 60 || return 2; continue; fi
            rv="$(cat "$(review_file "$head" verdict)" 2>/dev/null)"
            case "$rv" in
              approve) ;;
              changes)
                note "PR #$n @ ${head:0:10}: reviewer requested changes; left open"
                comment_once "$n" "review-${head:0:10}" "$(review_file "$head" md)"
                return 1 ;;
              *)
                # One retry for an errored / rate-limited review; after that, fail open
                # with a note (CI and the gate record still hold).
                if [[ ! -f "$(review_file "$head" retried)" ]]; then
                  touch "$(review_file "$head" retried)"
                  mv -f "$(review_file "$head" verdict)" "$(review_file "$head" verdict-1)" 2>/dev/null
                  start_review "$n" "$head" "$branch"; nap 60 || return 2; continue
                fi
                note "PR #$n @ ${head:0:10}: reviewer unavailable twice ($rv); merging on CI + gate" ;;
            esac
          fi
        fi
        log "PR #$n green at ${head:0:10} ($GATE_WHY); merging"
        tmo 120 gh pr merge "$n" -R "$REPO" --squash --match-head-commit "$head" >/dev/null 2>&1
        sleep 5
        if [[ "$(tmo 60 gh pr view "$n" -R "$REPO" --json state --jq .state)" == "MERGED" ]]; then
          note "merged PR #$n ($branch) at ${head:0:10}"
          tmo 60 git -C "$MAIN" push -q origin --delete "$branch" 2>/dev/null || true
          return 0
        fi
        note "PR #$n: merge command failed; left open for the next iteration"
        return 1 ;;
      red*)      note "PR #$n CI red on ${head:0:10}: ${verdict#red }"; return 1 ;;
      conflict)  note "PR #$n conflicts with main; merge origin/main into the branch and push (no force)"; return 1 ;;
      draft)     note "PR #$n is draft; not merging"; return 1 ;;
      closed*)   note "PR #$n is ${verdict#closed }"; return 0 ;;
      nochecks)  if (( $(date +%s) - t0 > 900 )); then note "PR #$n: no checks started in 15 min"; return 1; fi
                 nap 60 || return 2 ;;
      *)         nap 120 || return 2 ;;
    esac
  done
}

# Re-run the local-only gate steps on the new main: CI covers everything else.
main_health() {
  reset_worktree || return
  status "main health on $(git -C "$WT" rev-parse --short origin/main)"
  local out
  out="$(mktemp -d "$STATE/logs/main-health.XXXXXX")"
  ( cd "$WT" && GATE_OUT="$out" AUTOPILOT_STATE="$STATE" "$RUN_DIR/gate.sh" --only build,regress ) > "$out/gate.out" 2>&1
  { printf '#sha\t%s\n#date\t%s\n' "$(git -C "$WT" rev-parse origin/main)" "$(date '+%F %T')"
    cat "$out/record.tsv" 2>/dev/null; } > "$STATE/main-health.tsv"
  if grep -q $'\tFAIL' "$STATE/main-health.tsv"; then
    note "main health: $(grep $'\tFAIL' "$STATE/main-health.tsv" | cut -f1 | tr '\n' ' ')FAIL on $(git -C "$WT" rev-parse --short origin/main) (logs $out)"
  fi
}

# Open autopilot PRs left by a previous run (e.g. after a restart) get one pass.
drain_open_prs() {
  local n
  for n in $(tmo 60 gh pr list -R "$REPO" --state open --json number,headRefName,isDraft \
              --jq '.[] | select(.headRefName|startswith("autopilot/")) | select(.isDraft|not) | .number'); do
    wait_and_merge "$n" && main_health
  done
}

# ---------------------------------------------------------------- spin guards
# 2026-11-28: `blocked` had no wait, so a worker that kept answering "blocked
# (machine memory)" was restarted every 30s for 5h: ~385 iterations, ~$170, and
# the 5-hour usage limit. Every guard below costs no tokens and holds regardless
# of the result kind, so a new failure shape cannot spin either.
BLOCKED_MAX="${AUTOPILOT_BLOCKED_MAX:-3}"      # consecutive `blocked` → driver exits
SPIN_SECS="${AUTOPILOT_SPIN_SECS:-300}"        # a worker shorter than this is "short"
SPIN_MAX="${AUTOPILOT_SPIN_MAX:-4}"            # this many short ones without a merge → exit
DAILY_USD="${AUTOPILOT_DAILY_USD:-150}"        # all sessions (worker+review+scout) per day
MAX_WIRED_PCT="${AUTOPILOT_MAX_WIRED_PCT:-50}" # kernel-wired share of RAM that breaks gates

spent_today() {
  awk -F'\t' -v d="$(date +%F)" 'index($1, d) == 1 { c = $4; gsub(/\$/, "", c); t += c }
    END { printf "%d", t }' "$STATE/iterations.tsv" 2>/dev/null || echo 0
}

# Wired memory is the kernel's and only a reboot returns it (92 days up: 17.3 of
# 24 GiB). With it, regress and the gates fail on main too, and the worker can
# only say "blocked" — so check it here, before spending tokens.
wired_pct() {
  local page total wired
  page="$(sysctl -n hw.pagesize 2>/dev/null)"; total="$(sysctl -n hw.memsize 2>/dev/null)"
  wired="$(vm_stat 2>/dev/null | awk '/Pages wired down/ { gsub(/\./, "", $4); print $4 }')"
  [[ -n "$page" && -n "$total" && -n "$wired" ]] || { echo 0; return; }
  echo $(( wired * page * 100 / total ))
}

human_decisions() { local n; n="$(grep -c '^### HUMAN-DECISION' "$STATE/journal.md" 2>/dev/null)"; echo "${n:-0}"; }

# Sleep up to $1 seconds, waking early on STOP (→ 1) or a new HUMAN-DECISION (→ 0).
nap_for_human() {
  local end=$(( $(date +%s) + $1 )) h0; h0="$(human_decisions)"
  while (( $(date +%s) < end )); do
    stopping && return 1
    (( $(human_decisions) > h0 )) && { log "new HUMAN-DECISION; resuming"; return 0; }
    sleep 30
  done
  return 0
}

# Pre-flight before each worker. Returns 1 when the loop should go round again
# (after waiting), 2 when it should stop.
preflight() {
  local spent w until
  spent="$(spent_today)"
  if (( spent >= DAILY_USD )); then
    until=$(( $(date -j -v+1d -v0H -v5M -v0S +%s) ))
    if [[ "$(cat "$STATE/budget-noted" 2>/dev/null)" != "$(date +%F)" ]]; then
      date +%F > "$STATE/budget-noted"
      note "daily budget reached (\$$spent >= \$$DAILY_USD); pausing until $(date -r "$until" '+%m-%d %H:%M')"
      notify "daily budget \$$DAILY_USD reached; paused until tomorrow"
    fi
    status "paused: daily budget \$$spent/\$$DAILY_USD — until $(date -r "$until" '+%F %T')"
    nap $(( until - $(date +%s) )) || return 2
    return 1
  fi
  w="$(wired_pct)"
  if (( w > MAX_WIRED_PCT )); then
    if [[ ! -f "$STATE/wired-noted" ]]; then
      touch "$STATE/wired-noted"
      note "kernel wired memory ${w}% > ${MAX_WIRED_PCT}% — gates cannot pass; reboot the machine. Waiting (no tokens spent)"
      notify "wired memory ${w}%: reboot the Mac; autopilot is waiting"
    fi
    status "paused: wired memory ${w}% > ${MAX_WIRED_PCT}% (reboot needed) — rechecking every 30 min"
    nap 1800 || return 2
    return 1
  fi
  rm -f "$STATE/wired-noted"
  return 0
}

needs_human_count() { local n; n="$(grep -c 'NEEDS-HUMAN' "$STATE/journal.md" 2>/dev/null)"; echo "${n:-0}"; }

loop() {
  if ! mkdir "$STATE/lock.d" 2>/dev/null && [[ "$(cat "$STATE/lock.d/pid" 2>/dev/null)" != "$$" ]]; then
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

  local i=0 fails=0 idle=0 blocked=0 short=0 secs label logf pr nh0 rc
  drain_open_prs
  [[ -f "$STATE/main-health.tsv" ]] || main_health
  while (( i < MAX_ITERS )); do
    stopping && break
    maybe_release   # release.sh: weekly-ish, only from a green main
    stopping && break
    wait_scout
    preflight; rc=$?
    (( rc == 2 )) && break
    (( rc == 1 )) && continue
    i=$((i + 1))
    label="iter-$(date +%Y%m%d-%H%M)"
    logf="$STATE/logs/$label.jsonl"
    reset_worktree || { note "worktree reset failed"; nap 600 || break; continue; }
    self_update
    target_hygiene
    status "$label running (worker log $logf)"
    log "$label start"
    nh0="$(needs_human_count)"
    run_worker "$label" "$logf"
    log "$label -> [$RESULT_KIND] $RESULT_LINE (cost \$${COST:-?}, ${TURNS:-?} turns)"
    # The scout's brief was for this iteration; it is stale now.
    [[ -f "$STATE/next-task.md" ]] && mv -f "$STATE/next-task.md" "$STATE/scout/used-$label.md"
    (( $(needs_human_count) > nh0 )) && notify "NEEDS-HUMAN in journal ($label)"

    secs="$(cat "${logf%.jsonl}.secs" 2>/dev/null)"
    if [[ "$RESULT_KIND" != limit ]] && (( ${secs:-0} < SPIN_SECS )); then
      short=$((short + 1))
    else
      short=0
    fi
    if (( short >= SPIN_MAX )); then
      note "$short consecutive workers under ${SPIN_SECS}s with no merge (last: $RESULT_LINE); driver exiting"
      notify "autopilot stopped: $short short iterations in a row — see $STATE/driver-notes.md"
      break
    fi
    [[ "$RESULT_KIND" == blocked ]] || blocked=0

    case "$RESULT_KIND" in
      pr-open)
        fails=0; idle=0
        # The first `#N` after "pr-open" is the PR; later ones may be ledger
        # rows ("ledger #21/#22") — a greedy `.*#` once picked those instead.
        pr="$(echo "$RESULT_LINE" | sed -nE 's/.*pr-open[^#]*#([0-9]+).*/\1/p' | head -1)"
        if [[ -n "$pr" ]]; then
          wait_and_merge "$pr"; rc=$?
          [[ $rc == 2 ]] && break
          [[ $rc == 0 ]] && { short=0; main_health; }
        else note "$label said pr-open without a number: $RESULT_LINE"; fi ;;
      no-task)
        fails=0; idle=$((idle + 1))
        status "idle (no task) — next check in $(( idle * 2 ))h"
        nap $(( idle > 6 ? 43200 : idle * 7200 )) || break ;;
      blocked)
        # Blocked means a human must act; asking again changes nothing. Back off
        # (1h, 2h, …), wake early on a new HUMAN-DECISION, and give up after
        # BLOCKED_MAX in a row so the driver never spins on it.
        fails=0; blocked=$((blocked + 1))
        note "$label blocked ($blocked/$BLOCKED_MAX): $RESULT_LINE"
        notify "blocked: $RESULT_LINE"
        if (( blocked >= BLOCKED_MAX )); then
          note "blocked $blocked times in a row; driver exiting — write a HUMAN-DECISION in the journal, then run.sh start"
          break
        fi
        status "blocked ($blocked/$BLOCKED_MAX) — waiting for a HUMAN-DECISION, at most $(( 1 << (blocked - 1) ))h"
        nap_for_human $(( 3600 << (blocked - 1) )) || break ;;
      limit)
        local now; now=$(date +%s)
        local wait=$(( LIMIT_UNTIL > now ? LIMIT_UNTIL - now + 120 : 1800 ))
        status "usage limit — sleeping until $(date -r $((now + wait)) '+%F %T')"
        log "usage limit; sleeping ${wait}s"
        nap "$wait" || break ;;
      *)
        fails=$((fails + 1))
        note "$label ended without a result line ($fails/$MAX_FAILS): $RESULT_LINE"
        if (( fails >= MAX_FAILS )); then
          note "too many consecutive failures; driver exiting"
          notify "driver exiting after $MAX_FAILS failed iterations — see $STATE/driver-notes.md"
          break
        fi
        nap $(( fails * 300 )) || break ;;
    esac
  done
  wait_scout
  log "driver exiting after $i iteration(s)"
}

case "${1:-}" in
  start)
    mkdir -p "$RUN_DIR"
    for f in "${RUN_FILES[@]}"; do cp "$SELF_DIR/$f" "$RUN_DIR/"; done
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
    echo "health : $(grep -v '^#' "$STATE/main-health.tsv" 2>/dev/null | cut -f1,2 | tr '\t\n' ': ')"
    echo; echo "recent sessions:"; tail -n 10 "$STATE/iterations.tsv" 2>/dev/null
    echo; echo "driver notes:"; tail -n 10 "$STATE/driver-notes.md" 2>/dev/null ;;
  release)  # ask the running loop to cut a release at its next step, regardless of cadence
    touch "$STATE/RELEASE-NOW"
    echo "release requested; the loop cuts it before its next iteration (still needs a mergeable bump PR)" ;;
  release-check)  # would a release be due now, and which version?
    ensure_worktree >/dev/null && git -C "$WT" fetch -q --tags origin
    echo "last tag : $(last_tag)"; echo "next     : v$(next_version)"
    if release_due; then echo "due      : yes"; else echo "due      : no"; fi ;;
  prompt)  # preview what the next worker would be told
    RUN_DIR="$SELF_DIR"; ensure_worktree && render_prompt preview ;;
  loop) loop ;;
  once) MAX_ITERS=1 loop ;;
  *) sed -n '2,10p' "$0"; exit 2 ;;
esac
