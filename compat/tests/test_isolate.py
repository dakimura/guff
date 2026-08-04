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
