#!/usr/bin/env python3
"""corpus/status.py — how far the corpus is from 100 compatible targets.

Three questions a session starting from nothing has to answer, and one place
that answers all three:

    ./corpus/status.py check    exit 0 when the goal is reached
    ./corpus/status.py next     the single next task, one line
    ./corpus/status.py report   the table a person reads

`probe` regenerates `corpus/status.json` from whatever measurements are on
disk under `compat/results/`. Those result directories are gitignored, so the
ledger is what survives — it is the only durable record of "which targets are
at zero", and every iteration of the loop commits it.

The ledger is *generated*, never hand-written. A row nobody measured says
`unmeasured`, not `0`: an absent measurement and a clean one are different
answers, and the whole point of this file is that a fresh session can tell
them apart
(compat/README.md, and the `health.json` baselines for the same rule one level
down).
"""

from __future__ import annotations

import argparse
import collections
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPOS = ROOT / "corpus" / "repos.json"
HUNT = ROOT / "corpus" / "hunt.json"
CANDIDATES = ROOT / "corpus" / "candidates-100.json"
RESULTS = ROOT / "compat" / "results"
LEDGER = ROOT / "corpus" / "status.json"

GOAL = 100

# Targets that are surveyed and deliberately not adopted, with the reason.
# `corpus/README.md` carries the same table in prose; this copy is the one
# `next` reads, so a repo excluded there must be excluded here or it will keep
# being proposed.
EXCLUDED = {
    "pulumi": "declares module plugins; a stock golangci-lint refuses to start "
    "(compat/reject/cases/custom-module-plugin-missing)",
    "moby": "public tree has no root go.mod",
    "hugo": "no .golangci.yml on the default branch",
    "etcd": "no .golangci.yml on the default branch",
    "terraform": "no .golangci.yml on the default branch",
    "istio": "no .golangci.yml on the default branch",
    "cockroach": "no .golangci.yml on the default branch",
    "harness": "no tag carries a v2 config (newest tag v3.3.0 is 2025-08-14; the "
    "config went version: \"2\" on 2025-10-17, and no tag has been cut since), and "
    "./... on main measures only typecheck: one //go:embed of an unbuilt frontend "
    "deletes the whole report",
    "ollama": "app/ui/app.go embeds app/dist, a React build output that is in "
    ".gitignore and so exists in no tag or clone. go list ./... fails with "
    "\"pattern app/dist: no matching files found\", golangci-lint's whole report "
    "collapses to that one typecheck finding, and a typecheck finding deletes "
    "every other issue in the run — the same shape as harness. Stubbing the "
    "embed by hand loads 104 packages and gives golangci 0 / guff 10, so the "
    "repository itself is measurable; reachable if the schema ever grows a "
    "prepare step. Measured 2026-09-06 at v0.33.1",
    "opentelemetry-collector": "100 go.mod files and no go.work: ./... at the "
    "checkout root reaches exactly one package (internal/statusutil), and a "
    "submodule cannot be named from there (go list ./pdata/... -> \"directory "
    "prefix pdata does not contain main module\"). Upstream lints it as 100 runs "
    "(make golint cds into each module); the harness runs one",
}

# Targets whose numbers this host cannot produce. Measuring them anywhere but
# Linux records the platform, not the compatibility — cri-o is Linux-only and
# both tools go ill-typed on darwin (COMPAT-HARDENING 2026-08-30 続き 103).
#
# tetragon is the same shape and the clearest case of it: four of its packages
# (pkg/asm, pkg/constants, pkg/observer/observertesthelper, pkg/reader/namespace)
# ship only `_linux.go` / `_windows.go`, so `go build ./...` fails on darwin and
# **70 of its 219 packages** cannot be loaded at all (2026-09-06 続き 217).
PLATFORM_BOUND = {"cri-o": "linux", "buildah": "linux", "tetragon": "linux"}


def defined_targets() -> dict[str, dict]:
    out: dict[str, dict] = {}
    for path in (REPOS, HUNT):
        for entry in json.loads(path.read_text()):
            out[entry["name"]] = entry
    return out


