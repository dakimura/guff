# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./tsdb/...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 0.730 | 0.770 |
| peak_rss_bytes | 748,388,352 | 777,863,168 |
| guff_issues | 4 | 4 |
| golangci_issues | 4 | 0 |
| both | 4 | 0 |
| guff_only | 0 | 4 |
| golangci_only | 0 | 0 |
| precision | 1.0000 | 1.0000 |
| recall | 1.0000 | 1.0000 |

## FAIL

- `guff_only`: guff_only 4 > limit 0 (baseline 0)
- `both`: both 0 < floor 4 (baseline 4)
