#!/usr/bin/env python3
"""compat/coverage.py — check-level coverage ledger (COMPAT-HARDENING Phase 0).

Answers one question: **which of guff's checks has never fired in any test we
run?** A check that never fires is not "passing" — it is untested, and a recall
bug in it is invisible to every gate we have.

    ./compat/coverage.py inventory          # guff source -> coverage/inventory.json
    ./compat/coverage.py observe            # scan run artifacts -> coverage/observed.json
    ./compat/coverage.py report             # join -> docs/COVERAGE.md

`inventory` enumerates the checks guff *implements*, at the granularity a user
sees in a message: staticcheck check codes, gocritic checkers, revive rules,
gosec rule IDs, govet passes, and one entry per single-check linter.

`observe` mines `*.guff.json` / `*.golangci.json` produced by `compat/run.sh`
and `regress/run.sh` (they are kept under `compat/results/<stamp>/` and
`regress/results/<stamp>/`), plus a static scan of the Rust unit tests. Each
check gets a set of *sources* that have ever exercised it:

    unit      a check ID appears literally in a crates/*/tests/*.rs assertion
    isolate   fired in a compat isolate target (one linter enabled)
    oss       fired in a compat fixture/local/OSS target
    regress   fired in the prometheus regression harness

`unit` is a static scan, so it is a lower bound: it proves a test mentions the
ID, not that the assertion is meaningful. Treat it as the weakest source.
"""

from __future__ import annotations

import argparse
import collections
import glob
import json
import os
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
COVDIR = os.path.join(ROOT, "compat", "coverage")
INVENTORY = os.path.join(COVDIR, "inventory.json")
OBSERVED = os.path.join(COVDIR, "observed.json")
REPORT_MD = os.path.join(ROOT, "docs", "COVERAGE.md")

# Linters whose findings carry a sub-check ID in the message text. Everything
# else contributes exactly one check: the linter itself.
MULTI_CHECK = ("staticcheck", "gocritic", "revive", "gosec", "govet")

# `guff run` reports formatter diffs as findings too, under the formatter's own
# name, so they belong in the ledger. Source: COMPATIBILITY.md §1.1.
FORMATTERS = ("gci", "gofmt", "gofumpt", "goimports", "golines", "swaggo")


# --------------------------------------------------------------------------
# inventory
# --------------------------------------------------------------------------


def _rust_str_array(path: str, const: str) -> list[str]:
    """Extract the string literals of `const <const>: &[&str] = &[ ... ];`."""
    src = open(path, encoding="utf-8").read()
    m = re.search(rf"const {re.escape(const)}\s*:\s*&\[&str\]\s*=\s*&\[(.*?)\n\];", src, re.S)
    if not m:
        raise SystemExit(f"coverage: cannot find `{const}` in {path} (registry shape changed?)")
    return re.findall(r'"([^"]+)"', m.group(1))


def _guff_linters() -> list[str]:
    """Linter names straight from the binary, so the list cannot drift."""
    binary = os.path.join(ROOT, "target", "release", "guff")
    if not os.path.exists(binary):
        raise SystemExit("coverage: build first — cargo build --release -p guff-lint")
    with tempfile.TemporaryDirectory() as tmp:
        open(os.path.join(tmp, "go.mod"), "w").write("module example.com/p\n\ngo 1.24\n")
        open(os.path.join(tmp, "a.go"), "w").write("package p\n")
        out = subprocess.run(
            [binary, "linters"], cwd=tmp, capture_output=True, text=True
        ).stdout
    names = re.findall(r"^([a-z][a-z0-9_]*): ", out, re.M)
    if len(names) < 100:
        raise SystemExit(f"coverage: `guff linters` yielded only {len(names)} names")
    return sorted(set(names))


