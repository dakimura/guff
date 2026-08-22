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
    BASELINES = ("health.json", "health-hunt.json")

    def test_missing_baseline_reads_as_empty(self):
        self.assertEqual(load_baseline("/nonexistent/health.json"), {"targets": {}})

    def test_committed_baselines_are_wellformed(self):
        for name in self.BASELINES:
            data = json.loads((ROOT / "baselines" / name).read_text(encoding="utf-8"))
            self.assertIn("targets", data, name)
            for target, entry in data["targets"].items():
                self.assertIsInstance(entry.get("ill_typed"), int, f"{name}:{target}")
                # Zero is the default, so an explicit zero row is dead weight.
                self.assertGreater(entry["ill_typed"], 0, f"{name}:{target}")

    def test_tiers_do_not_share_baseline_rows(self):
        """A hunt refresh must not be able to rewrite a gated OSS number."""
        rows = []
        for name in self.BASELINES:
            data = json.loads((ROOT / "baselines" / name).read_text(encoding="utf-8"))
            rows.append(set(data["targets"]))
        self.assertEqual(rows[0] & rows[1], set())


class WiringTests(unittest.TestCase):
    """Every tier that runs guff over real Go must reach this gate.

    An analyzer that panics, and a package that fails type checking, both lose
    findings *without producing a diff*: the set-diff shows a run of
    golangci-only findings with no linter in common, or nothing at all. The
    gate exists precisely because that is invisible, so the way it fails is by
    not being wired into a tier at all — which is what happened to `hunt.sh`
    from the day the tier was added until 2026-08-22, and cost five linters'
    findings on syncthing's `lib/model` before anyone looked.
    """

    TIERS = ("run.sh", "hunt.sh", "golden/run.sh")

    def script(self, rel: str) -> str:
        return (ROOT / rel).read_text(encoding="utf-8")

    def test_every_tier_asks_guff_to_name_ill_typed_packages(self):
        for rel in self.TIERS:
            self.assertIn("GUFF_DEBUG_ILL_TYPED=1", self.script(rel), rel)

    def test_every_tier_checks_the_stderr_it_captured(self):
        for rel in self.TIERS:
            text = self.script(rel)
            self.assertIn("health.py", text, rel)
            self.assertIn("check", text, rel)

    def test_every_tier_fails_the_run_on_a_health_failure(self):
        """Counting a failure and then exiting 0 would be worse than no gate."""
        for rel in ("run.sh", "hunt.sh"):
            text = self.script(rel)
            self.assertIn("HEALTH_FAILED=$((HEALTH_FAILED + 1))", text, rel)
            self.assertRegex(text, r"HEALTH_FAILED.*-gt 0", rel)


if __name__ == "__main__":
    unittest.main()
