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

**Verdict: `GUFF_DEP_SOURCE` stays OFF by default** — but the root cause was
re-diagnosed (see below) as **latent guff analyzer bugs + more analysis
coverage**, *not* type-info corruption. Three real analyzer bugs were fixed this
session (1 bools, 3 ineffassign: expr traversal + for-loop back-edge), cutting
the hybrid-vs-baseline divergence **354 → 124** (ineffassign 204 → 2). The
remaining divergence is dominated by hybrid
type-checking packages the export path drops as ill-typed (so it runs analyzers
on more code) plus guff's *pre-existing* guff-vs-golangci divergences (ST1000,
generated-file lint scope, ineffassign over-report) surfacing on that extra
coverage. Making cold-source the default still needs those settled + the
`signature.rs:164` panic fixed, but hybrid is now understood to be *sounder*
than the first adjudication implied.

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
2. **Latent `ineffassign` analyzer bugs (FIXED this session) — NOT a type-info
   gap.** The initial hypothesis was that hybrid produced an incomplete
   `types.Info`. **This was disproven**: a go-gated probe loaded the *real*
   `config` package both ways and found `retention`'s `Info.uses`/`Info.defs`
   and object identity **identical** (def `ObjectId` == use `ObjectId`) in source
   and export mode. The FPs were instead in ineffassign's hand-rolled
   `walk_expr` (`crates/guff-ineffassign/src/cfg.rs`), which was **missing arms**
   for several expression kinds, so identifier uses (and address-of escapes)
   inside them were invisible → a live variable was falsely "ineffectual":
   - `CompositeLit` / `KeyValueExpr` — `retention := …; &T{F: &retention}`
   - `TypeAssertExpr` — `for _, cfg := range … { cfg.(T) }`
   - `IndexListExpr` — generic instantiation operands
   - `SliceExpr` bounds — `a := off[i]; … s[a:b]` (openmetricsparse.go)
   Plus a **for-loop CFG back-edge bug** (`walk_for`): the back-edge was added as
   `cond.children.push(start)` but `start == cond` (walking the condition creates
   no new block), i.e. a `cond → cond` self-loop, so the post block (`i++`) had no
   successor and the increment was reported "ineffectual" on ordinary/stepped
   loops (`for i := 0; i < n; i += 2`). Fixed to push from the current (post)
   block, matching `walk_range`.
   All are path-independent real bugs (also fix the default/export run).
   Regression fixture `tests/testdata/basic/composite_ok.go` covers each.
   Combined with the bools fix these cut hybrid-only **354 → 124** (ineffassign
   hybrid-only 204 → **2**).

   **Why the "config Info looked corrupt":** the export path marks `config`
   `ill_typed` when loaded in isolation (some dep type didn't resolve from export
   data) and therefore **skips analysis** (0 findings — a silent false negative),
   whereas hybrid type-checks it and **runs the analyzers**, exposing the latent
   bugs. So hybrid was *more* complete, not less. A separate isolated
   `signature.rs:164` `as_signature` panic (R25.2 DEFERRED) remains and does
   corrupt `Info` where it hits, but it is not what produced the config FPs.

   **Remaining hybrid-only (124)**: 20 generated-file + 23 ST1000 package-comment
   (both known guff-vs-golangci allowlist classes) + 81 "other" — now dominated
   by **staticcheck (75)**, concentrated in packages baseline drops as ill-typed
   (`storage/remote/otlptranslator/*`, `model/textparse/*`). ineffassign is
   essentially resolved (2 left). The staticcheck "other" is guff's *own*
   pre-existing behavior on newly-covered code:
   - **SA4006 loop-increment FP (~40, `this value of x is never used`)**: the
     `IncDecStmt` arm of `crates/guff-staticcheck/src/sa4006.rs` is a fragile AST
     heuristic — `ident_used_before` only scans uses *textually before* the `x++`
     and `node_reads_obj` only inspects Assign/ExprStmt/IncDec nodes, so it misses
     the loop **condition** use (`x < n`, a BinaryExpr) and flags loop counters.
     The correct fix is SSA-based liveness (as SA4006's *assignment* path already
     does via `has_use`), not the AST hack. **Not fixed this session** (SSA rework,
     needs its own SA4006 fixture pass).
   - smaller classes: `argument X overwritten before first use` (SA4009, ~12),
     `variable in loop condition never changes` (~8), `should use copy()` (S1001,
     genuine), ST1003 initialisms (`exemplarTs → exemplarTS`, genuine).
   None are hybrid-introduced type-info corruption; they are guff's existing
   analyzer behaviors surfacing on the extra packages hybrid type-checks.

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

