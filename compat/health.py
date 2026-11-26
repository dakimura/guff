#!/usr/bin/env python3
"""Gate on failures that never reach the finding-set diff (Phase 1).

Two ways for guff to lose findings *silently* — the diff shows nothing because
the findings were never produced, and both sides look fine when the affected
code happens to have nothing to report:

**Worker panics.** A panicking analyzer unwinds its worker and the run
continues. Every finding that analyzer would have produced for that package is
gone. The S1032 panic on helm and kubernetes sat there through eight OSS targets
at P = R = 100%, and on kubernetes it was truncating the run badly enough that
34 further packages never even reached the ill-typed check.

**Ill-typed packages.** A package that fails type checking is skipped whole by
every analyzer without `run_despite_errors`, and its findings quietly become 0.

**Seed dependency cycles.** The seed's dependency graph is acyclic whenever
guff reads each import path's edges off the same variant whose files it
compiles. When it does not, the wave scheduler stops being topological and
dependencies get merged after the packages that needed them — which shows up,
one step later, as an ill-typed package. This catches it at the cause, and on
the runs where the wrong order happens not to break anything yet.

Panics and seed cycles are never acceptable, so they fail unconditionally.
Ill-typed counts are a property of the corpus as much as of guff, so they gate
against a recorded baseline: they may shrink freely, never grow.

Requires `GUFF_DEBUG_ILL_TYPED=1` in the environment of the guff run whose
stderr is being scanned — `compat/run.sh` sets it.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

_PANIC = re.compile(r"panicked at ([^\n]+?):\s*$|panicked at ([^\n]+)")
_ILL_TYPED = re.compile(r"^guff: ill_typed (\S+) \((\d+) errors\):")
_SEED_CYCLE = re.compile(r"^guff: seed dep cycle (.+)$")

DEFAULT_BASELINE = Path(__file__).resolve().parent / "baselines" / "health.json"


def scan(stderr_path: Path | str) -> dict:
    """Extract panic sites and ill-typed packages from one guff stderr dump."""
    panics: list[str] = []
    ill_typed: list[str] = []
    seed_cycles: list[str] = []
    empty: dict = {"panics": [], "ill_typed": [], "seed_cycles": []}
    try:
        text = Path(stderr_path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return empty
    for line in text.splitlines():
        m = _ILL_TYPED.match(line)
        if m:
            ill_typed.append(m.group(1))
            continue
        m = _SEED_CYCLE.match(line)
        if m:
            seed_cycles.append(m.group(1))
            continue
        if "panicked at " in line:
            site = line.split("panicked at ", 1)[1].strip().rstrip(":")
            panics.append(site)
    # Same package can be reported by several analyzers; count distinct.
    return {
        "panics": panics,
        "ill_typed": sorted(set(ill_typed)),
        "seed_cycles": sorted(set(seed_cycles)),
    }


def load_baseline(path: Path | str) -> dict:
    p = Path(path)
    if not p.is_file():
        return {"targets": {}}
    data = json.loads(p.read_text(encoding="utf-8"))
    data.setdefault("targets", {})
    return data


def baseline_for(baseline: dict, target: str) -> int:
    """Ill-typed allowance for `target`. Unknown targets start strict at 0."""
    return int((baseline["targets"].get(target) or {}).get("ill_typed", 0))


def check(target: str, found: dict, baseline: dict) -> tuple[bool, list[str]]:
    allowed = baseline_for(baseline, target)
    n_ill = len(found["ill_typed"])
    problems: list[str] = []

    if found["panics"]:
        sites = sorted(set(found["panics"]))
        problems.append(f"{len(found['panics'])} worker panic(s): " + ", ".join(sites[:5]))
    if found.get("seed_cycles"):
        edges = found["seed_cycles"]
        problems.append(
            f"{len(edges)} seed dep cycle edge(s): " + "; ".join(edges[:3])
        )
    if n_ill > allowed:
        new = found["ill_typed"][:5]
        problems.append(
            f"ill-typed packages {n_ill} > baseline {allowed}; e.g. " + ", ".join(new)
        )
    return (not problems, problems)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_scan = sub.add_parser("scan", help="Print panic / ill-typed findings as JSON")
    p_scan.add_argument("stderr")

    p_check = sub.add_parser("check", help="Gate one target's stderr against the baseline")
    p_check.add_argument("--target", required=True)
    p_check.add_argument("--stderr", required=True)
    p_check.add_argument("--baseline", default=str(DEFAULT_BASELINE))
    p_check.add_argument(
        "--update",
        action="store_true",
        help="Record the observed ill-typed count as this target's baseline",
    )

    args = ap.parse_args(argv)

    if args.cmd == "scan":
        print(json.dumps(scan(args.stderr), indent=2))
        return 0

    found = scan(args.stderr)
    baseline_path = Path(args.baseline)
    baseline = load_baseline(baseline_path)

    if args.update:
        n = len(found["ill_typed"])
        old = baseline_for(baseline, args.target) if args.target in baseline["targets"] else None
        if n == 0:
            # Zero is the default for unknown targets, so recording it would add
            # a row per isolate fixture and bury the handful that matter.
            baseline["targets"].pop(args.target, None)
        else:
            baseline["targets"].setdefault(args.target, {})["ill_typed"] = n
        baseline_path.parent.mkdir(parents=True, exist_ok=True)
        baseline_path.write_text(
            json.dumps(baseline, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        if found["panics"] or found.get("seed_cycles"):
            # Never bake a panic or a broken seed order into a baseline — there
            # is no field for either.
            print(
                f"  {args.target}: PANIC / seed dep cycle still present; "
                "baseline not a fix",
                file=sys.stderr,
            )
            return 1
        print(f"  {args.target}: ill_typed baseline {old} -> {n}")
        return 0

    ok, problems = check(args.target, found, baseline)
    if ok:
        n = len(found["ill_typed"])
        if n:
            print(f"  {args.target}: health OK (ill-typed {n}, at baseline)")
        return 0
    for p in problems:
        print(f"  {args.target}: {p}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
