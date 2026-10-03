# OSS SCOREBOARD (guff vs golangci-lint, own-config)

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.5`
- guff: `0.8.0` (main @ `6d0a553`, measured before the version bump; same code as v0.8.0)
- golangci-lint: `2.12.2`
- Samples: 3 (median)
- Both tools use each repository's real golangci-lint **v2** config.
- GOCACHE warm; linter caches measured cold then warm; clone/mod excluded.
- Speedup = golangci / guff for the same mode (`>1` means guff faster).

| Target | config | guff cold | golangci cold | cold × | guff warm | golangci warm | warm × |
|--------|--------|----------:|--------------:|-------:|----------:|--------------:|-------:|
| gin | `.golangci.yml` | 0.438s | 3.846s | 8.78x | 0.037s | 0.364s | 9.98x |
| caddy | `.golangci.yml` | 0.958s | 8.707s | 9.09x | 0.072s | 0.839s | 11.70x |
| helm | `.golangci.yml` | 1.410s | 16.981s | 12.04x | 0.110s | 1.034s | 9.43x |
| k9s | `.golangci.yml` | 2.683s | 14.962s | 5.58x | 0.197s | 2.256s | 11.44x |
| cobra | `.golangci.yml` | 0.262s | 1.393s | 5.31x | 0.031s | 0.390s | 12.58x |
| go-client | `.golangci.yml` | 1.021s | 3.676s | 3.60x | 0.049s | 0.577s | 11.68x |
| consul | `.golangci.yml` | 4.511s | 33.650s | 7.46x | 0.307s | 1.724s | 5.62x |
| grafana | `.golangci.yml` | 28.806s | 357.966s | 12.43x | 1.547s | 5.747s | 3.72x |
| containerd | `.golangci.yml` | 0.411s | 5.014s | 12.19x | 0.041s | 0.521s | 12.75x |

Full run detail: `20261002T222858Z.md`

