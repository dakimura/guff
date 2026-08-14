# Benchmark results

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.5`
- guff: `0.4.1`
- golangci-lint: `2.12.2`
- Samples per cell: 3 (median reported; `FAIL` if any sample exited non-zero)
- Fixture/local: `benchmarks/standard.yml` (standard five linters)
- OSS: each repo's real golangci-lint v2 config (own-config)
- Protocol: GOCACHE warm (prepare), tool caches cold then warm; clone/mod download excluded

| Target | config | guff cold | guff warm | golangci cold | golangci warm | speedup (warm) |
|--------|--------|----------:|----------:|--------------:|--------------:|---------------:|
| fixture | `standard.yml` | 0.068s | 0.007s | 0.631s | 0.165s | 22.23x |
| local | `standard.yml` | 0.089s | 0.011s | 0.787s | 0.295s | 27.20x |
| gin | `.golangci.yml` | 0.418s | 0.028s | 3.829s | 0.369s | 13.18x |
| caddy | `.golangci.yml` | 0.931s | 0.063s | 9.040s | 0.861s | 13.66x |
| helm | `.golangci.yml` | 1.540s | 0.095s | 17.607s | 1.049s | 11.01x |
| k9s | `.golangci.yml` | 2.350s | 0.181s | 16.366s | 2.750s | 15.18x |
| cobra | `.golangci.yml` | 0.244s | 0.022s | 1.463s | 0.399s | 18.26x |

Speedup = golangci warm / guff warm. Values `>1.0x` mean guff was faster. ≈20x is a SCOREBOARD claim, not a hard CI fail threshold.
