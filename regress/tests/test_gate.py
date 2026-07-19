#!/usr/bin/env python3
"""Unit tests for regress/gate.py and measure.py parsers."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from gate import (  # noqa: E402
    DEFAULT_TOLERANCES,
    build_baseline,
    evaluate,
)
from measure import parse_time_output  # noqa: E402


def _baseline(**overrides):
    doc = {
        "prometheus_git_sha": "abc",
        "config": ".golangci.yml",
        "guff": {"wall_seconds": 10.0, "peak_rss_bytes": 5_000_000_000},
        "compat": {
            "guff_issues": 100,
            "golangci_issues": 80,
            "both": 70,
            "guff_only": 30,
            "golangci_only": 10,
            "precision": 0.7,
            "recall": 0.875,
        },
        "tolerances": dict(DEFAULT_TOLERANCES),
    }
    doc.update(overrides)
    return doc


def _measured(**overrides):
    doc = {
        "prometheus_git_sha": "abc",
        "config": ".golangci.yml",
        "guff": {"wall_seconds": 10.0, "peak_rss_bytes": 5_000_000_000},
        "compat": {
            "guff_issues": 100,
            "golangci_issues": 80,
            "both": 70,
            "guff_only": 30,
            "golangci_only": 10,
            "precision": 0.7,
            "recall": 0.875,
        },
    }
    for k, v in overrides.items():
        if k in ("guff", "compat") and isinstance(v, dict):
            doc[k] = {**doc[k], **v}
        else:
            doc[k] = v
    return doc


class ParseTimeTests(unittest.TestCase):
    def test_darwin(self):
        stderr = """
real 12.345
user 30.0
sys 1.0
             1234567  maximum resident set size
                     0  average shared memory size
"""
        wall, rss = parse_time_output(stderr, system="Darwin")
        self.assertAlmostEqual(wall, 12.345)
        self.assertEqual(rss, 1234567)

    def test_linux(self):
        stderr = """
	Command being timed: "guff"
	User time (seconds): 1.00
	Elapsed (wall clock) time (h:mm:ss or m:ss): 0:15.50
	Maximum resident set size (kbytes): 4096
"""
        wall, rss = parse_time_output(stderr, system="Linux")
        self.assertAlmostEqual(wall, 15.50)
        self.assertEqual(rss, 4096 * 1024)

    def test_linux_hours(self):
        stderr = """
	Elapsed (wall clock) time (h:mm:ss or m:ss): 1:02:03.5
	Maximum resident set size (kbytes): 100
"""
        wall, rss = parse_time_output(stderr, system="Linux")
        self.assertAlmostEqual(wall, 3723.5)
        self.assertEqual(rss, 100 * 1024)


class GateTests(unittest.TestCase):
    def test_pass_equal(self):
        self.assertEqual(evaluate(_baseline(), _measured()), [])

    def test_pass_improvement(self):
        m = _measured(guff={"wall_seconds": 5.0, "peak_rss_bytes": 1_000_000_000})
        m["compat"]["guff_only"] = 10
        m["compat"]["golangci_only"] = 5
        m["compat"]["both"] = 80
        self.assertEqual(evaluate(_baseline(), m), [])

    def test_fail_wall(self):
        # baseline 10s × 1.25 = 12.5; 13 fails
        fails = evaluate(_baseline(), _measured(guff={"wall_seconds": 13.0}))
        self.assertTrue(any(f.metric == "wall_seconds" for f in fails))

    def test_pass_wall_within_tolerance(self):
        fails = evaluate(_baseline(), _measured(guff={"wall_seconds": 12.0}))
        self.assertFalse(any(f.metric == "wall_seconds" for f in fails))

    def test_fail_rss(self):
        # 5e9 × 1.2 = 6e9; 6.1e9 fails
        fails = evaluate(
            _baseline(),
            _measured(guff={"peak_rss_bytes": 6_100_000_000}),
        )
        self.assertTrue(any(f.metric == "peak_rss_bytes" for f in fails))

    def test_fail_guff_only_increase(self):
        fails = evaluate(_baseline(), _measured(compat={"guff_only": 31}))
        self.assertTrue(any(f.metric == "guff_only" for f in fails))

    def test_fail_golangci_only_increase(self):
        fails = evaluate(_baseline(), _measured(compat={"golangci_only": 11}))
        self.assertTrue(any(f.metric == "golangci_only" for f in fails))

    def test_fail_both_decrease(self):
        fails = evaluate(_baseline(), _measured(compat={"both": 69}))
        self.assertTrue(any(f.metric == "both" for f in fails))

    def test_build_baseline_preserves_tolerances(self):
        prev = _baseline()
        prev["tolerances"]["wall_seconds_ratio"] = 1.5
        measured = _measured(guff={"wall_seconds": 9.0})
        measured["packages"] = ["./tsdb/..."]
        measured["concurrency"] = 1
        measured["rayon_threads"] = 2
        measured["isolate_gocache"] = False
        doc = build_baseline(measured, prometheus_git_sha="def", previous=prev)
        self.assertEqual(doc["prometheus_git_sha"], "def")
        self.assertEqual(doc["guff"]["wall_seconds"], 9.0)
        self.assertEqual(doc["tolerances"]["wall_seconds_ratio"], 1.5)
        self.assertEqual(doc["packages"], ["./tsdb/..."])
        self.assertEqual(doc["concurrency"], 1)


if __name__ == "__main__":
    unittest.main()
