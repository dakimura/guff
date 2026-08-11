#!/usr/bin/env python3
"""Unit tests for compat/isolate/make_config.py."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "isolate"))

from make_config import make_config  # noqa: E402


class MakeConfigTests(unittest.TestCase):
    def test_enables_single_linter(self):
        text = make_config("errcheck")
        self.assertIn('default: none', text)
        self.assertIn("- errcheck", text)
        self.assertIn("max-issues-per-linter: 0", text)
        self.assertIn("uniq-by-line: false", text)
        self.assertIn('version: "2"', text)

    def test_rejects_blank(self):
        with self.assertRaises(ValueError):
            make_config("  ")

    def test_write_output(self):
        with tempfile.TemporaryDirectory() as td:
            out = Path(td) / "errcheck.yml"
            from make_config import main

            self.assertEqual(main(["errcheck", "-o", str(out)]), 0)
            self.assertTrue(out.is_file())
            self.assertIn("- errcheck", out.read_text(encoding="utf-8"))

    def test_settings_overlay(self):
        with tempfile.TemporaryDirectory() as td:
            settings = Path(td) / "settings.yml"
            settings.write_text(
                "disable-dec-order-check: false\n",
                encoding="utf-8",
            )
            out = Path(td) / "decorder.yml"
            from make_config import main

            self.assertEqual(
                main(["decorder", "--settings", str(settings), "-o", str(out)]),
                0,
            )
            text = out.read_text(encoding="utf-8")
            self.assertIn("settings:", text)
            self.assertIn("decorder:", text)
            self.assertIn("disable-dec-order-check: false", text)


class FormatterConfigTests(unittest.TestCase):
    """golangci-lint v2 moved the formatters into a block of their own.

    `linters.enable: [golines]` is a config error there, not a no-op, so a
    formatter target needs a different template — which is why `golines` and
    `swaggo` had no isolate target at all.
    """

    def test_formatter_uses_formatters_block(self):
        text = make_config("golines")
        self.assertIn("formatters:\n  enable:\n    - golines", text)
        self.assertIn("linters:\n  default: none", text)
        # A formatter must not also be listed as a linter.
        self.assertNotIn("  enable:\n    - golines\n\nissues", text.split("formatters:")[0])

    def test_formatter_settings_land_under_formatters(self):
        text = make_config("golines", {"golines": {"max-len": 60}})
        formatters_block = text.split("formatters:", 1)[1]
        self.assertIn("settings:", formatters_block)
        self.assertIn("max-len: 60", formatters_block)

    def test_linter_still_uses_linters_block(self):
        text = make_config("errcheck")
        self.assertIn("linters:\n  default: none\n  enable:\n    - errcheck", text)
        self.assertNotIn("formatters:", text)


class LintersManifestTests(unittest.TestCase):
    def test_smoke_linters_have_fixtures(self):
        path = ROOT / "isolate" / "linters.txt"
        fixtures = ROOT / "isolate" / "fixtures"
        smoke = []
        for raw in path.read_text(encoding="utf-8").splitlines():
            line = raw.split("#", 1)[0].strip()
            if not line:
                continue
            parts = line.split()
            name = parts[0]
            tier = parts[1] if len(parts) > 1 else "full"
            fixture = fixtures / name
            self.assertTrue(fixture.is_dir(), f"missing fixture for {name}")
            self.assertTrue((fixture / "go.mod").is_file(), f"missing go.mod for {name}")
            if tier == "smoke":
                smoke.append(name)
        self.assertEqual(
            smoke,
            ["errcheck", "ineffassign", "unused", "govet", "staticcheck"],
        )


if __name__ == "__main__":
    unittest.main()
