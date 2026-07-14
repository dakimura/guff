# Benchmark results

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.4`
- guff: `0.1.0`
- golangci-lint: `2.12.2`
- Samples per cell: 3 (median reported; `FAIL` if any sample exited non-zero)
- Preset: standard five linters via `benchmarks/standard.yml`

| Target | guff cold | guff warm | golangci cold | golangci warm | guff/gcl warm |
|--------|----------:|----------:|--------------:|--------------:|--------------:|
| fixture | 0.934s | 0.924s | 0.707s | 0.176s | 5.26x |
| local | 1.774s | 1.755s | 0.820s | 0.296s | 5.92x |

Ratio `<1.0x` means guff warm was faster than golangci-lint warm.
OSS targets often `FAIL` on guff until SSA gaps (R17) land; prefer `fixture` / `local`.
