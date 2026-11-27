#!/bin/sh
# Install the golangci-lint release pinned in compat/pins.json into BINDIR
# (default $HOME/.local/bin). Used by every CI job that compares against it.
#
# Why not upstream's install.sh: it resolves the version by fetching the
# release *tag page* (github.com/.../releases/tag/vX), and that HTML page went
# to HTTP 504 for hours on 2026-11-27 while the release assets and the API kept
# answering 200. install.sh then prints "unable to find 'v2.15.0'" and every
# compat job dies in 20 seconds before running anything. The asset URL is fully
# determined by the pin, so we fetch it directly, retry transient failures, and
# verify it against the release's own checksums file — the same bytes
# install.sh would have installed.
set -eu

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BINDIR="${1:-$HOME/.local/bin}"
V="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["golangci_lint"])' "$ROOT/compat/pins.json")"

case "$(uname -s)" in
  Linux) OS=linux ;;
  Darwin) OS=darwin ;;
  *) echo "install-golangci-lint: unsupported OS $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) ARCH=amd64 ;;
  aarch64 | arm64) ARCH=arm64 ;;
  *) echo "install-golangci-lint: unsupported arch $(uname -m)" >&2; exit 1 ;;
esac

NAME="golangci-lint-$V-$OS-$ARCH"
BASE="https://github.com/golangci/golangci-lint/releases/download/v$V"
TMP="$(mktemp -d)"

fetch() {
  # --retry-all-errors: a 5xx from the CDN is exactly the failure this script
  # exists to ride out; plain --retry does not retry every HTTP error.
  curl -sSfL --retry 5 --retry-delay 5 --retry-all-errors -o "$2" "$1"
}

fetch "$BASE/$NAME.tar.gz" "$TMP/$NAME.tar.gz"
fetch "$BASE/golangci-lint-$V-checksums.txt" "$TMP/checksums.txt"

WANT="$(awk -v f="$NAME.tar.gz" '$2 == f { print $1 }' "$TMP/checksums.txt")"
if [ -z "$WANT" ]; then
  echo "install-golangci-lint: $NAME.tar.gz not listed in the v$V checksums" >&2
  exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
  GOT="$(sha256sum "$TMP/$NAME.tar.gz" | awk '{ print $1 }')"
else
  GOT="$(shasum -a 256 "$TMP/$NAME.tar.gz" | awk '{ print $1 }')"
fi
if [ "$GOT" != "$WANT" ]; then
  echo "install-golangci-lint: checksum mismatch for $NAME.tar.gz (got $GOT, want $WANT)" >&2
  exit 1
fi

tar -xzf "$TMP/$NAME.tar.gz" -C "$TMP"
mkdir -p "$BINDIR"
install -m 0755 "$TMP/$NAME/golangci-lint" "$BINDIR/golangci-lint"
"$BINDIR/golangci-lint" version
