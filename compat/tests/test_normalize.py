#!/usr/bin/env python3
"""Unit tests for compat/normalize.py (R21 harness smoke)."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from normalize import (  # noqa: E402
    diff_sets,
    extract_issues_json,
    format_report,
    issue_key,
    issue_keys,
    normalize_message,
    normalize_path,
    parse_allowlist,
)


class ExtractJsonTests(unittest.TestCase):
    def test_plain(self):
        raw = '{"Issues":[{"FromLinter":"x"}],"Report":null}\n'
        self.assertEqual(len(extract_issues_json(raw)["Issues"]), 1)

    def test_trailing_summary(self):
        raw = (
            '{"Issues":[{"FromLinter":"errcheck","Text":"t","Pos":'
            '{"Filename":"a.go","Line":1,"Column":1,"Offset":0}}],"Report":null}\n'
            "1 issues:\n* errcheck: 1\n"
        )
        self.assertEqual(extract_issues_json(raw)["Issues"][0]["FromLinter"], "errcheck")

    def test_leading_log(self):
        raw = 'level=info msg=hi\n{"Issues":[],"Report":null}\n'
        self.assertEqual(extract_issues_json(raw)["Issues"], [])


class NormalizeTests(unittest.TestCase):
    def test_path_abs_under_root(self):
        root = "/tmp/proj"
        self.assertEqual(
            normalize_path("/tmp/proj/pkg/a.go", root),
            "pkg/a.go",
        )

    def test_path_strips_module_dirname_prefix(self):
        root = "/tmp/fixture"
        self.assertEqual(normalize_path("fixture/main.go", root), "main.go")

    def test_errcheck_message_alias(self):
        self.assertEqual(
            normalize_message("errcheck", "unchecked error"),
            normalize_message("errcheck", "Error return value is not checked"),
        )

    def test_unused_strips_func_prefix(self):
        self.assertEqual(
            normalize_message("unused", "func unusedHelper is unused"),
            "unusedHelper is unused",
        )

    def test_staticcheck_strips_check_code(self):
        self.assertEqual(
            normalize_message("staticcheck", "QF1003: could use tagged switch on k % 3"),
            normalize_message("staticcheck", "could use tagged switch on k % 3"),
        )

    def test_modernize_strips_check_name_prefix(self):
        self.assertEqual(
            normalize_message(
                "modernize",
                "slicesbackward: backward loop over slice can be modernized using slices.Backward",
            ),
            normalize_message(
                "modernize",
                "backward loop over slice can be modernized using slices.Backward",
            ),
        )


class DiffTests(unittest.TestCase):
    def test_matching_keys(self):
        root = "/work/mod"
        guff = [
            {
                "FromLinter": "errcheck",
                "Text": "unchecked error",
                "Pos": {"Filename": "/work/mod/main.go", "Line": 10, "Column": 1, "Offset": 0},
            }
        ]
        gcl = [
            {
                "FromLinter": "errcheck",
                "Text": "Error return value is not checked",
                "Pos": {"Filename": "/work/mod/main.go", "Line": 10, "Column": 1, "Offset": 0},
            }
        ]
        r = diff_sets("t", issue_keys(guff, root), issue_keys(gcl, root), [])
        self.assertTrue(r.ok)
        self.assertEqual(r.precision, 1.0)
        self.assertEqual(r.recall, 1.0)
        self.assertEqual(
            issue_key(guff[0], root),
            "main.go:10:errcheck:Error return value is not checked",
        )

    def test_allowlist_covers_known_diff(self):
        guff = {"pkg/a.go:1:staticcheck:package comment"}
        gcl: set[str] = set()
        allow = parse_allowlist_lines(
            ["t guff-only pkg/a.go:1:staticcheck:package comment"]
        )
        r = diff_sets("t", guff, gcl, allow)
        self.assertTrue(r.ok)
        self.assertEqual(r.allowed_guff, guff)

    def test_unexpected_fails(self):
        r = diff_sets("t", {"a.go:1:govet:x"}, set(), [])
        self.assertFalse(r.ok)
        self.assertEqual(r.unexpected_guff, {"a.go:1:govet:x"})

    def test_format_report_contains_rates(self):
        r = diff_sets("fixture", {"a.go:1:govet:x"}, {"a.go:1:govet:x"}, [])
        text = format_report([r])
        self.assertIn("100.0%", text)
        self.assertIn("fixture", text)


def parse_allowlist_lines(lines: list[str]):
    with tempfile.NamedTemporaryFile("w", delete=False, encoding="utf-8") as fh:
        fh.write("\n".join(lines) + "\n")
        path = fh.name
    try:
        return parse_allowlist(path)
    finally:
        Path(path).unlink(missing_ok=True)


class RoundTripJsonTests(unittest.TestCase):
    def test_load_from_file_with_noise(self):
        payload = {
            "Issues": [
                {
                    "FromLinter": "ineffassign",
                    "Text": "ineffectual assignment to x",
                    "Pos": {
                        "Filename": "/mod/main.go",
                        "Line": 3,
                        "Column": 2,
                        "Offset": 0,
                    },
                }
            ],
            "Report": None,
        }
        raw = "warn\n" + json.dumps(payload) + "\n2 issues:\n"
        with tempfile.NamedTemporaryFile("w", delete=False, encoding="utf-8") as fh:
            fh.write(raw)
            path = fh.name
        try:
            from normalize import load_issues

            keys = issue_keys(load_issues(path), "/mod")
            self.assertEqual(keys, {"main.go:3:ineffassign:ineffectual assignment to x"})
        finally:
            Path(path).unlink(missing_ok=True)


if __name__ == "__main__":
    unittest.main()
