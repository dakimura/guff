# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./tsdb/...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 0.900 | 0.970 |
| peak_rss_bytes | 728,334,336 | 753,123,328 |
| guff_issues | 4 | 4 |
| golangci_issues | 4 | 4 |
| both | 4 | 4 |
| guff_only | 0 | 0 |
| golangci_only | 0 | 0 |
| precision | 1.0000 | 1.0000 |
| recall | 1.0000 | 1.0000 |

## PASS

No regressions vs baseline (within tolerances).
