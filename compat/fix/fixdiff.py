#!/usr/bin/env python3
"""Tree-level ``--fix`` comparison for the compat harness (Phase 3, fix tier).

Every other tier in this harness compares **what the two tools say**. This one
compares **what they write**: a case is materialized twice, each tool is run
over its own copy with ``--fix``, and the two resulting trees are diffed against
the pristine one. The key is the unified diff itself, byte for byte.

That gap is not theoretical. The golden key is
``path:line:col:linter:severity:text`` — the *rendered diagnostic* — and a
suggested fix's replacement text appears nowhere in it. Three independent
``--fix`` defects shipped under a fully green golden tier (COMPAT-HARDENING.md,
2026-08-24 続き 37): a conflict rule that dropped the wrong side, an
``errors.As`` fix that was never built at all, and no gofmt pass after applying.
The first two change which bytes land in the file; the third changes only the
indentation. All three are invisible to a finding-set comparison and all three
are one line of this tier's diff.

An **absent** ``expected/<case>.diff`` means upstream changed nothing, and guff
must change nothing either — the same convention as compat/health.py's baseline,
where a row that is not written is a target held at strictly zero. Recording
"no changes" as an empty file would make the two indistinguishable from a case
nobody has regenerated yet.
"""

from __future__ import annotations

import argparse
import collections
import difflib
import sys
from pathlib import Path

NO_NEWLINE = "\\ No newline at end of file"


def read_lines(path: Path) -> list[str] | None:
    """Return `path`'s lines with their endings, or None if it is not text."""
    try:
        text = path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError):
        return None
    return text.splitlines(keepends=True)


def render_hunk_lines(lines: list[str], prefix: str) -> list[str]:
    """Prefix diff lines, spelling a missing final newline the way git does."""
    out = []
    for line in lines:
        if line.endswith("\n"):
            out.append(prefix + line[:-1])
        else:
            out.append(prefix + line)
            out.append(NO_NEWLINE)
    return out


def fmt_range(start: int, stop: int) -> str:
    """A unified-diff range, spelled the way diff(1) and git spell it."""
    length = stop - start
    if length == 1:
        return str(start + 1)
    if not length:
        return f"{start},0"
    return f"{start + 1},{length}"


def file_diff(rel: str, before: Path | None, after: Path | None) -> list[str]:
    """Unified diff for one file. Either side may be None (added / removed)."""
    b = read_lines(before) if before is not None else []
    a = read_lines(after) if after is not None else []
    if b is None or a is None:
        return [f"Binary files a/{rel} and b/{rel} differ"]
    if b == a:
        return []

    left = "/dev/null" if before is None else f"a/{rel}"
    right = "/dev/null" if after is None else f"b/{rel}"
    out = [f"--- {left}", f"+++ {right}"]
    matcher = difflib.SequenceMatcher(a=b, b=a, autojunk=False)
    for group in matcher.get_grouped_opcodes(3):
        i1, i2 = group[0][1], group[-1][2]
        j1, j2 = group[0][3], group[-1][4]
        out.append(f"@@ -{fmt_range(i1, i2)} +{fmt_range(j1, j2)} @@")
        for tag, k1, k2, l1, l2 in group:
            if tag == "equal":
                out += render_hunk_lines(b[k1:k2], " ")
                continue
            if tag in ("replace", "delete"):
                out += render_hunk_lines(b[k1:k2], "-")
            if tag in ("replace", "insert"):
                out += render_hunk_lines(a[l1:l2], "+")
    return out


def walk(root: Path) -> dict[str, Path]:
    return {
        str(p.relative_to(root)): p for p in sorted(root.rglob("*")) if p.is_file()
    }


def tree_diff(before: Path, after: Path) -> str:
    """Normalized unified diff of two trees: no timestamps, POSIX-sorted."""
    b, a = walk(before), walk(after)
    lines: list[str] = []
    for rel in sorted(set(b) | set(a)):
        lines += file_diff(rel, b.get(rel), a.get(rel))
    return "\n".join(lines) + ("\n" if lines else "")