def latest_measurements() -> dict[str, dict]:
    """The newest `<target>.summary.json` per target, across every results dir.

    `compat/run.sh` and `compat/hunt.sh` write the same schema into differently
    named directories; both are timestamped, so sorting the directory names
    sorts by time.
    """
    best: dict[str, tuple[str, dict]] = {}
    if not RESULTS.is_dir():
        return {}
    for run_dir in sorted(p for p in RESULTS.iterdir() if p.is_dir()):
        stamp = run_dir.name.replace("hunt-", "")
        for summary in run_dir.glob("*.summary.json"):
            try:
                data = json.loads(summary.read_text())
            except (OSError, json.JSONDecodeError):
                continue
            name = data.get("target")
            if not name:
                continue
            if name not in best or stamp >= best[name][0]:
                best[name] = (stamp, data)
    return {name: {"at": stamp, **data} for name, (stamp, data) in best.items()}


def by_linter(keys: list[str]) -> dict[str, int]:
    """`path:line:linter:message` — the third field is the linter."""
    counter: collections.Counter[str] = collections.Counter()
    for key in keys:
        parts = key.split(":")
        counter[parts[2] if len(parts) >= 3 else "?"] += 1
    return dict(counter.most_common())


def host_platform() -> str:
    """`linux` / `darwin` / … — the key `PLATFORM_BOUND` values are written in."""
    return "linux" if sys.platform.startswith("linux") else sys.platform


def previous_rows() -> dict[str, dict]:
    """The rows already in the ledger, so a skipped probe keeps what CI wrote."""
    if not LEDGER.exists():
        return {}
    try:
        return json.loads(LEDGER.read_text()).get("targets", {})
    except (json.JSONDecodeError, OSError):
        return {}


def build() -> dict:
    defined = defined_targets()
    measured = latest_measurements()
    previous = previous_rows()
    host = host_platform()
    rows = {}
    for name, entry in sorted(defined.items()):
        row: dict = {"tier": entry.get("tier", "?")}
        if name in PLATFORM_BOUND:
            row["needs_platform"] = PLATFORM_BOUND[name]
            if PLATFORM_BOUND[name] != host:
                # A measurement taken on the wrong platform describes the
                # platform, not the compatibility: buildah does not even
                # `go build` on darwin (a vendored dependency has no darwin
                # files), and the findings that do appear are in `!linux` stub
                # files that Linux never compiles.
                #
                # Keep only a row the *right* platform wrote — `measured_on`
                # says which one did. A row without that stamp predates this
                # guard and was taken here, so it is dropped rather than
                # frozen; CI runs on ubuntu and will fill it in properly.
                keep = previous.get(name, {})
                if keep.get("measured_on") == PLATFORM_BOUND[name]:
                    row.update({k: v for k, v in keep.items() if k != "tier"})
                else:
                    row["state"] = "unmeasured"
                row["skipped_on"] = host
                rows[name] = row
                continue
        m = measured.get(name)
        if m is None:
            row["state"] = "unmeasured"
        else:
            guff_only = m.get("unexpected_guff") or []
            gcl_only = m.get("unexpected_golangci") or []
            row["state"] = "clean" if not guff_only and not gcl_only else "open"
            row["at"] = m["at"]
            if name in PLATFORM_BOUND:
                row["measured_on"] = host
            row["open"] = len(guff_only) + len(gcl_only)
            row["guff_only"] = len(guff_only)
            row["gcl_only"] = len(gcl_only)
            if guff_only or gcl_only:
                row["by_linter"] = by_linter(guff_only + gcl_only)
        rows[name] = row
    return {
        "_comment": [
            "Generated by ./corpus/status.py probe — do not hand-edit.",
            "compat/results/ is gitignored, so this ledger is the durable record",
            "of which targets have been measured and which are at zero.",
            "`unmeasured` is not `clean`: an absent measurement is a question.",
        ],
        "goal": GOAL,
        "defined": len(rows),
        "clean": sum(1 for r in rows.values() if r["state"] == "clean"),
        "open": sum(1 for r in rows.values() if r["state"] == "open"),
        "unmeasured": sum(1 for r in rows.values() if r["state"] == "unmeasured"),
        "targets": rows,
    }


