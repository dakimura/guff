# Prometheus regress gate

- Baseline SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Measured SHA: `66df005b9d8abe8a91a41a9afab022a71b313e7d`
- Config: `.golangci.yml`
- Packages: `./...`
- Concurrency: `-j 0` / `RAYON_NUM_THREADS=0`

| Metric | Baseline | Measured |
|--------|---------:|---------:|
| wall_seconds | 2.360 | 4.150 |
| peak_rss_bytes | 3,114,582,016 | 3,420,864,512 |
| guff_issues | 20 | 24 |
| golangci_issues | 20 | 20 |
| both | 20 | 20 |
| guff_only | 0 | 4 |
| golangci_only | 0 | 0 |
| precision | 1.0000 | 0.8333 |
| recall | 1.0000 | 1.0000 |

## FAIL

- `wall_seconds`: wall 4.150s > limit 2.510s (baseline 2.360s × 1.0 + 0.150s)
- `guff_only`: guff_only 4 > limit 0 (baseline 0)