HEADER = """\
# Expected `--fix` result — generated, do not hand-edit.
#
# Regenerate with: ./compat/fix/regen.sh {case}
# Produced by running `golangci-lint run --fix` on compat/golden/cases/{case}/
# and diffing the materialized tree before -> after.
#
# An absent file for a case means upstream's --fix changes nothing there, and
# guff must change nothing either.
"""


PENDING_HEADER = """\
# What guff's `--fix` writes TODAY for {case} — generated, do not hand-edit.
#
# A pending baseline, not an allowlist: the case's real expectation is
# expected/{case}.diff, and it is printed in full on every run. This file only
# keeps CI green while the gap is worked down, and it fails the gate if guff's
# output moves in *either* direction — including once guff gets it right, so the
# ledger cannot quietly outlive the defect it records.
#
# Re-record with: ./compat/fix/regen.sh --pending {case}
"""


def render_expected(case: str, body: str, tool_version: str) -> str:
    return HEADER.format(case=case) + f"# golangci-lint: {tool_version}\n\n" + body


def parse_expected(path: Path | str) -> str:
    """Strip the generated header. Everything from the first `--- ` is verbatim.

    Only the leading block is dropped, never a line-by-line filter: a `#` at the
    start of a *diff* line is ordinary Go content (`//go:build`, a shell line in
    a testdata script), and an empty line inside a hunk is a context line spelled
    as a single space, not as "".
    """
    text = Path(path).read_text(encoding="utf-8")
    for i, line in enumerate(text.splitlines(keepends=True)):
        if line.startswith("--- ") or line.startswith("Binary files "):
            return "".join(text.splitlines(keepends=True)[i:])
    return ""


def confirm(runs: list[str], confirmations: int) -> str | None:
    """Return the diff seen at least `confirmations` times, else None.

    Same reasoning as compat/golden/golden.py: golangci-lint is not a
    deterministic function of its input, and a --fix recording is strictly worse
    than a truncated golden — it would pin *fewer edits than upstream makes* and
    read as guff over-fixing forever after.
    """
    tally: collections.Counter[str] = collections.Counter()
    for run in runs:
        tally[run] += 1
        if tally[run] >= confirmations:
            return run
    return None


def check_pending(case: str, path: Path, expected: str, actual: str) -> int:
    """Hold a case whose --fix parity is known-missing, at exactly today's bytes.

    Fifteen linters carry a `DEFERRED: SuggestedFix` note in their module doc.
    Every one of them reports correctly and rewrites nothing, so the golden tier
    is green and a user's `--fix` silently leaves the finding in place. A gate
    that fails on all of them from day one gets turned off; a gate that ignores
    them measures nothing. So: record what guff writes *today*, print what
    upstream writes instead, and fail the moment either side moves.

    This is not an allowlist. Nothing is suppressed — the price of each deferral
    is printed on every run — and the file is deliberately annoying to keep:
    once guff matches upstream, leaving it in place fails the gate.
    """
    if not path.exists():
        return 1
    pending = parse_expected(path)
    if actual != pending:
        print(
            f"  {case}: PENDING BASELINE MOVED — guff's --fix is neither"
            f" upstream's answer nor the recorded one. Re-record with"
            f" ./compat/fix/regen.sh --pending {case} if this is the improvement"
            f" it looks like.",
            file=sys.stderr,
        )
        return 1
    print(
        f"  {case}: pending — upstream writes {len(expected.splitlines())} diff"
        f" line(s), guff writes {len(actual.splitlines())}"
    )
    return 0


