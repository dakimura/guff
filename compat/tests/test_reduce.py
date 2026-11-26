#!/usr/bin/env python3
"""Unit tests for compat/reduce.py's pure logic (COMPAT-HARDENING Phase 6).

The oracle and the go/ast helper need real toolchains, so what is covered here
is the part that has to be right for the reducer's output to mean anything: the
edit algebra and the two search loops. A bug in `apply_edits` would silently
change *which* program the oracle judged, and a bug in `ddmin` would report a
reproducer that no longer reproduces.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "golden"))

from reduce import Edit, Tree, apply_edits, ddmin, ddmin_grow  # noqa: E402


def tree(**files: str) -> Tree:
    return Tree(Path("/nowhere"), {k: v.encode() for k, v in files.items()})


def edit(rel: str, start: int, end: int, replace: str = "", kind: str = "decl") -> Edit:
    return Edit(rel=rel, kind=kind, start=start, end=end, replace=replace, label="")


class TestApplyEdits(unittest.TestCase):
    def test_deletes_a_span(self):
        t = tree(**{"a.go": "0123456789"})
        out = apply_edits(t, [edit("a.go", 2, 5)])
        self.assertEqual(out.files["a.go"], b"0156789")

    def test_replacement_text_is_substituted(self):
        t = tree(**{"a.go": "func f() { body }"})
        out = apply_edits(t, [edit("a.go", 9, 17, replace="{ panic() }", kind="body")])
        self.assertEqual(out.files["a.go"], b"func f() { panic() }")

    def test_disjoint_edits_all_apply(self):
        t = tree(**{"a.go": "abcdefghij"})
        out = apply_edits(t, [edit("a.go", 0, 2), edit("a.go", 6, 8)])
        self.assertEqual(out.files["a.go"], b"cdefij")

    def test_outer_span_wins_over_nested_one(self):
        """Spans nest — a statement lives inside its declaration — so ddmin
        routinely picks both. Taking the outer and skipping the inner keeps the
        result a pure function of the subset, which the cache relies on."""
        t = tree(**{"a.go": "0123456789"})
        both = apply_edits(t, [edit("a.go", 1, 8), edit("a.go", 3, 5)])
        outer_only = apply_edits(t, [edit("a.go", 1, 8)])
        self.assertEqual(both.files["a.go"], outer_only.files["a.go"])
        self.assertEqual(both.files["a.go"], b"089")

    def test_order_of_the_edit_list_does_not_matter(self):
        t = tree(**{"a.go": "0123456789"})
        a = apply_edits(t, [edit("a.go", 6, 8), edit("a.go", 1, 3)])
        b = apply_edits(t, [edit("a.go", 1, 3), edit("a.go", 6, 8)])
        self.assertEqual(a.files["a.go"], b.files["a.go"])

    def test_edit_to_a_missing_file_is_ignored(self):
        t = tree(**{"a.go": "abc"})
        out = apply_edits(t, [edit("gone.go", 0, 1)])
        self.assertEqual(out.files, t.files)

    def test_out_of_range_span_is_skipped(self):
        t = tree(**{"a.go": "abc"})
        out = apply_edits(t, [edit("a.go", 1, 99)])
        self.assertEqual(out.files["a.go"], b"abc")

    def test_input_tree_is_not_mutated(self):
        t = tree(**{"a.go": "abcdef"})
        apply_edits(t, [edit("a.go", 0, 3)])
        self.assertEqual(t.files["a.go"], b"abcdef")


class TestDdmin(unittest.TestCase):
    def test_finds_the_single_required_unit(self):
        units = list(range(20))
        calls = []

        def test(kept):
            calls.append(len(kept))
            return 7 in kept

        self.assertEqual(ddmin(units, test), [7])
        self.assertLess(len(calls), 60)  # not quadratic on this shape

    def test_keeps_a_required_pair(self):
        units = list(range(16))
        got = ddmin(units, lambda kept: 3 in kept and 11 in kept)
        self.assertEqual(got, [3, 11])

    def test_everything_required_keeps_everything(self):
        units = list(range(8))
        self.assertEqual(ddmin(units, lambda kept: len(kept) == 8), units)


class TestDdminGrow(unittest.TestCase):
    """`ddmin_grow` searches the other way: units are removals, and it wants the
    largest set that keeps the candidate interesting."""

    def test_applies_every_independent_removal(self):
        units = list(range(12))
        self.assertEqual(ddmin_grow(units, lambda a: True, lambda *_: None), units)

    def test_never_applies_a_removal_that_breaks_it(self):
        units = list(range(12))
        got = ddmin_grow(units, lambda applied: 5 not in applied, lambda *_: None)
        self.assertNotIn(5, got)
        self.assertEqual(got, [u for u in units if u != 5])

    def test_applies_nothing_when_no_removal_survives(self):
        units = list(range(6))
        self.assertEqual(ddmin_grow(units, lambda a: not a, lambda *_: None), [])


if __name__ == "__main__":
    unittest.main()
