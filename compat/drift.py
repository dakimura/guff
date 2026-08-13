#!/usr/bin/env python3
"""Upstream drift detection (COMPAT-HARDENING Phase 7).

Every other tier in `compat/` asks *"does guff agree with golangci-lint?"* and
holds golangci-lint still while it asks. That works until upstream moves. Then
the golden gate goes red and the diff says nothing about **which side changed**,
because the goldens are golangci-lint's answer recorded at one moment and there
is no record of any other moment to compare them to.

This asks the other question, with guff held still instead:

    keys(golangci@pin, case)  vs  keys(golangci@candidate, case)

The result is upstream's own changelog, measured on the 81 golden cases rather
than read from release notes — one line per check whose message, position,
severity or existence moved. It is a *guff-independent* measurement: guff is not
run for it at all.

guff does get run, for the second half of the report:

    keys(guff, case)  vs  keys(golangci@candidate, case)

which is what `compat/golden/run.sh` would say after the pin is bumped. Reading
the two halves together is the point — "23 new golden diffs, of which 21 are
upstream changing its mind and 2 are ours" is a bump you can plan; "23 new
golden diffs" is not.

Two more things move without touching a single finding, so they are measured
separately:

* **The linter inventory** (`help linters --json`, `help formatters --json`) —
  a linter added, removed, renamed, deprecated, or moved between the `standard`
  / `fast` groups changes what `linters.default` means for every user.
* **Config acceptance** — a settings key that upstream drops makes golangci-lint
  *exit non-zero* on a config guff still accepts. That is drift no finding-set
  comparison can see, so a case whose config the candidate rejects is reported
  as `config-rejected` rather than as an empty finding set.

## Usage

    compat/drift.py                     # pin (compat/pins.json) vs latest release
    compat/drift.py --candidate 2.11.4  # or against any specific version
    compat/drift.py --offline           # only versions already under compat/.tools
    compat/drift.py --update            # accept this run as the reviewed baseline

`--update` writes each entry's `why` as a placeholder; the run is only treated
as reviewed once every one has been replaced with what the drift actually is.

Findings are written to `compat/results/drift-<stamp>/REPORT.md`. Exit status is
1 when this run's drift is not the drift `compat/drift-ledger.json` records as
reviewed — including when the ledger was reviewed against a *different*
candidate, because "we looked at 2.13.0" says nothing about 2.14.0.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / "golden"))

from fuzz import Case, load_cases, materialize  # noqa: E402
from golden import confirm, issue_keys, sort_key  # noqa: E402
from normalize import load_issues  # noqa: E402
from reduce import resolve_guff  # noqa: E402

PINS = HERE / "pins.json"
LEDGER = HERE / "drift-ledger.json"
TOOLS = HERE / ".tools"
RESULTS = HERE / "results"
RELEASES = "https://api.github.com/repos/golangci/golangci-lint/releases/latest"
INSTALL_SH = "https://raw.githubusercontent.com/golangci/golangci-lint/HEAD/install.sh"

CONFIG_REJECTED = "<config-rejected>"


# --------------------------------------------------------------------------
# Binaries
# --------------------------------------------------------------------------


def pinned_version() -> str:
    return str(json.loads(PINS.read_text(encoding="utf-8"))["golangci_lint"]).lstrip("v")


def latest_version(timeout: int = 20) -> str:
    with urllib.request.urlopen(RELEASES, timeout=timeout) as fh:
        return str(json.load(fh)["tag_name"]).lstrip("v")


def binary_for(version: str, offline: bool) -> Path:
    """Return a golangci-lint of exactly `version`, installing it if allowed.

    Each version lives in its own directory: two golangci-lint binaries have to
    coexist for this tool to mean anything, and `install.sh -b` overwrites.
    """
    version = version.lstrip("v")
    dest = TOOLS / f"golangci-lint-{version}"
    binary = dest / "golangci-lint"
    if binary.is_file() and _version_of(binary) == version:
        return binary

    # The one already on PATH is worth checking before downloading anything —
    # it is usually the pin, and CI installs it before this script runs.
    on_path = os.environ.get("GOLANGCI_LINT_BIN") or shutil.which("golangci-lint")
    if on_path and _version_of(Path(on_path)) == version:
        return Path(on_path)

    if offline:
        raise SystemExit(
            f"--offline: golangci-lint {version} is not in {dest} and not on PATH"
        )

    dest.mkdir(parents=True, exist_ok=True)
    print(f"  installing golangci-lint v{version} -> {dest}")
    script = subprocess.run(
        ["curl", "-sSfL", INSTALL_SH], capture_output=True, text=True, check=True
    ).stdout
    subprocess.run(
        ["sh", "-s", "--", "-b", str(dest), f"v{version}"],
        input=script, text=True, check=True, capture_output=True,
    )
    got = _version_of(binary)
    if got != version:
        raise SystemExit(f"installed golangci-lint reports {got}, wanted {version}")
    return binary


def _version_of(binary: Path) -> str | None:
    try:
        r = subprocess.run(
            [str(binary), "version", "--short"], capture_output=True, text=True, timeout=60
        )
    except (OSError, subprocess.SubprocessError):
        return None
    return r.stdout.strip().lstrip("v") or None


# --------------------------------------------------------------------------
# Running one case
# --------------------------------------------------------------------------


def run_golangci(binary: Path, work: Path, config: Path, env: dict[str, str]) -> list[str] | None:
    """Keys for one run, or None when the candidate refuses the config."""
    cache = tempfile.mkdtemp(prefix="drift-gcl-")
    e = {**os.environ, **env, "GOLANGCI_LINT_CACHE": cache}
    try:
        r = subprocess.run(
            [str(binary), "run", "-c", str(config), "--output.json.path=stdout",
             "--path-mode", "abs", "--issues-exit-code", "0",
             "--allow-parallel-runners", "--timeout=5m", "./..."],
            cwd=work, capture_output=True, text=True, env=e,
        )
    finally:
        shutil.rmtree(cache, ignore_errors=True)
    # `--issues-exit-code 0` means a non-zero status is *not* about findings: it
    # is a rejected config, an unknown linter name, a load failure. Those are
    # exactly the drift this comparison would otherwise render as "0 findings".
    if r.returncode != 0:
        return None
    tmp = work / ".drift.json"
    tmp.write_text(r.stdout, encoding="utf-8")
    try:
        return issue_keys(load_issues(tmp), str(work))
    except Exception:
        return None
    finally:
        tmp.unlink(missing_ok=True)


def stable_keys(
    binary: Path, work: Path, config: Path, env: dict[str, str], attempts: int
) -> list[str] | None:
    """Repeat until two runs agree — upstream is not a function of its input.

    See compat/golden/README.md, "Upstream is not a function". A drift report
    built from single runs would blame upstream for a race in upstream, which is
    true but useless: it would fire every week and name a different set of
    findings each time.
    """
    runs: list[list[str]] = []
    for _ in range(attempts):
        keys = run_golangci(binary, work, config, env)
        if keys is None:
            return None
        runs.append(keys)
        agreed = confirm(runs, 2)
        if agreed is not None:
            return agreed
    return runs[0] if runs else None


def run_guff(guff: str, work: Path, config: Path, env: dict[str, str]) -> list[str]:
    cache = tempfile.mkdtemp(prefix="drift-guff-")
    e = {**os.environ, **env, "GUFF_CACHE": cache}
    try:
        r = subprocess.run(
            [guff, "run", "-c", str(config), "--out-format", "json",
             "--issues-exit-code", "0", "--no-cache", "--timeout", "5m", "./..."],
            cwd=work, capture_output=True, text=True, env=e,
        )
    finally:
        shutil.rmtree(cache, ignore_errors=True)
    tmp = work / ".drift-guff.json"
    tmp.write_text(r.stdout, encoding="utf-8")
    try:
        return issue_keys(load_issues(tmp), str(work))
    except Exception:
        return []
    finally:
        tmp.unlink(missing_ok=True)


# --------------------------------------------------------------------------
# Inventory
# --------------------------------------------------------------------------


def inventory(binary: Path) -> dict[str, dict]:
    """`{name: {kind, groups, deprecated, autoFix, fast}}` for linters + formatters."""
    out: dict[str, dict] = {}
    for kind in ("linters", "formatters"):
        r = subprocess.run(
            [str(binary), "help", kind, "--json"], capture_output=True, text=True
        )
        if r.returncode != 0:
            continue
        try:
            rows = json.loads(r.stdout)
        except json.JSONDecodeError:
            continue
        for row in rows:
            out[f"{kind[:-1]}:{row['name']}"] = {
                "groups": sorted(row.get("groups") or []),
                "deprecated": bool(row.get("deprecated")),
                "autoFix": bool(row.get("autoFix")),
                "fast": bool(row.get("fast")),
                "since": row.get("since") or "",
            }
    return out


def inventory_drift(pin: dict, cand: dict) -> dict[str, list[str]]:
    added = sorted(set(cand) - set(pin))
    removed = sorted(set(pin) - set(cand))
    changed = []
    for name in sorted(set(pin) & set(cand)):
        for field_name in ("groups", "deprecated", "autoFix", "fast"):
            if pin[name][field_name] != cand[name][field_name]:
                changed.append(
                    f"{name}: {field_name} {pin[name][field_name]!r} -> {cand[name][field_name]!r}"
                )
    return {"added": added, "removed": removed, "changed": changed}


# --------------------------------------------------------------------------
# Per-case result
# --------------------------------------------------------------------------


@dataclass
class CaseDrift:
    name: str
    pin_rejected: bool = False
    candidate_rejected: bool = False
    upstream_added: list[str] = field(default_factory=list)
    upstream_removed: list[str] = field(default_factory=list)
    guff_missing: list[str] = field(default_factory=list)
    guff_extra: list[str] = field(default_factory=list)

    @property
    def drifted(self) -> bool:
        return bool(
            self.upstream_added
            or self.upstream_removed
            or self.candidate_rejected
            or self.pin_rejected
        )

    def signature(self) -> dict:
        """What the ledger stores: the upstream half only.

        The guff half is deliberately excluded. It is a consequence of the
        upstream half plus whatever guff does today, so pinning it would make
        the ledger go stale on every guff commit and teach everyone to rerun
        `--update` without reading anything.
        """
        if self.candidate_rejected:
            return {"config_rejected_by_candidate": True}
        if self.pin_rejected:
            return {"config_rejected_by_pin": True}
        return {"added": self.upstream_added, "removed": self.upstream_removed}


def diff_lists(before: list[str], after: list[str]) -> tuple[list[str], list[str]]:
    import collections

    b, a = collections.Counter(before), collections.Counter(after)
    return (
        sorted((a - b).elements(), key=sort_key),
        sorted((b - a).elements(), key=sort_key),
    )


def measure(
    case: Case, pin_bin: Path, cand_bin: Path, guff: str, attempts: int, with_guff: bool
) -> CaseDrift:
    d = CaseDrift(name=case.name)
    work = Path(tempfile.mkdtemp(prefix=f"drift-{case.name}-"))
    try:
        materialize(case, work)
        pin_keys = stable_keys(pin_bin, work, case.config, case.env, attempts)
        cand_keys = stable_keys(cand_bin, work, case.config, case.env, attempts)
        if pin_keys is None:
            d.pin_rejected = True
            return d
        if cand_keys is None:
            d.candidate_rejected = True
            return d
        d.upstream_added, d.upstream_removed = diff_lists(pin_keys, cand_keys)
        if with_guff:
            gk = run_guff(guff, work, case.config, case.env)
            extra, missing = diff_lists(cand_keys, gk)
            d.guff_extra, d.guff_missing = extra, missing
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return d


# --------------------------------------------------------------------------
# Ledger
# --------------------------------------------------------------------------


def load_ledger() -> dict:
    if not LEDGER.is_file():
        return {}
    return json.loads(LEDGER.read_text(encoding="utf-8"))


def ledger_verdict(ledger: dict, pin: str, cand: str, results: list[CaseDrift],
                   inv: dict) -> list[str]:
    """Unreviewed drift, as human-readable lines."""
    if ledger.get("pin") != pin or ledger.get("candidate") != cand:
        drifted = [r.name for r in results if r.drifted]
        inv_moved = any(inv[k] for k in ("added", "removed", "changed"))
        if not drifted and not inv_moved:
            return []
        return [
            f"the ledger was reviewed against pin {ledger.get('pin')} -> candidate "
            f"{ledger.get('candidate')}, this run is {pin} -> {cand}",
            *(f"  case {n} drifted" for n in drifted),
            *(["  the linter inventory moved"] if inv_moved else []),
        ]

    unreviewed: list[str] = []
    known_cases = ledger.get("cases") or {}
    for r in results:
        if not r.drifted:
            continue
        entry = known_cases.get(r.name, {})
        want = entry.get("signature")
        if want != r.signature():
            unreviewed.append(f"  case {r.name}: drift differs from the reviewed baseline")
        elif not is_reviewed(entry.get("why")):
            unreviewed.append(
                f"  case {r.name}: signature matches but `why` is still the placeholder"
            )
    known_inv_entry = ledger.get("inventory") or {}
    known_inv = known_inv_entry.get("signature")
    if any(inv[k] for k in ("added", "removed", "changed")):
        if known_inv != inv:
            unreviewed.append("  the linter inventory drift differs from the reviewed baseline")
        elif not is_reviewed(known_inv_entry.get("why")):
            unreviewed.append(
                "  the linter inventory: signature matches but `why` is still the placeholder"
            )
    return unreviewed


WHY_PLACEHOLDER = "TODO: say what upstream changed and what guff must do about it"


def is_reviewed(why: object) -> bool:
    """Has a human actually written down what this drift is?

    `--update` writes every `why` as a placeholder, and the workflow tells the
    reviewer to fill them in before committing. Nothing enforced it, so a ledger
    committed straight from `--update` silenced the job while recording nothing
    — the exact shape COMPAT-HARDENING §1 is about, one level up: a gate that
    passes because it is not looking. Found on Phase 7's first real `--update`
    run (§4, 2026-08-13).
    """
    if not isinstance(why, str):
        return False
    text = why.strip()
    return bool(text) and text != WHY_PLACEHOLDER and not text.upper().startswith("TODO")


def write_ledger(pin: str, cand: str, results: list[CaseDrift], inv: dict) -> None:
    payload = {
        "_why": (
            "Upstream drift that has been looked at. Its only job is to keep the "
            "weekly job from re-reporting the same change forever; it suppresses "
            "nothing else, and it is scoped to one (pin, candidate) pair because "
            "reviewing 2.13.0 says nothing about 2.14.0."
        ),
        "pin": pin,
        "candidate": cand,
        "reviewed_at": datetime.now(timezone.utc).strftime("%Y-%m-%d"),
        "cases": {
            r.name: {
                "signature": r.signature(),
                "why": WHY_PLACEHOLDER,
            }
            for r in results
            if r.drifted
        },
        "inventory": {"signature": inv, "why": WHY_PLACEHOLDER},
    }
    LEDGER.write_text(json.dumps(payload, indent=2, sort_keys=False) + "\n", encoding="utf-8")


# --------------------------------------------------------------------------
# Report
# --------------------------------------------------------------------------


def render_report(pin: str, cand: str, results: list[CaseDrift], inv: dict,
                  with_guff: bool) -> str:
    drifted = [r for r in results if r.drifted]
    lines = [
        f"# Upstream drift: golangci-lint {pin} -> {cand}",
        "",
        f"Generated {datetime.now(timezone.utc).isoformat(timespec='seconds')} by "
        "`compat/drift.py` (COMPAT-HARDENING Phase 7).",
        "",
        f"- golden cases measured: **{len(results)}**",
        f"- cases whose findings moved: **{len(drifted)}**",
        f"- linters/formatters added: **{len(inv['added'])}**, "
        f"removed: **{len(inv['removed'])}**, changed: **{len(inv['changed'])}**",
        "",
    ]
    if inv["added"] or inv["removed"] or inv["changed"]:
        lines += ["## Inventory", ""]
        for k, marker in (("added", "+"), ("removed", "-")):
            for n in inv[k]:
                lines.append(f"- `{marker}` {n}")
        for c in inv["changed"]:
            lines.append(f"- `~` {c}")
        lines.append("")

    if not drifted:
        lines += ["## Findings", "", "No case's finding set moved.", ""]
    else:
        lines += ["## Findings", ""]
        for r in drifted:
            lines.append(f"### {r.name}")
            lines.append("")
            if r.pin_rejected:
                lines += [f"The **pinned** binary rejects this case's config.", ""]
                continue
            if r.candidate_rejected:
                lines += [
                    "The **candidate** rejects this case's config — a settings key or "
                    "linter name this case uses no longer exists. No finding set to "
                    "compare.",
                    "",
                ]
                continue
            for k in r.upstream_removed:
                lines.append(f"- `-{pin}` {k}")
            for k in r.upstream_added:
                lines.append(f"- `+{cand}` {k}")
            lines.append("")

    if with_guff:
        after = [r for r in results if r.guff_missing or r.guff_extra]
        lines += [
            "## What the golden gate would say after the bump",
            "",
            "guff against the **candidate**, not against the committed goldens. A "
            "case listed here is one the gate would report on the day the pin moves; "
            "cross-read it with Findings above to see which half is upstream's.",
            "",
        ]
        if not after:
            lines.append("Every case would still match exactly.")
        for r in after:
            lines.append(
                f"- **{r.name}**: missing {len(r.guff_missing)}, extra {len(r.guff_extra)}"
            )
        lines.append("")
    return "\n".join(lines) + "\n"


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--pin", help="Baseline version (default: compat/pins.json)")
    ap.add_argument("--candidate", help="Version to compare against (default: latest release)")
    ap.add_argument("--case", help="Measure one golden case")
    ap.add_argument("--attempts", type=int, default=6,
                    help="Runs allowed per binary per case while looking for agreement")
    ap.add_argument("--no-guff", action="store_true",
                    help="Skip the 'what the gate would say' half (upstream-only report)")
    ap.add_argument("--offline", action="store_true",
                    help="Never touch the network; both versions must already be present")
    ap.add_argument("--update", action="store_true",
                    help="Record this run in compat/drift-ledger.json as reviewed")
    ap.add_argument("-o", "--output", help="Report directory")
    args = ap.parse_args(argv)

    pin = (args.pin or pinned_version()).lstrip("v")
    if args.candidate:
        cand = args.candidate.lstrip("v")
    elif args.offline:
        raise SystemExit("--offline needs an explicit --candidate")
    else:
        cand = latest_version()

    print("guff upstream drift (COMPAT-HARDENING Phase 7)")
    print(f"  pin:       {pin}")
    print(f"  candidate: {cand}")
    if pin == cand:
        print("\nThe pin is the newest release: nothing upstream to compare against.")
        print("This is the expected steady state; the job exists for the week it changes.")
        return 0

    pin_bin = binary_for(pin, args.offline)
    cand_bin = binary_for(cand, args.offline)
    guff = resolve_guff()
    cases = load_cases(args.case)
    if not cases:
        raise SystemExit(f"no golden case matched {args.case!r}")
    print(f"  cases:     {len(cases)}\n")

    inv = inventory_drift(inventory(pin_bin), inventory(cand_bin))
    results: list[CaseDrift] = []
    for case in cases:
        d = measure(case, pin_bin, cand_bin, guff, args.attempts, not args.no_guff)
        results.append(d)
        if d.drifted:
            what = (
                "config rejected"
                if (d.candidate_rejected or d.pin_rejected)
                else f"+{len(d.upstream_added)} -{len(d.upstream_removed)}"
            )
            print(f"  {case.name}: DRIFT {what}")
        else:
            print(f"  {case.name}: same")

    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out = Path(args.output).resolve() if args.output else RESULTS / f"drift-{stamp}"
    out.mkdir(parents=True, exist_ok=True)
    report = render_report(pin, cand, results, inv, not args.no_guff)
    (out / "REPORT.md").write_text(report, encoding="utf-8")
    print(f"\nReport: {out / 'REPORT.md'}")

    if args.update:
        write_ledger(pin, cand, results, inv)
        print(f"Ledger updated: {LEDGER} — fill in every `why` before committing.")
        return 0

    unreviewed = ledger_verdict(load_ledger(), pin, cand, results, inv)
    if unreviewed:
        print("\nUnreviewed upstream drift:")
        for line in unreviewed:
            print(f"  {line}")
        print(
            "\nRead the report, decide per case whether guff follows, then record the "
            "decision with: compat/drift.py --candidate "
            f"{cand} --update"
        )
        return 1
    print("\nNo unreviewed drift.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