def build_inventory() -> dict:
    checks: dict[str, dict] = {}

    def add(check_id: str, linter: str, source: str) -> None:
        checks[check_id] = {"id": check_id, "linter": linter, "declared_in": source}

    # --- staticcheck: one module per check code (s1000.rs -> S1000) ---------
    sc_dir = os.path.join(ROOT, "crates", "guff-staticcheck", "src")
    for fn in sorted(os.listdir(sc_dir)):
        m = re.fullmatch(r"(s|sa|st|qf)(\d+)\.rs", fn)
        if m:
            add(f"{m.group(1).upper()}{m.group(2)}", "staticcheck", f"guff-staticcheck/src/{fn}")

    # --- gocritic: the two enable-all const arrays --------------------------
    gc = os.path.join(ROOT, "crates", "guff-style", "src", "gocritic.rs")
    for name in _rust_str_array(gc, "DEFAULT_CHECKS") + _rust_str_array(gc, "ENABLE_ALL_EXTRA_CHECKS"):
        add(f"gocritic/{name}", "gocritic", "guff-style/src/gocritic.rs")

    # --- revive: one module per rule (blank_imports.rs -> blank-imports) ----
    # Some modules under rules/ are shared infrastructure (shared_walk.rs), not
    # rules. A real rule registers its kebab-case name as a string literal, so
    # require that rather than trusting the filename alone. The literal may live
    # in a shared driver (ifelse.rs registers indent-error-flow), so search the
    # whole crate.
    rv_dir = os.path.join(ROOT, "crates", "guff-revive", "src", "rules")
    rv_src = "\n".join(
        open(p, encoding="utf-8", errors="ignore").read()
        for p in glob.glob(os.path.join(ROOT, "crates", "guff-revive", "src", "**", "*.rs"), recursive=True)
    )
    for fn in sorted(os.listdir(rv_dir)):
        if not fn.endswith(".rs") or fn == "mod.rs":
            continue
        rule = fn[:-3].replace("_", "-")
        if f'"{rule}"' in rv_src:
            add(f"revive/{rule}", "revive", f"guff-revive/src/rules/{fn}")

    # --- gosec: G-codes appearing as message prefixes -----------------------
    # Cut the `#[cfg(test)]` module first: its fixtures invent codes (`G999`).
    gs = os.path.join(ROOT, "crates", "guff-style", "src", "gosec.rs")
    gs_src = open(gs, encoding="utf-8").read().split("#[cfg(test)]")[0]
    for code in sorted(set(re.findall(r'"(G\d{3})[:"]', gs_src))):
        add(code, "gosec", "guff-style/src/gosec.rs")

    # --- govet: the analyzers() vec -----------------------------------------
    gv = open(os.path.join(ROOT, "crates", "guff-govet", "src", "lib.rs"), encoding="utf-8").read()
    m = re.search(r"pub fn analyzers\(\)[^{]*\{\s*vec!\[(.*?)\]\s*\}", gv, re.S)
    if not m:
        raise SystemExit("coverage: cannot parse guff-govet analyzers()")
    # The module name is not always the analyzer name (`testpass` declares the
    # `tests` analyzer), and observation recovers the id from the message
    # prefix, which is the *analyzer* name. Taking the module name here made
    # `govet/testpass` unobservable by construction — a permanent phantom in
    # the `never` column. Read the declared name out of each module instead.
    for pass_name in re.findall(r"(\w+)::analyzer\(\)", m.group(1)):
        src = os.path.join(ROOT, "crates", "guff-govet", "src", f"{pass_name}.rs")
        name = pass_name
        try:
            decl = open(src, encoding="utf-8").read()
        except OSError:
            decl = ""
        # Anchor on the `Analyzer { ... }` literal: other structs in these
        # modules have a `name` field too (bools' `BoolOp` is called "or").
        nm = re.search(r'Analyzer\s*\{[^}]*?\bname:\s*"([\w.-]+)"', decl, re.S)
        if nm:
            name = nm.group(1)
        add(f"govet/{name}", "govet", f"guff-govet/src/{pass_name}.rs")

    # --- every other linter contributes exactly one check -------------------
    linters = _guff_linters()
    for name in linters:
        if name not in MULTI_CHECK:
            add(name, name, "guff linters")

    # --- formatters: `guff run` reports them alongside linters --------------
    for name in FORMATTERS:
        add(name, name, "formatters")

    return {
        "linters": linters,
        "checks": [checks[k] for k in sorted(checks)],
        "counts": _counts(checks.values()),
    }


def _counts(rows) -> dict:
    by = collections.Counter(r["linter"] for r in rows)
    return {"total": sum(by.values()), "by_linter": dict(sorted(by.items()))}


# --------------------------------------------------------------------------
# observe
# --------------------------------------------------------------------------

_STATICCHECK_ID = re.compile(r"^(S|SA|ST|QF)\d+(?=:)")
_GOSEC_ID = re.compile(r"^G\d{3}(?=:)")
_PREFIX_ID = re.compile(r"^([A-Za-z][\w.-]*):")


