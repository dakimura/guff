# Benchmark results

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.5`
- guff: `0.4.1`
- golangci-lint: `n/a`
- Samples per cell: 3 (median reported; `FAIL` if any sample exited non-zero)
- Fixture/local: `benchmarks/standard.yml` (standard five linters)
- OSS: each repo's real golangci-lint v2 config (own-config)
- Protocol: GOCACHE warm (prepare), tool caches cold then warm; clone/mod download excluded

| Target | config | guff cold | guff warm | golangci cold | golangci warm | speedup (warm) |
|--------|--------|----------:|----------:|--------------:|--------------:|---------------:|
| fixture | `standard.yml` | 0.074s | 0.008s | FAIL | FAIL | — |
| local | `standard.yml` | 0.092s | 0.012s | FAIL | FAIL | — |
| gin | `.golangci.yml` | 0.521s | 0.043s | FAIL | FAIL | — |
| caddy | `.golangci.yml` | 2.214s | 0.146s | FAIL | FAIL | — |
| helm | `.golangci.yml` | 4.309s | 0.246s | FAIL | FAIL | — |
| k9s | `.golangci.yml` | 4.685s | 0.299s | FAIL | FAIL | — |
| cobra | `.golangci.yml` | 0.417s | 0.036s | FAIL | FAIL | — |

Speedup = golangci warm / guff warm. Values `>1.0x` mean guff was faster. ≈20x is a SCOREBOARD claim, not a hard CI fail threshold.
