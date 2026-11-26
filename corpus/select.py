#!/usr/bin/env python3
"""Select corpus repos from repos.json and print TSV rows.

Usage:
  python3 corpus/select.py [--tier pr|nightly|weekly|all] [--tier pr,nightly] ...

TSV columns (tab-separated):
  name  url  ref  packages  tier  timeout  config

``config`` is empty when the repo uses auto-discovery (.golangci.yml/.yaml).
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--repos",
        default=str(Path(__file__).resolve().parent / "repos.json"),
        help="Path to repos.json",
    )
    ap.add_argument(
        "--tier",
        action="append",
        default=[],
        help="Tier filter (repeatable or comma-separated). Default: all",
    )
    ap.add_argument(
        "--name",
        action="append",
        default=[],
        help="Optional name filter (repeatable)",
    )
    args = ap.parse_args(argv)

    tiers: set[str] = set()
    for raw in args.tier:
        for part in raw.split(","):
            part = part.strip()
            if part and part != "all":
                tiers.add(part)
    names = set(args.name)

    repos = json.loads(Path(args.repos).read_text(encoding="utf-8"))
    if not isinstance(repos, list):
        print("error: repos.json must be a JSON array", file=sys.stderr)
        return 2

    for repo in repos:
        name = repo["name"]
        tier = repo["tier"]
        if tiers and tier not in tiers:
            continue
        if names and name not in names:
            continue
        config = repo.get("config") or ""
        print(
            "\t".join(
                [
                    name,
                    repo["url"],
                    repo["ref"],
                    repo.get("packages") or "./...",
                    tier,
                    repo.get("timeout") or "15m",
                    config,
                ]
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
