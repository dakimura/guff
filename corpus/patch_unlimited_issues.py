#!/usr/bin/env python3
"""Write a copy of a golangci config with unlimited issue caps.

Sets top-level:
  issues.max-issues-per-linter: 0
  issues.max-same-issues: 0

Needed for stable finding-set diffs: the golangci-lint / guff defaults
(50 / 3) truncate identical messages (e.g. nolintlint) nondeterministically.

Usage:
  python3 corpus/patch_unlimited_issues.py INPUT.yml -o OUTPUT.yml
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


def patch(text: str) -> str:
    lines = text.splitlines(keepends=True)
    # Find top-level `issues:` (no indent)
    issues_idx = None
    for i, line in enumerate(lines):
        if re.match(r"^issues:\s*(?:#.*)?$", line):
            issues_idx = i
            break

    def is_top_level(s: str) -> bool:
        return bool(s.strip()) and not s.startswith((" ", "\t")) and not s.lstrip().startswith("#")

    keys = {
        "max-issues-per-linter": "  max-issues-per-linter: 0\n",
        "max-same-issues": "  max-same-issues: 0\n",
    }

    if issues_idx is None:
        # Append a new issues block.
        if lines and not lines[-1].endswith("\n"):
            lines[-1] = lines[-1] + "\n"
        lines.append("\n")
        lines.append("issues:\n")
        lines.append(keys["max-issues-per-linter"])
        lines.append(keys["max-same-issues"])
        return "".join(lines)

    # Scan issues block for existing keys; replace or insert.
    end = issues_idx + 1
    while end < len(lines) and not is_top_level(lines[end]):
        end += 1

    block = lines[issues_idx + 1 : end]
    found = {k: False for k in keys}
    new_block: list[str] = []
    for line in block:
        replaced = False
        for k, replacement in keys.items():
            if re.match(rf"^  {re.escape(k)}\s*:", line):
                new_block.append(replacement)
                found[k] = True
                replaced = True
                break
        if not replaced:
            new_block.append(line if line.endswith("\n") else line + "\n")

    for k, replacement in keys.items():
        if not found[k]:
            new_block.insert(0, replacement)

    return "".join(lines[: issues_idx + 1] + new_block + lines[end:])


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("input")
    ap.add_argument("-o", "--output", required=True)
    args = ap.parse_args(argv)
    src = Path(args.input).read_text(encoding="utf-8", errors="replace")
    Path(args.output).write_text(patch(src), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
