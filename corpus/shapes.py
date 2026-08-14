#!/usr/bin/env python3
"""corpus/shapes.py — input-shape ledger for the OSS corpus (COMPAT-HARDENING Phase 5).

Phase 0 asked "which *check* has never fired?". This asks the other half:
**which *shape of input* has no gated target at all?** A check can be `fired`
in `docs/COVERAGE.md` and still have never seen a generic instantiation, a cgo
package, or a file the build tags excluded — and a recall bug that only shows
up on that shape is invisible to every gate we have.

    ./corpus/shapes.py probe                # go list -> corpus/shapes.json
    ./corpus/shapes.py report               # ledger -> markdown table
    ./corpus/shapes.py check                # gate: every required shape covered

`probe` runs `go list -e -json <packages>` in each checkout with **the target's
real package pattern** — the same set the compat gate feeds both tools. That
distinction is the whole point: the grafana checkout contains 47 `go.mod`
files, but `./pkg/...` analyzes exactly one module, so "grafana covers
multi-module" would be false. Only what the pattern selects is counted.

`check` is the gate. It fails when a shape listed in `REQUIRED` is covered by
no gated target, so deleting or re-scoping a target can no longer silently
drop a shape. Shapes we decided *not* to cover live in `EXCLUDED` with the
measurement behind the decision, the same way COMPAT-HARDENING §6 records the
checks that are permanently unobservable.

The ledger is committed. `probe` needs prepared checkouts (`corpus/prepare.sh`)
and is slow (a `go list` per target); `check --offline` re-reads the committed
ledger instead, which is what a plain unit test can afford.

Every count here is a function of GOOS/GOARCH, because `go list` decides file
inclusion from the build constraints: `IgnoredGoFiles` is *by definition* the
files this platform excludes, and `GoFiles` — which the generics, embed and
test-only counts are read out of — is its complement. Probe containerd on
darwin/arm64 and `buildtags` is 43; probe the same checkout on linux/amd64 and
it is 33. A ledger recorded on a laptop therefore cannot hold on the runner,
which is what it did: the gate ran for the first time on 2026-08-14 (it sits
behind the health gate, which had been failing since it was added) and failed
on that exact 43 -> 33.

So the probe pins the platform rather than inheriting it. `go list` only
decides which files belong to a package, so it cross-lists without a
cross-compiler, and CGO_ENABLED is pinned too: it moves cgo files between
`CgoFiles` and `IgnoredGoFiles`, and its default is 1 natively but 0 when
GOOS differs from the host — which would have reintroduced the same split
between the runner and a laptop by another route.
"""

from __future__ import annotations

import os
import sys

# `corpus/select.py` shadows the stdlib `select` module, and `subprocess` pulls
# in `selectors`, which imports `select`. Python puts a script's own directory
# first on sys.path, so `import subprocess` from here resolves the wrong
# `select` and dies. Drop this directory before importing anything else.
_HERE = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _HERE]

import argparse  # noqa: E402
import json  # noqa: E402
import re  # noqa: E402
import subprocess  # noqa: E402
from pathlib import Path  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "corpus"
CACHE = Path(os.environ.get("CORPUS_CACHE") or (CORPUS / "cache"))
REPOS_JSON = CORPUS / "repos.json"
LEDGER = CORPUS / "shapes.json"

# The platform every count in the ledger is measured on, regardless of where
# `probe` runs. See the module docstring: without this the ledger is a laptop
# number the runner can never reproduce. linux/amd64 because that is what the
# compat gate itself runs on, so the ledger describes the packages the gate
# actually feeds both tools.
PROBE_ENV = {"GOOS": "linux", "GOARCH": "amd64", "CGO_ENABLED": "1"}

# Tiers CI actually runs (`.github/workflows/compat.yml`: `--oss --tier pr` on
# every PR, `--tier nightly` on main). `hunt` is a discovery tier and `weekly`
# is defined but not wired to a job, so neither can keep a shape honest —
# a shape only counts as covered when a failing gate would notice it going
# wrong. Add "weekly" here the day a job runs it.
GATED_TIERS = ("pr", "nightly")

