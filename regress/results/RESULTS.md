# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./tsdb/...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 2.100 | 2.190 |
| peak_rss_bytes | 1,400,078,336 | 1,424,605,184 |
| guff_issues | 74 | 74 |
| golangci_issues | 4 | 4 |
| both | 4 | 4 |
| guff_only | 70 | 70 |
| golangci_only | 0 | 0 |
| precision | 0.0541 | 0.0541 |
| recall | 1.0000 | 1.0000 |

## PASS

No regressions vs baseline (within tolerances).