def diff_of_diffs(case: str, expected: str, actual: str) -> str:
    """Show where guff's edits and upstream's stop agreeing."""
    lines = [
        f"{case}: guff's --fix differs from golangci-lint's",
        f"  ({len(expected.splitlines())} expected diff line(s),"
        f" {len(actual.splitlines())} from guff)",
    ]
    lines += [
        "  " + line.rstrip("\n")
        for line in difflib.unified_diff(
            expected.splitlines(keepends=True),
            actual.splitlines(keepends=True),
            fromfile="expected (golangci-lint)",
            tofile="actual (guff)",
            lineterm="",
        )
    ]
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_capture = sub.add_parser("capture", help="Render the before->after diff of a tree")
    p_capture.add_argument("--before", required=True)
    p_capture.add_argument("--after", required=True)
    p_capture.add_argument("-o", "--output", required=True)

    p_write = sub.add_parser("write", help="Record golangci-lint's --fix as expected")
    p_write.add_argument("--case", required=True)
    p_write.add_argument(
        "--run",
        action="append",
        required=True,
        metavar="DIFF",
        help="A captured diff; repeat to supply independent runs of the same case",
    )
    p_write.add_argument("--confirmations", type=int, default=2)
    p_write.add_argument("--tool-version", default="unknown")
    p_write.add_argument("-o", "--output", required=True)

    p_pending = sub.add_parser("pending", help="Record what guff writes today")
    p_pending.add_argument("--case", required=True)
    p_pending.add_argument("--actual", required=True)
    p_pending.add_argument(
        "--expected",
        required=True,
        help="expected/<case>.diff, so a case upstream never touches cannot be held",
    )
    p_pending.add_argument("-o", "--output", required=True)

    p_check = sub.add_parser("check", help="Compare guff's --fix against the expectation")
    p_check.add_argument("--case", required=True)
    p_check.add_argument("--actual", required=True)
    p_check.add_argument(
        "--expected",
        required=True,
        help="expected/<case>.diff; absent means upstream changes nothing",
    )
    p_check.add_argument(
        "--pending",
        help="pending/<case>.diff: a case whose --fix parity is known-missing",
    )

    args = ap.parse_args(argv)

    if args.cmd == "capture":
        Path(args.output).write_text(
            tree_diff(Path(args.before), Path(args.after)), encoding="utf-8"
        )
        return 0

    if args.cmd == "write":
        runs = [Path(p).read_text(encoding="utf-8") for p in args.run]
        body = confirm(runs, max(1, args.confirmations))
        if body is None:
            sizes = [len(r.splitlines()) for r in runs]
            print(
                f"  {args.case}: {len(runs)} run(s) of golangci-lint --fix did not"
                f" agree (diff sizes {sizes})",
                file=sys.stderr,
            )
            return 1
        out = Path(args.output)
        if not body.strip():
            if out.exists():
                out.unlink()
                print(f"  {args.case}: upstream fixes nothing — removed {out}")
            else:
                print(f"  {args.case}: upstream fixes nothing (no file)")
            return 0
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(
            render_expected(args.case, body, args.tool_version), encoding="utf-8"
        )
        print(f"  {args.case}: wrote {len(body.splitlines())} diff line(s) to {out}")
        return 0

    if args.cmd == "pending":
        body = Path(args.actual).read_text(encoding="utf-8")
        out = Path(args.output)
        expected_path = Path(args.expected)
        if body.strip() and not (
            expected_path.exists() and parse_expected(expected_path).strip()
        ):
            # A gap in guff's fixers is a deferral. Writing bytes into a file
            # upstream leaves alone is not one — it is guff editing somebody's
            # source on its own authority, which is how `omitempty` became
            # `omitzero` under a green harness — so it can never be held.
            print(
                f"  {args.case}: REFUSING to hold — golangci-lint --fix changes"
                f" nothing here and guff writes"
                f" {len(body.splitlines())} diff line(s). Fix guff.",
                file=sys.stderr,
            )
            return 1
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(PENDING_HEADER.format(case=args.case) + body, encoding="utf-8")
        n = len(body.splitlines())
        what = f"{n} diff line(s)" if n else "'guff writes nothing'"
        print(f"  {args.case}: recorded {what} in {out}")
        return 0

    if args.cmd == "check":
        expected_path = Path(args.expected)
        expected = parse_expected(expected_path) if expected_path.exists() else ""
        actual = Path(args.actual).read_text(encoding="utf-8")
        pending_path = Path(args.pending) if args.pending else None
        if expected == actual:
            n = len(actual.splitlines())
            print(f"  {args.case}: --fix matches ({n} diff line(s))")
            if pending_path is not None and pending_path.exists():
                print(
                    f"  {args.case}: guff now matches upstream — delete"
                    f" {pending_path}",
                    file=sys.stderr,
                )
                return 1
            return 0
        print(diff_of_diffs(args.case, expected, actual), file=sys.stderr)
        if pending_path is None:
            return 1
        return check_pending(args.case, pending_path, expected, actual)

    return 2


if __name__ == "__main__":
    raise SystemExit(main())