# One entry per shape COMPAT-HARDENING §2 Phase 5 lists as unexercised, plus
# the two the first probe found we were also missing (multi-module, old `go`
# directive). `desc` is what the number counts.
SHAPES = {
    "generics": "packages with a type-parameterized func/type declaration",
    "genericrecv": "files declaring a method on a generic type (`func (x T[P]) M()`)",
    "genericunion": "files with a `~T` / `A | B` type set in a constraint",
    "genericalias": "files with a generic type alias (`type A[T any] = B[T]`, go1.24)",
    "cgo": 'packages with cgo files (import "C")',
    "buildtags": "files excluded from the build by //go:build constraints",
    "gowork": "targets analyzed inside an active go.work workspace",
    "multimodule": "targets whose analyzed packages span >1 module",
    "vendormode": "targets resolving dependencies from vendor/ (-mod=vendor)",
    "embed": "packages with //go:embed files",
    "testonly": "packages with no non-test Go files",
    "asm": "assembly (.s) files in analyzed packages",
    "nonascii": "files declaring a non-ASCII identifier",
    "generated": 'files marked "Code generated ... DO NOT EDIT."',
    "biggen": "generated files over 200 KiB",
    "oldgo": "targets whose go directive predates 1.22 (per-iteration loop vars)",
}

# Shapes that must be covered by at least one gated target. `check` fails
# otherwise.
REQUIRED = (
    "generics",
    "genericrecv",
    "buildtags",
    "gowork",
    "multimodule",
    "vendormode",
    "embed",
    "testonly",
    "generated",
    "oldgo",
)

# Shapes deliberately left uncovered, with the reason. Mirrors the ledger in
# COMPAT-HARDENING §6: a shape belongs here only once it has been *measured*,
# never on a hunch.
EXCLUDED = {
    "cgo": (
        "Would make a C toolchain a prerequisite of the compat gate. Same "
        "decision as COMPAT-HARDENING §6 for govet/cgocall. guff's own "
        "handling of import \"C\" is covered by unit tests."
    ),
    "asm": (
        "golangci-lint 2.12.2 emits no diagnostics for .s files at all "
        "(measured in COMPAT-HARDENING §6, govet/framepointer), so an "
        "assembly target compares 0 against 0."
    ),
    "nonascii": (
        "No mainstream Go corpus repo declares non-ASCII identifiers. Covered "
        "by fixture instead: compat/golden/cases/nonascii."
    ),
    "genericalias": (
        "Measured at 0 on every target, gated or not — the form needs go1.24 "
        "and is still rare. Covered by fixture instead: the `Alias`/"
        "`hiddenAlias` declarations in compat/golden/cases/generics."
    ),
    "biggen": (
        "Subsumed by `generated`; size only changes wall-clock, not the "
        "finding set. consul and kubernetes each carry one anyway."
    ),
}

TYPEPARAM = re.compile(r"^(?:func\s+\w+\[[A-Z_]|type\s+\w+\[[A-Z_])", re.M)
# "generics is covered" was the shape ledger's answer; it says nothing about
# *which* generic form a target exercises, and the forms fail differently.
# A method on a generic type is its own shape: three of the 2026-08-12 bugs
# (revive's receiver rendering, revive's private-receiver skip, nilerr's blind
# spot on methods) only fire on `func (x T[P]) M()`, and no gated target had
# one. These are syntactic like TYPEPARAM above — a lower bound, not a census.
GENERIC_RECV = re.compile(r"^func\s+\(\s*\w+\s+\*?\w+\[", re.M)
GENERIC_UNION = re.compile(r"(?:^|[\s\[|])~\w|\w\s*\|\s*~\w", re.M)
GENERIC_ALIAS = re.compile(r"^type\s+\w+\[[^\]]*\]\s*=", re.M)
NONASCII_IDENT = re.compile(r"(?:func|var|const|type)\s+[^\x00-\x7f\W]\w*")
GENERATED = re.compile(r"^// Code generated .* DO NOT EDIT\.", re.M)
GO_DIRECTIVE = re.compile(r"^go (\d+)\.(\d+)", re.M)


def _iter_json(text: str):
    """`go list -json` emits concatenated objects, not an array."""
    dec = json.JSONDecoder()
    i = 0
    while i < len(text):
        while i < len(text) and text[i] in " \n\t\r":
            i += 1
        if i >= len(text):
            return
        obj, i = dec.raw_decode(text, i)
        yield obj


def go_directive(checkout: Path) -> tuple[int, int] | None:
    gomod = checkout / "go.mod"
    if not gomod.exists():
        return None
    m = GO_DIRECTIVE.search(gomod.read_text(encoding="utf-8", errors="replace"))
    return (int(m.group(1)), int(m.group(2))) if m else None


