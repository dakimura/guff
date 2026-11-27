# scripts/autopilot/release.sh — periodic releases, cut by the driver (no model).
#
# Sourced by run.sh; uses its globals (MAIN WT REPO STATE SELF_DIR) and helpers
# (log note status nap reset_worktree wait_and_merge).
#
# A release is due when all of these hold:
#   - AUTOPILOT_RELEASE is not 0
#   - at least AUTOPILOT_RELEASE_DAYS (default 7) days since the newest v* tag
#     (or there is no tag at all)
#   - at least one commit since that tag that is not itself release plumbing
#   - every workflow that ran on origin/main's head finished green, ignoring
#     AUTOPILOT_RELEASE_IGNORE (default "Action self-test", which tests the
#     *previous* release and so cannot gate the next one)
# `touch $STATE/RELEASE-NOW` skips the day count and the CI check once.
#
# Version: if Cargo.toml is already ahead of the newest tag, that version.
# Otherwise minor bump when compat/pins.json or .go-version changed since the
# tag (a new golangci-lint or Go target), patch bump otherwise.
#
# Steps: bump PR (merged only when green, like any PR) → annotated tag on the
# merge commit → wait for release.yml → PR pointing Formula/guff.rb and the
# docs/INSTALL.md example at the new assets' sha256.

RELEASE_DAYS="${AUTOPILOT_RELEASE_DAYS:-7}"
RELEASE_IGNORE="${AUTOPILOT_RELEASE_IGNORE:-Action self-test}"
RELEASE_PLUMBING='^(Bump version to |Formula: |Release v)'

cargo_version_at() {  # $1 = rev
  git -C "$WT" show "$1:Cargo.toml" | sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' | head -1
}

last_tag() {
  git -C "$WT" describe --tags --abbrev=0 --match 'v[0-9]*' origin/main 2>/dev/null
}

ver_gt() {  # $1 > $2 (semver, numeric parts only)
  python3 -c 'import sys
a, b = (tuple(int(x) for x in v.split(".")) for v in sys.argv[1:3])
sys.exit(0 if a > b else 1)' "$1" "$2"
}

main_head_green() {
  local sha; sha="$(git -C "$WT" rev-parse origin/main)"
  gh run list -R "$REPO" --branch main --limit 40 \
    --json headSha,workflowName,status,conclusion 2>/dev/null | python3 -c '
import json, sys
sha, ignore = sys.argv[1], set(filter(None, sys.argv[2].split(",")))
latest = {}
for r in json.load(sys.stdin):
    if r["headSha"] == sha:
        latest.setdefault(r["workflowName"], r)
runs = [r for n, r in latest.items() if n not in ignore]
ok = runs and all(r["status"] == "completed" and r["conclusion"] in ("success", "skipped", "neutral") for r in runs)
sys.exit(0 if ok else 1)
' "$sha" "$RELEASE_IGNORE"
}

next_version() {
  local last cur lastv
  last="$(last_tag)"
  cur="$(cargo_version_at origin/main)"
  lastv="${last#v}"
  if [[ -z "$last" ]] || ver_gt "$cur" "$lastv"; then
    echo "$cur"; return
  fi
  if git -C "$WT" diff --quiet "$last" origin/main -- compat/pins.json .go-version; then
    echo "$lastv" | awk -F. '{printf "%d.%d.%d\n", $1, $2, $3 + 1}'
  else
    echo "$lastv" | awk -F. '{printf "%d.%d.0\n", $1, $2 + 1}'
  fi
}

release_due() {
  [[ "${AUTOPILOT_RELEASE:-1}" == 0 ]] && return 1
  git -C "$WT" fetch -q --tags --prune origin || return 1
  if [[ -f "$STATE/RELEASE-NOW" ]]; then
    return 0
  fi
  local last age n
  last="$(last_tag)"
  if [[ -n "$last" ]]; then
    age=$(( ( $(date +%s) - $(git -C "$WT" log -1 --format=%ct "$last") ) / 86400 ))
    (( age < RELEASE_DAYS )) && return 1
    n="$(git -C "$WT" log --format=%s "$last..origin/main" | grep -cvE "$RELEASE_PLUMBING")"
    (( n == 0 )) && return 1
  fi
  main_head_green
}

# Commit what is staged in $WT on a new branch, open a PR, merge it when green.
release_pr() {  # $1 = branch, $2 = title, $3 = body
  local pr
  git -C "$WT" commit -q -m "$2" || return 1
  git -C "$WT" push -q -u origin "$1" || return 1
  pr="$(gh pr create -R "$REPO" --base main --head "$1" --title "$2" --body "$3" | sed -nE 's#.*/pull/([0-9]+).*#\1#p')"
  [[ -n "$pr" ]] || return 1
  log "release: opened PR #$pr ($2)"
  wait_and_merge "$pr"
}