def check_id_of(linter: str, text: str) -> str | None:
    """The check ID a finding belongs to, or None when it cannot be recovered.

    Mirrors how each linter renders its check name into the message. golangci
    and guff agree on these prefixes (that agreement is itself gated by the
    gocritic sweep and the staticcheck code prefix), so this works on both
    sides' JSON.
    """
    text = (text or "").lstrip()
    if linter == "staticcheck":
        m = _STATICCHECK_ID.match(text)
        return m.group(0) if m else None
    if linter == "gosec":
        m = _GOSEC_ID.match(text)
        return m.group(0) if m else None
    if linter in ("gocritic", "revive", "govet"):
        m = _PREFIX_ID.match(text)
        return f"{linter}/{m.group(1)}" if m else None
    return linter


def _scan_json(path: str) -> collections.Counter:
    fired = collections.Counter()
    try:
        data = json.load(open(path, encoding="utf-8"))
    except Exception:
        return fired
    for issue in data.get("Issues") or []:
        cid = check_id_of(issue.get("FromLinter") or "", issue.get("Text") or "")
        if cid:
            fired[cid] += 1
    return fired


def _source_of(json_path: str) -> str:
    base = os.path.basename(json_path)
    if base.startswith("golden-"):
        return "golden"
    if base.startswith("isolate-"):
        return "isolate"
    if os.sep + "regress" + os.sep in json_path:
        return "regress"
    return "oss"


_IGNORE_ATTR = re.compile(r"#\[ignore(?:\s*=\s*\"(?P<reason>(?:[^\"\\]|\\.)*)\")?\s*\]")


def _ignored_test_bodies() -> list[tuple[str, int, str, str]]:
    """Every `#[ignore]`d Rust test, as `(relpath, line, reason, text)`.

    `text` is the attribute plus the whole function that follows it, so a check
    named only inside the body counts. That is the case worth catching: SA1011
    sat in `never` for months next to an `#[ignore]` whose reason said
    "guff string literals for \\xNN differ from Go byte strings" and never
    mentioned SA1011 at all (COMPAT-HARDENING.md §4, 2026-08-10 5th entry).
    """
    out: list[tuple[str, int, str, str]] = []
    for path in sorted(
        glob.glob(os.path.join(ROOT, "crates", "*", "**", "*.rs"), recursive=True)
    ):
        try:
            src = open(path, encoding="utf-8", errors="ignore").read()
        except OSError:
            continue
        rel = os.path.relpath(path, ROOT)
        for m in _IGNORE_ATTR.finditer(src):
            # Body: from the attribute to the end of the function that follows,
            # found by brace matching from the first `{` after the signature.
            start = m.start()
            open_brace = src.find("{", m.end())
            end = open_brace
            if open_brace != -1:
                depth = 0
                for i in range(open_brace, len(src)):
                    if src[i] == "{":
                        depth += 1
                    elif src[i] == "}":
                        depth -= 1
                        if depth == 0:
                            end = i + 1
                            break
            out.append(
                (rel, src.count("\n", 0, start) + 1, m.group("reason") or "", src[start:end])
            )
    return out


def _scan_ignored_tests(ids: list[str]) -> dict[str, list[str]]:
    """check id -> the `#[ignore]`d tests that mention it.

    A disabled test and an unfired check are the same hole seen from two
    sides, and the ledger only ever showed one of them.
    """
    hits: dict[str, list[str]] = collections.defaultdict(list)
    bodies = _ignored_test_bodies()
    for cid in ids:
        needle = cid.split("/", 1)[1] if "/" in cid else cid
        # A bare word is only searched for when the id is distinctive enough
        # that a match cannot be prose: a code (`SA1000`, `G101`), a kebab
        # rule name (`blank-imports`) or a camelCase checker (`assignOp`).
        # Plain lowercase words (`tests`, `dupl`, `lll`) need the rendered
        # `name:` form, or every file path containing "tests" is a hit.
        distinctive = (
            re.fullmatch(r"[A-Z]{1,2}\d+", needle)
            or re.fullmatch(r"G\d{3}", needle)
            or "-" in needle
            or re.search(r"[a-z][A-Z]", needle)
        )
        pattern = (
            rf"(?<![\w-]){re.escape(needle)}(?![\w-])"
            if distinctive
            else rf"(?<![\w-]){re.escape(needle)}:"
        )
        word = re.compile(pattern)
        for rel, line, reason, text in bodies:
            if word.search(text):
                hits[cid].append(f"{rel}:{line}" + (f" — {reason}" if reason else ""))
    return dict(sorted(hits.items()))


