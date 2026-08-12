#!/usr/bin/env python3
"""Differential fuzzing over the golden fixtures (Phase 6).

The golden cases are hand-written, and a hand-written fixture only ever contains
shapes somebody thought to write. Most of the bugs in COMPAT-HARDENING §4 were
in shapes nobody would have: a comment where the analysis AST had none (found
eight separate times, once per linter), a receiver spelled by debug-printing the
AST, a column measured in runes against a length measured in bytes.

This mutates those fixtures and asks the only question that needs no oracle:
**do the two tools still agree?**

    seed fixture  ->  mutate  ->  go build  ->  guff vs golangci-lint
                                     |                    |
                                  reject if           report if
                                  it broke            they differ

A mutation has one obligation — the result must compile. It does **not** have to
preserve findings. That is what makes the technique cheap: no mutation needs an
argument that it is semantics-preserving, because the comparison is between the
two tools on the same mutant, not between the mutant and the original. A
mutation that changes every finding in the file is a perfectly good test.

Seeds whose baseline already differs (the cases carrying a `ratchet.json`) are
skipped by default. Their known diff would appear in every mutant and drown the
signal; `--allow-dirty-seeds` includes them and compares diff *counts* instead.

## Usage

    compat/fuzz.py                       # every clean golden case, 50 mutants each
    compat/fuzz.py --case gocritic -n 200
    compat/fuzz.py --seed 7 --mutations 3   # 3 edits per mutant, reproducible

Findings are written to `compat/results/fuzz-<stamp>/`, one directory per
disagreement, each holding the mutated sources and both tools' output — which is
the input `compat/reduce.py` takes.
"""

from __future__ import annotations

import argparse
import json
import re
import os
import random
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent / "golden"))
from golden import issue_key as golden_key  # noqa: E402  (compat/golden/golden.py)
from normalize import load_issues  # noqa: E402
from reduce import Edit, Spanner, Tree, apply_edits, resolve_guff  # noqa: E402

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
CASES = HERE / "golden" / "cases"
RESULTS = HERE / "results"

_SPANNER = Spanner()


# --------------------------------------------------------------------------
# Seeds
# --------------------------------------------------------------------------


@dataclass
class Case:
    name: str
    dir: Path
    config: Path
    env: dict[str, str]

    @property
    def has_ratchet(self) -> bool:
        return (self.dir / "ratchet.json").is_file()


def load_cases(filter_name: str | None) -> list[Case]:
    out: list[Case] = []
    for d in sorted(CASES.iterdir()):
        if not d.is_dir() or not (d / "config.yml").is_file():
            continue
        if filter_name and d.name != filter_name:
            continue
        env: dict[str, str] = {}
        envfile = d / "env"
        if envfile.is_file():
            for raw in envfile.read_text(encoding="utf-8").splitlines():
                line = raw.split("#", 1)[0].strip()
                if line and "=" in line:
                    k, v = line.split("=", 1)
                    env[k] = v
        out.append(Case(name=d.name, dir=d, config=d / "config.yml", env=env))
    return out


def materialize(case: Case, work: Path) -> None:
    """Lay a case out as a module, the way `compat/golden/run.sh` does.

    Kept in step with that script deliberately: the fuzzer must start from
    exactly the tree the gate compares, or a disagreement it reports might be an
    artefact of a differently-assembled module rather than a guff bug.
    """
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    shutil.copy2(case.dir / "go.mod", work / "go.mod")
    for raw in (case.dir / "sources.txt").read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        # Two or more spaces separate the columns; revive's fixture is a file
        # literally named "bad file.go", so a single space cannot be the split.
        parts = [p for p in line.split("  ") if p.strip()]
        if len(parts) < 2:
            raise SystemExit(f"{case.name}: bad sources.txt line: {raw}")
        dest, src = parts[0].strip(), parts[-1].strip()
        s = ROOT / src
        if not s.is_file():
            raise SystemExit(f"{case.name}: missing source {src}")
        d = work / dest
        d.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(s, d)


# --------------------------------------------------------------------------
# Running the two tools
# --------------------------------------------------------------------------


