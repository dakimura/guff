#!/usr/bin/env python3
"""Run a command under /usr/bin/time and parse wall clock + peak RSS.

Darwin: ``/usr/bin/time -lp``
Linux (GNU time): ``/usr/bin/time -v``

Optional live RSS watchdog (``--rss-limit-bytes``) polls the process group and
kills it if peak resident memory exceeds the limit — intended for ≤24GB hosts.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import signal
import subprocess
import sys
import threading
import time
from dataclasses import asdict, dataclass
from pathlib import Path


@dataclass
class Measurement:
    wall_seconds: float
    peak_rss_bytes: int
    exit_code: int
    killed_for_rss: bool = False


_DARWIN_RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size\s*$", re.M)
_LINUX_RSS_KB = re.compile(
    r"^\s*Maximum resident set size \(kbytes\):\s*(\d+)\s*$", re.M
)
_POSIX_REAL = re.compile(r"^real\s+(\d+(?:\.\d+)?)\s*$", re.M)
_GNU_ELAPSED = re.compile(
    r"Elapsed \(wall clock\) time \(h:mm:ss or m:ss\):\s*(?:(\d+):)?(\d+):(\d+(?:\.\d+)?)\s*$",
    re.M,
)


def parse_time_output(stderr: str, system: str | None = None) -> tuple[float | None, int | None]:
    """Return ``(wall_seconds, peak_rss_bytes)`` parsed from time(1) stderr."""
    system = system or platform.system()
    wall: float | None = None
    rss: int | None = None

    m = _POSIX_REAL.search(stderr)
    if m:
        wall = float(m.group(1))
    else:
        m = _GNU_ELAPSED.search(stderr)
        if m:
            hours = int(m.group(1) or 0)
            minutes = int(m.group(2))
            seconds = float(m.group(3))
            wall = hours * 3600 + minutes * 60 + seconds

    if system == "Darwin":
        m = _DARWIN_RSS.search(stderr)
        if m:
            rss = int(m.group(1))
    else:
        m = _LINUX_RSS_KB.search(stderr)
        if m:
            rss = int(m.group(1)) * 1024

    return wall, rss


def time_argv(system: str | None = None) -> list[str]:
    system = system or platform.system()
    if system == "Darwin":
        return ["/usr/bin/time", "-lp"]
    return ["/usr/bin/time", "-v"]


def _rss_bytes_for_pid(pid: int) -> int:
    """Best-effort current RSS for ``pid`` (0 if unavailable)."""
    system = platform.system()
    try:
        if system == "Darwin":
            out = subprocess.check_output(
                ["ps", "-o", "rss=", "-p", str(pid)],
                text=True,
                stderr=subprocess.DEVNULL,
            ).strip()
            # macOS ``ps`` RSS is in KiB.
            return int(out) * 1024 if out else 0
        out = subprocess.check_output(
            ["ps", "-o", "rss=", "-p", str(pid)],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
        return int(out) * 1024 if out else 0
    except (subprocess.CalledProcessError, ValueError, FileNotFoundError):
        return 0


def _descendant_pids(root_pid: int) -> list[int]:
    """Return ``root_pid`` plus descendants (best-effort, macOS/Linux)."""
    try:
        out = subprocess.check_output(["ps", "-ax", "-o", "pid=,ppid="], text=True)
    except (subprocess.CalledProcessError, FileNotFoundError):
        return [root_pid]
    children: dict[int, list[int]] = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) != 2:
            continue
        try:
            pid, ppid = int(parts[0]), int(parts[1])
        except ValueError:
            continue
        children.setdefault(ppid, []).append(pid)
    found = [root_pid]
    stack = [root_pid]
    while stack:
        cur = stack.pop()
        for child in children.get(cur, []):
            found.append(child)
            stack.append(child)
    return found


def _tree_rss_bytes(root_pid: int) -> int:
    return sum(_rss_bytes_for_pid(p) for p in _descendant_pids(root_pid))


def run_measured(
    cmd: list[str],
    *,
    cwd: str | Path | None = None,
    env: dict[str, str] | None = None,
    stdout_path: str | Path | None = None,
    stderr_path: str | Path | None = None,
    rss_limit_bytes: int | None = None,
    rss_poll_seconds: float = 0.5,
) -> Measurement:
    """Run ``cmd`` under time(1); write command stdout to ``stdout_path`` if set.

    If ``rss_limit_bytes`` is set, poll the process-tree RSS and SIGKILL the
    group when the live sum exceeds the limit.
    """
    time_cmd = time_argv()
    full = time_cmd + list(cmd)
    run_env = os.environ.copy()
    if env:
        run_env.update(env)

    killed = {"flag": False}
    t0 = time.perf_counter()

    with open(stdout_path or os.devnull, "wb") as out_fh:
        proc = subprocess.Popen(
            full,
            cwd=cwd,
            env=run_env,
            stdout=out_fh,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )

        def watchdog() -> None:
            if rss_limit_bytes is None:
                return
            while proc.poll() is None:
                rss = _tree_rss_bytes(proc.pid)
                if rss > rss_limit_bytes:
                    killed["flag"] = True
                    try:
                        os.killpg(proc.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    return
                time.sleep(rss_poll_seconds)

        watcher: threading.Thread | None = None
        if rss_limit_bytes is not None:
            watcher = threading.Thread(target=watchdog, daemon=True)
            watcher.start()

        time_stderr_bytes = b""
        try:
            _out, time_stderr_bytes = proc.communicate()
        finally:
            if watcher is not None:
                watcher.join(timeout=2.0)

    fallback_wall = time.perf_counter() - t0
    time_stderr = (time_stderr_bytes or b"").decode("utf-8", errors="replace")

    if stderr_path:
        Path(stderr_path).write_text(time_stderr, encoding="utf-8")

    wall, rss = parse_time_output(time_stderr)
    if wall is None:
        wall = fallback_wall
    if rss is None:
        if killed["flag"]:
            rss = int(rss_limit_bytes or 0)
        else:
            raise RuntimeError(
                "failed to parse peak RSS from time(1) output:\n" + time_stderr[-2000:]
            )

    exit_code = proc.returncode if proc.returncode is not None else 1
    if killed["flag"]:
        exit_code = 137

    return Measurement(
        wall_seconds=wall,
        peak_rss_bytes=rss,
        exit_code=exit_code,
        killed_for_rss=killed["flag"],
    )


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_parse = sub.add_parser("parse", help="Parse a saved time(1) stderr file")
    p_parse.add_argument("stderr_file")
    p_parse.add_argument("--system", default=None)

    p_run = sub.add_parser("run", help="Run a command under time(1)")
    p_run.add_argument("--cwd", default=None)
    p_run.add_argument("--stdout", default=None, help="Write child stdout here")
    p_run.add_argument("--stderr-out", default=None, help="Write time stderr here")
    p_run.add_argument("--json-out", default=None)
    p_run.add_argument(
        "--rss-limit-bytes",
        type=int,
        default=None,
        help="Kill process tree if live RSS sum exceeds this many bytes",
    )
    p_run.add_argument("command", nargs=argparse.REMAINDER)

    args = ap.parse_args(argv)

    if args.cmd == "parse":
        text = Path(args.stderr_file).read_text(encoding="utf-8", errors="replace")
        wall, rss = parse_time_output(text, system=args.system)
        print(json.dumps({"wall_seconds": wall, "peak_rss_bytes": rss}, indent=2))
        return 0 if wall is not None and rss is not None else 1

    if args.cmd == "run":
        command = list(args.command)
        if command and command[0] == "--":
            command = command[1:]
        if not command:
            ap.error("run requires a command after --")
        m = run_measured(
            command,
            cwd=args.cwd,
            stdout_path=args.stdout,
            stderr_path=args.stderr_out,
            rss_limit_bytes=args.rss_limit_bytes,
        )
        payload = asdict(m)
        if args.json_out:
            Path(args.json_out).write_text(
                json.dumps(payload, indent=2) + "\n", encoding="utf-8"
            )
        print(json.dumps(payload, indent=2))
        if m.killed_for_rss:
            print(
                f"error: killed: live RSS exceeded limit "
                f"({args.rss_limit_bytes:,} bytes)",
                file=sys.stderr,
            )
        return m.exit_code

    return 2


if __name__ == "__main__":
    raise SystemExit(main())
