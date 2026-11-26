# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./tsdb/...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 0.900 | 1.020 |
| peak_rss_bytes | 728,334,336 | 773,963,776 |
| guff_issues | 4 | 7 |
| golangci_issues | 4 | 7 |
| both | 4 | 7 |
| guff_only | 0 | 0 |
| golangci_only | 0 | 0 |
| precision | 1.0000 | 1.0000 |
| recall | 1.0000 | 1.0000 |

## PASS

No regressions vs baseline (within tolerances).
