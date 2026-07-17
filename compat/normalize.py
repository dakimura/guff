#!/usr/bin/env python3
"""Normalize golangci-lint / guff JSON issue dumps for set-diff comparison (R21).

Canonical key: ``relpath:line:linter:message``

Paths are relativized to the target module root. A small set of known
message phrasings (errcheck / unused prefixes) are canonicalized so that
equivalent findings from the two tools collide on the same key.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable


def extract_issues_json(raw: str) -> dict:
    """Parse the first JSON object that contains an ``Issues`` key.

    Both tools may emit logs before/after the JSON blob (golangci-lint
    prints a text summary after ``--output.json.path=stdout``).
    """
    decoder = json.JSONDecoder()
    for i, ch in enumerate(raw):
        if ch != "{":
            continue
        try:
            obj, _end = decoder.raw_decode(raw, i)
        except json.JSONDecodeError:
            continue
        if isinstance(obj, dict) and "Issues" in obj:
            return obj
    raise ValueError("no JSON object with an Issues key found")


def load_issues(path: Path | str) -> list[dict]:
    raw = Path(path).read_text(encoding="utf-8", errors="replace")
    return list(extract_issues_json(raw).get("Issues") or [])


def normalize_path(filename: str, root: str) -> str:
    """Return ``filename`` relative to ``root`` when possible."""
    filename = filename.replace("\\", "/")
    while filename.startswith("./"):
        filename = filename[2:]
    root_n = os.path.realpath(root).replace("\\", "/")
    root_slash = root_n.rstrip("/") + "/"
    base = os.path.basename(root_n)

    def under_root(abs_path: str) -> str | None:
        abs_path = os.path.realpath(abs_path).replace("\\", "/")
        if abs_path.startswith(root_slash):
            return abs_path[len(root_slash) :]
        if abs_path == root_n:
            return ""
        return None

    # Absolute paths: strip the root prefix when present.
    if os.path.isabs(filename):
        rel = under_root(filename)
        if rel is not None:
            return rel
        return filename

    # golangci sometimes prefixes the module directory name
    # (e.g. ``fixture/main.go`` when root ends with ``fixture``).
    parts = filename.split("/")
    if parts and parts[0] == base:
        stripped = "/".join(parts[1:])
        if stripped:
            return stripped

    # Relative path that already lives under root.
    if (Path(root_n) / filename).exists():
        return filename

    joined = under_root(os.path.join(root_n, filename))
    if joined is not None:
        return joined

    return filename


_UNUSED_PREFIX = re.compile(r"^(func|var|const|type|field|method)\s+")
_STATICCHECK_CODE = re.compile(r"^(?:SA|ST|S|QF)\d{4}:\s*")

# Known equivalent phrasings across guff and golangci-lint.
_ERRCHECK_EQUIV = {
    "unchecked error",
    "Error return value is not checked",
}


def normalize_message(linter: str, text: str) -> str:
    t = text.strip()
    if linter == "errcheck" and t in _ERRCHECK_EQUIV:
        return "Error return value is not checked"
    if linter == "unused":
        t = _UNUSED_PREFIX.sub("", t)
    if linter == "staticcheck":
        # golangci prefixes check codes (`QF1003: …`); guff often omits them.
        t = _STATICCHECK_CODE.sub("", t)
    return t


def issue_key(issue: dict, root: str) -> str:
    pos = issue["Pos"]
    path = normalize_path(pos["Filename"], root)
    linter = issue["FromLinter"]
    msg = normalize_message(linter, issue.get("Text") or "")
    return f"{path}:{pos['Line']}:{linter}:{msg}"


def issue_keys(issues: Iterable[dict], root: str) -> set[str]:
    return {issue_key(i, root) for i in issues}


@dataclass
class AllowEntry:
    target: str
    side: str  # "guff-only" | "golangci-only"
    key: str


def parse_allowlist(path: Path | str | None) -> list[AllowEntry]:
    if path is None:
        return []
    p = Path(path)
    if not p.is_file():
        return []
    out: list[AllowEntry] = []
    for raw in p.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        # target side key…  (key may contain spaces)
        parts = line.split(None, 2)
        if len(parts) != 3:
            raise ValueError(f"bad allowlist line: {raw!r}")
        target, side, key = parts
        if side not in ("guff-only", "golangci-only"):
            raise ValueError(f"bad allowlist side {side!r} in {raw!r}")
        out.append(AllowEntry(target=target, side=side, key=key))
    return out


@dataclass
class DiffResult:
    target: str
    guff: set[str]
    golangci: set[str]
    both: set[str] = field(init=False)
    guff_only: set[str] = field(init=False)
    golangci_only: set[str] = field(init=False)
    unexpected_guff: set[str] = field(default_factory=set)
    unexpected_golangci: set[str] = field(default_factory=set)
    allowed_guff: set[str] = field(default_factory=set)
    allowed_golangci: set[str] = field(default_factory=set)

    def __post_init__(self) -> None:
        self.both = self.guff & self.golangci
        self.guff_only = self.guff - self.golangci
        self.golangci_only = self.golangci - self.guff

    def apply_allowlist(self, entries: Iterable[AllowEntry]) -> None:
        allow_guff = {e.key for e in entries if e.target == self.target and e.side == "guff-only"}
        allow_gcl = {
            e.key for e in entries if e.target == self.target and e.side == "golangci-only"
        }
        # Also accept wildcard target "*"
        allow_guff |= {e.key for e in entries if e.target == "*" and e.side == "guff-only"}
        allow_gcl |= {e.key for e in entries if e.target == "*" and e.side == "golangci-only"}

        self.allowed_guff = self.guff_only & allow_guff
        self.allowed_golangci = self.golangci_only & allow_gcl
        self.unexpected_guff = self.guff_only - allow_guff
        self.unexpected_golangci = self.golangci_only - allow_gcl

    @property
    def precision(self) -> float:
        return len(self.both) / len(self.guff) if self.guff else 1.0

    @property
    def recall(self) -> float:
        return len(self.both) / len(self.golangci) if self.golangci else 1.0

    @property
    def ok(self) -> bool:
        return not self.unexpected_guff and not self.unexpected_golangci

    def per_linter(self) -> dict[str, dict[str, float | int]]:
        def lint_of(key: str) -> str:
            # path:line:linter:message — path may contain ":" on Windows drive;
            # we always emit POSIX-style keys, so split from the right carefully:
            # last two separators around line/linter are stable.
            # Format is fixed: relpath:line:linter:message
            parts = key.split(":", 3)
            if len(parts) < 4:
                return "?"
            return parts[2]

        stats: dict[str, dict[str, float | int]] = {}
        linters = {lint_of(k) for k in self.guff | self.golangci}
        for lint in sorted(linters):
            g = {k for k in self.guff if lint_of(k) == lint}
            c = {k for k in self.golangci if lint_of(k) == lint}
            both = g & c
            stats[lint] = {
                "guff": len(g),
                "golangci": len(c),
                "both": len(both),
                "precision": (len(both) / len(g)) if g else 1.0,
                "recall": (len(both) / len(c)) if c else 1.0,
            }
        return stats


def diff_sets(
    target: str,
    guff_keys: set[str],
    golangci_keys: set[str],
    allowlist: Iterable[AllowEntry] | None = None,
) -> DiffResult:
    result = DiffResult(target=target, guff=guff_keys, golangci=golangci_keys)
    result.apply_allowlist(allowlist or [])
    return result


def format_report(results: list[DiffResult]) -> str:
    lines: list[str] = [
        "# Compatibility report (guff vs golangci-lint)",
        "",
        "| Target | guff | golangci | both | P | R | unexpected |",
        "|--------|-----:|---------:|-----:|--:|--:|-----------:|",
    ]
    for r in results:
        unexpected = len(r.unexpected_guff) + len(r.unexpected_golangci)
        mark = "" if r.ok else " **"
        lines.append(
            f"| {r.target} | {len(r.guff)} | {len(r.golangci)} | {len(r.both)} | "
            f"{r.precision:.1%} | {r.recall:.1%} | {unexpected}{mark} |"
        )
    lines.append("")
    lines.append(
        "Precision = |intersection| / |guff|; Recall = |intersection| / |golangci|. "
        "`unexpected` counts diffs not covered by `allowlist.txt`."
    )
    lines.append("")

    for r in results:
        lines.append(f"## {r.target}")
        lines.append("")
        lines.append("| Linter | guff | golangci | both | P | R |")
        lines.append("|--------|-----:|---------:|-----:|--:|--:|")
        for lint, s in r.per_linter().items():
            lines.append(
                f"| {lint} | {s['guff']} | {s['golangci']} | {s['both']} | "
                f"{s['precision']:.1%} | {s['recall']:.1%} |"
            )
        lines.append("")
        if r.unexpected_guff:
            lines.append("### Unexpected guff-only")
            for k in sorted(r.unexpected_guff):
                lines.append(f"- `{k}`")
            lines.append("")
        if r.unexpected_golangci:
            lines.append("### Unexpected golangci-only")
            for k in sorted(r.unexpected_golangci):
                lines.append(f"- `{k}`")
            lines.append("")
        if r.allowed_guff or r.allowed_golangci:
            n = len(r.allowed_guff) + len(r.allowed_golangci)
            lines.append(f"### Allowed known diffs ({n})")
            # Keep RESULTS.md readable; full keys live in allowlist.txt.
            shown = 0
            for k in sorted(r.allowed_guff):
                if shown >= 8:
                    break
                lines.append(f"- guff-only: `{k}`")
                shown += 1
            for k in sorted(r.allowed_golangci):
                if shown >= 8:
                    break
                lines.append(f"- golangci-only: `{k}`")
                shown += 1
            remaining = n - shown
            if remaining > 0:
                lines.append(f"- … and {remaining} more (see `allowlist.txt`)")
            lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_keys = sub.add_parser("keys", help="Print normalized keys from one JSON dump")
    p_keys.add_argument("json")
    p_keys.add_argument("--root", required=True)

    p_diff = sub.add_parser("diff", help="Diff two JSON dumps")
    p_diff.add_argument("--target", required=True)
    p_diff.add_argument("--root", required=True)
    p_diff.add_argument("--guff", required=True)
    p_diff.add_argument("--golangci", required=True)
    p_diff.add_argument("--allowlist", default=None)
    p_diff.add_argument("--report", default=None, help="Write markdown report path")
    p_diff.add_argument(
        "--json-out",
        default=None,
        help="Write machine-readable summary JSON",
    )

    p_report = sub.add_parser(
        "report",
        help="Build a multi-target report from a TSV of target\\troot\\tguff.json\\tgcl.json",
    )
    p_report.add_argument("manifest")
    p_report.add_argument("--allowlist", default=None)
    p_report.add_argument("--report", required=True)
    p_report.add_argument("--json-out", default=None)

    args = ap.parse_args(argv)

    if args.cmd == "keys":
        for k in sorted(issue_keys(load_issues(args.json), args.root)):
            print(k)
        return 0

    if args.cmd == "diff":
        allow = parse_allowlist(args.allowlist)
        result = diff_sets(
            args.target,
            issue_keys(load_issues(args.guff), args.root),
            issue_keys(load_issues(args.golangci), args.root),
            allow,
        )
        text = format_report([result])
        if args.report:
            Path(args.report).write_text(text, encoding="utf-8")
        else:
            sys.stdout.write(text)
        if args.json_out:
            Path(args.json_out).write_text(
                json.dumps(
                    {
                        "target": result.target,
                        "guff": len(result.guff),
                        "golangci": len(result.golangci),
                        "both": len(result.both),
                        "precision": result.precision,
                        "recall": result.recall,
                        "ok": result.ok,
                        "unexpected_guff": sorted(result.unexpected_guff),
                        "unexpected_golangci": sorted(result.unexpected_golangci),
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )
        return 0 if result.ok else 1

    if args.cmd == "report":
        allow = parse_allowlist(args.allowlist)
        results: list[DiffResult] = []
        for raw in Path(args.manifest).read_text(encoding="utf-8").splitlines():
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            target, root, guff_json, gcl_json = line.split("\t")
            results.append(
                diff_sets(
                    target,
                    issue_keys(load_issues(guff_json), root),
                    issue_keys(load_issues(gcl_json), root),
                    allow,
                )
            )
        text = format_report(results)
        Path(args.report).write_text(text, encoding="utf-8")
        if args.json_out:
            Path(args.json_out).write_text(
                json.dumps(
                    [
                        {
                            "target": r.target,
                            "guff": len(r.guff),
                            "golangci": len(r.golangci),
                            "both": len(r.both),
                            "precision": r.precision,
                            "recall": r.recall,
                            "ok": r.ok,
                            "unexpected_guff": sorted(r.unexpected_guff),
                            "unexpected_golangci": sorted(r.unexpected_golangci),
                        }
                        for r in results
                    ],
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )
        return 0 if all(r.ok for r in results) else 1

    return 2


if __name__ == "__main__":
    raise SystemExit(main())
