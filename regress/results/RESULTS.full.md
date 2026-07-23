# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 7.730 | 7.730 |
| peak_rss_bytes | 7,653,179,392 | 7,653,179,392 |
| guff_issues | 411 | 411 |
| golangci_issues | 20 | 20 |
| both | 16 | 16 |
| guff_only | 395 | 395 |
| golangci_only | 4 | 4 |
| precision | 0.0389 | 0.0389 |
| recall | 0.8000 | 0.8000 |

## PASS

No regressions vs baseline (within tolerances).
