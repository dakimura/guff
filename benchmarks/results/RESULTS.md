# Benchmark results

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.4`
- guff: `0.1.0`
- golangci-lint: `2.12.2`
- Samples per cell: 3 (median reported; `FAIL` if any sample exited non-zero)
- Cold tool cache = empty `GUFF_CACHE` / `GOLANGCI_LINT_CACHE`
- Hybrid dependency type-checking is **on by default** (`GUFF_DEP_SOURCE=0` opts out)

## Prometheus (real `.golangci.yml`, empty `GOCACHE`)

Corpus: [prometheus/prometheus](https://github.com/prometheus/prometheus) @ `66df005b9`.
Config: prometheus’s own `.golangci.yml` (~20 analyzers + `gofumpt` / `gci` /
`goimports`; `nilnesserr` skipped on guff). Concurrency: auto (`available_parallelism`).

| Target | guff cold | golangci cold | guff is |
|--------|----------:|--------------:|--------:|
| `./tsdb/...` | 38.9s | 54.6s | **1.4× faster** |

This is the agent / CI sandbox case (no warm compile cache). With a warm
`GOCACHE` both tools skip dependency export compiles and the gap shrinks.

`./...` under hybrid + this config still hits residual SSA builder panics on
some packages (opt out with `GUFF_DEP_SOURCE=0`); prefer `./tsdb/...` for
apples-to-apples timing until those land.

## Synthetic (standard five-linter preset, warm `GOCACHE`)

| Target | guff cold | guff warm | golangci cold | golangci warm | guff/gcl warm |
|--------|----------:|----------:|--------------:|--------------:|--------------:|
| fixture | 0.222s | 0.136s | 0.655s | 0.177s | 0.77x |
| local | 0.741s | 0.156s | 0.775s | 0.287s | 0.54x |

Ratio `<1.0x` means guff warm was faster than golangci-lint warm.

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
| **Hybrid cold deps (default)** | `go list` without `-export` for third-party deps; type-check them from source. Biggest win on empty `GOCACHE` |

On `local` (13 packages) a warm run now type-checks **0 of 13** roots and
finishes in ~0.15s. Editing one package re-type-checks only that package and
its dependents (correct dependency invalidation via the dep-hash registry).

Set `GUFF_DEBUG_CACHE=1` to print per-run cache hit/miss and how many roots were
type-checked. Set `GUFF_DEP_SOURCE=0` to force the legacy export-data path.