cut_release() {
  local ver cur last sha run tmp concl
  ver="$(next_version)"
  cur="$(cargo_version_at origin/main)"
  last="$(last_tag)"
  status "releasing v$ver"
  note "release: cutting v$ver (last tag ${last:-none}, Cargo.toml $cur)"

  # 1. version bump PR, unless Cargo.toml is already there.
  if [[ "$cur" != "$ver" ]]; then
    reset_worktree || return 1
    git -C "$WT" switch -q -C "release/v$ver" || return 1
    (
      cd "$WT" || exit 1
      sed -i '' "/^\[workspace.package\]/,/^\[/s/^version = \"$cur\"/version = \"$ver\"/" Cargo.toml
      sed -i '' -E "s#dakimura/guff@v${cur//./\\.}#dakimura/guff@v$ver#g; s#ghcr\.io/dakimura/guff:${cur//./\\.}#ghcr.io/dakimura/guff:$ver#g" README*.md
      sed -i '' "s/(e.g. v$cur)/(e.g. v$ver)/" action.yml
      cargo update -w --offline -q
      git add Cargo.toml Cargo.lock README*.md action.yml
    ) || return 1
    release_pr "release/v$ver" "Bump version to $ver" \
"Periodic release by the autopilot driver (\`scripts/autopilot/release.sh\`).

- workspace version $cur → $ver
- README / \`action.yml\` examples follow

After merge the driver tags \`v$ver\` on the merge commit and updates the Homebrew formula once \`release.yml\` has published the assets." \
      || { note "release: bump PR for v$ver did not merge; giving up this round"; return 1; }
    git -C "$WT" fetch -q origin
  fi

  # 2. tag the commit that carries the version.
  sha="$(git -C "$WT" rev-parse origin/main)"
  if [[ "$(cargo_version_at "$sha")" != "$ver" ]]; then
    note "release: origin/main ${sha:0:10} is not at $ver; not tagging"; return 1
  fi
  if ! git -C "$WT" rev-parse -q --verify "refs/tags/v$ver" >/dev/null; then
    git -C "$WT" tag -a "v$ver" -m "v$ver" "$sha" && git -C "$WT" push -q origin "v$ver" \
      || { note "release: pushing tag v$ver failed"; return 1; }
  fi
  rm -f "$STATE/RELEASE-NOW"
  note "release: tagged v$ver at ${sha:0:10}"

  # 3. wait for release.yml.
  local t0; t0=$(date +%s)
  while :; do
    run="$(gh run list -R "$REPO" --workflow Release --branch "v$ver" --limit 1 --json databaseId,status,conclusion \
           --jq '.[0] | "\(.databaseId) \(.status) \(.conclusion)"' 2>/dev/null)"
    set -- $run
    if [[ "${2:-}" == completed ]]; then concl="$3"; break; fi
    (( $(date +%s) - t0 > 7200 )) && { note "release: release.yml for v$ver not done in 2h"; return 1; }
    nap 60 || return 1
  done
  # Judge by the job that publishes the assets: Docker (GHCR) can fail on its own
  # (e.g. package permissions) without affecting the tarballs the formula needs.
  local rel_job
  rel_job="$(gh run view "$1" -R "$REPO" --json jobs --jq '.jobs[] | select(.name == "GitHub Release") | .conclusion')"
  if [[ "$rel_job" != success ]]; then
    note "release: release.yml for v$ver ended '$concl', GitHub Release job '${rel_job:-missing}' (run $1); formula not updated"; return 1
  fi
  [[ "$concl" != success ]] && note "release: v$ver assets published, but release.yml ended '$concl' (run $1) — check the Docker jobs"

  # 4. formula / INSTALL.md PR.
  tmp="$(mktemp -d "$STATE/release-v$ver.XXXXXX")"
  gh release download "v$ver" -R "$REPO" -p '*.sha256' -D "$tmp" || { note "release: downloading v$ver sha256 failed"; return 1; }
  reset_worktree || return 1
  git -C "$WT" switch -q -C "release/v$ver-formula" || return 1
  python3 "$SELF_DIR/update_formula.py" "$WT" "$ver" "$tmp" || { note "release: update_formula.py failed for v$ver"; return 1; }
  git -C "$WT" add Formula/guff.rb docs/INSTALL.md
  if git -C "$WT" diff --cached --quiet; then
    note "release: v$ver published; formula already current"; return 0
  fi
  release_pr "release/v$ver-formula" "Formula: guff $ver" \
"Points \`Formula/guff.rb\` and the \`docs/INSTALL.md\` download example at the v$ver release assets (sha256 taken from the \`.sha256\` files \`release.yml\` uploaded)." \
    || { note "release: formula PR for v$ver did not merge"; return 1; }
  note "release: v$ver published, formula updated"
}

maybe_release() {
  release_due || return 0
  cut_release
  reset_worktree
}
