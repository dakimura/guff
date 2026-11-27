#!/usr/bin/env python3
"""Point Formula/guff.rb and the docs/INSTALL.md download example at a release.

    update_formula.py <repo-root> <version> <dir-with-*.tar.gz.sha256>

<version> is without the leading "v". The .sha256 files are the ones
release.yml uploads next to each tarball (`<sha>  guff_<ver>_<os>_<arch>.tar.gz`).
Fails without writing anything if any of the four platforms is missing.
"""
import pathlib
import re
import sys

PLATFORMS = [("darwin", "arm64"), ("darwin", "amd64"), ("linux", "arm64"), ("linux", "amd64")]


def main() -> int:
    root, ver, shadir = pathlib.Path(sys.argv[1]), sys.argv[2].lstrip("v"), pathlib.Path(sys.argv[3])
    shas = {}
    for os_, arch in PLATFORMS:
        f = shadir / f"guff_{ver}_{os_}_{arch}.tar.gz.sha256"
        if not f.exists():
            print(f"missing {f}", file=sys.stderr)
            return 1
        sha = f.read_text().split()[0]
        if not re.fullmatch(r"[0-9a-f]{64}", sha):
            print(f"bad sha in {f}: {sha!r}", file=sys.stderr)
            return 1
        shas[(os_, arch)] = sha

    formula = root / "Formula" / "guff.rb"
    s = formula.read_text()
    s = re.sub(r'version "[^"]+"', f'version "{ver}"', s, count=1)
    for (os_, arch), sha in shas.items():
        # url "…/download/vX/guff_X_<os>_<arch>.tar.gz"  followed by its sha256 line
        pat = re.compile(
            r'(url "https://github\.com/dakimura/guff/releases/download/)v[^/]+/guff_[^_]+_'
            + os_ + "_" + arch + r'(\.tar\.gz"\s*\n\s*sha256 ")[0-9a-f]{64}"'
        )
        s, n = pat.subn(lambda m: f'{m.group(1)}v{ver}/guff_{ver}_{os_}_{arch}{m.group(2)}{sha}"', s)
        if n != 1:
            print(f"Formula: expected one url/sha256 pair for {os_}/{arch}, found {n}", file=sys.stderr)
            return 1
    formula.write_text(s)

    install = root / "docs" / "INSTALL.md"
    t = install.read_text()
    t = re.sub(r"releases/download/v[0-9][^/]*/guff_[0-9][^_]*_", f"releases/download/v{ver}/guff_{ver}_", t)
    install.write_text(t)
    return 0


if __name__ == "__main__":
    sys.exit(main())