def probe_target(repo: dict) -> dict | None:
    """Count shapes over exactly the packages the compat gate analyzes."""
    name = repo["name"]
    packages = repo.get("packages") or "./..."
    checkout = CACHE / name
    if not checkout.exists():
        print(f"skip {name}: not cloned (run corpus/prepare.sh)", file=sys.stderr)
        return None

    argv = ["go", "list", "-e", "-json", *packages.split()]
    proc = subprocess.run(
        argv,
        cwd=checkout,
        capture_output=True,
        text=True,
        timeout=3600,
        env={**os.environ, **PROBE_ENV},
    )
    if not proc.stdout.strip():
        print(f"error {name}: go list produced nothing\n{proc.stderr[:400]}", file=sys.stderr)
        return None

    counts = {k: 0 for k in SHAPES}
    modules: set[str] = set()
    npkg = nfile = 0

    for pkg in _iter_json(proc.stdout):
        npkg += 1
        mod = pkg.get("Module") or {}
        if mod.get("Path"):
            modules.add(mod["Path"])
        if pkg.get("CgoFiles"):
            counts["cgo"] += 1
        counts["asm"] += len(pkg.get("SFiles") or [])
        counts["buildtags"] += len(pkg.get("IgnoredGoFiles") or [])
        if pkg.get("EmbedFiles") or pkg.get("TestEmbedFiles"):
            counts["embed"] += 1

        go_files = (pkg.get("GoFiles") or []) + (pkg.get("CgoFiles") or [])
        test_files = (pkg.get("TestGoFiles") or []) + (pkg.get("XTestGoFiles") or [])
        if not go_files and test_files:
            counts["testonly"] += 1

        pkgdir = Path(pkg.get("Dir") or "")
        for fname in go_files + test_files:
            nfile += 1
            try:
                text = (pkgdir / fname).read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            if TYPEPARAM.search(text):
                counts["generics"] += 1
            if GENERIC_RECV.search(text):
                counts["genericrecv"] += 1
            if GENERIC_UNION.search(text):
                counts["genericunion"] += 1
            if GENERIC_ALIAS.search(text):
                counts["genericalias"] += 1
            if NONASCII_IDENT.search(text):
                counts["nonascii"] += 1
            if GENERATED.search(text):
                counts["generated"] += 1
                if len(text) > 200_000:
                    counts["biggen"] += 1

    counts["multimodule"] = len(modules) if len(modules) > 1 else 0
    counts["gowork"] = 1 if (checkout / "go.work").exists() else 0
    counts["vendormode"] = 1 if (checkout / "vendor" / "modules.txt").exists() else 0

    directive = go_directive(checkout)
    counts["oldgo"] = 1 if directive and directive < (1, 22) else 0

    return {
        "tier": repo.get("tier"),
        "packages": packages,
        "ref": repo.get("ref"),
        "go": ".".join(str(p) for p in directive) if directive else None,
        "packages_analyzed": npkg,
        "files_analyzed": nfile,
        "modules": sorted(modules),
        "shapes": counts,
    }


def load_repos() -> list[dict]:
    return json.loads(REPOS_JSON.read_text(encoding="utf-8"))


def cmd_probe(args) -> int:
    repos = [r for r in load_repos() if not args.name or r["name"] in args.name]
    ledger = {}
    if args.merge and LEDGER.exists():
        ledger = json.loads(LEDGER.read_text(encoding="utf-8")).get("targets", {})
    for repo in repos:
        result = probe_target(repo)
        if result is not None:
            ledger[repo["name"]] = result
            print(f"probed {repo['name']}", file=sys.stderr)
    payload = {
        "_comment": (
            "Generated by corpus/shapes.py probe. Counts are over each target's "
            "real package pattern, i.e. what the compat gate actually analyzes, "
            "and are measured on the fixed platform in probe_env — every count "
            "here is a function of GOOS/GOARCH, so inheriting the host's would "
            "make the ledger unreproducible off the runner."
        ),
        "probe_env": PROBE_ENV,
        "shapes": SHAPES,
        "required": list(REQUIRED),
        "excluded": EXCLUDED,
        "targets": dict(sorted(ledger.items())),
    }
    LEDGER.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {LEDGER.relative_to(ROOT)}")
    return 0


def coverage(targets: dict) -> dict[str, list[str]]:
    """shape -> gated target names that exercise it."""
    out: dict[str, list[str]] = {k: [] for k in SHAPES}
    for name, entry in sorted(targets.items()):
        if entry.get("tier") not in GATED_TIERS:
            continue
        for shape, count in entry.get("shapes", {}).items():
            if count:
                out.setdefault(shape, []).append(name)
    return out


