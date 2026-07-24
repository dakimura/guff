#!/bin/sh
# Install guff from GitHub Releases.
#
# Usage:
#   curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/install.sh | sh -s -- [options] [version]
#
# Options:
#   -b <dir>   Install directory (default: $GUFF_INSTALL_DIR, else ~/.local/bin)
#   -d         Enable debug tracing
#
# Version:
#   latest (default) or a tag like v0.1.0 / 0.1.0
#
# Auth (private repos):
#   Set GITHUB_TOKEN or GH_TOKEN to a token with repo read access.
set -e

REPO="dakimura/guff"
GITHUB_API="https://api.github.com"
GITHUB_DL="https://github.com"

usage() {
  cat <<'EOF'
Usage: install.sh [-b bindir] [-d] [version]

Install guff from GitHub Releases into bindir (default: ~/.local/bin).

  -b <dir>   install directory
  -d         debug (set -x)
  version    release tag (v0.1.0) or "latest" (default)

Set GITHUB_TOKEN or GH_TOKEN when downloading from a private repository.
EOF
}

BINDIR="${GUFF_INSTALL_DIR:-${HOME}/.local/bin}"
VERSION="latest"
TOKEN="${GITHUB_TOKEN:-${GH_TOKEN:-}}"

api_curl() {
  # api_curl <accept> <url> [-o outfile]
  accept="$1"
  shift
  if [ -n "$TOKEN" ]; then
    curl -fsSL \
      -H "Authorization: Bearer ${TOKEN}" \
      -H "Accept: ${accept}" \
      -H "X-GitHub-Api-Version: 2022-11-28" \
      "$@"
  else
    curl -fsSL \
      -H "Accept: ${accept}" \
      -H "X-GitHub-Api-Version: 2022-11-28" \
      "$@"
  fi
}

asset_id_from_json() {
  # stdin: release JSON, $1: asset name → prints asset id
  name="$1"
  if command -v python3 >/dev/null 2>&1; then
    python3 -c 'import json,sys
name=sys.argv[1]
data=json.load(sys.stdin)
for a in data.get("assets",[]):
  if a.get("name")==name:
    print(a["id"]); break
' "$name"
  elif command -v jq >/dev/null 2>&1; then
    jq -r --arg n "$name" '.assets[] | select(.name==$n) | .id' | head -n1
  else
    echo "need python3 or jq to parse release JSON" >&2
    return 1
  fi
}

while getopts "b:dh" opt; do
  case "$opt" in
    b) BINDIR="$OPTARG" ;;
    d) set -x ;;
    h) usage; exit 0 ;;
    *) usage; exit 1 ;;
  esac
done
shift $((OPTIND - 1))

if [ $# -ge 1 ]; then
  VERSION="$1"
fi

if [ "$VERSION" != "latest" ]; then
  case "$VERSION" in
    v*) ;;
    *) VERSION="v${VERSION}" ;;
  esac
fi

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
case "$arch" in
  x86_64|amd64) arch="amd64" ;;
  aarch64|arm64) arch="arm64" ;;
  *)
    echo "unsupported architecture: $arch" >&2
    exit 1
    ;;
esac
case "$os" in
  linux|darwin) ;;
  *)
    echo "unsupported OS: $os" >&2
    exit 1
    ;;
esac

if [ "$VERSION" = "latest" ]; then
  echo "Resolving latest release…"
  release_url="${GITHUB_API}/repos/${REPO}/releases/latest"
else
  release_url="${GITHUB_API}/repos/${REPO}/releases/tags/${VERSION}"
fi

release_json=$(api_curl "application/vnd.github+json" "$release_url") || {
  echo "failed to fetch release metadata (private repo? set GITHUB_TOKEN)" >&2
  exit 1
}

if command -v python3 >/dev/null 2>&1; then
  VERSION=$(printf '%s' "$release_json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["tag_name"])')
else
  VERSION=$(printf '%s' "$release_json" | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)
fi
if [ -z "$VERSION" ]; then
  echo "failed to parse release tag" >&2
  exit 1
fi

ver_num="${VERSION#v}"
asset="guff_${ver_num}_${os}_${arch}.tar.gz"
asset_id=$(printf '%s' "$release_json" | asset_id_from_json "$asset") || true

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT
dest="${tmpdir}/${asset}"

if [ -n "$asset_id" ]; then
  echo "Downloading ${asset} (id ${asset_id})"
  api_curl "application/octet-stream" \
    "${GITHUB_API}/repos/${REPO}/releases/assets/${asset_id}" \
    -o "$dest" || {
      echo "asset download failed" >&2
      exit 1
    }
else
  url="${GITHUB_DL}/${REPO}/releases/download/${VERSION}/${asset}"
  echo "Downloading ${url}"
  curl -fsSL -o "$dest" "$url" || {
    echo "download failed; if the repo is private set GITHUB_TOKEN / GH_TOKEN" >&2
    exit 1
  }
fi

echo "Extracting…"
tar -xzf "$dest" -C "$tmpdir"
if [ ! -f "${tmpdir}/guff" ]; then
  echo "archive did not contain guff binary" >&2
  exit 1
fi

mkdir -p "$BINDIR"
install -m 755 "${tmpdir}/guff" "${BINDIR}/guff"
echo "Installed ${BINDIR}/guff (${VERSION})"
"${BINDIR}/guff" version --short || "${BINDIR}/guff" version || true
