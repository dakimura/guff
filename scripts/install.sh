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

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

asset_name() {
  # $1: tag (v-prefixed) → release asset file name
  echo "guff_${1#v}_${os}_${arch}.tar.gz"
}

dest=""

# Fast path: a pinned tag already names its asset, and the public download host
# serves it without touching the API. That is one request instead of two, and it
# spends no REST quota — which matters on CI, where a shared runner IP and a
# matrix of jobs can otherwise push a repo into rate limiting. A private repo
# answers 404 here; the authenticated API path below then handles it.
if [ "$VERSION" != "latest" ]; then
  asset=$(asset_name "$VERSION")
  url="${GITHUB_DL}/${REPO}/releases/download/${VERSION}/${asset}"
  echo "Downloading ${url}"
  if curl -fsSL -o "${tmpdir}/${asset}" "$url"; then
    dest="${tmpdir}/${asset}"
  else
    echo "direct download unavailable, falling back to the release API…"
  fi
fi

if [ -z "$dest" ]; then
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

  asset=$(asset_name "$VERSION")
  asset_id=$(printf '%s' "$release_json" | asset_id_from_json "$asset") || true
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
