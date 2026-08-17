# Benchmark results

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.5`
- guff: `0.5.0`
- golangci-lint: `2.12.2`
- Samples per cell: 3 (median reported; `FAIL` if any sample exited non-zero)
- Fixture/local: `benchmarks/standard.yml` (standard five linters)
- OSS: each repo's real golangci-lint v2 config (own-config)
- Protocol: GOCACHE warm (prepare), tool caches cold then warm; clone/mod download excluded

| Target | config | guff cold | guff warm | golangci cold | golangci warm | speedup (warm) |
|--------|--------|----------:|----------:|--------------:|--------------:|---------------:|
| fixture | `standard.yml` | 0.065s | 0.008s | 0.629s | 0.163s | 21.21x |
| local | `standard.yml` | 0.082s | 0.011s | 0.784s | 0.289s | 26.16x |
| gin | `.golangci.yml` | 0.367s | 0.023s | 3.894s | 0.367s | 15.75x |
| caddy | `.golangci.yml` | 0.909s | 0.057s | 8.728s | 0.823s | 14.42x |
| helm | `.golangci.yml` | 1.330s | 0.096s | 17.409s | 1.028s | 10.70x |
| k9s | `.golangci.yml` | 2.104s | 0.180s | 14.336s | 2.272s | 12.64x |
| cobra | `.golangci.yml` | 0.221s | 0.018s | 1.388s | 0.392s | 21.58x |
| consul | `.golangci.yml` | 4.733s | 0.283s | 39.377s | 1.975s | 6.98x |
| grafana | `.golangci.yml` | 17.780s | 1.469s | 290.389s | 4.659s | 3.17x |
| containerd | `.golangci.yml` | 0.401s | 0.029s | 4.105s | 0.511s | 17.39x |

Speedup = golangci warm / guff warm. Values `>1.0x` mean guff was faster. ≈20x is a SCOREBOARD claim, not a hard CI fail threshold.
