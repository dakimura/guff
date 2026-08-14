# OSS SCOREBOARD (guff vs golangci-lint, own-config)

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.5`
- guff: `0.4.1`
- golangci-lint: `n/a`
- Samples: 3 (median)
- Both tools use each repository's real golangci-lint **v2** config.
- GOCACHE warm; linter caches measured cold then warm; clone/mod excluded.
- Speedup = golangci / guff for the same mode (`>1` means guff faster).

| Target | config | guff cold | golangci cold | cold × | guff warm | golangci warm | warm × |
|--------|--------|----------:|--------------:|-------:|----------:|--------------:|-------:|
| gin | `.golangci.yml` | 0.521s | FAIL | — | 0.043s | FAIL | — |
| caddy | `.golangci.yml` | 2.214s | FAIL | — | 0.146s | FAIL | — |
| helm | `.golangci.yml` | 4.309s | FAIL | — | 0.246s | FAIL | — |
| k9s | `.golangci.yml` | 4.685s | FAIL | — | 0.299s | FAIL | — |
| cobra | `.golangci.yml` | 0.417s | FAIL | — | 0.036s | FAIL | — |

Full run detail: `20260813T234259Z.md`

