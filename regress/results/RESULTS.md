# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./tsdb/...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 0.730 | 0.890 |
| peak_rss_bytes | 748,388,352 | 959,053,824 |
| guff_issues | 4 | 4 |
| golangci_issues | 4 | 4 |
| both | 4 | 4 |
| guff_only | 0 | 0 |
| golangci_only | 0 | 0 |
| precision | 1.0000 | 1.0000 |
| recall | 1.0000 | 1.0000 |

## FAIL

- `wall_seconds`: wall 0.890s > limit 0.880s (baseline 0.730s × 1.0 + 0.150s)
- `peak_rss_bytes`: peak RSS 959,053,824 > limit 898,066,022 (baseline 748,388,352 × 1.2)
