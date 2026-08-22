#!/usr/bin/env python3
"""Unit tests for corpus/patch_unlimited_issues.py.

The patched copy is what *both* tools run, so a key this script gets wrong is
a difference neither tool caused and no allowlist explains.
"""

from __future__ import annotations

import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "corpus"))

from patch_unlimited_issues import patch  # noqa: E402

WITH_ISSUES = """version: "2"

linters:
  default: none

issues:
  uniq-by-line: true
  max-same-issues: 3

run:
  timeout: 5m
"""

NO_ISSUES = """version: "2"

linters:
  default: none
"""


def value_of(text: str, key: str) -> str | None:
    m = re.search(rf"^\s+{re.escape(key)}:\s*(\S+)\s*$", text, re.M)
    return m.group(1) if m else None


def count_of(text: str, key: str) -> int:
    return len(re.findall(rf"^\s+{re.escape(key)}:", text, re.M))


class CapsTests(unittest.TestCase):
    def test_caps_are_zeroed_in_an_existing_issues_block(self):
        out = patch(WITH_ISSUES)
        self.assertEqual(value_of(out, "max-issues-per-linter"), "0")
        self.assertEqual(value_of(out, "max-same-issues"), "0")

    def test_caps_are_added_when_there_is_no_issues_block(self):
        out = patch(NO_ISSUES)
        self.assertIn("issues:", out)
        self.assertEqual(value_of(out, "max-issues-per-linter"), "0")
        self.assertEqual(value_of(out, "max-same-issues"), "0")

    def test_the_rest_of_the_config_survives(self):
        out = patch(WITH_ISSUES)
        self.assertIn("default: none", out)
        self.assertIn("timeout: 5m", out)


class UniqByLineTests(unittest.TestCase):
    def test_default_leaves_the_config_alone(self):
        """The OSS tier shares this script and must keep upstream's default."""
        self.assertEqual(value_of(patch(WITH_ISSUES), "uniq-by-line"), "true")
        self.assertIsNone(value_of(patch(NO_ISSUES), "uniq-by-line"))

    def test_false_overrides_an_explicit_true(self):
        self.assertEqual(value_of(patch(WITH_ISSUES, False), "uniq-by-line"), "false")

    def test_false_is_added_when_the_config_never_mentions_it(self):
        """ON is the default, so silence is not the same as off."""
        self.assertEqual(value_of(patch(NO_ISSUES, False), "uniq-by-line"), "false")

    def test_the_key_is_written_once(self):
        for src in (WITH_ISSUES, NO_ISSUES):
            self.assertEqual(count_of(patch(src, False), "uniq-by-line"), 1)

    def test_true_can_be_forced_back_on(self):
        self.assertEqual(value_of(patch(NO_ISSUES, True), "uniq-by-line"), "true")


class WiringTests(unittest.TestCase):
    """hunt compares whole repos, where the survivor of a shared line is not a
    claim either tool is making — see the header of the patch script."""

    def test_hunt_turns_uniq_by_line_off(self):
        text = (ROOT / "compat" / "hunt.sh").read_text(encoding="utf-8")
        self.assertRegex(text, r"patch_unlimited[^\n]*--uniq-by-line false|PATCH_UNLIMITED[^\n]*--uniq-by-line false")

    def test_the_oss_tier_does_not(self):
        """Its allowlists are recorded against upstream's default."""
        text = (ROOT / "compat" / "run.sh").read_text(encoding="utf-8")
        self.assertNotIn("--uniq-by-line", text)


if __name__ == "__main__":
    unittest.main()
