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
| gin | `.golangci.yml` | 0.409s | 4.485s | 10.97x | 0.024s | 0.380s | 15.74x |
| caddy | `.golangci.yml` | 0.968s | 10.336s | 10.67x | 0.060s | 0.911s | 15.15x |
| helm | `.golangci.yml` | 1.381s | 20.436s | 14.80x | 0.099s | 1.158s | 11.67x |
| k9s | `.golangci.yml` | 2.767s | 16.583s | 5.99x | 0.182s | 2.789s | 15.32x |
| cobra | `.golangci.yml` | 0.234s | 1.398s | 5.97x | 0.018s | 0.404s | 22.20x |
| go-client | `.golangci.yml` | 3.128s | 4.500s | 1.44x | 0.030s | 0.576s | 18.90x |
| consul | `.golangci.yml` | 4.240s | 39.732s | 9.37x | 0.296s | 1.879s | 6.36x |
| grafana | `.golangci.yml` | 22.333s | 271.036s | 12.14x | 1.491s | 5.925s | 3.97x |
| containerd | `.golangci.yml` | 0.379s | 5.096s | 13.46x | 0.028s | 0.518s | 18.58x |

Full run detail: `20260830T000524Z.md`

