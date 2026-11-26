#!/usr/bin/env python3
"""Unit tests for compat/drift.py (COMPAT-HARDENING Phase 7).

The parts worth testing are the ones with no golangci-lint in them: what counts
as drift, and what counts as *reviewed* drift. A ledger that accidentally
accepts a change nobody read would turn the weekly job into a green light for
whatever upstream did.
"""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "golden"))

from drift import (  # noqa: E402
    WHY_PLACEHOLDER,
    CaseDrift,
    diff_lists,
    inventory_drift,
    ledger_verdict,
    pinned_version,
)


class PinTests(unittest.TestCase):
    def test_pin_is_read_from_pins_json(self):
        pins = json.loads((ROOT / "pins.json").read_text(encoding="utf-8"))
        self.assertEqual(pinned_version(), pins["golangci_lint"].lstrip("v"))

    def test_pin_carries_no_leading_v(self):
        # install.sh wants `v2.12.2`, `version --short` prints `2.12.2`, and the
        # comparison happens on the latter. One spelling, normalized on read.
        self.assertFalse(pinned_version().startswith("v"))


class DiffTests(unittest.TestCase):
    # Real golden keys: the diff sorts by position, so `a`/`b` would not survive
    # `sort_key`. Using the shape the tool actually sees is also the only way
    # this test would notice the key format changing under it.
    A = "a.go:1:1:gosec:medium:G124: x"
    B = "a.go:2:1:gosec:medium:G124: y"
    C = "b.go:1:1:govet::inline: z"

    def test_added_and_removed_are_named_from_the_candidate_side(self):
        added, removed = diff_lists([self.A, self.B], [self.B, self.C])
        self.assertEqual((added, removed), ([self.C], [self.A]))

    def test_duplicate_keys_are_a_multiset(self):
        added, removed = diff_lists([self.A], [self.A, self.A])
        self.assertEqual((added, removed), ([self.A], []))


class InventoryTests(unittest.TestCase):
    PIN = {
        "linter:gosec": {"groups": ["all"], "deprecated": False, "autoFix": False,
                         "fast": False, "since": "v1.0.0"},
        "linter:wsl": {"groups": ["all"], "deprecated": True, "autoFix": True,
                       "fast": False, "since": "v1.20.0"},
    }

    def test_added_and_removed_linters(self):
        cand = dict(self.PIN)
        cand.pop("linter:wsl")
        cand["linter:brandnew"] = {"groups": ["all"], "deprecated": False,
                                   "autoFix": False, "fast": False, "since": "v2.13.0"}
        d = inventory_drift(self.PIN, cand)
        self.assertEqual(d["added"], ["linter:brandnew"])
        self.assertEqual(d["removed"], ["linter:wsl"])
        self.assertEqual(d["changed"], [])

    def test_a_linter_becoming_deprecated_is_drift(self):
        cand = {k: dict(v) for k, v in self.PIN.items()}
        cand["linter:gosec"]["deprecated"] = True
        d = inventory_drift(self.PIN, cand)
        self.assertEqual(d, {"added": [], "removed": [],
                             "changed": ["linter:gosec: deprecated False -> True"]})

    def test_group_membership_is_drift(self):
        # `standard` membership decides what `linters.default: standard` means
        # for every user, without a single finding changing on our fixtures.
        cand = {k: dict(v) for k, v in self.PIN.items()}
        cand["linter:gosec"]["groups"] = ["all", "standard"]
        self.assertEqual(len(inventory_drift(self.PIN, cand)["changed"]), 1)

    def test_since_is_not_drift(self):
        # It records when a linter first appeared; it moves with no behaviour.
        cand = {k: dict(v) for k, v in self.PIN.items()}
        cand["linter:gosec"]["since"] = "v9.9.9"
        self.assertEqual(inventory_drift(self.PIN, cand)["changed"], [])


EMPTY_INV = {"added": [], "removed": [], "changed": []}


