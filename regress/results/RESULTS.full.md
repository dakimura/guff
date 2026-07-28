# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 4.940 | 3.830 |
| peak_rss_bytes | 7,608,352,768 | 7,271,366,656 |
| guff_issues | 20 | 20 |
| golangci_issues | 20 | 20 |
| both | 20 | 20 |
| guff_only | 0 | 0 |
| golangci_only | 0 | 0 |
| precision | 1.0000 | 1.0000 |
| recall | 1.0000 | 1.0000 |

## PASS

No regressions vs baseline (within tolerances).
