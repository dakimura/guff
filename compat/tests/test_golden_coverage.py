#!/usr/bin/env python3
"""Every linter must have a check-level golden case.

The other tiers key findings on `path:line:linter:message` *after*
`compat/normalize.py` canonicalizes seven known phrasing differences. A wrong
**column**, a wrong **severity**, and any message difference the normalizer
erases all compare equal there. Only the golden tier keys on
`path:line:col:linter:severity:text` byte for byte — so a linter with no golden
case has never had its column or its severity checked by anything.

That is not a hypothetical. Adding the 84 missing cases on 2026-08-24 turned up
thirteen defects in one pass, every one of them a position or a message that the
normalized tiers were structurally unable to see: five linters reporting a
function's *name* where upstream reports the `func` keyword, two reporting an
AST node where upstream reports a go/ssa instruction (the `(` of a call), and
one dropping a trailing newline upstream actually prints.

So the count is a gate, not a report. A linter added without a golden case fails
here rather than joining a backlog.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CASES = ROOT / "golden" / "cases"
LINTERS = ROOT / "isolate" / "linters.txt"

# A case may legitimately expect zero findings — but only by saying so, because
# an empty golden is also what a case whose module failed to build produces, and
# the two are indistinguishable from the outside. `cases/ginkgolinter` wrote a
# zero-key golden on 2026-08-24 for exactly that reason: upstream needs a
# `github.com/onsi/gomega/types` package to be reachable before it looks at
# anything, the stub had none, and the run was silently a no-op.
EMPTY_OK_MARKER = "golden-may-be-empty"


def enabled_linters(config: str) -> set[str]:
    """The linters a case's config.yml turns on, from `enable:` lists."""
    out: set[str] = set()
    in_enable = False
    for line in config.splitlines():
        s = line.strip()
        if re.fullmatch(r"enable:", s):
            in_enable = True
            continue
        if in_enable:
            m = re.fullmatch(r"-\s+([A-Za-z0-9_]+)", s)
            if m:
                out.add(m.group(1))
                continue
            if s and not s.startswith("#"):
                in_enable = False
    return out


def declared_linters() -> list[str]:
    """The linter roster, from the isolate tier's own list."""
    names = []
    for line in LINTERS.read_text(encoding="utf-8").splitlines():
        s = line.split("#", 1)[0].strip()
        if s:
            names.append(s.split()[0])
    return names


def cases() -> list[Path]:
    return sorted(p for p in CASES.iterdir() if (p / "config.yml").is_file())


class CoverageTests(unittest.TestCase):
    def test_every_declared_linter_has_a_golden_case(self):
        covered: set[str] = set()
        for c in cases():
            covered |= enabled_linters((c / "config.yml").read_text(encoding="utf-8"))
        missing = sorted(set(declared_linters()) - covered)
        self.assertEqual(
            missing,
            [],
            "no golden case enables these linters, so nothing checks their "
            "column or severity: " + ", ".join(missing),
        )

    def test_the_roster_is_not_empty(self):
        """A gate that reads an empty roster passes without checking anything."""
        self.assertGreater(len(declared_linters()), 100)


class GoldenShapeTests(unittest.TestCase):
    def test_no_case_has_a_silently_empty_golden(self):
        """Zero keys means one of two things and the file cannot say which."""
        empty = []
        for c in cases():
            golden = c / "expected.golden"
            if not golden.is_file():
                continue
            body = [
                ln
                for ln in golden.read_text(encoding="utf-8").splitlines()
                if ln.strip() and not ln.startswith("#")
            ]
            if body:
                continue
            if EMPTY_OK_MARKER in (c / "config.yml").read_text(encoding="utf-8"):
                continue
            empty.append(c.name)
        self.assertEqual(
            empty,
            [],
            "these goldens hold no findings, which is also what a case whose "
            f"module failed to build produces; say `{EMPTY_OK_MARKER}` in "
            "config.yml if it is deliberate: " + ", ".join(empty),
        )

    def test_every_case_declares_the_issue_limits(self):
        """Both tools default to 50/3; a truncated golden compares less than it
        claims to. run.sh enforces this too — here so it fails in the fast job."""
        for c in cases():
            cfg = (c / "config.yml").read_text(encoding="utf-8")
            for key in ("max-issues-per-linter", "max-same-issues"):
                self.assertRegex(cfg, rf"(?m)^\s*{key}:", f"{c.name}: {key}")

    def test_every_case_source_exists(self):
        """sources.txt points into the repo; a stale path fails the run late and
        confusingly (`missing source …`), so catch it in the fast job."""
        for c in cases():
            for raw in (c / "sources.txt").read_text(encoding="utf-8").splitlines():
                line = raw.split("#", 1)[0].strip()
                if not line:
                    continue
                parts = re.split(r"\s{2,}", line, maxsplit=1)
                self.assertEqual(len(parts), 2, f"{c.name}: {raw!r}")
                self.assertTrue(
                    (ROOT.parent / parts[1]).is_file(), f"{c.name}: {parts[1]}"
                )


if __name__ == "__main__":
    unittest.main()