def _scan_unit_tests(ids: list[str]) -> set[str]:
    """Static scan: which check IDs are mentioned in Rust test sources.

    A lower bound — it proves a test *names* the check, not that it asserts
    anything useful about it.
    """
    blob: list[str] = []
    for path in glob.glob(os.path.join(ROOT, "crates", "*", "tests", "**", "*.rs"), recursive=True):
        try:
            blob.append(open(path, encoding="utf-8", errors="ignore").read())
        except OSError:
            continue
    haystack = "\n".join(blob)
    hit = set()
    for cid in ids:
        needle = cid.split("/", 1)[1] if "/" in cid else cid
        # Require the rendered form (`SA1000:` / `assignOp:` / `blank-imports:`)
        # so a bare substring in prose does not count.
        if f"{needle}:" in haystack:
            hit.add(cid)
    return hit


def build_observed(inventory: dict, previous: dict | None = None) -> dict:
    """Scan run artifacts and merge into the ledger.

    `compat/results/` and `regress/results/` are gitignored, so a fresh clone
    sees no artifacts at all. The ledger is therefore **cumulative**: a check
    that fired on some machine stays `fired` until `observe --reset`.
    """
    ids = [c["id"] for c in inventory["checks"]]
    sources: dict[str, set[str]] = collections.defaultdict(set)
    totals: collections.Counter = collections.Counter()
    scanned = collections.Counter()

    # Only the source sets carry over — they are a set union, so re-scanning the
    # same directories is idempotent. Fire counts and artifact counts describe
    # *this* scan and would double on every run if merged.
    if previous:
        for cid, srcs in (previous.get("observed") or {}).items():
            sources[cid].update(srcs)

    patterns = [
        os.path.join(ROOT, "compat", "results", "*", "*.guff.json"),
        os.path.join(ROOT, "compat", "results", "*", "*.golangci.json"),
        os.path.join(ROOT, "regress", "results", "*", "*.json"),
    ]
    for pattern in patterns:
        for path in glob.glob(pattern):
            fired = _scan_json(path)
            if not fired:
                continue
            src = _source_of(path)
            scanned[src] += 1
            for cid, n in fired.items():
                sources[cid].add(src)
                totals[cid] += n

    for cid in _scan_unit_tests(ids):
        sources[cid].add("unit")

    return {
        "scanned_artifacts": dict(scanned),
        "observed": {cid: sorted(srcs) for cid, srcs in sorted(sources.items())},
        "fire_counts": dict(totals.most_common()),
        # Not a source: a disabled test is the *absence* of evidence. It is
        # recorded so the report can put it next to the status column.
        "ignored_tests": _scan_ignored_tests(ids),
    }


# --------------------------------------------------------------------------
# report
# --------------------------------------------------------------------------

RUNTIME_SOURCES = ("golden", "isolate", "oss", "regress")


