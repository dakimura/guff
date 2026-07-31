# OSS SCOREBOARD (guff vs golangci-lint, own-config)

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.4`
- guff: `0.1.0`
- golangci-lint: `2.12.2`
- Samples: 1 (median)
- Both tools use each repository's real golangci-lint **v2** config.
- GOCACHE warm; linter caches measured cold then warm; clone/mod excluded.

| Target | config | guff warm | golangci warm | speedup |
|--------|--------|----------:|--------------:|--------:|
| gin | `.golangci.yml` | 0.029s | 0.373s | 13.05x |
| caddy | `.golangci.yml` | 0.054s | 0.869s | 16.06x |
| helm | `.golangci.yml` | 0.097s | 1.170s | 12.03x |

Full run detail: `20260731T163409Z.md`

