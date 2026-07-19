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

## ⚠ Stage 3 — the remaining gate (correctness parity)

The hybrid currently produces **different diagnostics** from the export path:
- shared **2663**, **+342 only in hybrid** (cluster in files like
  `config/config.go`: `bools`, `ineffassign`, `ST1000`), **−2 only in baseline**
  (`unreachable`; possibly the known flaky govet-unreachable ordering).

Diff them with:
```bash
comm -13 <(sort /tmp/base.out) <(sort /tmp/hyb.out)   # only in hybrid
comm -23 <(sort /tmp/base.out) <(sort /tmp/hyb.out)   # only in baseline
```

**Open question**: are the +342 *real findings the export path was missing*
(more complete dependency type info from source), or *false positives* from
imperfect third-party source types (generics/protobuf/cgo in deps)?

### Stage 3 task list (start here next session)

1. **Adjudicate against golangci-lint** — the source of truth. Run golangci-lint
   on Prometheus and see whether the +342 are in its output. Use the `compat/`
   harness (`compat/run.sh`; it keys on `relpath:line:linter:message`,
   normalizes via `compat/normalize.py`). Prometheus is **not** in
   `compat/repos.txt` yet — add it or run a one-off. Classify each of the +342 as
   true-positive-recovered vs false-positive.
2. **If false positives** → fix guff-types on the offending third-party patterns
   (minimize into `crates/guff-types/tests/*.rs` regression cases, as in
   `docs/PURE-SOURCE-TYPECHECK.md` §method). If genuine recoveries → update the
   compat baseline/allowlist and treat as an improvement.
3. **Determinism**: confirm byte-identical output across `-j 1`,
   `RAYON_NUM_THREADS=1`, and parallel (`docs/DEVELOPMENT.md` §8). Watch the
   R25.2 `u32` Pos ceiling — source-checking many third-party files grows the
   shared `FileSet` (`position.rs::add_file` probe).
4. **Warm non-regression**: `benchmarks/run.sh` warm must not regress (warm keeps
   the export path; guff's lazy type-check means warm rarely type-checks at all).
   Consider caching the 2nd stdlib `go list -export` call (R24.4-style).
5. **Only after parity**: decide whether to keep `GUFF_DEP_SOURCE` opt-in or make
   cold-source the default.

## Persisted context (for the assistant)

`cold-golist-export-cost`, `stdlib-source-typecheck-gap`, `hybrid-cold-benchmark`
capture the measurements and rationale.
