# Cold hybrid source mode — status & Stage 3 resume guide

> Feature flag: **`GUFF_DEP_SOURCE=1`** (env), plumbed to `Config.dep_source` /
> `TypecheckEnv.from_source`. **Default off** — nothing below changes normal runs.
> Companion docs: `docs/PURE-SOURCE-TYPECHECK.md` (the *next-next* step: stdlib
> from source too). This file tracks the **hybrid** work and what Stage 3 needs.

## Goal

Cut cold-start time by not making `go list -export` compile export data for the
1300+ third-party dependency packages. Instead, type-check those from source in
Rust (guff-types), and keep export data only for stdlib (a cheap ~4s subset).
This is a pure performance play; `go` is still required.

## What's implemented (this session)

- `Config.dep_source` (`crates/guff-packages/src/config.rs`) and
  `TypecheckEnv.from_source` (`crates/guff-packages/src/typecheck.rs`).
- `crates/guff-packages/src/golist.rs`:
  - `uses_export_data()` returns `false` in `dep_source` mode → main `go list`
    runs **without `-export`**; requests the `Standard` field.
  - `fetch_stdlib_exports()` — a **second** `go list -export` restricted to the
    stdlib packages, whose `.a` paths are merged back onto those packages.
- `crates/guff-packages/src/typecheck.rs`: `build_source_seed()` builds the
  shared `ExportSeed` by (a) attaching an `ExportImporter` for packages that have
  export data (stdlib), and (b) registering source via `Checker::add_dependency_source`
  for those that don't (third-party), then `source_preload` DFS-loads each once.
  Returns the **same `ExportSeed` type** as the export path, so all downstream
  (SSA, parallel workers, R25.1 arenas) is unchanged.
- `crates/guff-lint/src/lib.rs`: reads `GUFF_DEP_SOURCE`, sets `dep_source` on
  both configs and `from_source` on the typecheck env.
- Tests: `crates/guff-packages/tests/dep_source_vs_export.rs` (go-free differential:
  source path == export path on the `withdep`/`simple` fixture),
  `crates/guff-packages/tests/hybrid_load.rs` (go-gated e2e on the new
  `tests/testdata/typecheck/hybrid` fixture).

Reused, key enabler: guff-types **already had** a built-in source importer
(`add_dependency_source` / `check_dependency` / `import_package`, source takes
precedence over the pluggable `ExportImporter`) — no new importer/crate needed.

## Measured (Prometheus, 113 roots / 1530 pkgs, fresh GOCACHE, `--no-cache`)

| phase | baseline (export) | hybrid |
|---|--:|--:|
| go list | 32.4s | 7.0s |
| typecheck_roots | 1.7s | 10.2s |
| analyze | 4.4s | 2.9s |
| **wall** | **42.7s** | **23.6s** (1.8×) |
| user CPU | 327s | 36s (9×) |

Reproduce (from the `prometheus/` checkout, `$GUFF` = `target/release/guff`):
```bash
cargo build --release -p guff-lint
C=$(mktemp -d); GOCACHE=$C GUFF_DEBUG_CACHE=1 /usr/bin/time -p $GUFF run --no-cache ./... >/tmp/base.out 2>&1; rm -rf $C
C=$(mktemp -d); GOCACHE=$C GUFF_DEP_SOURCE=1 GUFF_DEBUG_CACHE=1 /usr/bin/time -p $GUFF run --no-cache ./... >/tmp/hyb.out 2>&1; rm -rf $C
```

## Stage 3 — correctness parity gate: **RESOLVED (verdict: keep opt-in)**

Adjudicated 2026-07-19 against golangci-lint on Prometheus under
`compat/standard.yml` (5 std linters, `--no-cache`). Method: three JSON dumps
(golangci-lint / guff-baseline / guff-hybrid), normalized via
`compat/normalize.py` (`relpath:line:linter:message`), set-diffed in Python
(**not `comm`** — locale collation gives false zero-overlaps).

**Verdict: the hybrid does NOT meet correctness parity — `GUFF_DEP_SOURCE`
stays OFF by default.** The +342 (measured here as **+329 after the bools fix
below**) are **not confirmed by golangci-lint** and spot-checks prove at least
some are **false positives caused by hybrid's incomplete `types.Info`** for
packages that import source-checked third-party deps.

### What the adjudication found

- Under `standard.yml`: golangci=885, baseline=591, hybrid=918 normalized keys.
  hybrid−baseline = **354** (later 329); baseline−hybrid = **27**, *all* of which
  are `govet:unreachable code` in generated `*.pb.go`/`decoder.go` — the known
  flaky govet-unreachable ordering (R25.2 DEFERRED), not a real loss.