def load_or_build() -> dict:
    if LEDGER.exists():
        return json.loads(LEDGER.read_text())
    return build()


def next_task(ledger: dict) -> tuple[str, str]:
    """(kind, description). Kinds: close, measure, adopt, done."""
    rows = ledger["targets"]

    # 1. Close an open target before adopting new ones — a corpus of targets
    #    nobody has brought to zero is a longer list, not more compatibility.
    #    Fewest diffs first: it closes targets soonest, and one bug standing in
    #    several repos gets found either way (gocritic appendAssign was 4
    #    findings across 3 targets).
    open_rows = [
        (r.get("open", 0), n)
        for n, r in rows.items()
        if r["state"] == "open" and "needs_platform" not in r
    ]
    if open_rows:
        count, name = min(open_rows)
        linters = rows[name].get("by_linter", {})
        detail = ", ".join(f"{k}={v}" for k, v in linters.items())
        return "close", f"close {name} ({count} open: {detail})"

    unmeasured = [
        n for n, r in rows.items() if r["state"] == "unmeasured" and "needs_platform" not in r
    ]
    if unmeasured:
        return "measure", f"measure {sorted(unmeasured)[0]}"

    if ledger["defined"] < ledger["goal"]:
        pick = next_candidate(set(rows))
        if pick is None:
            return "adopt", "adopt (no candidate left in candidates-100.json — refresh the survey)"
        return "adopt", f"adopt {pick['name']} ({pick.get('_size_mb', '?')}MB, {pick['url']})"

    return "done", f"done — {ledger['clean']}/{ledger['goal']} targets at zero"


def next_candidate(taken: set[str]) -> dict | None:
    """Smallest unadopted candidate. Small first: a cheap target measured is
    worth more than a large one queued, and every adoption pays a clone plus its
    module downloads."""
    if not CANDIDATES.exists():
        return None
    rows = [
        c
        for c in json.loads(CANDIDATES.read_text())
        if c["name"] not in taken and c["name"] not in EXCLUDED
    ]
    if not rows:
        return None
    return min(rows, key=lambda c: (c.get("_size_mb") or 1e9, c["name"]))


def report(ledger: dict) -> str:
    rows = ledger["targets"]
    out = [
        f"# Corpus status — {ledger['clean']}/{ledger['goal']} at zero "
        f"({ledger['defined']} defined, {ledger['open']} open, "
        f"{ledger['unmeasured']} unmeasured)",
        "",
        "| target | tier | state | open | by linter | measured |",
        "|---|---|---|--:|---|---|",
    ]
    for name, r in sorted(rows.items(), key=lambda kv: (kv[1]["state"], kv[0])):
        linters = ", ".join(f"{k}={v}" for k, v in (r.get("by_linter") or {}).items())
        note = f" ({r['needs_platform']} only)" if "needs_platform" in r else ""
        out.append(
            f"| {name}{note} | {r['tier']} | {r['state']} | "
            f"{r.get('open', '')} | {linters} | {r.get('at', '—')} |"
        )
    kind, task = next_task(ledger)
    out += ["", f"**next**: `{task}` ({kind})"]
    return "\n".join(out) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("command", choices=["probe", "report", "next", "check"])
    args = ap.parse_args()

    if args.command == "probe":
        ledger = build()
        LEDGER.write_text(json.dumps(ledger, indent=2, ensure_ascii=False) + "\n")
        print(
            f"wrote {LEDGER.relative_to(ROOT)}: {ledger['defined']} defined, "
            f"{ledger['clean']} clean, {ledger['open']} open, "
            f"{ledger['unmeasured']} unmeasured"
        )
        return 0

    ledger = load_or_build()
    if args.command == "report":
        print(report(ledger), end="")
        return 0
    if args.command == "next":
        kind, task = next_task(ledger)
        print(task)
        return 0 if kind != "done" else 0

    # check
    kind, task = next_task(ledger)
    if kind == "done":
        print(task)
        return 0
    print(f"not done: {task}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
