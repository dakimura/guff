#!/usr/bin/env python3
"""Unit tests for compat/reject/reject.py — the "upstream refuses" tier.

The tier's whole job is to notice when only one of the two tools refuses a
config, so the tests that matter here are the ones that make it *fail*: a tool
that exits 0, and a tool that refuses for a different reason. Every sample
below is verbatim output from golangci-lint 2.12.2 / guff 0.4.1.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "reject"))

from reject import check_accept, check_case, reason_golangci, reason_guff  # noqa: E402

GCL_CONFIG_ERROR = (
    "Error: can't load config: error in exclude rule #0: "
    "at least 2 of (text, source, path[-except], linters) should be set\n"
    "The command is terminated due to an error: can't load config: error in exclude rule #0: "
    "at least 2 of (text, source, path[-except], linters) should be set\n"
)
GCL_LINTER_ERROR = (
    'level=error msg="[linters_context] gocritic: invalid settings: '
    'enable-all and enabled-tags options must not be combined"\n'
)
REASON_CONFIG = (
    "can't load config: error in exclude rule #0: "
    "at least 2 of (text, source, path[-except], linters) should be set"
)
REASON_LINTER = (
    "gocritic: invalid settings: enable-all and enabled-tags options must not be combined"
)


class ReasonTests(unittest.TestCase):
    def test_config_error_line(self):
        self.assertEqual(reason_golangci(GCL_CONFIG_ERROR), REASON_CONFIG)

    def test_logger_fatal_drops_the_component_tag(self):
        # `[linters_context]` names the log component, not the problem.
        self.assertEqual(reason_golangci(GCL_LINTER_ERROR), REASON_LINTER)

    def test_quotes_inside_the_log_message_survive(self):
        raw = 'level=error msg="[linters_context] unsupported output path mode \\"rel\\""\n'
        self.assertEqual(reason_golangci(raw), 'unsupported output path mode "rel"')

    def test_a_clean_run_has_no_reason(self):
        self.assertIsNone(reason_golangci("0 issues.\n"))
        self.assertIsNone(reason_guff(""))

    def test_guff_prefix_is_stripped(self):
        self.assertEqual(
            reason_guff("guff: can't load config: invalid preset: stdErrorHandling\n"),
            "can't load config: invalid preset: stdErrorHandling",
        )

    def test_warnings_before_the_error_do_not_win(self):
        raw = (
            'level=warning msg="[linters_context] gocritic: no need to disable check"\n'
            + GCL_LINTER_ERROR
        )
        self.assertEqual(reason_golangci(raw), REASON_LINTER)


class CheckCaseTests(unittest.TestCase):
    def test_both_refuse_alike(self):
        problems = check_case(
            "c", REASON_CONFIG, f"guff: {REASON_CONFIG}\n", 2, GCL_CONFIG_ERROR, 3
        )
        self.assertEqual(problems, [])

    def test_guff_running_it_is_the_failure_the_tier_exists_for(self):
        problems = check_case("c", REASON_CONFIG, "0 issues\n", 0, GCL_CONFIG_ERROR, 3)
        self.assertEqual(len(problems), 1)
        self.assertIn("guff exited 0", problems[0])

    def test_guff_refusing_for_another_reason_fails(self):
        problems = check_case(
            "c", REASON_CONFIG, 'guff: linter "gofmt" is not available yet\n', 2,
            GCL_CONFIG_ERROR, 3,
        )
        self.assertEqual(len(problems), 1)
        self.assertIn("guff's reason differs", problems[0])

    def test_upstream_no_longer_refusing_fails_too(self):
        # A recorded expectation that upstream has stopped producing is a case
        # that has quietly stopped testing anything.
        problems = check_case(
            "c", REASON_CONFIG, f"guff: {REASON_CONFIG}\n", 2, "0 issues.\n", 0
        )
        self.assertEqual(len(problems), 1)
        self.assertIn("no longer refuses", problems[0])

    def test_upstream_reason_moving_fails(self):
        moved = "Error: can't load config: something else entirely\n"
        problems = check_case("c", REASON_CONFIG, f"guff: {REASON_CONFIG}\n", 2, moved, 3)
        self.assertTrue(any("reason moved" in p for p in problems))


class ControlTests(unittest.TestCase):
    def test_control_passes_when_both_run(self):
        self.assertEqual(check_accept("_control", 0, 0), [])

    def test_control_catches_a_tier_that_rejects_everything(self):
        self.assertEqual(len(check_accept("_control", 2, 0)), 1)
        self.assertEqual(len(check_accept("_control", 0, 3)), 1)
        self.assertEqual(len(check_accept("_control", 2, 3)), 2)


if __name__ == "__main__":
    unittest.main()