def read_ledger() -> dict:
    if not LEDGER.exists():
        print(f"error: missing {LEDGER}; run corpus/shapes.py probe", file=sys.stderr)
        raise SystemExit(2)
    return json.loads(LEDGER.read_text(encoding="utf-8"))


def cmd_report(args) -> int:
    data = read_ledger()
    targets = data["targets"]
    gated = {k: v for k, v in targets.items() if v.get("tier") in GATED_TIERS}
    cols = list(SHAPES)

    print("| target | tier | go | pkgs | files | " + " | ".join(cols) + " |")
    print("|" + "---|" * (len(cols) + 5))
    for name, entry in gated.items():
        cells = [str(entry["shapes"].get(c, 0)) for c in cols]
        print(
            f"| {name} | {entry['tier']} | {entry.get('go') or '?'} | "
            f"{entry['packages_analyzed']} | {entry['files_analyzed']} | "
            + " | ".join(cells)
            + " |"
        )

    print()
    cov = coverage(targets)
    print("| shape | gated targets | status |")
    print("|---|---|---|")
    for shape in cols:
        who = cov.get(shape) or []
        if shape in EXCLUDED:
            status = "excluded"
        elif shape in REQUIRED:
            status = "**UNCOVERED**" if not who else "covered"
        else:
            status = "optional"
        print(f"| `{shape}` | {', '.join(who) or '—'} | {status} |")
    return 0


def cmd_check(args) -> int:
    data = read_ledger()
    targets = data["targets"]

    # A ledger probed on a different platform is not comparable to a live probe,
    # and the drift check cannot always tell: a target whose counts happen to
    # match on both would pass while the rest of the ledger is wrong. Say so.
    recorded = data.get("probe_env")
    if recorded != PROBE_ENV:
        print(
            f"shape ledger was probed on {recorded or 'an unrecorded platform'}, "
            f"but this probe measures {PROBE_ENV} — re-run corpus/shapes.py probe",
            file=sys.stderr,
        )
        return 1

    if not args.offline:
        live: dict[str, dict] = {}
        for repo in load_repos():
            result = probe_target(repo)
            if result is not None:
                live[repo["name"]] = result
        drift = []
        for name, entry in live.items():
            old = targets.get(name)
            if old is None:
                drift.append(f"{name}: not in the ledger")
            elif old.get("shapes") != entry.get("shapes"):
                for shape in SHAPES:
                    a, b = old.get("shapes", {}).get(shape), entry["shapes"].get(shape)
                    if a != b:
                        drift.append(f"{name}.{shape}: ledger {a} -> measured {b}")
        if drift:
            print("shape ledger is stale (run corpus/shapes.py probe):", file=sys.stderr)
            for line in drift:
                print(f"  {line}", file=sys.stderr)
            return 1
        # Merge, never replace. A CI job only clones the tier it runs, so
        # `live` is missing every other tier's targets — replacing would drop
        # their shapes and fail the coverage check for the wrong reason.
        targets = {**targets, **live}

    cov = coverage(targets)
    missing = [s for s in REQUIRED if not cov.get(s)]
    for shape in sorted(SHAPES):
        who = cov.get(shape) or []
        mark = "ok " if who else ("MISS" if shape in REQUIRED else "-   ")
        note = "" if who else EXCLUDED.get(shape, "")
        print(f"{mark} {shape:<12} {', '.join(who) or note}")
    if missing:
        print(
            "\nerror: no gated target covers: " + ", ".join(missing),
            file=sys.stderr,
        )
        return 1
    print(f"\nOK: {len(REQUIRED)} required shape(s) covered by gated targets")
    return 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("probe", help="go list each target -> corpus/shapes.json")
    p.add_argument("--name", action="append", default=[], help="only these targets")
    p.add_argument(
        "--merge",
        action="store_true",
        help="keep ledger entries for targets not probed this run",
    )
    p.set_defaults(func=cmd_probe)

    p = sub.add_parser("report", help="markdown table of the ledger")
    p.set_defaults(func=cmd_report)

    p = sub.add_parser("check", help="gate: every required shape has a gated target")
    p.add_argument(
        "--offline",
        action="store_true",
        help="trust the committed ledger instead of re-running go list",
    )
    p.set_defaults(func=cmd_check)

    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
