# OSS SCOREBOARD (guff vs golangci-lint, own-config)

- Host: `Darwin 25.2.0 arm64`
- Go: `go1.26.4`
- guff: `0.2.0`
- golangci-lint: `2.12.2`
- Samples: 3 (median)
- Both tools use each repository's real golangci-lint **v2** config.
- GOCACHE warm; linter caches measured cold then warm; clone/mod excluded.
- Speedup = golangci / guff for the same mode (`>1` means guff faster).

| Target | config | guff cold | golangci cold | cold × | guff warm | golangci warm | warm × |
|--------|--------|----------:|--------------:|-------:|----------:|--------------:|-------:|
| gin | `.golangci.yml` | 0.384s | 4.219s | 10.99x | 0.032s | 0.375s | 11.80x |
| caddy | `.golangci.yml` | 0.989s | 10.019s | 10.13x | 0.060s | 0.892s | 14.92x |
| helm | `.golangci.yml` | 1.674s | 22.083s | 13.19x | 0.103s | 1.246s | 12.14x |
| consul | `.golangci.yml` | 6.083s | 47.242s | 7.77x | 0.319s | 2.353s | 7.37x |
| grafana | `.golangci.yml` | 23.831s | 394.845s | 16.57x | 1.906s | 7.233s | 3.80x |
| containerd | `.golangci.yml` | 0.520s | 7.281s | 13.99x | 0.038s | 0.696s | 18.55x |
| vault | `.golangci.yml` | 4.454s | 40.427s | 9.08x | 0.227s | 3.573s | 15.74x |
| kubernetes | `hack/golangci.yaml` | 0.636s | 6.272s | 9.87x | 0.057s | 1.048s | 18.29x |

Full run detail: `20260803T061939Z.md`

