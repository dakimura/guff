# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./tsdb/...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 3.470 | 3.470 |
| peak_rss_bytes | 1,392,279,552 | 1,392,279,552 |
| guff_issues | 76 | 76 |
| golangci_issues | 4 | 4 |
| both | 4 | 4 |
| guff_only | 72 | 72 |
| golangci_only | 0 | 0 |
| precision | 0.0526 | 0.0526 |
| recall | 1.0000 | 1.0000 |

## PASS

No regressions vs baseline (within tolerances).
