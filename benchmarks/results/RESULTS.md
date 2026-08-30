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
| fixture | `standard.yml` | 0.069s | 0.008s | 0.625s | 0.164s | 20.63x |
| local | `standard.yml` | 0.082s | 0.011s | 0.770s | 0.298s | 27.87x |
| gin | `.golangci.yml` | 0.409s | 0.024s | 4.485s | 0.380s | 15.74x |
| caddy | `.golangci.yml` | 0.968s | 0.060s | 10.336s | 0.911s | 15.15x |
| helm | `.golangci.yml` | 1.381s | 0.099s | 20.436s | 1.158s | 11.67x |
| k9s | `.golangci.yml` | 2.767s | 0.182s | 16.583s | 2.789s | 15.32x |
| cobra | `.golangci.yml` | 0.234s | 0.018s | 1.398s | 0.404s | 22.20x |
| go-client | `.golangci.yml` | 3.128s | 0.030s | 4.500s | 0.576s | 18.90x |
| consul | `.golangci.yml` | 4.240s | 0.296s | 39.732s | 1.879s | 6.36x |
| grafana | `.golangci.yml` | 22.333s | 1.491s | 271.036s | 5.925s | 3.97x |
| containerd | `.golangci.yml` | 0.379s | 0.028s | 5.096s | 0.518s | 18.58x |

Speedup = golangci warm / guff warm. Values `>1.0x` mean guff was faster. ≈20x is a SCOREBOARD claim, not a hard CI fail threshold.
