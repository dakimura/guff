# OSS SCOREBOARD (guff vs golangci-lint, own-config)

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.5`
- guff: `0.5.0`
- golangci-lint: `2.12.2`
- Samples: 3 (median)
- Both tools use each repository's real golangci-lint **v2** config.
- GOCACHE warm; linter caches measured cold then warm; clone/mod excluded.
- Speedup = golangci / guff for the same mode (`>1` means guff faster).

| Target | config | guff cold | golangci cold | cold × | guff warm | golangci warm | warm × |
|--------|--------|----------:|--------------:|-------:|----------:|--------------:|-------:|
| gin | `.golangci.yml` | 0.367s | 3.894s | 10.61x | 0.023s | 0.367s | 15.75x |
| caddy | `.golangci.yml` | 0.909s | 8.728s | 9.60x | 0.057s | 0.823s | 14.42x |
| helm | `.golangci.yml` | 1.330s | 17.409s | 13.09x | 0.096s | 1.028s | 10.70x |
| k9s | `.golangci.yml` | 2.104s | 14.336s | 6.81x | 0.180s | 2.272s | 12.64x |
| cobra | `.golangci.yml` | 0.221s | 1.388s | 6.29x | 0.018s | 0.392s | 21.58x |
| consul | `.golangci.yml` | 4.733s | 39.377s | 8.32x | 0.283s | 1.975s | 6.98x |
| grafana | `.golangci.yml` | 17.780s | 290.389s | 16.33x | 1.469s | 4.659s | 3.17x |
| containerd | `.golangci.yml` | 0.401s | 4.105s | 10.25x | 0.029s | 0.511s | 17.39x |

Full run detail: `20260816T231923Z.md`

