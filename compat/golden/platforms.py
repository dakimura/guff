#!/usr/bin/env python3
"""Reject golden fixtures whose build constraints depend on the host platform.

A golden is a recording of golangci-lint's answer, and the whole tier rests on
that answer being the same everywhere. Build constraints break that quietly: a
file carrying ``//go:build linux`` is invisible to ``go list`` on a laptop and
compiled on the runner, so a golden recorded on darwin is short exactly the
findings the file would have produced, and the runner then reports them as
guff's extras. Nothing in the diff says "platform" — it reads as a precision
bug in whichever check happened to fire.

That is how ``sa4019/bad.go`` and ``sa4032/bad.go`` spent their whole life in
``cases/staticcheck-sa``: both were ``linux``-only, both were absent from a
darwin-recorded golden, and both showed up on CI as unexplained extras. The
fixtures now use a constraint that is true on every platform the project
supports; this module is what stops the next one.

The invariant is not "no build constraints" — a check like SA4032 *is* about
build constraints and cannot be tested without them. It is:

    every file's include/exclude decision must be the same on every platform
    in `SUPPORTED`, whatever the non-platform tags are set to.

``//go:build !plan9`` satisfies that (it is true on all of them). ``linux``
does not. A tag that is neither a GOOS nor a GOARCH — ``custom``, ``go1.24``,
``!nope`` — is opaque: it may well change the answer, but it changes it the
same way everywhere, so it is tried both ways and each way must be constant.

A case that pins the platform in ``cases/<name>/env`` (cases/staticcheck-386
cross-compiles to linux/386, the only way SA1027 can fire at all) has already
removed the axis by construction; the pinned pair is then the only one checked.
"""

from __future__ import annotations

import argparse
import itertools
import sys
from pathlib import Path

# The platforms a `./compat/golden/run.sh` must agree on: the four the project
# releases binaries for (.github/workflows/release.yml). CI runs linux/amd64
# and development happens on darwin/arm64, which is the pair that hid the two
# fixtures above; the other two come along because nothing costs less.
SUPPORTED = (
    ("linux", "amd64"),
    ("linux", "arm64"),
    ("darwin", "amd64"),
    ("darwin", "arm64"),
)

# `go tool dist list`, split. Only membership matters here: a token in either
# set is resolved against the platform, anything else is opaque.
GOOS = frozenset(
    """aix android darwin dragonfly freebsd hurd illumos ios js linux nacl
       netbsd openbsd plan9 solaris wasip1 windows zos""".split()
)
GOARCH = frozenset(
    """386 amd64 amd64p32 arm armbe arm64 arm64be loong64 mips mipsle mips64
       mips64le mips64p32 mips64p32le ppc ppc64 ppc64le riscv riscv64 s390
       s390x sparc sparc64 wasm""".split()
)

# Enumerating opaque tags is exponential. Fixtures use one or two; a file that
# needs more than this is not something to guess about.
MAX_OPAQUE = 8


class ConstraintError(Exception):
    """A constraint this module cannot evaluate, which is never a pass."""


# --------------------------------------------------------------------------
# Parsing
# --------------------------------------------------------------------------


def header_lines(text: str) -> list[str]:
    """The lines before the package clause — the only place a constraint counts.

    ``misplaced_plus.go`` in the govet case carries a ``// +build`` line *after*
    its package clause on purpose (that is the shape it tests), and go/build
    ignores it there. Reading the whole file would turn that fixture into a
    linux-only one that is not.
    """
    out = []
    for line in text.splitlines():
        if line.lstrip().startswith("package "):
            break
        out.append(line)
    return out


def plus_build_to_expr(args: str) -> str:
    """Translate one ``// +build`` line into ``//go:build`` syntax.

    Space separates OR terms, comma separates AND terms, ``!`` negates.
    """
    terms = []
    for term in args.split():
        parts = [f"!{p[1:]}" if p.startswith("!") else p for p in term.split(",")]
        terms.append("(" + " && ".join(parts) + ")")
    return "(" + " || ".join(terms) + ")"


def file_constraint(path: Path, text: str) -> str | None:
    """The file's effective build constraint, or None if it has none.

    Combines, in Go's own precedence: the ``//go:build`` line if there is one
    (it supersedes the legacy form outright), else the AND of every
    ``// +build`` line, and in either case the implicit constraint the
    ``_GOOS_GOARCH`` filename suffix carries.
    """
    go_build = None
    plus = []
    for line in header_lines(text):
        stripped = line.strip()
        if stripped.startswith("//go:build "):
            if go_build is None:
                go_build = stripped[len("//go:build ") :].strip()
        elif stripped.startswith("// +build "):
            plus.append(plus_build_to_expr(stripped[len("// +build ") :].strip()))

    parts = []
    if go_build is not None:
        parts.append(f"({go_build})")
    elif plus:
        parts.extend(plus)
    suffix = filename_constraint(path.name)
    if suffix:
        parts.append(suffix)
    if not parts:
        return None
    return " && ".join(parts)


def filename_constraint(name: str) -> str | None:
    """The constraint implied by a ``_GOOS``/``_GOARCH``/``_GOOS_GOARCH`` suffix.

    Go only reads the suffix when something precedes it: ``linux.go`` is an
    ordinary file, ``bar_linux.go`` is not.
    """
    if not name.endswith(".go"):
        return None
    stem = name[: -len(".go")]
    if stem.endswith("_test"):
        stem = stem[: -len("_test")]
    parts = stem.split("_")
    tags = []
    if len(parts) >= 3 and parts[-2] in GOOS and parts[-1] in GOARCH:
        tags = [parts[-2], parts[-1]]
    elif len(parts) >= 2 and parts[-1] in GOOS:
        tags = [parts[-1]]
    elif len(parts) >= 2 and parts[-1] in GOARCH:
        tags = [parts[-1]]
    if not tags:
        return None
    return "(" + " && ".join(tags) + ")"