- **Confounders make golangci a noisy 1:1 adjudicator at std-preset scale**:
  guff lints generated protobuf (`prompb/`, ~300 findings) that golangci
  excludes; guff barely fires `errcheck` (0–2 vs golangci's 718); golangci
  reports 0 `ineffassign`/`unused`. These are *pre-existing* guff-vs-golangci
  gaps, unrelated to hybrid.
- **The decisive test is baseline-vs-hybrid on a single package.** On
  `./config/...` alone: baseline (export, full linters) type-checks config fine
  (48 revive/whitespace findings, 0 govet/ineffassign) — matching golangci
  (nothing in `config.go`). Hybrid emits 17 findings in `config.go` including
  `govet: redundant and: (_ EQL 0) LAND (_ EQL 0)` and
  `ineffassign: ineffectual assignment to retention`. Both are **false
  positives** (source: `c.ScrapeInterval == 0 && c.ScrapeTimeout == 0 …` is not
  redundant; `retention` is used via `&retention`).

### Root causes (two independent)

1. **Latent `bools` analyzer bug (FIXED this session).** `bools::expr_key`
   (`crates/guff-govet/src/bools.rs`) collapsed every `SelectorExpr` (and
   call/index/unary/star) to `"_"`, so `a.x == 0 && a.y == 0` keyed identically →
   spurious "redundant and". Fixed to render operands structurally (mirrors
   `expreq::expr_equal`). This is a real FP in **both** paths: it removed **5**
   false positives from the default/export path (`model/*`) and **25** from
   hybrid, adding none. Regression test:
   `crates/guff-govet/tests/checks_test.rs::bools_distinct_selectors_not_redundant`
   (+ `tests/testdata/bools_sel/main.go`).
2. **guff-types source-importer `Info` fidelity gap (OPEN — the real blocker).**
   The remaining 329 hybrid-only findings (ineffassign 204, staticcheck 106,
   govet 17, errcheck 2) trace to hybrid producing a *different* `types.Info`
   than the export path for dependents of source-checked deps:
   - `ineffassign` FPs (e.g. config `retention`): the checker fails to record the
     `use` of a local whose type is a source-checked named type
     (`model.Duration`) inside a composite literal `&T{F: &retention}` →
     `Info.uses` is missing the entry → ineffassign flags a live variable. **No
     panic involved** — silent incompleteness.
   - a separate isolated **`signature.rs:164` panic** (`as_signature` expected
     Signature, got other) fires on some packages under hybrid (R25.2 DEFERRED),
     which *also* corrupts `Info` where it hits.
   Fixing this means hardening the guff-types built-in source importer so a
   source-checked dependency yields `Info` (types/defs/uses/const-values)
   identical to `gcexportdata`. Minimize offenders into
   `crates/guff-types/tests/*.rs` (see `docs/PURE-SOURCE-TYPECHECK.md` §method).

### Determinism (checked)

Excluding the two **pre-existing** flaky analyzer classes, hybrid is
**count-stable (781/781/781)** across `-j 1`, `RAYON_NUM_THREADS=1`, and parallel,
with a 776-key stable core — i.e. the hybrid seed build / `source_preload` is
deterministic. The instability is entirely:
- `govet:unreachable code` on generated `*.pb.go` (count swings 76–114) — also
  flaky in the **export** path (two baseline runs flipped ~34 of these);
- `ineffassign` **tie-break** on multi-var assignment sites (e.g.
  `tsdb/record/record.go:605` reports `st` under parallel/-j1, `ref` under
  RAYON=1) — HashMap iteration order in the analyzer, also path-independent.

Neither is introduced by hybrid; both are separate pre-existing bugs.

### Reproduce the adjudication
```bash
GUFF=target/release/guff; CFG=$PWD/compat/standard.yml; PROM=$PWD/prometheus
cargo build --release -p guff-lint
( cd "$PROM"
  golangci-lint run -c "$CFG" --output.json.path=stdout --path-mode abs --issues-exit-code 0 ./... >/tmp/gcl.json
  "$GUFF" run -c "$CFG" --out-format json --issues-exit-code 0 --no-cache ./... >/tmp/base.json 2>/dev/null
  GUFF_DEP_SOURCE=1 "$GUFF" run -c "$CFG" --out-format json --issues-exit-code 0 --no-cache ./... >/tmp/hyb.json 2>/dev/null )
python3 - <<'PY'
import sys; sys.path.insert(0,"compat")
from normalize import load_issues, issue_keys
r="prometheus"; K=lambda f: issue_keys(load_issues(f),r)
g,b,h=K("/tmp/gcl.json"),K("/tmp/base.json"),K("/tmp/hyb.json")
print("gcl",len(g),"base",len(b),"hyb",len(h),"hyb_only",len(h-b),"confirmed_by_gcl",len((h-b)&g))
PY
```

### Remaining work (next session, in priority order)

1. **Close the `Info` fidelity gap (task 2, the blocker)** — start with the
   `ineffassign`/`config.retention` minimal repro: a package importing a
   source-checked named type used by address inside a composite literal, asserting
   `Info.uses` parity with the export path. Then the `signature.rs:164` panic.
2. **Then re-adjudicate** and, if hybrid-only shrinks to the known guff-vs-golangci
   allowlist classes, consider making cold-source the default.
3. **(Independent) analyzer determinism** — govet-unreachable ordering and
   ineffassign multi-var tie-break; both help the export path too.
4. **Warm**: unchanged (warm keeps the export path; guff's lazy type-check rarely
   type-checks on warm). Optionally cache the 2nd stdlib `go list -export`
   (R24.4-style).

## Persisted context (for the assistant)

`cold-golist-export-cost`, `stdlib-source-typecheck-gap`, `hybrid-cold-benchmark`
capture the measurements and rationale.