class Runner:
    def __init__(self, guff: str, golangci: str, timeout: str = "5m") -> None:
        self.guff = guff
        self.golangci = golangci
        self.timeout = timeout
        self.guff_time = 0.0
        self.gcl_time = 0.0

    def _keys(self, raw: str, root: Path) -> list[str]:
        tmp = root / ".fuzz-issues.json"
        tmp.write_text(raw, encoding="utf-8")
        try:
            issues = load_issues(tmp)
        except Exception:
            return []
        finally:
            tmp.unlink(missing_ok=True)
        return sorted(golden_key(i, str(root)) for i in issues)

    # `go build` links every `package main` it finds, and a fixture that exists
    # to hold one check rarely bothers to declare `func main`. That is a link
    # error, not a compile error — the package type-checked fine — so treating
    # it as "does not build" locked the fuzzer out of `staticcheck-sa`,
    # `staticcheck-s` and `staticcheck-go114`, which between them carry 160 of
    # the checks worth mutating.
    _LINK_ONLY = re.compile(r"function main is undeclared in the main package")

    def build(self, work: Path, env: dict[str, str]) -> tuple[bool, str]:
        e = {**os.environ, **env}
        r = subprocess.run(
            ["go", "build", "./..."], cwd=work, capture_output=True, text=True, env=e
        )
        if r.returncode == 0:
            return True, ""
        problems = [
            ln
            for ln in r.stderr.splitlines()
            if ln.strip() and not ln.startswith("#") and not self._LINK_ONLY.search(ln)
        ]
        return (not problems), r.stderr

    def run_guff(self, work: Path, config: Path, env: dict[str, str]) -> tuple[list[str], str]:
        cache = tempfile.mkdtemp(prefix="guff-fuzz-")
        e = {**os.environ, **env, "GUFF_CACHE": cache, "GUFF_DEBUG_ILL_TYPED": "1"}
        t0 = time.time()
        try:
            r = subprocess.run(
                [
                    self.guff, "run", "-c", str(config), "--out-format", "json",
                    "--issues-exit-code", "0", "--no-cache", "--timeout", self.timeout, "./...",
                ],
                cwd=work, capture_output=True, text=True, env=e,
            )
        finally:
            shutil.rmtree(cache, ignore_errors=True)
            self.guff_time += time.time() - t0
        return self._keys(r.stdout, work), r.stderr

    def run_golangci(self, work: Path, config: Path, env: dict[str, str]) -> list[str]:
        cache = tempfile.mkdtemp(prefix="gcl-fuzz-")
        e = {**os.environ, **env, "GOLANGCI_LINT_CACHE": cache}
        t0 = time.time()
        try:
            r = subprocess.run(
                [
                    self.golangci, "run", "-c", str(config),
                    "--output.json.path=stdout", "--path-mode", "abs",
                    "--issues-exit-code", "0", f"--timeout={self.timeout}",
                    "--allow-parallel-runners", "./...",
                ],
                cwd=work, capture_output=True, text=True, env=e,
            )
        finally:
            shutil.rmtree(cache, ignore_errors=True)
            self.gcl_time += time.time() - t0
        return self._keys(r.stdout, work)


def diff_keys(guff: list[str], gcl: list[str]) -> tuple[list[str], list[str]]:
    import collections

    g, c = collections.Counter(guff), collections.Counter(gcl)
    return sorted((c - g).elements()), sorted((g - c).elements())


# --------------------------------------------------------------------------
# Fuzzing one case
# --------------------------------------------------------------------------


@dataclass
class Stats:
    mutants: int = 0
    rejected_build: int = 0
    rejected_nosites: int = 0
    agreed: int = 0
    disagreed: int = 0


