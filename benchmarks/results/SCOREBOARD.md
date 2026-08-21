# OSS SCOREBOARD (guff vs golangci-lint, own-config)

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.5`
- guff: `0.6.0`
- golangci-lint: `2.12.2`
- Samples: 3 (median)
- Both tools use each repository's real golangci-lint **v2** config.
- GOCACHE warm; linter caches measured cold then warm; clone/mod excluded.
- Speedup = golangci / guff for the same mode (`>1` means guff faster).

| Target | config | guff cold | golangci cold | cold × | guff warm | golangci warm | warm × |
|--------|--------|----------:|--------------:|-------:|----------:|--------------:|-------:|
| gin | `.golangci.yml` | 0.385s | 3.946s | 10.26x | 0.024s | 0.368s | 15.29x |
| caddy | `.golangci.yml` | 0.854s | 9.069s | 10.62x | 0.059s | 0.831s | 14.14x |
| helm | `.golangci.yml` | 1.357s | 17.490s | 12.89x | 0.097s | 1.024s | 10.61x |
| k9s | `.golangci.yml` | 2.168s | 14.611s | 6.74x | 0.184s | 2.385s | 12.96x |
| cobra | `.golangci.yml` | 0.234s | 1.418s | 6.06x | 0.018s | 0.403s | 21.81x |
| consul | `.golangci.yml` | 5.222s | 37.991s | 7.28x | 0.292s | 1.797s | 6.15x |
| grafana | `.golangci.yml` | 19.806s | 279.799s | 14.13x | 1.470s | 5.822s | 3.96x |
| containerd | `.golangci.yml` | 0.373s | 5.176s | 13.86x | 0.029s | 0.528s | 18.51x |

Full run detail: `20260821T003601Z.md`

