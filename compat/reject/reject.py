#!/usr/bin/env python3
"""Compare the *refusals* of guff and golangci-lint (COMPAT-HARDENING §4).

Every other tier in `compat/` compares two finding sets. This one exists for the
configs where upstream produces no finding set at all: golangci-lint validates
its config before it runs anything (`config.Config.Validate`, and per-linter
settings validators such as gocritic's `validateOptionsCombinations`), and a
config that fails validation exits the process. A tool that instead *runs* such
a config is not compatible — it lints with an enable set the user never asked
for, and nothing downstream can tell.

The comparison is on the reason, not on the rendering: golangci-lint prints
config errors as ``Error: <reason>`` and linter-settings errors through its
logger as ``level=error msg="[linters_context] <reason>"``, while guff prefixes
its own name. Expected reasons are never hand-written — ``--regen`` records what
golangci-lint actually said, exactly like ``compat/golden``.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# `Error: can't load config: …` — the config loader's own failure path.
_GCL_ERROR = re.compile(r"^Error:\s*(?P<reason>.+?)\s*$", re.MULTILINE)
# `level=error msg="[linters_context] gocritic: invalid settings: …"` — a
# logger.Fatalf from a linter's context setter. The bracketed tag is the log
# component, not part of what went wrong.
_GCL_LOG = re.compile(
    r'^level=error\s+msg="(?:\[[^\]]+\]\s*)?(?P<reason>.*)"\s*$', re.MULTILINE
)
_GUFF_ERROR = re.compile(r"^guff:\s*(?P<reason>.+?)\s*$", re.MULTILINE)


def _unescape(text: str) -> str:
    """Undo the quoting logfmt applies inside msg="…"."""
    return text.replace('\\"', '"').replace("\\\\", "\\")


def reason_golangci(output: str) -> str | None:
    """The reason golangci-lint refused, or None if it did not refuse."""
    m = _GCL_ERROR.search(output)
    if m:
        return m.group("reason")
    m = _GCL_LOG.search(output)
    if m:
        return _unescape(m.group("reason"))
    return None


def reason_guff(output: str) -> str | None:
    """The reason guff refused, or None if it did not refuse."""
    m = _GUFF_ERROR.search(output)
    return m.group("reason") if m else None


# Some refusals end by telling the user how to list the linters, and each tool
# can only name its own command:
#
#   golangci-lint: … run 'golangci-lint help linters' to see the list …
#   guff:          … run 'guff linters' to see the list …
#
# That tail is advice, not the reason, and guff printing `golangci-lint help
# linters` at a user would be a bug rather than compatibility. Collapse both
# spellings to one token so the comparison stays on what went wrong. This is
# the *only* rewriting this tier does — everything else is compared verbatim,
# which is the point of it.
_HELP_POINTER = re.compile(r"run '(?:golangci-lint help linters|guff linters)'")


def canonical_reason(reason: str | None) -> str | None:
    """`reason` with each tool's own how-to-list-linters pointer collapsed."""
    if reason is None:
        return None
    return _HELP_POINTER.sub("run '<tool> linters'", reason)


def check_case(
    case: str, expected: str, guff_output: str, guff_rc: int, golangci_output: str, golangci_rc: int
) -> list[str]:
    """Return the problems with one case; empty means the case passes."""
    problems: list[str] = []

    # The recorded reason is re-checked against this run, not trusted: a golden
    # that stops matching upstream has stopped comparing anything.
    got_gcl = reason_golangci(golangci_output)
    if golangci_rc == 0:
        problems.append(f"{case}: golangci-lint exited 0 — it no longer refuses this config")
    elif canonical_reason(got_gcl) != canonical_reason(expected):
        problems.append(
            f"{case}: golangci-lint's reason moved\n"
            f"    expected {expected!r}\n"
            f"    got      {got_gcl!r}\n"
            f"    (regenerate with ./compat/reject/run.sh --regen after reading the diff)"
        )

    got_guff = reason_guff(guff_output)
    if guff_rc == 0:
        problems.append(f"{case}: guff exited 0 — it ran a config golangci-lint refuses")
    elif canonical_reason(got_guff) != canonical_reason(expected):
        problems.append(
            f"{case}: guff's reason differs\n"
            f"    golangci {expected!r}\n"
            f"    guff     {got_guff!r}"
        )
    return problems


def check_accept(case: str, guff_rc: int, golangci_rc: int) -> list[str]:
    """A control case: both tools must run it. Keeps the tier from passing red."""
    problems = []
    if golangci_rc != 0:
        problems.append(f"{case}: golangci-lint refused the control config (rc={golangci_rc})")
    if guff_rc != 0:
        problems.append(f"{case}: guff refused the control config (rc={guff_rc})")
    return problems


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_write = sub.add_parser("write", help="Record golangci-lint's reason as the expectation")
    p_write.add_argument("--case", required=True)
    p_write.add_argument("--golangci-output", required=True)
    p_write.add_argument("--golangci-rc", type=int, required=True)
    p_write.add_argument("-o", "--output", required=True)

    p_check = sub.add_parser("check", help="Compare both tools' refusals against the expectation")
    p_check.add_argument("--case", required=True)
    p_check.add_argument("--expected", required=True)
    p_check.add_argument("--guff-output", required=True)
    p_check.add_argument("--guff-rc", type=int, required=True)
    p_check.add_argument("--golangci-output", required=True)
    p_check.add_argument("--golangci-rc", type=int, required=True)

    p_accept = sub.add_parser("accept", help="Assert both tools run a control config")
    p_accept.add_argument("--case", required=True)
    p_accept.add_argument("--guff-rc", type=int, required=True)
    p_accept.add_argument("--golangci-rc", type=int, required=True)

    args = ap.parse_args(argv)

    if args.cmd == "write":
        out = Path(args.golangci_output).read_text(encoding="utf-8", errors="replace")
        reason = reason_golangci(out)
        if args.golangci_rc == 0 or reason is None:
            print(
                f"  {args.case}: golangci-lint did not refuse this config "
                f"(rc={args.golangci_rc}); nothing to record",
                file=sys.stderr,
            )
            return 1
        Path(args.output).write_text(reason + "\n", encoding="utf-8")
        print(f"  {args.case}: recorded {reason!r}")
        return 0

    if args.cmd == "check":
        problems = check_case(
            args.case,
            Path(args.expected).read_text(encoding="utf-8").strip("\n"),
            Path(args.guff_output).read_text(encoding="utf-8", errors="replace"),
            args.guff_rc,
            Path(args.golangci_output).read_text(encoding="utf-8", errors="replace"),
            args.golangci_rc,
        )
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        if not problems:
            print(f"  {args.case}: both refuse, same reason")
        return 1 if problems else 0

    if args.cmd == "accept":
        problems = check_accept(args.case, args.guff_rc, args.golangci_rc)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        if not problems:
            print(f"  {args.case}: both accept (control)")
        return 1 if problems else 0

    return 2


if __name__ == "__main__":
    raise SystemExit(main())
