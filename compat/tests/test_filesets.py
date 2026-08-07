#!/usr/bin/env python3
"""Unit tests for compat/filesets.py."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from filesets import PROBE_TEMPLATE, build_config, load_lenient  # noqa: E402


class BuildConfigTests(unittest.TestCase):
    def test_enables_only_the_probe(self):
        cfg = build_config(None)
        self.assertEqual(cfg["linters"]["default"], "none")
        self.assertEqual(cfg["linters"]["enable"], ["goheader"])
        self.assertEqual(
            cfg["linters"]["settings"]["goheader"]["template"], PROBE_TEMPLATE
        )

    def test_caps_and_uniq_would_eat_the_signal(self):
        cfg = build_config({"issues": {"max-same-issues": 3, "uniq-by-line": True}})
        self.assertEqual(cfg["issues"]["max-issues-per-linter"], 0)
        self.assertEqual(cfg["issues"]["max-same-issues"], 0)
        self.assertIs(cfg["issues"]["uniq-by-line"], False)

    def test_keeps_run_block(self):
        """build-tags / tests / go version are what decide the file set."""
        cfg = build_config({"run": {"build-tags": ["integration"], "tests": False}})
        self.assertEqual(cfg["run"]["build-tags"], ["integration"])
        self.assertIs(cfg["run"]["tests"], False)

    def test_defaults_tests_on_when_unset(self):
        self.assertIs(build_config(None)["run"]["tests"], True)

    def test_keeps_linter_exclusions(self):
        """Whether a file is excluded is part of 'was it analyzed'."""
        src = {"linters": {"default": "all", "exclusions": {"paths": ["vendor"]}}}
        cfg = build_config(src)
        self.assertEqual(cfg["linters"]["exclusions"], {"paths": ["vendor"]})
        self.assertEqual(cfg["linters"]["enable"], ["goheader"])

    def test_drops_formatters_and_severity(self):
        cfg = build_config({"formatters": {"enable": ["gofmt"]}, "severity": {"default": "error"}})
        self.assertNotIn("formatters", cfg)
        self.assertNotIn("severity", cfg)


class LenientLoadTests(unittest.TestCase):
    def _write(self, text: str) -> Path:
        fh = tempfile.NamedTemporaryFile("w", suffix=".yml", delete=False)
        fh.write(text)
        fh.close()
        return Path(fh.name)

    def test_trailing_tab_is_tolerated(self):
        """kubernetes' config has `gocritic:\\t`; Go's YAML accepts it, PyYAML does not."""
        p = self._write("linters:\n  settings:\n    gocritic:\t\n      enable-all: true\n")
        data = load_lenient(p)
        self.assertIsNotNone(data)
        self.assertIn("linters", data)

    def test_plain_config_loads(self):
        p = self._write("run:\n  tests: false\n")
        self.assertEqual(load_lenient(p), {"run": {"tests": False}})

    def test_non_mapping_is_none(self):
        self.assertIsNone(load_lenient(self._write("- a\n- b\n")))


if __name__ == "__main__":
    unittest.main()
