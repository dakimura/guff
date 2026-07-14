# Benchmark results

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.4`
- guff: `0.1.0`
- golangci-lint: `2.12.2`
- Samples per cell: 3 (median reported; `FAIL` if any sample exited non-zero)
- Preset: standard five linters via `benchmarks/standard.yml`

| Target | guff cold | guff warm | golangci cold | golangci warm | guff/gcl warm |
|--------|----------:|----------:|--------------:|--------------:|--------------:|
| fixture | 0.222s | 0.136s | 0.655s | 0.177s | 0.77x |
| local | 0.741s | 0.156s | 0.775s | 0.287s | 0.54x |

Ratio `<1.0x` means guff warm was faster than golangci-lint warm. **guff is now
faster than golangci-lint on both cold and warm for these targets.**
OSS targets often `FAIL` on guff until SSA gaps (R17) land; prefer `fixture` / `local`.

## Performance history

The R11 baseline was 5.26x / 5.92x *slower* than golangci-lint on warm. The
perf pass below closed and reversed that gap.

| Change | Effect |
|--------|--------|
| `[profile.release]` (fat LTO, `codegen-units=1`) | ~10–20% on the CPU-bound parse/type-check path |
| Parallel type-checking (rayon) | Package type-checking scales across cores; was a single-threaded loop |
| Deterministic cache salt (`SettingsBag` Debug sorted) | The salt fingerprinted a `HashMap` in random order, so ~half of warm runs missed the whole cache |
| Deterministic `NeedAllDeps` hash (flat `deps` + registry) | Dependency hashing walked the import graph, whose depth varied run to run, flipping every package hit↔miss. Now hashed from `go list`'s flat `deps` against a full self-hash registry |
| **Lazy type-check** | The issues-cache check now runs *before* parsing/type-checking. Cache hits restore issues straight from disk (positions already resolved, no `FileSet`); only cache misses are parsed + type-checked. A fully-warm tree does zero type-checking |

On `local` (13 packages) a warm run now type-checks **0 of 13** roots and
finishes in ~0.15s. Editing one package re-type-checks only that package and
its dependents (correct dependency invalidation via the dep-hash registry).

As a side effect the atomic `FileSet` base-allocation fix corrected an errcheck
column (`f00.go:14:7` → `14:9`) to match golangci-lint.

Set `GUFF_DEBUG_CACHE=1` to print per-run cache hit/miss and how many roots were
type-checked.
