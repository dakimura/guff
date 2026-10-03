# Benchmark results

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.5`
- guff: `0.8.0` (main @ `6d0a553`, measured before the version bump; same code as v0.8.0)
- golangci-lint: `2.12.2`
- Samples per cell: 3 (median reported; `FAIL` if any sample exited non-zero)
- Fixture/local: `benchmarks/standard.yml` (standard five linters)
- OSS: each repo's real golangci-lint v2 config (own-config)
- Protocol: GOCACHE warm (prepare), tool caches cold then warm; clone/mod download excluded

| Target | config | guff cold | guff warm | golangci cold | golangci warm | speedup (warm) |
|--------|--------|----------:|----------:|--------------:|--------------:|---------------:|
| fixture | `standard.yml` | 0.073s | 0.013s | 0.624s | 0.164s | 12.58x |
| local | `standard.yml` | 0.090s | 0.016s | 0.786s | 0.292s | 18.12x |
| gin | `.golangci.yml` | 0.438s | 0.037s | 3.846s | 0.364s | 9.98x |
| caddy | `.golangci.yml` | 0.958s | 0.072s | 8.707s | 0.839s | 11.70x |
| helm | `.golangci.yml` | 1.410s | 0.110s | 16.981s | 1.034s | 9.43x |
| k9s | `.golangci.yml` | 2.683s | 0.197s | 14.962s | 2.256s | 11.44x |
| cobra | `.golangci.yml` | 0.262s | 0.031s | 1.393s | 0.390s | 12.58x |
| go-client | `.golangci.yml` | 1.021s | 0.049s | 3.676s | 0.577s | 11.68x |
| consul | `.golangci.yml` | 4.511s | 0.307s | 33.650s | 1.724s | 5.62x |
| grafana | `.golangci.yml` | 28.806s | 1.547s | 357.966s | 5.747s | 3.72x |
| containerd | `.golangci.yml` | 0.411s | 0.041s | 5.014s | 0.521s | 12.75x |

Speedup = golangci warm / guff warm. Values `>1.0x` mean guff was faster. ≈20x is a SCOREBOARD claim, not a hard CI fail threshold.
