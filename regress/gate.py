#!/usr/bin/env python3
"""Compare Prometheus regress measurements against a checked-in baseline.

Fails only on regression (worse wall / RSS / finding-set). Absolute golangci
parity is not required.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


DEFAULT_TOLERANCES: dict[str, float | int] = {
    # Fail on any wall increase vs the checked-in baseline (no proportional slack).
    "wall_seconds_ratio": 1.0,
    # Absolute seconds of measurement noise allowed above baseline × ratio.
    # Cold-cache laptop runs swing ~50–100ms; keep ratio at 1.0 (no % slack).
    "wall_seconds_epsilon": 0.15,
    "peak_rss_ratio": 1.20,
    "max_guff_only_delta": 0,
    "max_golangci_only_delta": 0,
    "min_both_delta": 0,
}


@dataclass
class GateFailure:
    metric: str
    message: str


def load_json(path: Path | str) -> dict[str, Any]:
    return json.loads(Path(path).read_text(encoding="utf-8"))


def evaluate(
    baseline: dict[str, Any],
    measured: dict[str, Any],
    *,
    prometheus_git_sha: str | None = None,
) -> list[GateFailure]:
    """Return a list of failures (empty => PASS)."""
    failures: list[GateFailure] = []
    tol = {**DEFAULT_TOLERANCES, **(baseline.get("tolerances") or {})}

    base_guff = baseline["guff"]
    meas_guff = measured["guff"]
    wall_limit = (
        float(base_guff["wall_seconds"]) * float(tol["wall_seconds_ratio"])
        + float(tol.get("wall_seconds_epsilon", 0.0))
    )
    rss_limit = int(base_guff["peak_rss_bytes"]) * float(tol["peak_rss_ratio"])

    if float(meas_guff["wall_seconds"]) > wall_limit:
        failures.append(
            GateFailure(
                "wall_seconds",
                f"wall {meas_guff['wall_seconds']:.3f}s > limit {wall_limit:.3f}s "
                f"(baseline {base_guff['wall_seconds']:.3f}s × {tol['wall_seconds_ratio']}"
                f" + {float(tol.get('wall_seconds_epsilon', 0.0)):.3f}s)",
            )
        )
    if int(meas_guff["peak_rss_bytes"]) > rss_limit:
        failures.append(
            GateFailure(
                "peak_rss_bytes",
                f"peak RSS {int(meas_guff['peak_rss_bytes']):,} > limit {int(rss_limit):,} "
                f"(baseline {int(base_guff['peak_rss_bytes']):,} × {tol['peak_rss_ratio']})",
            )
        )

    base_c = baseline["compat"]
    meas_c = measured["compat"]
    guff_only_limit = int(base_c["guff_only"]) + int(tol["max_guff_only_delta"])
    gcl_only_limit = int(base_c["golangci_only"]) + int(tol["max_golangci_only_delta"])
    both_floor = int(base_c["both"]) + int(tol["min_both_delta"])

    if int(meas_c["guff_only"]) > guff_only_limit:
        failures.append(
            GateFailure(
                "guff_only",
                f"guff_only {meas_c['guff_only']} > limit {guff_only_limit} "
                f"(baseline {base_c['guff_only']})",
            )
        )
    if int(meas_c["golangci_only"]) > gcl_only_limit:
        failures.append(
            GateFailure(
                "golangci_only",
                f"golangci_only {meas_c['golangci_only']} > limit {gcl_only_limit} "
                f"(baseline {base_c['golangci_only']})",
            )
        )
    if int(meas_c["both"]) < both_floor:
        failures.append(
            GateFailure(
                "both",
                f"both {meas_c['both']} < floor {both_floor} "
                f"(baseline {base_c['both']})",
            )
        )

    # SHA mismatch is advisory only (caller may print a warning).
    _ = prometheus_git_sha
    return failures


def build_baseline(
    measured: dict[str, Any],
    *,
    prometheus_git_sha: str,
    tolerances: dict[str, float | int] | None = None,
    previous: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Construct a baseline document from a measured payload."""
    prev_tol = (previous or {}).get("tolerances") or {}
    return {
        "prometheus_git_sha": prometheus_git_sha,
        "config": measured.get("config", ".golangci.yml"),
        "packages": list(measured.get("packages") or ["./tsdb/..."]),
        "concurrency": int(measured.get("concurrency", 1)),
        "rayon_threads": int(measured.get("rayon_threads", 2)),
        "isolate_gocache": bool(measured.get("isolate_gocache", False)),
        "guff": {
            "wall_seconds": float(measured["guff"]["wall_seconds"]),
            "peak_rss_bytes": int(measured["guff"]["peak_rss_bytes"]),
        },
        "compat": {
            "guff_issues": int(measured["compat"]["guff_issues"]),
            "golangci_issues": int(measured["compat"]["golangci_issues"]),
            "both": int(measured["compat"]["both"]),
            "guff_only": int(measured["compat"]["guff_only"]),
            "golangci_only": int(measured["compat"]["golangci_only"]),
            "precision": float(measured["compat"]["precision"]),
            "recall": float(measured["compat"]["recall"]),
        },
        "tolerances": {**DEFAULT_TOLERANCES, **prev_tol, **(tolerances or {})},
    }


