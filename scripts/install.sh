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
EOF
}

BINDIR="${GUFF_INSTALL_DIR:-${HOME}/.local/bin}"
VERSION="latest"

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

# Normalize: accept 0.1.0 or v0.1.0
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
  tag=$(curl -fsSL "${GITHUB_API}/repos/${REPO}/releases/latest" | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)
  if [ -z "$tag" ]; then
    echo "failed to resolve latest release tag" >&2
    exit 1
  fi
  VERSION="$tag"
fi

# Strip leading v for the asset filename stem used in release.yml
ver_num="${VERSION#v}"
asset="guff_${ver_num}_${os}_${arch}.tar.gz"
url="${GITHUB_DL}/${REPO}/releases/download/${VERSION}/${asset}"

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

echo "Downloading ${url}"
curl -fsSL -o "${tmpdir}/${asset}" "$url"

echo "Extracting…"
tar -xzf "${tmpdir}/${asset}" -C "$tmpdir"
if [ ! -f "${tmpdir}/guff" ]; then
  echo "archive did not contain guff binary" >&2
  exit 1
fi

mkdir -p "$BINDIR"
install -m 755 "${tmpdir}/guff" "${BINDIR}/guff"
echo "Installed ${BINDIR}/guff (${VERSION})"
"${BINDIR}/guff" version --short || "${BINDIR}/guff" version || true
