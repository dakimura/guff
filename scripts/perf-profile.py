#!/usr/bin/env python3
"""Summarise a samply profile on the terminal (no browser needed).

`samply record` normally opens the Firefox Profiler UI, which is the right tool
for exploring a profile interactively. This script covers the other case: a
headless shell (CI, an SSH session, a coding agent) that needs numbers it can
paste into `docs/PERF_TASKS_V2.md` as GO/NO-GO evidence.

Requires a profile recorded with `--unstable-presymbolicate`, which drops a
`<name>.syms.json` sidecar next to the profile; without it every frame is a raw
hex address. See docs/DEVELOPMENT.md §9.4.

    scripts/perf-profile.py /tmp/guff.json.gz                 # self-CPU top 40
    scripts/perf-profile.py /tmp/guff.json.gz --top 80
    scripts/perf-profile.py /tmp/guff.json.gz --inclusive 'Scanner|to_vec'
    scripts/perf-profile.py /tmp/guff.json.gz --threads       # per-thread CPU
    scripts/perf-profile.py /tmp/guff.json.gz --subtree 'build_source_seed_inner'

`--inclusive` says a phase costs 1.7s; `--callers` says who reached a leaf.
`--subtree` answers the third question a phase investigation needs — what that
1.7s is *made of* — by charging each sample taken anywhere under the matching
frame to its own leaf. Percentages are of the subtree, not of the run.

CPU time comes from each sample's `threadCPUDelta`, not from wall-clock sample
counts, so a thread parked in `go list` or on a rayon barrier contributes ~0.
That makes the totals **summed CPU across every worker thread** — NOT wall time.
A 6s total on a 10-core box can be a 1s phase. Never quote these numbers as
wall-clock; measure wall with `GUFF_DEBUG_CACHE=1` phase timers instead
(PERF_TASKS.md §1.6).
"""

from __future__ import annotations

import argparse
import bisect
import gzip
import json
import re
import sys
from collections import defaultdict
from pathlib import Path


def load_profile(path: Path) -> dict:
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rt") as fh:
        return json.load(fh)


def syms_path_for(path: Path) -> Path:
    """`/tmp/p.json.gz` -> `/tmp/p.json.syms.json` (samply's own convention)."""
    stem = path.name[:-3] if path.name.endswith(".gz") else path.name
    return path.with_name(stem + ".syms.json")


def breakpad_to_debug_id(breakpad_id: str | None) -> str | None:
    """`05160B71CC513114C0A09EF6370A33DC0` -> `05160b71-cc51-3114-c0a0-9ef6370a33dc`.

    The profile identifies libraries by Breakpad id (32 hex nibbles plus a
    trailing age digit); the syms.json sidecar keys them by dashed lowercase
    UUID. Matching the two is the only reason this conversion exists.
    """
    if not breakpad_id or len(breakpad_id) < 32:
        return None
    h = breakpad_id[:32].lower()
    return f"{h[0:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:32]}"


class SymbolIndex:
    """Address -> symbol name, per shared library, from the syms.json sidecar.

    Keyed on `debug_id` because a single run maps several builds of the same
    `debug_name` (guff's own `go list` children pull in the Go toolchain).
    """

    def __init__(self, syms: dict) -> None:
        strings = syms["string_table"]
        self.by_debug_id: dict[str, tuple[list[int], list[str], list[int]]] = {}
        for entry in syms["data"]:
            table = entry.get("symbol_table") or []
            # Sorted once here so lookups are a bisect rather than a scan; the
            # sidecar order is not guaranteed.
            table.sort(key=lambda s: s["rva"])
            rvas = [s["rva"] for s in table]
            names = [strings[s["symbol"]] for s in table]
            sizes = [s.get("size") or 0 for s in table]
            self.by_debug_id[entry["debug_id"]] = (rvas, names, sizes)

    def lookup(self, debug_id: str | None, address: int) -> str | None:
        entry = self.by_debug_id.get(debug_id or "")
        if entry is None or address < 0:
            return None
        rvas, names, sizes = entry
        i = bisect.bisect_right(rvas, address) - 1
        if i < 0:
            return None
        # Reject addresses past the end of a sized symbol: padding and data
        # between functions would otherwise be charged to the symbol before it.
        size = sizes[i]
        if size and address >= rvas[i] + size:
            return None
        return names[i]


def frame_names(thread: dict, libs: list[dict], symbols: SymbolIndex) -> list[str]:
    """Resolved name for every frame index in one thread's frameTable."""
    strings = thread["stringArray"]
    func_table = thread["funcTable"]
    frame_table = thread["frameTable"]
    resource_table = thread["resourceTable"]

    debug_ids: list[str | None] = []
    for res_lib in resource_table["lib"]:
        lib = libs[res_lib] if res_lib is not None and res_lib < len(libs) else None
        debug_ids.append(breakpad_to_debug_id(lib.get("breakpadId")) if lib else None)

    out: list[str] = []
    for i in range(frame_table["length"]):
        func = frame_table["func"][i]
        address = frame_table["address"][i]
        resource = func_table["resource"][func]
        debug_id = debug_ids[resource] if 0 <= resource < len(debug_ids) else None
        name = symbols.lookup(debug_id, address if address is not None else -1)
        if name is None:
            name = strings[func_table["name"][func]]
        out.append(name)
    return out


def walk_stack(stack_table: dict, stack: int) -> list[int]:
    """Frame indices for a stack, leaf first."""
    frames = []
    while stack is not None:
        frames.append(stack_table["frame"][stack])
        stack = stack_table["prefix"][stack]
    return frames


