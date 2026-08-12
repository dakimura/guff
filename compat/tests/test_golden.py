#!/usr/bin/env python3
"""Unit tests for compat/golden/golden.py."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "golden"))

from golden import (  # noqa: E402
    confirm,
    diff,
    escape,
    format_unconfirmed,
    issue_key,
    issue_keys,
    parse_golden,
    sort_key,
)


def issue(line, col, text, linter="gocritic", severity="", filename="/root/a.go"):
    return {
        "FromLinter": linter,
        "Text": text,
        "Severity": severity,
        "Pos": {"Filename": filename, "Line": line, "Column": col},
    }


class KeyTests(unittest.TestCase):
    def test_key_carries_column_and_severity(self):
        k = issue_key(issue(12, 6, "assignOp: replace x", severity="warning"), "/root")
        self.assertEqual(k, "a.go:12:6:gocritic:warning:assignOp: replace x")

    def test_message_is_not_normalized(self):
        """The whole point of this tier: no canonicalization at all."""
        k = issue_key(issue(1, 1, "SA1234: could omit type int.", linter="staticcheck"), "/root")
        self.assertEqual(k, "a.go:1:1:staticcheck::SA1234: could omit type int.")

    def test_columns_distinguish_otherwise_equal_findings(self):
        a = issue_key(issue(5, 2, "x"), "/root")
        b = issue_key(issue(5, 6, "x"), "/root")
        self.assertNotEqual(a, b)

    def test_escapes_newlines(self):
        self.assertEqual(escape("a\nb"), "a\\nb")
        self.assertEqual(escape("a\\b"), "a\\\\b")
        self.assertNotIn("\n", issue_key(issue(1, 1, "a\nb"), "/root"))

    def test_related_information_rows_are_dropped(self):
        rows = [issue(1, 1, "SA5011: x"), issue(2, 1, "SA5011(related information): y")]
        self.assertEqual(len(issue_keys(rows, "/root")), 1)


class OrderTests(unittest.TestCase):
    def test_sorts_by_position_not_lexically(self):
        keys = ["a.go:10:1:l::m", "a.go:2:1:l::m", "a.go:1:1:l::m"]
        self.assertEqual(
            sorted(keys, key=sort_key),
            ["a.go:1:1:l::m", "a.go:2:1:l::m", "a.go:10:1:l::m"],
        )

    def test_sort_key_tolerates_colons_in_message(self):
        self.assertEqual(sort_key("a.go:3:4:gocritic::x: y: z"), ("a.go", 3, 4, "gocritic::x: y: z"))


class DiffTests(unittest.TestCase):
    def test_identical_sets_have_no_diff(self):
        self.assertEqual(diff(["a", "b"], ["a", "b"]), ([], []))

    def test_reports_both_directions(self):
        missing, extra = diff(["a.go:1:1:l::x"], ["a.go:2:1:l::y"])
        self.assertEqual(missing, ["a.go:1:1:l::x"])
        self.assertEqual(extra, ["a.go:2:1:l::y"])

    def test_duplicates_are_a_multiset(self):
        """Two identical findings on one line must not collapse into one."""
        missing, extra = diff(["a.go:1:1:l::x", "a.go:1:1:l::x"], ["a.go:1:1:l::x"])
        self.assertEqual(missing, ["a.go:1:1:l::x"])
        self.assertEqual(extra, [])


class ConfirmationTests(unittest.TestCase):
    """golangci-lint can drop whole packages' findings; a golden must not.

    See golden.confirm's docstring and compat/golden/README.md,
    "Upstream is not a function".
    """

    A = ["a.go:1:1:l::x", "a.go:2:1:l::y"]
    SHORT = ["a.go:1:1:l::x"]

    def test_one_run_is_never_enough_by_default(self):
        self.assertIsNone(confirm([self.A], confirmations=2))

    def test_two_identical_runs_confirm(self):
        self.assertEqual(confirm([self.A, list(self.A)], confirmations=2), self.A)

    def test_a_truncated_run_between_two_good_ones_does_not_win(self):
        runs = [self.A, self.SHORT, list(self.A)]
        self.assertEqual(confirm(runs, confirmations=2), self.A)

    def test_all_runs_distinct_is_unconfirmed(self):
        runs = [self.A, self.SHORT, []]
        self.assertIsNone(confirm(runs, confirmations=2))

    def test_confirmation_is_not_a_union(self):
        # The union of a truncated run and a run that moved a finding would
        # record both positions as expected; "seen twice" records neither.
        moved = ["a.go:1:1:l::x", "b.go:2:1:l::y"]
        self.assertIsNone(confirm([self.A, moved], confirmations=2))

    def test_unconfirmed_report_names_the_unstable_keys(self):
        text = format_unconfirmed("revive", [self.A, self.SHORT])
        self.assertIn("did not agree", text)
        self.assertIn("a.go:2:1:l::y", text)
        self.assertNotIn("a.go:1:1:l::x", text)


class GoldenFileTests(unittest.TestCase):
    def test_parse_skips_comments_and_blanks(self):
        import tempfile

        with tempfile.NamedTemporaryFile("w", suffix=".golden", delete=False) as fh:
            fh.write("# header\n\na.go:1:1:l::x\n# trailing\nb.go:2:3:l::y\n")
            path = fh.name
        self.assertEqual(parse_golden(path), ["a.go:1:1:l::x", "b.go:2:3:l::y"])

    def test_committed_gocritic_golden_is_parseable(self):
        path = ROOT / "golden" / "cases" / "gocritic" / "expected.golden"
        keys = parse_golden(path)
        self.assertGreater(len(keys), 100)
        for k in keys:
            # Every key must survive the position-aware sort used to render it.
            sort_key(k)


if __name__ == "__main__":
    unittest.main()