def render_report(inventory: dict, observed: dict) -> str:
    obs = observed["observed"]
    rows = inventory["checks"]
    known = {c["id"] for c in rows}

    # Findings whose check ID is not in the inventory: either the inventory
    # extractor missed a check, or guff renders a name it does not declare.
    unknown = sorted(set(obs) - known)

    def status(cid: str) -> str:
        srcs = set(obs.get(cid, ()))
        if srcs & set(RUNTIME_SOURCES):
            return "fired"
        if "unit" in srcs:
            return "unit-only"
        return "never"

    by_linter: dict[str, list[str]] = collections.defaultdict(list)
    for c in rows:
        by_linter[c["linter"]].append(c["id"])

    tally = collections.Counter(status(c["id"]) for c in rows)
    total = len(rows)

    out: list[str] = []
    w = out.append
    w("# チェック単位カバレッジ台帳（COMPAT-HARDENING Phase 0）\n")
    w("> 自動生成: `./compat/coverage.py inventory && ./compat/coverage.py observe && ./compat/coverage.py report`。")
    w("> 手で編集しない。計画は [`COMPAT-HARDENING.md`](COMPAT-HARDENING.md)。\n")
    w("**`never` = どのテストでも一度も発火していない = 完全未検証。**")
    w("recall バグがあっても既存のどのゲートにも現れない。ここが Phase 3 のターゲットリスト。\n")
    w("| 状態 | 意味 | 件数 | 割合 |")
    w("|------|------|-----:|-----:|")
    for key, label in (
        ("fired", "golden / isolate / OSS / regress の実行で発火した"),
        ("unit-only", "Rust 単体テストが ID に言及するのみ（静的スキャン。golangci-lint との突合なし）"),
        ("never", "**どこでも発火していない**"),
    ):
        n = tally.get(key, 0)
        w(f"| `{key}` | {label} | {n} | {n * 100.0 / total:.1f}% |")
    w(f"| — | **合計** | **{total}** | 100.0% |\n")

    w("## linter 別\n")
    w("| linter | checks | fired | unit-only | never |")
    w("|--------|-------:|------:|----------:|------:|")
    for linter in sorted(by_linter):
        ids = by_linter[linter]
        t = collections.Counter(status(i) for i in ids)
        w(
            f"| {linter} | {len(ids)} | {t.get('fired', 0)} | "
            f"{t.get('unit-only', 0)} | {t.get('never', 0)} |"
        )
    w("")

    never = [c["id"] for c in rows if status(c["id"]) == "never"]
    w(f"## 一度も発火していない check（{len(never)} 件）\n")
    if never:
        grouped: dict[str, list[str]] = collections.defaultdict(list)
        for c in rows:
            if status(c["id"]) == "never":
                grouped[c["linter"]].append(c["id"])
        for linter in sorted(grouped):
            names = ", ".join(f"`{i}`" for i in grouped[linter])
            w(f"- **{linter}** ({len(grouped[linter])}): {names}")
    else:
        w("なし。")
    w("")

    # `#[ignore]` されたテストが言及する check。無効化されたテストと未発火の
    # check は同じ穴の裏表で、台帳はこれまで片側しか映していなかった
    # （SA1011 の実例は COMPAT-HARDENING.md §4 / 2026-08-10 5 本目）。
    ignored = observed.get("ignored_tests") or {}
    w(f"## `#[ignore]` されたテストが言及する check（{len(ignored)} 件）\n")
    if ignored:
        w("`never` / `unit-only` の行が **その check を無効化されたテストが名指ししている**")
        w("という意味なので、まずそこを読むこと。`fired` なら別のゲートが見ている。\n")
        w("| check | 状態 | `#[ignore]` されたテスト |")
        w("|-------|------|--------------------------|")
        for cid, where in ignored.items():
            st = status(cid) if cid in known else "—"
            mark = "**" if st in ("never", "unit-only") else ""
            w(f"| `{cid}` | {mark}{st}{mark} | {'<br>'.join(where)} |")
    else:
        w("なし（`#[ignore]` の付いたテストはどの check ID にも言及していない）。")
    w("")

    if unknown:
        w(f"## インベントリ外の check ID（{len(unknown)} 件）\n")
        w("実行結果には出たが、インベントリ抽出が拾えていない ID。抽出器のバグか、")
        w("guff が宣言していない名前を描画している。\n")
        w(", ".join(f"`{u}`" for u in unknown))
        w("")

    w("## 集計の元データ\n")
    w(f"- 走査した実行アーティファクト: `{observed['scanned_artifacts']}`")
    w(f"- インベントリ: {total} checks / {len(inventory['linters'])} linters")
    w("- `unit` は Rust テストソースの静的スキャン（下限値）。ID に言及していることの証明であって、")
    w("  golangci-lint と突き合わせている証明ではない。")
    return "\n".join(out) + "\n"


# --------------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("command", choices=["inventory", "observe", "report", "all"])
    ap.add_argument(
        "--reset",
        action="store_true",
        help="observe: discard the previous ledger instead of merging into it",
    )
    args = ap.parse_args()

    os.makedirs(COVDIR, exist_ok=True)

    if args.command in ("inventory", "all"):
        inv = build_inventory()
        json.dump(inv, open(INVENTORY, "w", encoding="utf-8"), indent=2, sort_keys=True)
        print(f"inventory: {inv['counts']['total']} checks -> {os.path.relpath(INVENTORY, ROOT)}")

    if args.command in ("observe", "all"):
        inv = json.load(open(INVENTORY, encoding="utf-8"))
        prev = None
        if not args.reset and os.path.exists(OBSERVED):
            prev = json.load(open(OBSERVED, encoding="utf-8"))
        obs = build_observed(inv, prev)
        json.dump(obs, open(OBSERVED, "w", encoding="utf-8"), indent=2, sort_keys=True)
        print(
            f"observed: {len(obs['observed'])} check ids across "
            f"{obs['scanned_artifacts']} -> {os.path.relpath(OBSERVED, ROOT)}"
        )

    if args.command in ("report", "all"):
        inv = json.load(open(INVENTORY, encoding="utf-8"))
        obs = json.load(open(OBSERVED, encoding="utf-8"))
        md = render_report(inv, obs)
        open(REPORT_MD, "w", encoding="utf-8").write(md)
        print(f"report -> {os.path.relpath(REPORT_MD, ROOT)}")
        for line in md.splitlines():
            if line.startswith("| `never`") or line.startswith("| `fired`") or line.startswith("| `unit-only`"):
                print("  " + line)

    return 0


if __name__ == "__main__":
    sys.exit(main())
