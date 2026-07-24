# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 7.370 | 7.530 |
| peak_rss_bytes | 7,482,376,192 | 7,585,202,176 |
| guff_issues | 379 | 379 |
| golangci_issues | 20 | 20 |
| both | 16 | 16 |
| guff_only | 363 | 363 |
| golangci_only | 4 | 4 |
| precision | 0.0422 | 0.0422 |
| recall | 0.8000 | 0.8000 |

## PASS

No regressions vs baseline (within tolerances).