def format_report(
    baseline: dict[str, Any],
    measured: dict[str, Any],
    failures: list[GateFailure],
    *,
    sha_warning: str | None = None,
) -> str:
    base_pkgs = " ".join(baseline.get("packages") or [])
    meas_pkgs = " ".join(measured.get("packages") or [])
    lines = [
        "# Prometheus regress gate",
        "",
        f"- Baseline SHA: `{baseline.get('prometheus_git_sha', '?')}`",
        f"- Measured SHA: `{measured.get('prometheus_git_sha', '?')}`",
        f"- Config: `{measured.get('config', baseline.get('config', '?'))}`",
        f"- Packages: `{meas_pkgs or '?'}`",
        f"- Concurrency: `-j {measured.get('concurrency', '?')}` / "
        f"`RAYON_NUM_THREADS={measured.get('rayon_threads', '?')}`",
        "",
        "| Metric | Baseline | Measured |",
        "|--------|---------:|---------:|",
        f"| wall_seconds | {baseline['guff']['wall_seconds']:.3f} | {measured['guff']['wall_seconds']:.3f} |",
        f"| peak_rss_bytes | {baseline['guff']['peak_rss_bytes']:,} | {measured['guff']['peak_rss_bytes']:,} |",
        f"| guff_issues | {baseline['compat']['guff_issues']} | {measured['compat']['guff_issues']} |",
        f"| golangci_issues | {baseline['compat']['golangci_issues']} | {measured['compat']['golangci_issues']} |",
        f"| both | {baseline['compat']['both']} | {measured['compat']['both']} |",
        f"| guff_only | {baseline['compat']['guff_only']} | {measured['compat']['guff_only']} |",
        f"| golangci_only | {baseline['compat']['golangci_only']} | {measured['compat']['golangci_only']} |",
        f"| precision | {baseline['compat']['precision']:.4f} | {measured['compat']['precision']:.4f} |",
        f"| recall | {baseline['compat']['recall']:.4f} | {measured['compat']['recall']:.4f} |",
        "",
    ]
    if base_pkgs and meas_pkgs and base_pkgs != meas_pkgs:
        lines.append(
            f"**Warning:** package set changed (`{base_pkgs}` → `{meas_pkgs}`); "
            "re-run with `--update-baseline` if intentional."
        )
        lines.append("")
    if sha_warning:
        lines.append(f"**Warning:** {sha_warning}")
        lines.append("")
    if failures:
        lines.append("## FAIL")
        lines.append("")
        for f in failures:
            lines.append(f"- `{f.metric}`: {f.message}")
        lines.append("")
    else:
        lines.append("## PASS")
        lines.append("")
        lines.append("No regressions vs baseline (within tolerances).")
        lines.append("")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_check = sub.add_parser("check", help="Compare measured.json to baseline.json")
    p_check.add_argument("--baseline", required=True)
    p_check.add_argument("--measured", required=True)
    p_check.add_argument("--report", default=None)

    p_update = sub.add_parser(
        "update-baseline",
        help="Rewrite baseline.json from measured.json (keeps prior tolerances)",
    )
    p_update.add_argument("--baseline", required=True)
    p_update.add_argument("--measured", required=True)

    args = ap.parse_args(argv)

    if args.cmd == "check":
        baseline = load_json(args.baseline)
        measured = load_json(args.measured)
        base_sha = str(baseline.get("prometheus_git_sha") or "")
        meas_sha = str(measured.get("prometheus_git_sha") or "")
        sha_warning = None
        if base_sha and meas_sha and base_sha != meas_sha:
            sha_warning = (
                f"prometheus git SHA changed ({base_sha[:12]} → {meas_sha[:12]}); "
                "re-run with --update-baseline if the corpus drift is intentional"
            )
            print(f"warning: {sha_warning}", file=sys.stderr)

        failures = evaluate(baseline, measured, prometheus_git_sha=meas_sha)
        text = format_report(baseline, measured, failures, sha_warning=sha_warning)
        if args.report:
            Path(args.report).write_text(text, encoding="utf-8")
        sys.stdout.write(text)
        return 1 if failures else 0

    if args.cmd == "update-baseline":
        measured = load_json(args.measured)
        previous = load_json(args.baseline) if Path(args.baseline).is_file() else None
        sha = str(measured.get("prometheus_git_sha") or "unknown")
        doc = build_baseline(measured, prometheus_git_sha=sha, previous=previous)
        Path(args.baseline).write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8")
        print(f"Updated {args.baseline}")
        return 0

    return 2


if __name__ == "__main__":
    raise SystemExit(main())
