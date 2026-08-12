#!/usr/bin/env python3
"""Shrink a compat disagreement to a minimal reproducer (Phase 6).

`hunt.sh` and the OSS tier answer "do the two tools disagree, and where". They
do not answer "on what", and that gap is the expensive one: the 2026-08-12
session left `manager.Manager has no field or method GetCache` — sixteen
ill-typed packages — parked with the note *"no minimal repro yet, needs
compat/reduce.py first"*, because finding the shape by hand means reading a
700-file module.

This is delta debugging (Zeller's ddmin) over a Go module, with two things a
generic line reducer does not have:

**Syntax-aware edits.** Candidate edits come from `compat/gospans` (go/ast), so
one edit can be a whole declaration, one method of an interface, one field of a
struct, or a function body replaced by `panic(...)`. A line reducer stalls at
the first brace it cannot balance; these edits leave the file parseable by
construction, so nearly every test run is informative.

**A ground-truth invariant.** The reducer never accepts an edit that stops the
real Go toolchain from accepting the module. Without that guard, hunting an
ill-typed report converges on garbage in about four steps: the fastest way to
make guff say *"Manager has no field or method GetCache"* is to delete
`GetCache` from the interface, and a reducer with no notion of "this is still
valid Go" will happily do exactly that and report success. The invariant is
what makes the output a **guff bug** rather than a broken file:

    go build (or `go vet`, for test files) accepts it  AND  guff still misbehaves

Which is the same rule COMPAT-HARDENING §7 arrived at from the other side —
"a fixture never read by a real Go toolchain" is how three ill-typed forms sat
green in `testdata/gosec/bad.go` for months.

## Usage

    # An ill-typed package: guff rejects what `go build` accepts.
    compat/reduce.py \
        --dir corpus/cache/controller-runtime \
        --config corpus/cache/controller-runtime/.golangci.yml \
        --packages ./pkg/controller/priorityqueue/... \
        --guff-stderr 'does not implement' \
        -o /tmp/reduced

    # A finding golangci-lint makes and guff does not (needs golangci-lint).
    compat/reduce.py --dir … --config … --packages ./pkg/… --diff-key 'revive'

`go build` does not type-check `_test.go`, so when the behaviour lives in a test
file pass `--build-cmd 'go vet ./pkg/...'` instead. `go vet` type-checks tests
without linking (measured on controller-runtime: 0.6s warm, against 4.6s for
`go test -c` and 8.5s cold), at the cost of rejecting a candidate that trips one
of vet's own checks — conservative, never unsound.

Predicates (`--guff-stderr`, `--guff-finding`, `--diff-key`, `--any-diff`,
`--guff-fails`) are ANDed, and at least one is required: with none, every
candidate is interesting and the reducer deletes the module.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent / "golden"))
from golden import issue_key as golden_key  # noqa: E402  (compat/golden/golden.py)
from normalize import load_issues  # noqa: E402

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
GOSPANS_DIR = HERE / "gospans"

# `go build` names an unused import in one of two shapes depending on the
# toolchain; both have been seen in the versions this harness supports.
_UNUSED_IMPORT = re.compile(
    r'^(?P<file>[^:\s][^:]*\.go):\d+:\d+: (?:"(?P<a>[^"]+)" imported and not used'
    r'|imported and not used: "(?P<b>[^"]+)")'
)


# --------------------------------------------------------------------------
# The tree under reduction
# --------------------------------------------------------------------------


@dataclass
class Tree:
    """The Go files of the module, in memory.

    Only `.go` files are tracked. Everything else in the work copy (go.mod,
    go.sum, testdata, the config) is written once and never edited: reducing a
    go.sum is not a shape anyone needs, and reducing a go.mod changes what the
    build even resolves.
    """

    root: Path
    files: dict[str, bytes] = field(default_factory=dict)

    def clone(self) -> "Tree":
        return Tree(self.root, dict(self.files))

    def digest(self) -> str:
        h = hashlib.sha256()
        for rel in sorted(self.files):
            h.update(rel.encode())
            h.update(b"\0")
            h.update(self.files[rel])
            h.update(b"\0")
        return h.hexdigest()

    def size(self) -> tuple[int, int]:
        return len(self.files), sum(len(b) for b in self.files.values())


def collect_go_files(root: Path) -> dict[str, bytes]:
    out: dict[str, bytes] = {}
    for p in sorted(root.rglob("*.go")):
        if not p.is_file():
            continue
        rel = p.relative_to(root).as_posix()
        # `testdata` is excluded from the build by the go tool, and vendor
        # trees are somebody else's source: reducing either changes nothing
        # about what is compiled but costs a test run per file.
        parts = rel.split("/")
        if "testdata" in parts or "vendor" in parts:
            continue
        out[rel] = p.read_bytes()
    return out


# --------------------------------------------------------------------------
# Spans
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Edit:
    """One candidate change: `file[start:end] = replace`."""

    rel: str
    kind: str
    start: int
    end: int
    replace: str
    label: str

    def __repr__(self) -> str:  # keeps failure output readable
        return f"{self.rel}:{self.kind}[{self.start}:{self.end}]{self.label and ' ' + self.label}"


class Spanner:
    """Runs `compat/gospans` over the tree and caches the result per digest."""

    def __init__(self) -> None:
        self._bin: str | None = None

    def _ensure_built(self) -> str:
        """Compile gospans once. `go run` would rebuild it on every pass."""
        if self._bin:
            return self._bin
        out = Path(tempfile.mkdtemp(prefix="guff-gospans-")) / "gospans"
        r = subprocess.run(
            ["go", "build", "-o", str(out), "."],
            cwd=GOSPANS_DIR,
            capture_output=True,
            text=True,
        )
        if r.returncode != 0:
            raise SystemExit(f"compat/gospans does not build:\n{r.stderr}")
        self._bin = str(out)
        return self._bin

    def spans(self, tree: Tree, workdir: Path, command: str = "spans") -> list[Edit]:
        """Materialize the tree and ask gospans for its spans.

        `command` selects which question: "spans" for what can be deleted (the
        reducer) or "mutations" for what can be changed (`compat/fuzz.py`).
        Both answer in the same `Edit` shape, so `apply_edits` serves both.
        """
        binary = self._ensure_built()
        paths = [str(workdir / rel) for rel in sorted(tree.files)]
        if not paths:
            return []
        edits: list[Edit] = []
        # argv has a length limit and modules can carry hundreds of files.
        for batch in _chunks(paths, 200):
            r = subprocess.run(
                [binary, command, *batch],
                capture_output=True,
                text=True,
            )
            if r.returncode != 0:
                raise SystemExit(f"gospans failed:\n{r.stderr}")
            for entry in json.loads(r.stdout):
                rel = Path(entry["path"]).relative_to(workdir).as_posix()
                if entry.get("error"):
                    # A file that no longer parses cannot be reduced further by
                    # spans; it is still a legal member of the tree (the build
                    # invariant will have rejected it if it mattered).
                    continue
                for s in entry.get("spans") or []:
                    edits.append(
                        Edit(
                            rel=rel,
                            kind=s["kind"],
                            start=s["start"],
                            end=s["end"],
                            replace=s.get("replace", ""),
                            label=s.get("label", ""),
                        )
                    )
        return edits


def _chunks(seq: list, n: int):
    for i in range(0, len(seq), n):
        yield seq[i : i + n]


def apply_edits(tree: Tree, edits: list[Edit]) -> Tree:
    """Apply a set of edits, dropping any that a wider edit already covers.

    Spans nest — a statement lives inside the declaration that lives inside the
    file — so an arbitrary subset chosen by ddmin routinely contains both an
    outer span and spans within it. Taking the outer one and skipping the rest
    is well defined and keeps the result a pure function of the subset, which
    is what the cache and ddmin's bookkeeping both rely on.
    """
    out = tree.clone()
    by_file: dict[str, list[Edit]] = {}
    for e in edits:
        by_file.setdefault(e.rel, []).append(e)
    for rel, es in by_file.items():
        src = out.files.get(rel)
        if src is None:
            continue
        # Widest-first at each start offset, so the outer span wins.
        es.sort(key=lambda e: (e.start, -e.end))
        pieces: list[bytes] = []
        cursor = 0
        for e in es:
            if e.start < cursor:
                continue  # covered by an edit already taken
            if e.end > len(src):
                continue
            pieces.append(src[cursor : e.start])
            if e.replace:
                pieces.append(e.replace.encode())
            cursor = e.end
        pieces.append(src[cursor:])
        out.files[rel] = b"".join(pieces)
    return out


# --------------------------------------------------------------------------
# The interestingness oracle
# --------------------------------------------------------------------------


@dataclass
class Predicate:
    guff_stderr: re.Pattern | None = None
    guff_finding: re.Pattern | None = None
    diff_key: re.Pattern | None = None
    any_diff: bool = False
    guff_fails: bool = False

    def needs_golangci(self) -> bool:
        return self.any_diff or self.diff_key is not None

    def describe(self) -> str:
        bits = []
        if self.guff_stderr:
            bits.append(f"guff stderr ~ /{self.guff_stderr.pattern}/")
        if self.guff_finding:
            bits.append(f"guff finding ~ /{self.guff_finding.pattern}/")
        if self.diff_key:
            bits.append(f"diff key ~ /{self.diff_key.pattern}/")
        if self.any_diff:
            bits.append("any diff")
        if self.guff_fails:
            bits.append("guff exits non-zero")
        return " AND ".join(bits) or "(none)"


class Oracle:
    """Materializes a candidate tree and decides whether it is interesting.

    Every candidate goes through the same two gates, in this order:

    1. **Invariant** — the real Go toolchain still accepts the module. Checked
       first because it is the cheap one and it rejects most bad candidates.
    2. **Predicate** — guff (and, when asked, golangci-lint) still misbehaves.
    """

    def __init__(
        self,
        workdir: Path,
        config: Path,
        packages: str,
        predicate: Predicate,
        guff_bin: str,
        golangci_bin: str | None,
        build_cmd: str,
        timeout: str,
    ) -> None:
        self.workdir = workdir
        self.config = config
        self.packages = packages
        self.predicate = predicate
        self.guff = guff_bin
        self.golangci = golangci_bin
        self.build_cmd = build_cmd
        self.build_cmd_is_default = build_cmd == f"go build {packages}"
        self.timeout = timeout
        self.on_disk: dict[str, bytes] = {}
        self.cache: dict[str, tuple[bool, dict[str, bytes] | None]] = {}
        self.runs = 0
        self.build_runs = 0
        self.spent = 0.0

    # -- materialization ---------------------------------------------------

    def sync(self, files: dict[str, bytes]) -> None:
        """Write only what changed. Most candidates differ in one file."""
        for rel in list(self.on_disk):
            if rel not in files:
                (self.workdir / rel).unlink(missing_ok=True)
                del self.on_disk[rel]
        for rel, data in files.items():
            if self.on_disk.get(rel) == data:
                continue
            p = self.workdir / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
            self.on_disk[rel] = data

    # -- the two gates -----------------------------------------------------

    def build(self) -> tuple[bool, str]:
        self.build_runs += 1
        r = subprocess.run(
            self.build_cmd,
            shell=True,
            cwd=self.workdir,
            capture_output=True,
            text=True,
        )
        return r.returncode == 0, r.stderr + r.stdout

    def fix_unused_imports(self, files: dict[str, bytes], stderr: str) -> dict[str, bytes] | None:
        """Delete the imports the compiler just said are unused.

        Deleting a declaration usually orphans an import, and an orphaned import
        is a compile error, so without this almost every useful edit would be
        rejected by the invariant. Which imports are dead is decided by the
        compiler rather than here on purpose: answering it locally means
        resolving each path to its package name, which needs the build list.
        """
        dead: dict[str, set[str]] = {}
        saw_other = False
        for line in stderr.splitlines():
            line = line.strip()
            if not line or line.startswith(("#", "go: ")):
                continue
            m = _UNUSED_IMPORT.match(line.lstrip("./"))
            if m:
                path = m.group("a") or m.group("b")
                rel = m.group("file")
                dead.setdefault(rel, set()).add(path)
            elif re.match(r"^[^:\s][^:]*\.go:\d+:\d+:", line):
                saw_other = True
        if not dead or saw_other:
            # Mixed errors mean the candidate is broken for a real reason;
            # stripping imports would only mask it.
            return None
        edits: list[Edit] = []
        spanner_tree = Tree(self.workdir, files)
        self.sync(files)
        for e in _SPANNER.spans(spanner_tree, self.workdir):
            if e.kind != "import":
                continue
            for rel, paths in dead.items():
                if e.rel.endswith(rel) and e.label in paths:
                    edits.append(e)
        if not edits:
            return None
        return apply_edits(Tree(self.workdir, files), edits).files

    def run_guff(self) -> tuple[int, str, str]:
        env = dict(os.environ)
        env["GUFF_DEBUG_ILL_TYPED"] = "1"
        cache = tempfile.mkdtemp(prefix="guff-reduce-")
        env["GUFF_CACHE"] = cache
        try:
            r = subprocess.run(
                [
                    self.guff, "run", "-c", str(self.config),
                    "--out-format", "json", "--issues-exit-code", "0",
                    "--no-cache", "--timeout", self.timeout,
                    *self.packages.split(),
                ],
                cwd=self.workdir,
                capture_output=True,
                text=True,
                env=env,
            )
        finally:
            shutil.rmtree(cache, ignore_errors=True)
        return r.returncode, r.stdout, r.stderr

    def run_golangci(self) -> tuple[int, str, str]:
        env = dict(os.environ)
        cache = tempfile.mkdtemp(prefix="gcl-reduce-")
        env["GOLANGCI_LINT_CACHE"] = cache
        try:
            r = subprocess.run(
                [
                    self.golangci, "run", "-c", str(self.config),
                    "--output.json.path=stdout", "--path-mode", "abs",
                    "--issues-exit-code", "0", f"--timeout={self.timeout}",
                    "--max-issues-per-linter=0", "--max-same-issues=0",
                    "--allow-parallel-runners",
                    *self.packages.split(),
                ],
                cwd=self.workdir,
                capture_output=True,
                text=True,
                env=env,
            )
        finally:
            shutil.rmtree(cache, ignore_errors=True)
        return r.returncode, r.stdout, r.stderr

    def _keys(self, raw: str) -> list[str]:
        tmp = self.workdir / ".reduce-issues.json"
        tmp.write_text(raw, encoding="utf-8")
        try:
            issues = load_issues(tmp)
        except Exception:
            return []
        finally:
            tmp.unlink(missing_ok=True)
        return [golden_key(i, str(self.workdir)) for i in issues]

    def interesting(self, files: dict[str, bytes]) -> tuple[bool, dict[str, bytes] | None]:
        """Return (interesting, files) — files may have had dead imports removed."""
        key = hashlib.sha256(
            self.packages.encode()
            + b"\2"
            + b"\0".join(f"{r}".encode() + b"\1" + files[r] for r in sorted(files))
        ).hexdigest()
        if key in self.cache:
            return self.cache[key]
        t0 = time.time()
        result = self._interesting_uncached(files)
        self.spent += time.time() - t0
        self.runs += 1
        self.cache[key] = result
        return result

    def _interesting_uncached(
        self, files: dict[str, bytes]
    ) -> tuple[bool, dict[str, bytes] | None]:
        self.sync(files)
        ok, err = self.build()
        if not ok:
            fixed = self.fix_unused_imports(files, err)
            if fixed is None:
                return (False, None)
            files = fixed
            self.sync(files)
            ok, _ = self.build()
            if not ok:
                return (False, None)

        code, out, errtxt = self.run_guff()
        p = self.predicate
        if p.guff_fails and code == 0:
            return (False, None)
        if p.guff_stderr and not p.guff_stderr.search(errtxt):
            return (False, None)
        guff_keys = None
        if p.guff_finding is not None:
            guff_keys = self._keys(out)
            if not any(p.guff_finding.search(k) for k in guff_keys):
                return (False, None)
        if p.needs_golangci():
            if guff_keys is None:
                guff_keys = self._keys(out)
            _, gout, _ = self.run_golangci()
            gcl_keys = self._keys(gout)
            missing = sorted(set(gcl_keys) - set(guff_keys))
            extra = sorted(set(guff_keys) - set(gcl_keys))
            if p.any_diff and not (missing or extra):
                return (False, None)
            if p.diff_key is not None and not any(
                p.diff_key.search(k) for k in missing + extra
            ):
                return (False, None)
        return (True, files)


_SPANNER = Spanner()


# --------------------------------------------------------------------------
# ddmin
# --------------------------------------------------------------------------


def ddmin(units: list, test, log=lambda *_: None) -> list:
    """Zeller's ddmin over the units to **keep**.

    `test(kept)` must be true for the full list. Returns a 1-minimal sublist:
    one where removing any single remaining chunk at the finest granularity
    makes it false.
    """
    kept = list(units)
    n = 2
    while len(kept) >= 2:
        chunk = max(1, len(kept) // n)
        chunks = [kept[i : i + chunk] for i in range(0, len(kept), chunk)]
        progressed = False
        for i in range(len(chunks)):
            candidate = [u for j, c in enumerate(chunks) if j != i for u in c]
            if not candidate:
                continue
            if test(candidate):
                removed = len(kept) - len(candidate)
                kept = candidate
                n = max(n - 1, 2)
                progressed = True
                log(f"      -{removed} (kept {len(kept)})")
                break
        if progressed:
            continue
        if n >= len(kept):
            break
        n = min(2 * n, len(kept))
    return kept


# --------------------------------------------------------------------------
# Passes
# --------------------------------------------------------------------------

# Order matters: the coarsest edits are tried first because one accepted edit
# at the top removes hundreds of candidates below it. Running `stmt` before
# `decl` on controller-runtime means testing ~40,000 statements that a later
# whole-file deletion would have taken in one step.
PASS_KINDS: list[tuple[str, tuple[str, ...]]] = [
    ("decls", ("decl", "importdecl")),
    ("bodies", ("body",)),
    ("members", ("spec", "imethod", "field", "elt")),
    ("stmts", ("stmt",)),
    ("imports", ("import",)),
]


class Reducer:
    def __init__(
        self, oracle: Oracle, verbose: bool = False, reduce_root_set: bool = True
    ) -> None:
        self.oracle = oracle
        self.verbose = verbose
        self.reduce_root_set = reduce_root_set

    def log(self, msg: str) -> None:
        print(msg, flush=True)

    def prune_to_closure(self, tree: Tree) -> Tree:
        """Drop every file outside the target packages' dependency closure.

        One oracle call for what ddmin over files would need hundreds of runs to
        discover: controller-runtime is 359 files, of which the closure of
        `./pkg/controller/priorityqueue/...` is nine. `-test` is passed because
        a target can be a `_test` package, and `-e` so that a module with a
        package that does not load still lists the rest.

        It is a candidate like any other — if the oracle says the pruned tree is
        no longer interesting (the disagreement needed a file `go list` does not
        consider a dependency), the full tree is kept and ddmin proceeds.
        """
        r = subprocess.run(
            ["go", "list", "-e", "-deps", "-test", "-f", "{{.Dir}}", *self.oracle.packages.split()],
            cwd=self.oracle.workdir,
            capture_output=True,
            text=True,
        )
        if r.returncode != 0:
            return tree
        root = self.oracle.workdir.resolve()
        dirs: set[str] = set()
        for line in r.stdout.splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                rel = Path(line).resolve().relative_to(root).as_posix()
            except ValueError:
                continue  # outside the module: the module cache
            dirs.add(rel)
        if not dirs:
            return tree
        kept = {
            rel: data
            for rel, data in tree.files.items()
            if (Path(rel).parent.as_posix() if "/" in rel else ".") in dirs
        }
        if len(kept) == len(tree.files):
            return tree
        ok, fixed = self.oracle.interesting(kept)
        if not ok:
            self.log(f"    closure prune rejected ({len(kept)} of {len(tree.files)} files)")
            return tree
        return Tree(tree.root, fixed if fixed is not None else kept)

    def reduce_roots(self, tree: Tree) -> None:
        """Shrink the **analysed package set**, before touching a single file.

        Some misbehaviour is not a property of any file: it is a property of
        which packages were asked for. `manager.Manager has no field or method
        GetCache` reproduced under `./pkg/...` and not under
        `./pkg/metrics/filters/...` — same bytes, same config, different answer —
        because a package that is a *root* is loaded differently from the same
        package as a *dependency*. A file-level reducer cannot express that, and
        two and a half hours of one had got 349 files down to 155 without ever
        naming the cause.

        ddmin over the package list found it in minutes: 64 roots to 3. The point
        is not that this pass is fast (it is one oracle call per candidate, like
        any other) but that it shrinks the *oracle* — every later pass runs
        against three packages instead of sixty-four — and that a three-package
        answer is small enough to reason about directly.

        Ask what the reproduction is a function of before assuming it is the
        source.
        """
        r = subprocess.run(
            ["go", "list", "-e", *self.oracle.packages.split()],
            cwd=self.oracle.workdir,
            capture_output=True,
            text=True,
        )
        roots = [ln.strip() for ln in r.stdout.splitlines() if ln.strip()]
        if r.returncode != 0 or len(roots) < 2:
            return
        # `go list` prints import paths; the tools take patterns. A path is a
        # valid pattern for exactly the package it names, which is what we want.
        original_packages = self.oracle.packages
        original_build = self.oracle.build_cmd

        def test(kept: list[str]) -> bool:
            self.oracle.packages = " ".join(kept)
            if self.oracle.build_cmd_is_default:
                self.oracle.build_cmd = f"go build {self.oracle.packages}"
            ok, _ = self.oracle.interesting(tree.files)
            return ok

        if not test(roots):
            # The expansion is not equivalent to the pattern (a `_test` package,
            # a pattern `go list` resolves differently). Leave the roots alone.
            self.oracle.packages = original_packages
            self.oracle.build_cmd = original_build
            return
        kept = ddmin(roots, test, self.log if self.verbose else (lambda *_: None))
        test(kept)
        self.log(f"    -> {len(kept)} of {len(roots)} package(s)")
        for pkg in kept:
            self.log(f"       {pkg}")

    def reduce_files(self, tree: Tree) -> Tree:
        """Delete whole files. The single biggest win on a real module."""
        rels = sorted(tree.files)
        if len(rels) < 2:
            return tree

        def test(kept: list[str]) -> bool:
            ok, _ = self.oracle.interesting({r: tree.files[r] for r in kept})
            return ok

        kept = ddmin(rels, test, self.log if self.verbose else (lambda *_: None))
        final = {r: tree.files[r] for r in kept}
        ok, fixed = self.oracle.interesting(final)
        if not ok:
            return tree
        return Tree(tree.root, fixed if fixed is not None else final)

    def reduce_spans(self, tree: Tree, kinds: tuple[str, ...]) -> Tree:
        """ddmin over the spans of the given kinds, pooled across all files."""
        self.oracle.sync(tree.files)
        all_edits = [e for e in _SPANNER.spans(tree, self.oracle.workdir) if e.kind in kinds]
        if not all_edits:
            return tree
        # ddmin keeps units; a "unit" here is a span we do NOT remove.
        # Removing every span at once is the maximal edit, so the full kept set
        # is "remove everything" — which is exactly the input ddmin needs, with
        # the roles inverted: keep-set = spans still applied as removals.
        units = list(range(len(all_edits)))

        def test(kept: list[int]) -> bool:
            cand = apply_edits(tree, [all_edits[i] for i in kept])
            ok, _ = self.oracle.interesting(cand.files)
            return ok

        # Is removing everything of this kind interesting? Usually not, but when
        # it is we are done in one run.
        if test(units):
            best = units
        else:
            best = ddmin_grow(units, test, self.log if self.verbose else (lambda *_: None))
        if not best:
            return tree
        cand = apply_edits(tree, [all_edits[i] for i in best])
        ok, fixed = self.oracle.interesting(cand.files)
        if not ok:
            return tree
        return Tree(tree.root, fixed if fixed is not None else cand.files)

    def run(self, tree: Tree, max_rounds: int = 6) -> Tree:
        for rnd in range(1, max_rounds + 1):
            before = tree.digest()
            nf, nb = tree.size()
            self.log(f"\n=== round {rnd} — {nf} files, {nb} bytes ===")

            if rnd == 1:
                if self.reduce_root_set:
                    self.log("  [roots]")
                    self.reduce_roots(tree)
                self.log("  [closure]")
                tree = self.prune_to_closure(tree)
                self.log(f"    -> {tree.size()[0]} files, {tree.size()[1]} bytes")

            self.log("  [files]")
            tree = self.reduce_files(tree)
            self.log(f"    -> {tree.size()[0]} files, {tree.size()[1]} bytes")

            for name, kinds in PASS_KINDS:
                self.log(f"  [{name}]")
                tree = self.reduce_spans(tree, kinds)
                self.log(f"    -> {tree.size()[0]} files, {tree.size()[1]} bytes")

            if tree.digest() == before:
                self.log(f"\nfixpoint after round {rnd}")
                break
        return tree


def ddmin_grow(units: list[int], test, log) -> list[int]:
    """Find a large subset of *removals* that keeps the candidate interesting.

    ddmin proper minimizes a kept set. Here the units are removals and we want
    the **largest** applicable set, so the search runs the other way: start from
    nothing removed and try chunks from coarse to fine, keeping every chunk
    whose addition survives the oracle and setting the rest aside for the next,
    finer granularity. This is the greedy shape creduce uses per pass; it costs
    about 2n tests and gets the common case — most removals independent — in
    the first few coarse rounds.
    """
    applied: list[int] = []
    remaining = list(units)
    chunk = max(1, len(remaining) // 2)
    while remaining:
        pieces = [remaining[i : i + chunk] for i in range(0, len(remaining), chunk)]
        still: list[int] = []
        for piece in pieces:
            candidate = sorted(applied + piece)
            if test(candidate):
                applied = candidate
                log(f"      +{len(piece)} removals (total {len(applied)})")
            else:
                still.extend(piece)
        remaining = still
        if chunk == 1:
            break
        chunk = max(1, chunk // 2)
    return applied


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def resolve_guff() -> str:
    if os.environ.get("GUFF_BIN"):
        return os.environ["GUFF_BIN"]
    p = ROOT / "target" / "release" / "guff"
    if p.is_file():
        return str(p)
    found = shutil.which("guff")
    if found:
        return found
    raise SystemExit("guff not found; cargo build --release -p guff-lint")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--dir", required=True, help="Module to reduce (copied, never edited)")
    ap.add_argument("--config", required=True, help="golangci-lint v2 config both tools use")
    ap.add_argument("--packages", default="./...", help="Package pattern passed to both tools")
    ap.add_argument("-o", "--output", help="Where to write the reduced module")
    ap.add_argument("--workdir", help="Scratch copy (default: a temp dir)")
    ap.add_argument("--build-cmd", help="Invariant command (default: 'go build <packages>')")
    ap.add_argument("--no-build", action="store_true", help="Drop the toolchain invariant (unsafe)")
    ap.add_argument("--timeout", default="5m")
    ap.add_argument("--rounds", type=int, default=6)
    ap.add_argument(
        "--no-reduce-roots",
        action="store_true",
        help="Skip the root-set pass (see Reducer.reduce_roots)",
    )
    ap.add_argument("-v", "--verbose", action="store_true")

    g = ap.add_argument_group("predicate (ANDed; at least one required)")
    g.add_argument("--guff-stderr", help="Regex guff's stderr must keep matching")
    g.add_argument("--guff-finding", help="Regex a guff finding key must keep matching")
    g.add_argument("--diff-key", help="Regex a guff/golangci diff key must keep matching")
    g.add_argument("--any-diff", action="store_true", help="Any guff/golangci diff at all")
    g.add_argument("--guff-fails", action="store_true", help="guff must keep exiting non-zero")

    args = ap.parse_args(argv)

    pred = Predicate(
        guff_stderr=re.compile(args.guff_stderr) if args.guff_stderr else None,
        guff_finding=re.compile(args.guff_finding) if args.guff_finding else None,
        diff_key=re.compile(args.diff_key) if args.diff_key else None,
        any_diff=args.any_diff,
        guff_fails=args.guff_fails,
    )
    if not any(
        [args.guff_stderr, args.guff_finding, args.diff_key, args.any_diff, args.guff_fails]
    ):
        ap.error("at least one predicate is required, or the reducer deletes the module")

    src = Path(args.dir).resolve()
    if not src.is_dir():
        raise SystemExit(f"not a directory: {src}")
    config = Path(args.config).resolve()
    if not config.is_file():
        raise SystemExit(f"not a file: {config}")

    golangci = os.environ.get("GOLANGCI_LINT_BIN") or shutil.which("golangci-lint")
    if pred.needs_golangci() and not golangci:
        raise SystemExit("--diff-key/--any-diff need golangci-lint on PATH")

    workdir = Path(args.workdir).resolve() if args.workdir else Path(
        tempfile.mkdtemp(prefix="guff-reduce-work-")
    )
    if workdir.exists() and args.workdir:
        shutil.rmtree(workdir)
    print(f"copying {src} -> {workdir}")
    shutil.copytree(src, workdir, dirs_exist_ok=True, symlinks=True)

    build_cmd = args.build_cmd or f"go build {args.packages}"
    if args.no_build:
        build_cmd = "true"

    tree = Tree(workdir, collect_go_files(workdir))
    nf, nb = tree.size()

    oracle = Oracle(
        workdir=workdir,
        config=config,
        packages=args.packages,
        predicate=pred,
        guff_bin=resolve_guff(),
        golangci_bin=golangci,
        build_cmd=build_cmd,
        timeout=args.timeout,
    )

    print(f"predicate: {pred.describe()}")
    print(f"invariant: {build_cmd}")
    print(f"start:     {nf} files, {nb} bytes")

    ok, fixed = oracle.interesting(tree.files)
    if not ok:
        print(
            "\nERROR: the starting tree is not interesting.\n"
            "  Either the predicate does not match, or the invariant command fails\n"
            "  on the unmodified module. Re-run the two by hand before reducing.",
            file=sys.stderr,
        )
        return 1
    if fixed is not None:
        tree = Tree(workdir, fixed)

    t0 = time.time()
    tree = Reducer(
        oracle, verbose=args.verbose, reduce_root_set=not args.no_reduce_roots
    ).run(tree, max_rounds=args.rounds)
    elapsed = time.time() - t0

    oracle.sync(tree.files)
    nf2, nb2 = tree.size()
    print(
        f"\nreduced: {nf} -> {nf2} files, {nb} -> {nb2} bytes "
        f"({100 * (1 - nb2 / max(nb, 1)):.1f}% smaller)"
    )
    print(f"  oracle runs: {oracle.runs} ({oracle.build_runs} builds) in {elapsed:.0f}s")

    if args.output:
        out = Path(args.output).resolve()
        if out.exists():
            shutil.rmtree(out)
        out.mkdir(parents=True)
        for extra in ("go.mod", "go.sum"):
            if (workdir / extra).is_file():
                shutil.copy2(workdir / extra, out / extra)
        if config.is_relative_to(src):
            shutil.copy2(config, out / config.name)
        for rel, data in sorted(tree.files.items()):
            p = out / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
        print(f"  wrote {out}")
        for rel in sorted(tree.files):
            print(f"    {rel} ({len(tree.files[rel])} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