Fixed this session (committed): bools `expr_key`; ineffassign `walk_expr`
traversal (CompositeLit/KeyValueExpr/TypeAssertExpr/IndexListExpr/SliceExpr
bounds); ineffassign for-loop back-edge (`walk_for`). Hybrid-only **354 → 124**;
ineffassign 204 → 2. Remaining 124 = 20 generated + 23 ST1000 + 81 "other"
(staticcheck 75 + errcheck 2 + ineffassign 2 + govet 2).

1. **Rework SA4006's `IncDecStmt` arm to use SSA** (`crates/guff-staticcheck/src/sa4006.rs`,
   ~40 of the 75 staticcheck "other"). The AST heuristic `ident_used_before` +
   `node_reads_obj` misses uses in conditions/calls/index exprs and only scans
   *backward*, so it flags loop counters (`for x := 0; x < n; x++`). SA4006's
   assignment path already does it right via `has_use` on the SSA value; port the
   IncDec case to the same. Re-run guff-staticcheck's SA4006 fixtures.
2. **Triage the remaining staticcheck** (SA4009 `argument overwritten before
   first use`, `variable in loop condition never changes`) the same way; keep the
   genuine ones (S1001 `copy()`, ST1003 initialisms).
3. **Fix the isolated `signature.rs:164` `as_signature` panic** (R25.2 DEFERRED):
   a non-Signature `TypeId` reaches `as_signature` (called from `stmt.rs`
   defer/go handling, `mono.rs`, `typestring.rs`) — likely a cross-arena /
   unmaterialized method signature in the source importer. It is already isolated
   (caught) so it doesn't abort; do **not** replace the panic with a silent
   fallback (that substitutes wrong types — worse than a clean drop). Root-cause
   the bad TypeId instead.
4. **Then re-adjudicate**; if hybrid-only reduces to the compat allowlist classes
   (generated-file scope + ST1000 + ineffassign over-report), make cold-source
   the default.
5. **(Independent) analyzer determinism** — govet-unreachable ordering and
   ineffassign multi-var tie-break (HashMap iteration → which of several vars at
   one assignment site is reported); both help the export path too.
6. **Warm**: unchanged (warm keeps the export path). Optionally cache the 2nd
   stdlib `go list -export` (R24.4-style).

Reliable e2e repro harness (baseline drops the package, hybrid covers it):
```bash
cat >/tmp/std.yml <<'Y'
version: "2"
linters: {default: none, enable: [staticcheck, ineffassign]}
issues: {max-issues-per-linter: 0, max-same-issues: 0}
run: {tests: true}
Y
cd prometheus
target/release/guff run -c /tmp/std.yml --no-cache ./storage/remote/otlptranslator/...              # baseline (drops pkgs)
GUFF_DEP_SOURCE=1 target/release/guff run -c /tmp/std.yml --no-cache ./storage/remote/otlptranslator/...  # hybrid (covers, surfaces SA4006 loop FPs)
```
`Info` integrity is proven correct (a go-gated probe loaded `config` both ways
and diffed `Info.uses`/`defs` + def/use `ObjectId` identity per identifier — all
identical; see git history for the `config_uses_probe` scaffold). So the
remaining divergence is analyzer behavior, not type-info.

## Persisted context (for the assistant)

`cold-golist-export-cost`, `stdlib-source-typecheck-gap`, `hybrid-cold-benchmark`
capture the measurements and rationale.