def collect(profile: dict, symbols: SymbolIndex, process: str, callers_re=None, depth=1, subtree_re=None):
    """Returns (self_us, inclusive_us, per_thread_us, callers_us, subtree_us, total_us)."""
    libs = profile["libs"]
    self_us: dict[str, float] = defaultdict(float)
    # Inclusive time counts a function once per sample even if it recurses,
    # otherwise a recursive walker would be charged N times for one sample.
    incl_us: dict[str, float] = defaultdict(float)
    per_thread: dict[str, float] = defaultdict(float)
    callers_us: dict[str, float] = defaultdict(float)
    subtree_us: dict[str, float] = defaultdict(float)
    total = 0.0

    for thread in profile["threads"]:
        if process and thread.get("processName") != process:
            continue
        samples = thread["samples"]
        cpu = samples.get("threadCPUDelta")
        if cpu is None:
            continue
        names = frame_names(thread, libs, symbols)
        stack_table = thread["stackTable"]
        label = f"{thread['name']}/{thread['tid']}"
        for i in range(samples["length"]):
            us = cpu[i] or 0
            if not us:
                continue
            stack = samples["stack"][i]
            if stack is None:
                continue
            total += us
            per_thread[label] += us
            frames = walk_stack(stack_table, stack)
            self_us[names[frames[0]]] += us
            for name in {names[f] for f in frames}:
                incl_us[name] += us
            if callers_re is not None:
                # Charge the sample to the first frame *above* the deepest match,
                # skipping further matches so a recursive or wrapper-heavy hit
                # (memmove inside memmove-ish shims) still names a real caller.
                hit = next((i for i, f in enumerate(frames) if callers_re.search(names[f])), None)
                if hit is not None:
                    up = [names[f] for f in frames[hit + 1 :] if not callers_re.search(names[f])]
                    if up:
                        callers_us[" <- ".join(up[:depth])] += us
            if subtree_re is not None and any(subtree_re.search(names[f]) for f in frames):
                subtree_us[names[frames[0]]] += us
    return self_us, incl_us, per_thread, callers_us, subtree_us, total


def print_table(title: str, rows, total: float, top: int) -> None:
    print(f"\n{title}  (total CPU across threads: {total / 1e6:.2f}s)")
    print(f"{'CPU (s)':>9}  {'%':>6}  symbol")
    print("-" * 78)
    for name, us in rows[:top]:
        pct = 100.0 * us / total if total else 0.0
        print(f"{us / 1e6:9.3f}  {pct:6.2f}  {name}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("profile", type=Path, help="profile.json.gz from `samply record --save-only`")
    ap.add_argument("--syms", type=Path, help="syms.json sidecar (default: derived from profile name)")
    ap.add_argument("--process", default="guff", help="processName to include (default: guff; '' for all)")
    ap.add_argument("--top", type=int, default=40, help="rows to print (default: 40)")
    ap.add_argument("--inclusive", metavar="REGEX", help="also print inclusive CPU for symbols matching REGEX")
    ap.add_argument("--callers", metavar="REGEX", help="attribute CPU spent in symbols matching REGEX to their callers")
    ap.add_argument("--depth", type=int, default=1, help="caller frames to show per row with --callers (default: 1)")
    ap.add_argument(
        "--subtree",
        metavar="REGEX",
        help="self CPU of samples taken anywhere under a frame matching REGEX "
        "(i.e. what one phase's inclusive time is made of)",
    )
    ap.add_argument("--threads", action="store_true", help="print per-thread CPU instead of symbols")
    args = ap.parse_args()

    profile = load_profile(args.profile)
    syms = args.syms or syms_path_for(args.profile)
    if not syms.exists():
        print(
            f"error: {syms} not found — re-record with `samply record --save-only "
            "--unstable-presymbolicate`, otherwise frames are hex addresses only.",
            file=sys.stderr,
        )
        return 2
    symbols = SymbolIndex(json.loads(syms.read_text()))

    callers_re = re.compile(args.callers) if args.callers else None
    subtree_re = re.compile(args.subtree) if args.subtree else None
    self_us, incl_us, per_thread, callers_us, subtree_us, total = collect(
        profile, symbols, args.process, callers_re, max(1, args.depth), subtree_re
    )
    if total == 0:
        print(f"error: no samples for processName={args.process!r}", file=sys.stderr)
        return 2

    if args.threads:
        print(f"\nper-thread CPU  (total {total / 1e6:.2f}s)")
        print(f"{'CPU (s)':>9}  {'%':>6}  thread")
        print("-" * 78)
        for label, us in sorted(per_thread.items(), key=lambda kv: -kv[1]):
            print(f"{us / 1e6:9.3f}  {100.0 * us / total:6.2f}  {label}")
        return 0

    rows = sorted(self_us.items(), key=lambda kv: -kv[1])
    print_table("self CPU by symbol", rows, total, args.top)

    if args.inclusive:
        pattern = re.compile(args.inclusive)
        matched = sorted(
            ((n, us) for n, us in incl_us.items() if pattern.search(n)),
            key=lambda kv: -kv[1],
        )
        print_table(f"inclusive CPU matching /{args.inclusive}/", matched, total, args.top)
        if not matched:
            print("  (no symbol matched)")

    if callers_re is not None:
        rows = sorted(callers_us.items(), key=lambda kv: -kv[1])
        print_table(f"callers of /{args.callers}/", rows, total, args.top)
        if not rows:
            print("  (no symbol matched)")

    if subtree_re is not None:
        # Percentages here are of the subtree, not of the run: the question
        # --subtree answers is "what is this phase made of", and 16% of the seed
        # build is the useful number, not the 2% of total CPU it also is.
        rows = sorted(subtree_us.items(), key=lambda kv: -kv[1])
        sub_total = sum(subtree_us.values())
        print_table(f"self CPU inside /{args.subtree}/", rows, sub_total, args.top)
        if not rows:
            print("  (no symbol matched)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
