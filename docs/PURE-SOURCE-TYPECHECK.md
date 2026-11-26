# Pure-source type-checking (future migration from the hybrid cold path)

> Status: **future work**. The cold-speedup effort ships as a **hybrid** first
> (third-party dependencies type-checked from source, stdlib resolved from
> compiler export data). This document is the detailed plan for the *next* step:
> type-checking **stdlib too** from source, so the `go list -export` build is
> eliminated entirely. Keep it in sync as guff-types gains stdlib coverage.

## Why (and why it's deferred)

Cold `go list` on Prometheus (113 pkg, isolated `GOCACHE`, 2026-07-19):

| call | packages | cold wall |
|------|---------:|----------:|
| `go list -json -compiled -deps -export ./...` | 1530 (1305 third-party + 225 stdlib) | 33.2s |
| `go list -json -compiled -deps ./...` (no `-export`) | 1530 | 5.6s |
| `go list -json -compiled -export std` | 225 | 4.2s |

The ~27s cold cost is the `-export` compilation. Of that, **stdlib is only ~4.2s;
the other ~29s is the 1305 third-party packages.** The hybrid captures the ~29s
by source-checking third-party deps (normal Go that guff-types already handles)
while paying the cheap ~4.2s to keep stdlib on export data.

**Pure source** removes that last ~4.2s (cold → ~6s) and is the prerequisite for
any future **go-less** operation (no toolchain needed at all). It is deferred
because guff-types cannot yet type-check the low-level stdlib from source.

## The concrete blocker (measured)

Source-checking a trivial `package main`'s dependency closure = the 27-package
`runtime`/`internal/*` closure. guff-types produced **347 type errors** on it.
Registered packages (all failing to varying degrees):

```
runtime, internal/abi, internal/bytealg, internal/byteorder, internal/cpu,
internal/goarch, internal/goos, internal/runtime/{atomic,maps,math,sys,exithook,gc,...},
internal/chacha8rand, internal/stringslite, internal/strconv, math/bits, ...
```

Representative error messages (all point at the same root causes):

- `cannot range over invalid type`
- `cannot index sep` / `cannot index b` / `cannot slice s`
- `invalid argument: sep for built-in len`
- `non-boolean condition in if statement`

These indicate **function parameter / element types resolving to `invalid`** —
i.e. some declaration in these packages fails to type-check and the error
cascades into every dependent expression.

## Suspected root-cause classes (to confirm, then fix)

These are the low-level features concentrated in the `runtime`/`internal` layer.
Confirm each against `guff-types` before fixing:

1. **Bodiless / assembly-implemented functions** — `func addr(p unsafe.Pointer) uintptr`
   declared in a `.go` file with the body in a `.s` file. go/types accepts a
   declaration with no body; verify guff-types does not error and still records
   the signature. (`internal/bytealg`, `internal/runtime/atomic`, `math/bits`
   are heavy here — likely the `sep`/`b`/`s` param errors originate from a
   bytealg function whose signature failed.)
2. **`//go:linkname`, `//go:noescape`, `//go:nosplit`, `//go:build`** directives —
   must be tolerated/ignored by the checker (file selection is already handled by
   `go list` CompiledGoFiles; guff-types only needs to not choke on the comments).
3. **`unsafe` intrinsics** — `unsafe.Pointer`, `Sizeof`, `Offsetof`, `Add`,
   `Slice`, `SliceData`, `String`, `StringData`. Confirm all are modeled.
4. **Generics in the runtime** — `internal/runtime/maps`, `internal/runtime/gc`
   use type parameters and instantiation patterns heavier than typical user code.
5. **Predeclared/compiler-magic** — e.g. `runtime` references to compiler-provided
   symbols, `//go:cgo_import_dynamic`, and package-level `init` ordering.

## Method (incremental, differential — do NOT big-bang)

Reuse the harness style already in the repo (`crates/guff-types/tests/*.rs`, one
file per feature; `go_available()` gating; `compat/allowlist.txt`-style lists).

1. **Per-package differential harness.** For a stdlib package `P`: obtain its
   `CompiledGoFiles` from `go list` (no `-export`) and its dep set; type-check `P`
   from source with its deps resolved from **export data** (so only `P` itself is
   under source test); compare guff-types' diagnostics for `P` against the
   export-data baseline (which is error-free by construction). Any diagnostic on
   `P` is a guff-types gap.
2. **Leaves-first allowlist.** Maintain a growing `STDLIB_SOURCE_OK` list. Start at
   the leaves (`internal/goarch`, `internal/goos`, `internal/cpu`,
   `internal/byteorder`, `math/bits`) and walk up toward `runtime`. Add a package
   to the list only when it source-checks clean.
3. **Fix → regression test.** For each failing package, minimize the failure into
   a `crates/guff-types/tests/<feature>.rs` case (e.g. `bodiless_func.rs`,
   `unsafe_slicedata.rs`), fix guff-types, keep the case as a regression guard.
4. **Flip the switch per-tier.** The hybrid seed builder already resolves a package
   from source when registered and from export data otherwise (source takes
   precedence — see `Checker::import_package`). So "graduating" a stdlib package
   to source is just: stop requesting its `-export` and register its source. This
   lets pure-source roll out **package-by-package** behind the allowlist, never
   all-or-nothing.
5. **Done = stdlib closure of the Prometheus roots source-checks clean**, diagnostics
   byte-identical to the export path, and `-export` can be dropped entirely.

## Wiring notes (where the code is)

- Hybrid seed builder: `build_source_seed` in `crates/guff-packages/src/typecheck.rs`.
  For pure source, pass an empty `export_paths` (or an allowlist-shrunk one) so all
  deps register as source.
- Built-in source importer: `Checker::add_dependency_source` /
  `check_dependency` / `import_package` in `crates/guff-types/src/check.rs`
  (source registration already takes precedence over the pluggable `ExportImporter`).
- Driver flag: `Config.dep_source` (`crates/guff-packages/src/config.rs`) and
  `TypecheckEnv.from_source` (`typecheck.rs`); `uses_export_data`
  (`crates/guff-packages/src/golist.rs`) drops `-export` in source mode.

## Risks specific to pure source (beyond the hybrid)

- **Position (`Pos`) space**: source-checking stdlib adds thousands more files to
  the shared `FileSet`. Watch the R25.2 `u32` Pos ceiling (`position.rs::add_file`
  probe). Prometheus source (fewer files than the full stdlib×pkg fan-out) already
  approached limits historically.
- **Performance**: source-checking 225 stdlib packages once (into the seed) must
  stay well under the 4.2s it replaces. The 27-pkg runtime closure took 1.7s in
  the probe, so the full stdlib is plausibly a few seconds — measure.
- **Determinism**: keep `-j 1` / `RAYON_NUM_THREADS=1` / parallel byte-identical.

## Relationship to go-less

Pure source is necessary-but-not-sufficient for running without a Go toolchain.
Go-less additionally needs module resolution without `go list` (vendor/ dir or
`$GOMODCACHE` reading, GOROOT/src discovery). That is a **separate** effort
(different value axis: deployment, not speed) and out of scope here.