class LedgerTests(unittest.TestCase):
    def drifted(self):
        d = CaseDrift(name="gosec")
        d.upstream_removed = ["a.go:1:1:gosec:medium:G124: ..."]
        return d

    def test_no_drift_needs_no_ledger(self):
        self.assertEqual(ledger_verdict({}, "2.12.2", "2.13.0", [CaseDrift("x")], EMPTY_INV), [])

    def test_drift_without_a_ledger_is_unreviewed(self):
        v = ledger_verdict({}, "2.12.2", "2.13.0", [self.drifted()], EMPTY_INV)
        self.assertTrue(v)

    def test_a_ledger_for_another_candidate_does_not_carry_over(self):
        ledger = {
            "pin": "2.12.2",
            "candidate": "2.13.0",
            "cases": {"gosec": {"signature": self.drifted().signature(), "why": "read it"}},
        }
        v = ledger_verdict(ledger, "2.12.2", "2.14.0", [self.drifted()], EMPTY_INV)
        self.assertTrue(v, "reviewing 2.13.0 says nothing about 2.14.0")

    def test_matching_ledger_clears_the_drift(self):
        d = self.drifted()
        ledger = {
            "pin": "2.12.2",
            "candidate": "2.13.0",
            "cases": {"gosec": {"signature": d.signature(), "why": "upstream dropped G999"}},
        }
        self.assertEqual(ledger_verdict(ledger, "2.12.2", "2.13.0", [d], EMPTY_INV), [])

    def test_a_placeholder_why_is_not_a_review(self):
        # `--update` writes every `why` as a placeholder and tells the reviewer
        # to fill them in. Nothing enforced it, so a ledger committed straight
        # out of `--update` silenced the weekly job while recording nothing.
        # Found on Phase 7's first real `--update` run (COMPAT-HARDENING §4,
        # 2026-08-13).
        d = self.drifted()
        for why in (WHY_PLACEHOLDER, "", "   ", "TODO", "todo: later", None):
            ledger = {
                "pin": "2.12.2",
                "candidate": "2.13.0",
                "cases": {"gosec": {"signature": d.signature(), "why": why}},
            }
            self.assertTrue(
                ledger_verdict(ledger, "2.12.2", "2.13.0", [d], EMPTY_INV),
                f"{why!r} should not count as reviewed",
            )

    def test_a_placeholder_inventory_why_is_not_a_review(self):
        inv = {"added": ["linter:brandnew"], "removed": [], "changed": []}
        ledger = {
            "pin": "2.12.2",
            "candidate": "2.13.0",
            "cases": {},
            "inventory": {"signature": inv, "why": WHY_PLACEHOLDER},
        }
        self.assertTrue(ledger_verdict(ledger, "2.12.2", "2.13.0", [], inv))
        ledger["inventory"]["why"] = "brandnew is new in 2.13.0; guff does not implement it yet"
        self.assertEqual(ledger_verdict(ledger, "2.12.2", "2.13.0", [], inv), [])

    def test_more_drift_than_was_reviewed_is_unreviewed_again(self):
        reviewed = self.drifted()
        ledger = {
            "pin": "2.12.2",
            "candidate": "2.13.0",
            "cases": {"gosec": {"signature": reviewed.signature(), "why": "read it"}},
        }
        worse = self.drifted()
        worse.upstream_added = ["a.go:2:1:gosec:high:G999: something new"]
        self.assertTrue(ledger_verdict(ledger, "2.12.2", "2.13.0", [worse], EMPTY_INV))

    def test_a_rejected_config_is_its_own_signature(self):
        # A settings key upstream dropped makes golangci-lint exit non-zero on a
        # config guff still accepts — drift no finding-set comparison can see.
        d = CaseDrift(name="revive-settings", candidate_rejected=True)
        self.assertTrue(d.drifted)
        self.assertEqual(d.signature(), {"config_rejected_by_candidate": True})

    def test_inventory_drift_alone_is_unreviewed(self):
        inv = {"added": ["linter:brandnew"], "removed": [], "changed": []}
        self.assertTrue(ledger_verdict({}, "2.12.2", "2.13.0", [], inv))


if __name__ == "__main__":
    unittest.main()
