#!/usr/bin/env bash
# scripts/autopilot/signals.sh — what the next autopilot iteration should know,
# computed without spending model tokens. Prints Markdown on stdout.
#
# Run from a checkout whose origin/main is fresh (the driver fetches first).
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT" || exit 2
REPO="${AUTOPILOT_REPO:-$(git remote get-url origin | sed -E 's#(git@github.com:|https://github.com/)##; s#\.git$##')}"
STATE="${AUTOPILOT_STATE:-$HOME/.guff-autopilot}"

main_sha="$(git rev-parse origin/main)"
echo "## signals ($(date -u +%Y-%m-%dT%H:%MZ))"
echo
echo "- repo: \`$REPO\`, origin/main = \`${main_sha:0:10}\` — $(git log -1 --format=%s origin/main)"

# --- CI on origin/main's head -------------------------------------------------
runs="$(gh run list -R "$REPO" --branch main --limit 30 \
  --json databaseId,headSha,workflowName,status,conclusion 2>/dev/null || echo '[]')"
echo "$runs" | python3 -c '
import json, sys
sha = sys.argv[1]
runs = [r for r in json.load(sys.stdin) if r["headSha"] == sha]
if not runs:
    print("- main CI: no runs for this commit yet")
    sys.exit()
seen = {}
for r in runs:  # newest first; keep the latest attempt per workflow
    seen.setdefault(r["workflowName"], r)
bad = [r for r in seen.values() if r["status"] == "completed" and r["conclusion"] not in ("success", "skipped", "neutral")]
pend = [r for r in seen.values() if r["status"] != "completed"]
ok = [r for r in seen.values() if r not in bad and r not in pend]
print(f"- main CI: {len(ok)} green, {len(pend)} running, **{len(bad)} red**")
for r in bad:
    w, i = r["workflowName"], r["databaseId"]
    print(f"  - RED `{w}` → `gh run view {i} -R {sys.argv[2]} --log-failed`")
for r in pend:
    w = r["workflowName"]
    print(f"  - running `{w}`")
' "$main_sha" "$REPO"

# --- open PRs -----------------------------------------------------------------
prs="$(gh pr list -R "$REPO" --state open --json number,title,headRefName,isDraft 2>/dev/null || echo '[]')"
echo "$prs" | python3 -c '
import json, sys
prs = json.load(sys.stdin)
print(f"- open PRs: {len(prs)}")
for p in prs:
    d = " (draft)" if p["isDraft"] else ""
    n, h, t = p["number"], p["headRefName"], p["title"]
    print(f"  - #{n} `{h}` {t}{d}")
'

# --- upstream versions ----------------------------------------------------------
pin="$(python3 -c 'import json;print(json.load(open("compat/pins.json"))["golangci_lint"])')"
latest_gl="$(gh release view -R golangci/golangci-lint --json tagName --jq .tagName 2>/dev/null | sed 's/^v//')"
if [[ -n "$latest_gl" && "$latest_gl" != "$pin" ]]; then
  echo "- **golangci-lint: new release v$latest_gl** (pin v$pin) → upstream-follow task"
else
  echo "- golangci-lint: pin v$pin is the latest release"
fi
gover_ci="$(cat .go-version 2>/dev/null)"
gos="$(curl -fsS 'https://go.dev/dl/?mode=json' 2>/dev/null | python3 -c 'import json,sys;print(" ".join(r["version"] for r in json.load(sys.stdin)))' 2>/dev/null)"
echo "- Go: CI uses go$gover_ci (.go-version); go.dev stable = ${gos:-unknown}"
if [[ -n "$gos" ]]; then
  newest="$(echo "$gos" | tr ' ' '\n' | head -1)"
  if [[ "$newest" != "go$gover_ci" ]]; then
    echo "  - CI Go is behind the newest stable ($newest) — but .go-version is capped by the export"
    echo "    data the pinned golangci-lint can read (comment in .github/workflows/compat.yml). Raise it"
    echo "    only with a golangci-lint built with a Go whose importer reads the new toolchain's output."
  fi
fi
golangci_go="$("$ROOT/compat/.tools/golangci-lint-$pin/golangci-lint" version 2>/dev/null | sed -nE 's/.*built with (go[0-9.]+).*/\1/p')"
[[ -n "$golangci_go" ]] && echo "  - pinned golangci-lint binary is built with $golangci_go"

# --- local queues -------------------------------------------------------------
if [[ -x corpus/status.py ]]; then
  echo "- corpus queue: $(./corpus/status.py next 2>/dev/null | tail -1)"
fi
for l in $(ls docs/PHASE*-LEDGER.md 2>/dev/null | sort -V | tail -1); do
  n="$(grep -c 'needs-port' "$l" 2>/dev/null)"
  echo "- newest ledger: \`$l\` (lines mentioning needs-port: $n)"
done

# --- previous iterations ------------------------------------------------------
if [[ -f "$STATE/journal.md" ]]; then
  echo
  echo "### last journal entries ($STATE/journal.md)"
  echo
  tail -n 12 "$STATE/journal.md"
fi
if [[ -f "$STATE/driver-notes.md" ]]; then
  echo
  echo "### notes from the driver"
  echo
  tail -n 20 "$STATE/driver-notes.md"
fi
