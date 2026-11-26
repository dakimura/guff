#!/bin/sh
# Remove a guff binary installed by scripts/install.sh (or a manual copy).
#
# Usage:
#   curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/uninstall.sh | sh
#   ./scripts/uninstall.sh -b ~/.local/bin
#   GUFF_INSTALL_DIR=/usr/local/bin ./scripts/uninstall.sh
set -e

BINDIR="${GUFF_INSTALL_DIR:-${HOME}/.local/bin}"

usage() {
  cat <<'EOF'
Usage: uninstall.sh [-b bindir] [-h]

Remove the guff binary from bindir (default: $GUFF_INSTALL_DIR, else ~/.local/bin).

Does not touch:
  - Homebrew / aqua / mise / cargo installs (use those tools' uninstall)
  - Project configs (.golangci.yml / .guff.yml)
  - Analysis cache (~/.cache/guff or platform equivalent; run: guff cache clean)
EOF
}

while getopts "b:h" opt; do
  case "$opt" in
    b) BINDIR="$OPTARG" ;;
    h) usage; exit 0 ;;
    *) usage; exit 1 ;;
  esac
done

TARGET="${BINDIR}/guff"
if [ ! -e "$TARGET" ] && [ ! -L "$TARGET" ]; then
  echo "guff not found at ${TARGET}" >&2
  echo "Hint: brew uninstall guff | aqua rm dakimura/guff | cargo uninstall guff-lint" >&2
  exit 1
fi

rm -f "$TARGET"
echo "Removed ${TARGET}"
echo "Optional: guff cache clean   # if another guff remains on PATH"
echo "Optional: remove CI uses: dakimura/guff@… and Docker pulls of ghcr.io/dakimura/guff"
