#!/usr/bin/env python3
"""Byte-diff harness: reference Go formatters vs guff native (PERF_TASKS Task 1a).

Compares the system tool (gofmt / gofumpt / goimports / gci) against
``guff-fmt-native`` on every ``*.go`` in the chosen corpus. One differing
byte → FAIL with the file path. Exit 2 from the native binary means
"not implemented yet" and is reported separately (Task 1b–1e incomplete).

Examples::

    # Build the candidate once
    cargo build --release -p guff-fmt --bin guff-fmt-native

    # Identity smoke (reference vs reference) — always should PASS
    ./regress/fmt_diff.py --formatter gofmt --self-check --corpus prometheus --limit 50

    # Native vs reference (FAIL / NOT_IMPLEMENTED until Task 1b+)
    ./regress/fmt_diff.py --formatter gofmt --corpus both
    ./regress/fmt_diff.py --formatter gofumpt --extra --corpus prometheus
    ./regress/fmt_diff.py --formatter goimports \\
        --local github.com/prometheus/prometheus --corpus prometheus --limit 100
    ./regress/fmt_diff.py --formatter gci \\
        --section standard --section default \\
        --section 'prefix(github.com/prometheus/prometheus)' \\
        --corpus prometheus --limit 100
"""

from __future__ import annotations

import argparse
import concurrent.futures
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Sequence

REPO_ROOT = Path(__file__).resolve().parents[1]


def default_prometheus_dir() -> Path | None:
    env = os.environ.get("PROMETHEUS_DIR")
    if env:
        p = Path(env)
        return p if p.is_dir() else None
    link = REPO_ROOT / "prometheus"
    if link.is_dir():
        return link.resolve()
    return None


def default_goroot_src() -> Path | None:
    try:
        out = subprocess.check_output(["go", "env", "GOROOT"], text=True).strip()
    except (OSError, subprocess.CalledProcessError):
        return None
    src = Path(out) / "src"
    return src if src.is_dir() else None


def default_native_bin() -> Path:
    env = os.environ.get("GUFF_FMT_NATIVE")
    if env:
        return Path(env)
    release = REPO_ROOT / "target" / "release" / "guff-fmt-native"
    debug = REPO_ROOT / "target" / "debug" / "guff-fmt-native"
    if release.is_file():
        return release
    if debug.is_file():
        return debug
    return release  # preferred default for error messages


def collect_go_files(roots: Sequence[Path], *, limit: int | None) -> list[Path]:
    files: list[Path] = []
    for root in roots:
        for dirpath, dirnames, filenames in os.walk(root):
            # Skip module/vendor caches and testdata noise that formatters
            # often reject or that is intentionally malformed.
            dirnames[:] = [
                d
                for d in dirnames
                if d not in {".git", "vendor", "testdata", "node_modules"}
                and not d.startswith(".")
            ]
            for name in filenames:
                if not name.endswith(".go"):
                    continue
                # Skip known non-Go / broken fixtures under GOROOT.
                if name.endswith("_test.go") and "testdata" in Path(dirpath).parts:
                    continue
                files.append(Path(dirpath) / name)
    files.sort()
    if limit is not None and limit >= 0:
        files = files[:limit]
    return files


@dataclass
class RefArgs:
    formatter: str
    simplify: bool = False
    extra: bool = False
    lang: str | None = None
    modpath: str | None = None
    local: str | None = None
    sections: list[str] = field(default_factory=list)
    custom_order: bool = False
    no_lex_order: bool = False


def build_ref_cmd(ref: RefArgs, path: Path, staging: Path | None) -> list[str]:
    """Build argv for the reference tool. ``staging`` is a temp .go path for gci."""
    if ref.formatter == "gofmt":
        cmd = ["gofmt"]
        if ref.simplify:
            cmd.append("-s")
        return cmd
    if ref.formatter == "gofumpt":
        cmd = ["gofumpt"]
        if ref.extra:
            cmd.append("-extra")
        if ref.lang:
            cmd.extend(["-lang", ref.lang])
        if ref.modpath:
            cmd.extend(["-modpath", ref.modpath])
        return cmd
    if ref.formatter == "goimports":
        cmd = ["goimports"]
        if ref.local:
            cmd.extend(["-local", ref.local])
        cmd.extend(["-srcdir", str(path)])
        return cmd
    if ref.formatter == "gci":
        assert staging is not None
        cmd = ["gci", "print"]
        sections = ref.sections or ["standard", "default"]
        for s in sections:
            cmd.extend(["-s", s])
        if ref.custom_order:
            cmd.append("--custom-order")
        if ref.no_lex_order:
            cmd.append("--no-lex-order")
        cmd.append(str(staging))
        return cmd
    raise ValueError(f"unknown formatter {ref.formatter}")


