# OSS SCOREBOARD (guff vs golangci-lint, own-config)

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.5`
- guff: `0.4.1`
- golangci-lint: `2.12.2`
- Samples: 3 (median)
- Both tools use each repository's real golangci-lint **v2** config.
- GOCACHE warm; linter caches measured cold then warm; clone/mod excluded.
- Speedup = golangci / guff for the same mode (`>1` means guff faster).

| Target | config | guff cold | golangci cold | cold × | guff warm | golangci warm | warm × |
|--------|--------|----------:|--------------:|-------:|----------:|--------------:|-------:|
| gin | `.golangci.yml` | 0.418s | 3.829s | 9.17x | 0.028s | 0.369s | 13.18x |
| caddy | `.golangci.yml` | 0.931s | 9.040s | 9.71x | 0.063s | 0.861s | 13.66x |
| helm | `.golangci.yml` | 1.540s | 17.607s | 11.44x | 0.095s | 1.049s | 11.01x |
| k9s | `.golangci.yml` | 2.350s | 16.366s | 6.96x | 0.181s | 2.750s | 15.18x |
| cobra | `.golangci.yml` | 0.244s | 1.463s | 5.99x | 0.022s | 0.399s | 18.26x |

Full run detail: `20260814T014721Z.md`

