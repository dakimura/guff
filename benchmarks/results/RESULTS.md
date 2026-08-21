# Benchmark results

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.5`
- guff: `0.6.0`
- golangci-lint: `2.12.2`
- Samples per cell: 3 (median reported; `FAIL` if any sample exited non-zero)
- Fixture/local: `benchmarks/standard.yml` (standard five linters)
- OSS: each repo's real golangci-lint v2 config (own-config)
- Protocol: GOCACHE warm (prepare), tool caches cold then warm; clone/mod download excluded

| Target | config | guff cold | guff warm | golangci cold | golangci warm | speedup (warm) |
|--------|--------|----------:|----------:|--------------:|--------------:|---------------:|
| fixture | `standard.yml` | 0.066s | 0.008s | 0.647s | 0.164s | 21.06x |
| local | `standard.yml` | 0.082s | 0.011s | 0.780s | 0.284s | 27.03x |
| gin | `.golangci.yml` | 0.385s | 0.024s | 3.946s | 0.368s | 15.29x |
| caddy | `.golangci.yml` | 0.854s | 0.059s | 9.069s | 0.831s | 14.14x |
| helm | `.golangci.yml` | 1.357s | 0.097s | 17.490s | 1.024s | 10.61x |
| k9s | `.golangci.yml` | 2.168s | 0.184s | 14.611s | 2.385s | 12.96x |
| cobra | `.golangci.yml` | 0.234s | 0.018s | 1.418s | 0.403s | 21.81x |
| consul | `.golangci.yml` | 5.222s | 0.292s | 37.991s | 1.797s | 6.15x |
| grafana | `.golangci.yml` | 19.806s | 1.470s | 279.799s | 5.822s | 3.96x |
| containerd | `.golangci.yml` | 0.373s | 0.029s | 5.176s | 0.528s | 18.51x |

Speedup = golangci warm / guff warm. Values `>1.0x` mean guff was faster. ≈20x is a SCOREBOARD claim, not a hard CI fail threshold.
