#!/usr/bin/env bash
# corpus/prepare.sh — shallow-clone OSS targets, verify golangci-lint v2 config,
# warm module download + go list.
#
# Usage:
#   ./corpus/prepare.sh --tier pr
#   ./corpus/prepare.sh --tier nightly
#   ./corpus/prepare.sh --tier pr,nightly
#   ./corpus/prepare.sh --name gin
#
# Env:
#   CORPUS_CACHE  override clone root (default: corpus/cache)
#
# Prints one TSV line per prepared repo to stdout:
#   name  dir  config  packages  timeout  tier
#
# Progress / errors go to stderr.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORPUS_DIR="$ROOT/corpus"
CACHE="${CORPUS_CACHE:-$CORPUS_DIR/cache}"
SELECT="$CORPUS_DIR/select.py"
REPOS_JSON="$CORPUS_DIR/repos.json"
mkdir -p "$CACHE"

TIER_ARGS=()
NAME_ARGS=()
WARM=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tier)
      TIER_ARGS+=(--tier "$2")
      shift 2
      ;;
    --tier=*)
      TIER_ARGS+=(--tier "${1#*=}")
      shift
      ;;
    --name)
      NAME_ARGS+=(--name "$2")
      shift 2
      ;;
    --name=*)
      NAME_ARGS+=(--name "${1#*=}")
      shift
      ;;
    --no-warm)
      WARM=0
      shift
      ;;
    -h|--help)
      sed -n '2,20p' "$0"
      exit 0
      ;;
    *)
      echo "unknown arg: $1" >&2
      exit 2
      ;;
  esac
done

die() { echo "error: $*" >&2; exit 1; }

command -v git >/dev/null 2>&1 || die "git not found"
command -v python3 >/dev/null 2>&1 || die "python3 not found"
command -v go >/dev/null 2>&1 || die "go not found"
[[ -f "$SELECT" ]] || die "missing $SELECT"
[[ -f "$REPOS_JSON" ]] || die "missing $REPOS_JSON"

is_v2_config() {
  local path="$1"
  python3 - "$path" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8", errors="replace").read()
# Match top-level `version: "2"` / `version: 2` / `version: '2'`
for line in text.splitlines():
    s = line.strip()
    if s.startswith("#"):
        continue
    m = re.match(r'^version:\s*["\']?2["\']?\s*(?:#.*)?$', s)
    if m:
        sys.exit(0)
    # Stop at first non-empty non-comment if we already saw nested content;
    # version may appear after a comment block, so keep scanning a while.
sys.exit(1)
PY
}

discover_config() {
  local dir="$1"
  local override="$2"
  if [[ -n "$override" ]]; then
    if [[ -f "$dir/$override" ]]; then
      echo "$dir/$override"
      return 0
    fi
    die "config override not found: $dir/$override"
  fi
  local f
  for f in .golangci.yml .golangci.yaml; do
    if [[ -f "$dir/$f" ]]; then
      echo "$dir/$f"
      return 0
    fi
  done
  return 1
}

clone_repo() {
  local name="$1"
  local url="$2"
  local ref="$3"
  local dest="$CACHE/$name"
  if [[ -d "$dest/.git" ]]; then
    git -C "$dest" fetch --depth 1 origin "refs/tags/${ref}:refs/tags/${ref}" >/dev/null 2>&1 \
      || git -C "$dest" fetch --depth 1 origin "$ref" >/dev/null 2>&1 \
      || true
    git -C "$dest" checkout -q "$ref" 2>/dev/null \
      || git -C "$dest" checkout -q "tags/$ref" 2>/dev/null \
      || die "cannot checkout $ref in $dest"
  else
    echo "cloning $name ($ref)..." >&2
    rm -rf "$dest"
    if ! git clone --depth 1 --branch "$ref" "$url" "$dest" >/dev/null 2>&1; then
      git clone --depth 1 "$url" "$dest" >/dev/null
      git -C "$dest" fetch --depth 1 origin tag "$ref" >/dev/null 2>&1 || true
      git -C "$dest" checkout -q "$ref" 2>/dev/null \
        || git -C "$dest" checkout -q "tags/$ref" \
        || die "clone/checkout failed for $name@$ref"
    fi
  fi
  echo "$dest"
}

warm_module() {
  local dir="$1"
  local packages="$2"
  (
    cd "$dir"
    go mod download >/dev/null 2>&1 || true
    # shellcheck disable=SC2086
    go list $packages >/dev/null 2>&1 || true
  )
}

while IFS=$'\t' read -r name url ref packages tier timeout config_override; do
  [[ -z "${name:-}" ]] && continue
  dest="$(clone_repo "$name" "$url" "$ref")"
  cfg=""
  if ! cfg="$(discover_config "$dest" "$config_override")"; then
    die "$name@$ref: no .golangci.yml/.yaml (and no config override)"
  fi
  if ! is_v2_config "$cfg"; then
    die "$name@$ref: $cfg is not golangci-lint v2 (need top-level version: \"2\")"
  fi
  if [[ "$WARM" -eq 1 ]]; then
    echo "warming $name ($packages)..." >&2
    warm_module "$dest" "$packages"
  fi
  # stdout TSV for harnesses
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$dest" "$cfg" "$packages" "$timeout" "$tier"
done < <(python3 "$SELECT" --repos "$REPOS_JSON" ${TIER_ARGS[@]+"${TIER_ARGS[@]}"} ${NAME_ARGS[@]+"${NAME_ARGS[@]}"})