def fuzz_case(
    case: Case,
    runner: Runner,
    rng: random.Random,
    n_mutants: int,
    n_edits: int,
    out_dir: Path,
    allow_dirty: bool,
    verbose: bool,
) -> Stats:
    st = Stats()
    work = Path(tempfile.mkdtemp(prefix=f"guff-fuzz-{case.name}-"))
    try:
        materialize(case, work)
        base_files = {
            p.relative_to(work).as_posix(): p.read_bytes() for p in sorted(work.rglob("*.go"))
        }
        if not base_files:
            return st
        base = Tree(work, base_files)

        ok, err = runner.build(work, case.env)
        if not ok:
            print(f"  {case.name}: SKIP — seed does not build:\n{err[:400]}")
            return st

        g0, _ = runner.run_guff(work, case.config, case.env)
        c0 = runner.run_golangci(work, case.config, case.env)
        miss0, extra0 = diff_keys(g0, c0)
        baseline = len(miss0) + len(extra0)
        if baseline and not allow_dirty:
            print(
                f"  {case.name}: SKIP — seed already differs "
                f"(missing {len(miss0)}, extra {len(extra0)}; has ratchet="
                f"{case.has_ratchet}). Use --allow-dirty-seeds to fuzz it anyway."
            )
            return st

        sites = _SPANNER.spans(base, work, command="mutations")
        if not sites:
            st.rejected_nosites += 1
            return st
        by_file: dict[str, list[Edit]] = {}
        for s in sites:
            by_file.setdefault(s.rel, []).append(s)

        for _ in range(n_mutants):
            st.mutants += 1
            # Edits are drawn from ONE file per mutant and forced not to overlap:
            # two edits to the same bytes would make the mutant depend on the
            # order apply_edits happens to resolve them in, and a finding that
            # only reproduces under one order is not a finding anyone can act on.
            rel = rng.choice(sorted(by_file))
            picks = _pick_disjoint(by_file[rel], n_edits, rng)
            if not picks:
                continue
            mutant = apply_edits(base, picks)
            for r, data in mutant.files.items():
                (work / r).write_bytes(data)

            ok, _ = runner.build(work, case.env)
            if not ok:
                st.rejected_build += 1
                _restore(work, base_files)
                continue

            gk, gstderr = runner.run_guff(work, case.config, case.env)
            ck = runner.run_golangci(work, case.config, case.env)
            missing, extra = diff_keys(gk, ck)
            interesting = (len(missing) + len(extra)) > baseline
            if not interesting:
                st.agreed += 1
                _restore(work, base_files)
                continue

            st.disagreed += 1
            slug = f"{case.name}-{st.disagreed:03d}"
            dest = out_dir / slug
            dest.mkdir(parents=True, exist_ok=True)
            for r, data in mutant.files.items():
                p = dest / r
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_bytes(data)
            shutil.copy2(work / "go.mod", dest / "go.mod")
            shutil.copy2(case.config, dest / "config.yml")
            (dest / "report.json").write_text(
                json.dumps(
                    {
                        "case": case.name,
                        "mutations": [
                            {"file": e.rel, "kind": e.kind, "label": e.label,
                             "start": e.start, "end": e.end, "replace": e.replace}
                            for e in picks
                        ],
                        "missing_from_guff": missing,
                        "extra_in_guff": extra,
                        "baseline_diff": baseline,
                        "guff_stderr": gstderr[-4000:],
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )
            kinds = ",".join(sorted({e.kind for e in picks}))
            print(f"  {case.name}: DIFF [{kinds}] missing={len(missing)} extra={len(extra)} -> {dest}")
            if verbose:
                for k in missing[:5]:
                    print(f"      -gcl  {k}")
                for k in extra[:5]:
                    print(f"      +guff {k}")
            _restore(work, base_files)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return st


def _pick_disjoint(sites: list[Edit], n: int, rng: random.Random) -> list[Edit]:
    chosen: list[Edit] = []
    for cand in rng.sample(sites, min(len(sites), n * 6)):
        if any(not (cand.end <= c.start or cand.start >= c.end) for c in chosen):
            continue
        chosen.append(cand)
        if len(chosen) == n:
            break
    return chosen


def _restore(work: Path, base: dict[str, bytes]) -> None:
    for rel, data in base.items():
        (work / rel).write_bytes(data)


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--case", help="Fuzz one golden case instead of all of them")
    ap.add_argument("-n", "--mutants", type=int, default=50, help="Mutants per case")
    ap.add_argument("--mutations", type=int, default=1, help="Edits per mutant")
    ap.add_argument("--seed", type=int, default=0, help="RNG seed (runs are reproducible)")
    ap.add_argument("--allow-dirty-seeds", action="store_true")
    ap.add_argument("-o", "--output", help="Where to write findings")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args(argv)

    golangci = os.environ.get("GOLANGCI_LINT_BIN") or shutil.which("golangci-lint")
    if not golangci:
        raise SystemExit("golangci-lint not on PATH")
    guff = resolve_guff()

    cases = load_cases(args.case)
    if not cases:
        raise SystemExit(f"no cases matched {args.case!r} under {CASES}")

    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_dir = Path(args.output).resolve() if args.output else RESULTS / f"fuzz-{stamp}"
    out_dir.mkdir(parents=True, exist_ok=True)

    print("guff differential fuzzer (COMPAT-HARDENING Phase 6)")
    print(f"  guff:     {guff}")
    print(f"  golangci: {golangci}")
    print(f"  cases:    {len(cases)}  mutants/case: {args.mutants}  edits/mutant: {args.mutations}")
    print(f"  seed:     {args.seed}")
    print(f"  results:  {out_dir}\n")

    runner = Runner(guff, golangci)
    total = Stats()
    t0 = time.time()
    for case in cases:
        rng = random.Random(f"{args.seed}:{case.name}")
        st = fuzz_case(
            case, runner, rng, args.mutants, args.mutations,
            out_dir, args.allow_dirty_seeds, args.verbose,
        )
        for f in ("mutants", "rejected_build", "rejected_nosites", "agreed", "disagreed"):
            setattr(total, f, getattr(total, f) + getattr(st, f))

    elapsed = time.time() - t0
    print(
        f"\n{total.mutants} mutants in {elapsed:.0f}s "
        f"(guff {runner.guff_time:.0f}s, golangci {runner.gcl_time:.0f}s)"
    )
    print(
        f"  agreed {total.agreed} / disagreed {total.disagreed} / "
        f"rejected-by-build {total.rejected_build}"
    )
    if total.disagreed:
        print(f"\n{total.disagreed} disagreement(s) under {out_dir}")
        print("Next: minimize one with compat/reduce.py --dir <finding> --config <finding>/config.yml")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
