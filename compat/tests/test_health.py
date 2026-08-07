#!/usr/bin/env python3
"""Unit tests for compat/health.py."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from health import baseline_for, check, load_baseline, scan  # noqa: E402

PANIC = (
    "thread '<unnamed>' (29083944) panicked at crates/guff-staticcheck/src/s1032.rs:15:46:\n"
    "index out of bounds: the len is 0 but the index is 0\n"
    "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n"
)
ILL = (
    "guff: ill_typed k8s.io/apimachinery/pkg/api/meta (3 errors):\n"
    "  /x/y.go:1:2: nope (Type)\n"
    "guff: ill_typed k8s.io/apimachinery/pkg/runtime (1 errors):\n"
)


def write(text: str) -> str:
    fh = tempfile.NamedTemporaryFile("w", suffix=".stderr", delete=False)
    fh.write(text)
    fh.close()
    return fh.name


class ScanTests(unittest.TestCase):
    def test_extracts_panic_site(self):
        got = scan(write(PANIC))
        self.assertEqual(got["panics"], ["crates/guff-staticcheck/src/s1032.rs:15:46"])

    def test_extracts_ill_typed_packages(self):
        got = scan(write(ILL))
        self.assertEqual(
            got["ill_typed"],
            ["k8s.io/apimachinery/pkg/api/meta", "k8s.io/apimachinery/pkg/runtime"],
        )

    def test_ill_typed_is_deduplicated(self):
        """Several analyzers skip the same package; it is still one package."""
        got = scan(write(ILL + ILL))
        self.assertEqual(len(got["ill_typed"]), 2)

    def test_backtrace_note_is_not_a_second_panic(self):
        self.assertEqual(len(scan(write(PANIC))["panics"]), 1)

    def test_clean_stderr(self):
        self.assertEqual(scan(write("")), {"panics": [], "ill_typed": []})

    def test_missing_file_is_not_an_error(self):
        self.assertEqual(scan("/nonexistent/nope.stderr"), {"panics": [], "ill_typed": []})


class CheckTests(unittest.TestCase):
    BASE = {"targets": {"consul": {"ill_typed": 14}}}

    def test_panic_always_fails_even_with_headroom(self):
        found = {"panics": ["a.rs:1:1"], "ill_typed": []}
        ok, problems = check("consul", found, self.BASE)
        self.assertFalse(ok)
        self.assertIn("panic", problems[0])

    def test_ill_typed_growth_fails(self):
        found = {"panics": [], "ill_typed": [f"p{i}" for i in range(15)]}
        ok, _ = check("consul", found, self.BASE)
        self.assertFalse(ok)

    def test_ill_typed_at_baseline_passes(self):
        found = {"panics": [], "ill_typed": [f"p{i}" for i in range(14)]}
        self.assertTrue(check("consul", found, self.BASE)[0])

    def test_ill_typed_shrink_passes(self):
        found = {"panics": [], "ill_typed": ["p0"]}
        self.assertTrue(check("consul", found, self.BASE)[0])

    def test_unknown_target_is_strict(self):
        """A new target starts at zero rather than silently unbounded."""
        self.assertEqual(baseline_for(self.BASE, "brand-new"), 0)
        found = {"panics": [], "ill_typed": ["p0"]}
        self.assertFalse(check("brand-new", found, self.BASE)[0])


class BaselineFileTests(unittest.TestCase):
    def test_missing_baseline_reads_as_empty(self):
        self.assertEqual(load_baseline("/nonexistent/health.json"), {"targets": {}})

    def test_committed_baseline_is_wellformed(self):
        path = ROOT / "baselines" / "health.json"
        data = json.loads(path.read_text(encoding="utf-8"))
        self.assertIn("targets", data)
        for target, entry in data["targets"].items():
            self.assertIsInstance(entry.get("ill_typed"), int, target)
            # Zero is the default, so an explicit zero row is dead weight.
            self.assertGreater(entry["ill_typed"], 0, target)


if __name__ == "__main__":
    unittest.main()
