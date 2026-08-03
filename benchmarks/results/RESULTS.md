# Benchmark results

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.4`
- guff: `0.2.0`
- golangci-lint: `2.12.2`
- Samples per cell: 3 (median reported; `FAIL` if any sample exited non-zero)
- Fixture/local: `benchmarks/standard.yml` (standard five linters)
- OSS: each repo's real golangci-lint v2 config (own-config)
- Protocol: GOCACHE warm (prepare), tool caches cold then warm; clone/mod download excluded

| Target | config | guff cold | guff warm | golangci cold | golangci warm | speedup (warm) |
|--------|--------|----------:|----------:|--------------:|--------------:|---------------:|
| fixture | `standard.yml` | 0.068s | 0.008s | 0.635s | 0.178s | 23.37x |
| local | `standard.yml` | 0.089s | 0.012s | 0.852s | 0.288s | 25.01x |
| gin | `.golangci.yml` | 0.384s | 0.032s | 4.219s | 0.375s | 11.80x |
| caddy | `.golangci.yml` | 0.989s | 0.060s | 10.019s | 0.892s | 14.92x |
| helm | `.golangci.yml` | 1.674s | 0.103s | 22.083s | 1.246s | 12.14x |
| consul | `.golangci.yml` | 6.083s | 0.319s | 47.242s | 2.353s | 7.37x |
| grafana | `.golangci.yml` | 23.831s | 1.906s | 394.845s | 7.233s | 3.80x |
| containerd | `.golangci.yml` | 0.520s | 0.038s | 7.281s | 0.696s | 18.55x |
| vault | `.golangci.yml` | 4.454s | 0.227s | 40.427s | 3.573s | 15.74x |
| kubernetes | `hack/golangci.yaml` | 0.636s | 0.057s | 6.272s | 1.048s | 18.29x |

Speedup = golangci warm / guff warm. Values `>1.0x` mean guff was faster. ≈20x is a SCOREBOARD claim, not a hard CI fail threshold.
