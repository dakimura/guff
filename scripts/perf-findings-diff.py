#!/usr/bin/env python3
"""scripts/perf-findings-diff.py — the findings-identity check every perf change must pass.

A performance change must not move a single finding. `compat/normalize.py`
compares on `path:line:linter:message`, which is right for guff-vs-golangci
(they legitimately disagree on columns) but too loose here: a perf change that
shifted a column or reordered the output would slip through.

So this compares everything — order, column, severity, source lines — and
excludes exactly one field:

    Pos.Offset   a byte offset into the shared FileSet, whose base depends on
                 the order packages happened to be added. It varies run to run
                 on the *same* binary, so it cannot be part of an identity
                 check. Nothing downstream reads it either.

Usage:
    scripts/perf-findings-diff.py before.json after.json

Exit: 0 identical, 1 same set but different order, 2 different findings.
"""

from __future__ import annotations

import json
import sys


def load(path: str) -> list[str]:
    with open(path, encoding="utf-8") as fh:
        doc = json.load(fh)
    out = []
    for issue in doc.get("Issues") or []:
        issue = json.loads(json.dumps(issue))
        pos = issue.get("Pos")
        if isinstance(pos, dict):
            pos.pop("Offset", None)
        out.append(json.dumps(issue, sort_keys=True))
    return out


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    a, b = load(argv[1]), load(argv[2])

    if a == b:
        print(f"IDENTICAL ({len(a)} issues, order-sensitive)")
        return 0
    if sorted(a) == sorted(b):
        print(f"SAME SET, DIFFERENT ORDER ({len(a)} issues)")
        print("  Order is part of the contract: golangci-lint emits a stable")
        print("  order and the golden gate compares line by line.")
        return 1

    only_a = [x for x in a if x not in b]
    only_b = [x for x in b if x not in a]
    print(f"DIFFER: {len(a)} vs {len(b)} issues; "
          f"only-in-A={len(only_a)} only-in-B={len(only_b)}")
    for x in only_a[:10]:
        print("  A only:", x[:280])
    for x in only_b[:10]:
        print("  B only:", x[:280])
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