def build_native_cmd(native_bin: Path, ref: RefArgs, path: Path) -> list[str]:
    cmd = [str(native_bin), ref.formatter]
    if ref.simplify:
        cmd.append("--simplify")
    if ref.extra:
        cmd.append("--extra")
    if ref.lang:
        cmd.extend(["--lang", ref.lang])
    if ref.modpath:
        cmd.extend(["--modpath", ref.modpath])
    if ref.local:
        cmd.extend(["--local", ref.local])
    for s in ref.sections:
        cmd.extend(["--section", s])
    if ref.custom_order:
        cmd.append("--custom-order")
    if ref.no_lex_order:
        cmd.append("--no-lex-order")
    cmd.extend(["--filename", str(path)])
    return cmd


@dataclass
class FileResult:
    path: Path
    status: str  # "ok" | "diff" | "ref_error" | "native_error" | "not_implemented" | "skip"
    detail: str = ""


def run_bytes(cmd: list[str], stdin: bytes | None) -> tuple[int, bytes, bytes]:
    proc = subprocess.run(
        cmd,
        input=stdin,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    return proc.returncode, proc.stdout, proc.stderr


def format_one(
    path: Path,
    *,
    ref: RefArgs,
    native_bin: Path | None,
    self_check: bool,
) -> FileResult:
    try:
        src = path.read_bytes()
    except OSError as e:
        return FileResult(path, "skip", f"read: {e}")

    # Reference
    staging: Path | None = None
    tmpdir: tempfile.TemporaryDirectory[str] | None = None
    try:
        if ref.formatter == "gci":
            # gci print needs a real file; stage next to the original so
            # localmodule can see go.mod when present.
            tmpdir = tempfile.TemporaryDirectory(dir=str(path.parent), prefix=".fmt_diff_")
            staging = Path(tmpdir.name) / path.name
            staging.write_bytes(src)
            ref_cmd = build_ref_cmd(ref, path, staging)
            rc, ref_out, ref_err = run_bytes(ref_cmd, None)
        else:
            ref_cmd = build_ref_cmd(ref, path, None)
            rc, ref_out, ref_err = run_bytes(ref_cmd, src)

        if rc != 0:
            # Malformed / build-constraint-only files under GOROOT are expected.
            msg = ref_err.decode("utf-8", "replace").strip() or f"exit {rc}"
            return FileResult(path, "ref_error", msg)

        if self_check:
            # Second reference pass must be idempotent.
            if ref.formatter == "gci":
                assert staging is not None
                staging.write_bytes(ref_out)
                rc2, out2, err2 = run_bytes(build_ref_cmd(ref, path, staging), None)
            else:
                rc2, out2, err2 = run_bytes(build_ref_cmd(ref, path, None), ref_out)
            if rc2 != 0:
                return FileResult(
                    path,
                    "ref_error",
                    f"idempotent pass failed: {err2.decode('utf-8', 'replace').strip()}",
                )
            if out2 != ref_out:
                return FileResult(path, "diff", "reference tool is not idempotent")
            return FileResult(path, "ok")

        assert native_bin is not None
        ncmd = build_native_cmd(native_bin, ref, path)
        nrc, nout, nerr = run_bytes(ncmd, src)
        if nrc == 2:
            return FileResult(
                path,
                "not_implemented",
                nerr.decode("utf-8", "replace").strip() or "exit 2",
            )
        if nrc != 0:
            return FileResult(
                path,
                "native_error",
                nerr.decode("utf-8", "replace").strip() or f"exit {nrc}",
            )
        if nout != ref_out:
            # Compact first-diff hint for triage.
            i = next((i for i, (a, b) in enumerate(zip(nout, ref_out)) if a != b), None)
            if i is None:
                detail = f"length native={len(nout)} ref={len(ref_out)}"
            else:
                detail = f"first byte diff at offset {i}"
            return FileResult(path, "diff", detail)
        return FileResult(path, "ok")
    finally:
        if tmpdir is not None:
            tmpdir.cleanup()


def resolve_corpus(name: str) -> list[Path]:
    roots: list[Path] = []
    if name in {"prometheus", "both"}:
        p = default_prometheus_dir()
        if p is None:
            raise SystemExit(
                "prometheus corpus not found (symlink repo-root/prometheus or set PROMETHEUS_DIR)"
            )
        roots.append(p)
    if name in {"goroot", "both"}:
        g = default_goroot_src()
        if g is None:
            raise SystemExit("GOROOT/src not found (is `go` on PATH?)")
        roots.append(g)
    if not roots:
        raise SystemExit(f"unknown corpus {name!r} (prometheus|goroot|both)")
    return roots


def main(argv: Sequence[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument(
        "--formatter",
        required=True,
        choices=["gofmt", "gofumpt", "goimports", "gci"],
    )
    ap.add_argument(
        "--corpus",
        default="prometheus",
        choices=["prometheus", "goroot", "both"],
        help="which *.go trees to walk (default: prometheus)",
    )
    ap.add_argument("--limit", type=int, default=None, help="max files (after sort)")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    ap.add_argument(
        "--self-check",
        action="store_true",
        help="diff reference tool against itself (idempotence smoke; no native needed)",
    )
    ap.add_argument("--simplify", action="store_true", help="gofmt -s")
    ap.add_argument("--extra", action="store_true", help="gofumpt -extra")
    ap.add_argument("--lang", default=None, help="gofumpt -lang")
    ap.add_argument("--modpath", default=None, help="gofumpt -modpath")
    ap.add_argument("--local", default=None, help="goimports -local")
    ap.add_argument(
        "--section",
        action="append",
        default=[],
        dest="sections",
        help="gci -s (repeatable)",
    )
    ap.add_argument("--custom-order", action="store_true")
    ap.add_argument("--no-lex-order", action="store_true")
    ap.add_argument(
        "--native-bin",
        type=Path,
        default=None,
        help="path to guff-fmt-native (default: target/release then target/debug)",
    )
    ap.add_argument(
        "--fail-fast",
        action="store_true",
        help="stop after the first diff / native_error",
    )
    ap.add_argument(
        "--allow-not-implemented",
        action="store_true",
        help="treat native exit 2 as soft skip (exit 0 if no diffs); default: exit 3",
    )
    args = ap.parse_args(argv)

    # Require the reference binary up front.
    if shutil.which(args.formatter if args.formatter != "gci" else "gci") is None:
        print(f"FAIL: reference binary {args.formatter!r} not on PATH", file=sys.stderr)
        return 1

    roots = resolve_corpus(args.corpus)
    files = collect_go_files(roots, limit=args.limit)
    if not files:
        print("FAIL: no .go files found", file=sys.stderr)
        return 1

    native_bin: Path | None = None
    if not args.self_check:
        native_bin = args.native_bin or default_native_bin()
        if not native_bin.is_file():
            print(
                f"FAIL: native binary not found at {native_bin}\n"
                f"  build with: cargo build --release -p guff-fmt --bin guff-fmt-native",
                file=sys.stderr,
            )
            return 1

    ref = RefArgs(
        formatter=args.formatter,
        simplify=args.simplify,
        extra=args.extra,
        lang=args.lang,
        modpath=args.modpath,
        local=args.local,
        sections=list(args.sections),
        custom_order=args.custom_order,
        no_lex_order=args.no_lex_order,
    )

    mode = "self-check" if args.self_check else f"native={native_bin}"
    print(
        f"fmt_diff: formatter={args.formatter} corpus={args.corpus} "
        f"files={len(files)} jobs={args.jobs} {mode}",
        flush=True,
    )

    counts = {
        "ok": 0,
        "diff": 0,
        "ref_error": 0,
        "native_error": 0,
        "not_implemented": 0,
        "skip": 0,
    }
    diffs: list[FileResult] = []
    native_errors: list[FileResult] = []
    not_impl_sample: str | None = None

    def _work(p: Path) -> FileResult:
        return format_one(p, ref=ref, native_bin=native_bin, self_check=args.self_check)

    stop = False
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        futs = {pool.submit(_work, p): p for p in files}
        for fut in concurrent.futures.as_completed(futs):
            if stop:
                fut.cancel()
                continue
            r = fut.result()
            counts[r.status] = counts.get(r.status, 0) + 1
            if r.status == "diff":
                diffs.append(r)
                rel = _rel(r.path)
                print(f"DIFF  {rel}  ({r.detail})", flush=True)
                if args.fail_fast:
                    stop = True
            elif r.status == "native_error":
                native_errors.append(r)
                rel = _rel(r.path)
                print(f"NATIVE_ERR  {rel}  ({r.detail})", flush=True)
                if args.fail_fast:
                    stop = True
            elif r.status == "not_implemented" and not_impl_sample is None:
                not_impl_sample = r.detail

    print(
        "summary: "
        + " ".join(f"{k}={v}" for k, v in counts.items() if v or k in {"ok", "diff"}),
        flush=True,
    )

    if diffs or native_errors:
        print(
            f"FAIL: {len(diffs)} byte-diff(s), {len(native_errors)} native error(s)",
            file=sys.stderr,
        )
        return 1

    if counts["not_implemented"] > 0:
        print(
            f"NOT_IMPLEMENTED: native {args.formatter} stub still active "
            f"({counts['not_implemented']} files). {not_impl_sample or ''}",
            file=sys.stderr,
        )
        if args.allow_not_implemented:
            print(
                "PASS (soft): no byte diffs; native not ready (--allow-not-implemented)",
                flush=True,
            )
            return 0
        return 3

    print("PASS: all compared files byte-identical", flush=True)
    return 0


def _rel(path: Path) -> str:
    for base in (REPO_ROOT, default_prometheus_dir(), default_goroot_src()):
        if base is None:
            continue
        try:
            return str(path.relative_to(base))
        except ValueError:
            continue
    return str(path)


if __name__ == "__main__":
    sys.exit(main())
