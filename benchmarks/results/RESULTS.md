# Benchmark results

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.4`
- guff: `0.1.0`
- golangci-lint: `2.12.2`
- Samples per cell: 1 (median reported; `FAIL` if any sample exited non-zero)
- Fixture/local: `benchmarks/standard.yml` (standard five linters)
- OSS: each repo's real golangci-lint v2 config (own-config)
- Protocol: GOCACHE warm (prepare), tool caches cold then warm; clone/mod download excluded

| Target | config | guff cold | guff warm | golangci cold | golangci warm | speedup (warm) |
|--------|--------|----------:|----------:|--------------:|--------------:|---------------:|
| fixture | `standard.yml` | 0.076s | 0.007s | 0.625s | 0.167s | 23.53x |
| local | `standard.yml` | 0.093s | 0.011s | 0.784s | 0.295s | 26.61x |
| gin | `.golangci.yml` | 0.385s | 0.029s | 3.905s | 0.373s | 13.05x |
| caddy | `.golangci.yml` | 0.962s | 0.054s | 8.620s | 0.869s | 16.06x |
| helm | `.golangci.yml` | 1.333s | 0.097s | 17.011s | 1.170s | 12.03x |

Speedup = golangci warm / guff warm. Values `>1.0x` mean guff was faster. ≈20x is a SCOREBOARD claim, not a hard CI fail threshold.