# --------------------------------------------------------------------------
# Evaluation
# --------------------------------------------------------------------------


def tokenize(expr: str) -> list[str]:
    toks, i = [], 0
    while i < len(expr):
        c = expr[i]
        if c.isspace():
            i += 1
        elif expr.startswith("&&", i) or expr.startswith("||", i):
            toks.append(expr[i : i + 2])
            i += 2
        elif c in "!()":
            toks.append(c)
            i += 1
        elif c.isalnum() or c in "._":
            j = i
            while j < len(expr) and (expr[j].isalnum() or expr[j] in "._"):
                j += 1
            toks.append(expr[i:j])
            i = j
        else:
            raise ConstraintError(f"unparsable character {c!r} in {expr!r}")
    return toks


def idents(expr: str) -> set[str]:
    return {t for t in tokenize(expr) if t not in ("!", "(", ")", "&&", "||")}


def opaque_idents(expr: str) -> set[str]:
    return {t for t in idents(expr) if t not in GOOS and t not in GOARCH and t != "unix"}


def evaluate(expr: str, goos: str, goarch: str, env: dict[str, bool]) -> bool:
    toks = tokenize(expr)
    pos = 0

    def peek():
        return toks[pos] if pos < len(toks) else None

    def parse_or():
        nonlocal pos
        v = parse_and()
        while peek() == "||":
            pos += 1
            v = parse_and() or v
        return v

    def parse_and():
        nonlocal pos
        v = parse_unary()
        while peek() == "&&":
            pos += 1
            v = parse_unary() and v
        return v

    def parse_unary():
        nonlocal pos
        if peek() == "!":
            pos += 1
            return not parse_unary()
        if peek() == "(":
            pos += 1
            v = parse_or()
            if peek() != ")":
                raise ConstraintError(f"unbalanced parentheses in {expr!r}")
            pos += 1
            return v
        tok = peek()
        if tok is None or tok in ("&&", "||", ")"):
            raise ConstraintError(f"expected a tag in {expr!r}")
        pos += 1
        return resolve(tok, goos, goarch, env)

    value = parse_or()
    if pos != len(toks):
        raise ConstraintError(f"trailing tokens in {expr!r}")
    return value


def resolve(tag: str, goos: str, goarch: str, env: dict[str, bool]) -> bool:
    if tag == "unix":
        return goos not in ("windows", "plan9", "js", "wasip1")
    if tag in GOOS:
        return tag == goos
    if tag in GOARCH:
        return tag == goarch
    return env[tag]


def offending_platforms(expr: str, platforms) -> list[tuple[str, str, bool]] | None:
    """None if the constraint is platform-invariant, else what it evaluates to.

    Invariant means: for every setting of the opaque tags, the include/exclude
    decision is the same on every platform. The returned list is the first
    setting that splits them, so the error message can name real platforms.
    """
    unknown = sorted(opaque_idents(expr))
    if len(unknown) > MAX_OPAQUE:
        raise ConstraintError(f"{len(unknown)} non-platform tags in {expr!r}")
    for combo in itertools.product((False, True), repeat=len(unknown)):
        env = dict(zip(unknown, combo))
        results = [(os_, arch, evaluate(expr, os_, arch, env)) for os_, arch in platforms]
        if len({r[2] for r in results}) > 1:
            return results
    return None


# --------------------------------------------------------------------------
# Case check
# --------------------------------------------------------------------------


def platforms_for(goos: str | None, goarch: str | None):
    """`SUPPORTED`, narrowed by whatever the case pinned in its `env` file."""
    if goos is None and goarch is None:
        return SUPPORTED
    matching = tuple(
        p
        for p in SUPPORTED
        if (goos is None or p[0] == goos) and (goarch is None or p[1] == goarch)
    )
    # A pinned pair outside the supported set (linux/386) is still pinned, and
    # pinned is the whole point: one platform cannot disagree with itself.
    return matching or ((goos or "linux", goarch or "amd64"),)


def check_tree(root: Path, platforms) -> list[str]:
    problems = []
    for path in sorted(root.rglob("*.go")):
        if path.name.startswith(("_", ".")):
            continue
        rel = path.relative_to(root)
        text = path.read_text(encoding="utf-8", errors="replace")
        try:
            expr = file_constraint(path, text)
            if expr is None:
                continue
            split = offending_platforms(expr, platforms)
        except ConstraintError as exc:
            problems.append(f"{rel}: {exc}")
            continue
        if split is not None:
            built = ", ".join(f"{o}/{a}" for o, a, v in split if v) or "nothing"
            skipped = ", ".join(f"{o}/{a}" for o, a, v in split if not v) or "nothing"
            problems.append(
                f"{rel}: build constraint {expr} is compiled on {built} and "
                f"skipped on {skipped}, so a golden recorded on one is missing "
                f"whatever this file reports on the other"
            )
    return problems


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--case", required=True, help="case name, for the message")
    ap.add_argument("--root", required=True, type=Path, help="materialized work dir")
    ap.add_argument("--goos", default=None, help="GOOS the case pins in its env file")
    ap.add_argument("--goarch", default=None, help="GOARCH the case pins in its env file")
    args = ap.parse_args(argv)

    problems = check_tree(args.root, platforms_for(args.goos, args.goarch))
    if not problems:
        return 0
    print(
        f"{args.case}: platform-dependent fixture(s) — the golden would differ "
        f"between a developer's machine and CI (compat/golden/platforms.py):",
        file=sys.stderr,
    )
    for p in problems:
        print(f"  {p}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
